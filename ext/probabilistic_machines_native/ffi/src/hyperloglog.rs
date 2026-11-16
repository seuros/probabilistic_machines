//! `ProbabilisticMachines::HyperLogLog`: how many distinct items a stream held.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RModule, RString, Ruby, Value, function, method, prelude::*};
use matryona_machines::ruby::integer;
use probabilistic_machines::{HyperLogLog, Refusal};

use crate::data::bytes;
use crate::{Pinned, answer, decoded_or_refused, define_class, partner, sizes, unsupported};

/// Register counts [`Sketch`] is monomorphised for.
const REGISTERS: [usize; 4] = [1024, 4096, 16384, 65536];

#[derive(Clone)]
enum Sketch {
    M1024(Box<HyperLogLog<1024>>),
    M4096(Box<HyperLogLog<4096>>),
    M16384(Box<HyperLogLog<16384>>),
    M65536(Box<HyperLogLog<65536>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! sketch {
    ($sketch:expr, |$inner:ident| $body:expr) => {
        match $sketch {
            Sketch::M1024($inner) => $body,
            Sketch::M4096($inner) => $body,
            Sketch::M16384($inner) => $body,
            Sketch::M65536($inner) => $body,
        }
    };
}

/// The class, pinned at init.
static CLASS: Pinned = Pinned::new("probabilistic_machines");

#[magnus::wrap(class = "ProbabilisticMachines::HyperLogLog", free_immediately, size)]
struct Wrapper(RefCell<Sketch>);

const fn registers_of<const M: usize>(_: &HyperLogLog<M>) -> usize {
    M
}

const fn standard_error_of<const M: usize>(_: &HyperLogLog<M>) -> f64 {
    HyperLogLog::<M>::standard_error()
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the match below at once.
#[inline(never)]
fn boxed_new<const N: usize>() -> Box<HyperLogLog<N>> {
    Box::new(HyperLogLog::new())
}

#[inline(never)]
fn boxed_decode<const N: usize>(frame: &[u8]) -> Result<Box<HyperLogLog<N>>, Refusal> {
    let mut sketch = boxed_new::<N>();
    sketch.decode_into(frame)?;
    Ok(sketch)
}

/// The sketch a frame holds. A precision this build does not carry is read
/// as the smallest shape it does, which answers exactly what the core answers
/// a type of another shape: `Truncated`, `WrongStructure` or `WrongShape`.
fn decoded(frame: &[u8]) -> Result<Sketch, Refusal> {
    Ok(match frame.get(1) {
        Some(12) => Sketch::M4096(boxed_decode(frame)?),
        Some(14) => Sketch::M16384(boxed_decode(frame)?),
        Some(16) => Sketch::M65536(boxed_decode(frame)?),
        Some(_) | None => Sketch::M1024(boxed_decode(frame)?),
    })
}

impl Wrapper {
    /// `HyperLogLog.new(registers)`. `RangeError` outside [`REGISTERS`].
    fn new(ruby: &Ruby, registers: Value) -> Result<Self, Error> {
        let registers: usize = integer(registers, "registers")?;
        let sketch = match registers {
            1024 => Sketch::M1024(boxed_new()),
            4096 => Sketch::M4096(boxed_new()),
            16384 => Sketch::M16384(boxed_new()),
            65536 => Sketch::M65536(boxed_new()),
            _ => return Err(unsupported(ruby, "registers", registers, &REGISTERS)),
        };
        Ok(Self(RefCell::new(sketch)))
    }

    /// `HyperLogLog.decode(bytes)`: a sketch, or the refusal symbol.
    fn decode(ruby: &Ruby, bytes: Value) -> Result<Value, Error> {
        let bytes = self::bytes(bytes, "bytes")?;
        Ok(decoded_or_refused(
            ruby,
            decoded(&bytes).map(|held| Self(RefCell::new(held))),
        ))
    }

    fn decode_into(ruby: &Ruby, this: Obj<Self>, bytes: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let bytes = self::bytes(bytes, "bytes")?;
        let outcome = sketch!(&mut *this.0.borrow_mut(), |sketch| sketch
            .decode_into(&bytes));
        Ok(answer(ruby, outcome, "decoded"))
    }

    fn registers(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| registers_of(&**sketch))
    }

    fn add(this: Obj<Self>, item: Value) -> Result<(), Error> {
        this.check_frozen()?;
        let item = bytes(item, "item")?;
        sketch!(&mut *this.0.borrow_mut(), |sketch| sketch.add(&item));
        Ok(())
    }

    fn add_hash(this: Obj<Self>, hash: Value) -> Result<(), Error> {
        this.check_frozen()?;
        let hash: u64 = integer(hash, "hash")?;
        sketch!(&mut *this.0.borrow_mut(), |sketch| sketch.add_hash(hash));
        Ok(())
    }

    fn estimate(&self) -> f64 {
        sketch!(&*self.0.borrow(), |sketch| sketch.estimate())
    }

    fn standard_error(&self) -> f64 {
        sketch!(&*self.0.borrow(), |sketch| standard_error_of(&**sketch))
    }

    fn is_empty(&self) -> bool {
        sketch!(&*self.0.borrow(), |sketch| sketch.is_empty())
    }

    /// `:merged`, or `:wrong_shape` for a sketch of another register count
    /// (in Rust that merge does not compile).
    fn merge(ruby: &Ruby, this: Obj<Self>, other: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let other: Obj<Self> = partner(ruby, &CLASS, other)?;
        let source = other.0.borrow().clone();
        let merged = match (&mut *this.0.borrow_mut(), &source) {
            (Sketch::M1024(target), Sketch::M1024(source)) => {
                target.merge(source);
                Ok(())
            }
            (Sketch::M4096(target), Sketch::M4096(source)) => {
                target.merge(source);
                Ok(())
            }
            (Sketch::M16384(target), Sketch::M16384(source)) => {
                target.merge(source);
                Ok(())
            }
            (Sketch::M65536(target), Sketch::M65536(source)) => {
                target.merge(source);
                Ok(())
            }
            (
                Sketch::M1024(_) | Sketch::M4096(_) | Sketch::M16384(_) | Sketch::M65536(_),
                Sketch::M1024(_) | Sketch::M4096(_) | Sketch::M16384(_) | Sketch::M65536(_),
            ) => Err(Refusal::WrongShape),
        };
        Ok(answer(ruby, merged, "merged"))
    }

    fn encoded(ruby: &Ruby, this: &Self) -> RString {
        let frame = sketch!(&*this.0.borrow(), |sketch| sketch.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<ProbabilisticMachines::HyperLogLog registers={} estimate={:.1}>",
            self.registers(),
            self.estimate()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = define_class(ruby, module, "HyperLogLog", &CLASS)?;
    class.const_set("REGISTERS", sizes(ruby, &REGISTERS))?;
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("decode_into", method!(Wrapper::decode_into, 1))?;
    class.define_method("registers", method!(Wrapper::registers, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("add_hash", method!(Wrapper::add_hash, 1))?;
    class.define_method("estimate", method!(Wrapper::estimate, 0))?;
    class.define_method("standard_error", method!(Wrapper::standard_error, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
