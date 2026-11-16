//! `ProbabilisticMachines::CountMinSketch`: how often each item was seen.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RModule, RString, Ruby, Value, function, method, prelude::*};
use matryona_machines::ruby::integer;
use probabilistic_machines::{CountMinSketch, Refusal};

use crate::data::bytes;
use crate::{Pinned, answer, decoded_or_refused, define_class, partner, sizes, unsupported};

/// Row widths [`Sketch`] is monomorphised for.
///
/// Capped so the widest sketch is 128 KiB. The core returns by value, so a
/// wider shape could not be built on Ruby's 256 KiB fiber stack without an
/// in-place constructor, which safe Rust cannot write.
const WIDTHS: [usize; 2] = [1024, 4096];
/// Row counts [`Sketch`] is monomorphised for. Every width pairs with every
/// depth.
const DEPTHS: [usize; 2] = [4, 8];

#[derive(Clone)]
enum Sketch {
    W1024D4(Box<CountMinSketch<1024, 4>>),
    W1024D8(Box<CountMinSketch<1024, 8>>),
    W4096D4(Box<CountMinSketch<4096, 4>>),
    W4096D8(Box<CountMinSketch<4096, 8>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! sketch {
    ($sketch:expr, |$inner:ident| $body:expr) => {
        match $sketch {
            Sketch::W1024D4($inner) => $body,
            Sketch::W1024D8($inner) => $body,
            Sketch::W4096D4($inner) => $body,
            Sketch::W4096D8($inner) => $body,
        }
    };
}

/// The class, pinned at init.
static CLASS: Pinned = Pinned::new("probabilistic_machines");

#[magnus::wrap(
    class = "ProbabilisticMachines::CountMinSketch",
    free_immediately,
    size
)]
struct Wrapper(RefCell<Sketch>);

const fn width_of<const W: usize, const D: usize>(_: &CountMinSketch<W, D>) -> usize {
    W
}

const fn depth_of<const W: usize, const D: usize>(_: &CountMinSketch<W, D>) -> usize {
    D
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the match below at once.
#[inline(never)]
fn boxed_new<const W: usize, const D: usize>() -> Box<CountMinSketch<W, D>> {
    Box::new(CountMinSketch::new())
}

#[inline(never)]
fn boxed_decode<const W: usize, const D: usize>(
    frame: &[u8],
) -> Result<Box<CountMinSketch<W, D>>, Refusal> {
    let mut sketch = boxed_new::<W, D>();
    sketch.decode_into(frame)?;
    Ok(sketch)
}

/// The sketch a frame holds. A shape this build does not carry is read as the
/// smallest one it does, which answers exactly what the core answers a type of
/// another shape: `Truncated`, `WrongStructure` or `WrongShape`.
fn decoded(frame: &[u8]) -> Result<Sketch, Refusal> {
    Ok(match (frame.get(1), frame.get(2)) {
        (Some(10), Some(8)) => Sketch::W1024D8(boxed_decode(frame)?),
        (Some(12), Some(4)) => Sketch::W4096D4(boxed_decode(frame)?),
        (Some(12), Some(8)) => Sketch::W4096D8(boxed_decode(frame)?),
        (Some(_) | None, Some(_) | None) => Sketch::W1024D4(boxed_decode(frame)?),
    })
}

impl Wrapper {
    /// `CountMinSketch.new(width, depth)`. `RangeError` outside [`WIDTHS`] by
    /// [`DEPTHS`].
    fn new(ruby: &Ruby, width: Value, depth: Value) -> Result<Self, Error> {
        let (width, depth): (usize, usize) = (integer(width, "width")?, integer(depth, "depth")?);
        if !WIDTHS.contains(&width) {
            return Err(unsupported(ruby, "width", width, &WIDTHS));
        }
        let sketch = match (width, depth) {
            (1024, 4) => Sketch::W1024D4(boxed_new()),
            (1024, 8) => Sketch::W1024D8(boxed_new()),
            (4096, 4) => Sketch::W4096D4(boxed_new()),
            (4096, 8) => Sketch::W4096D8(boxed_new()),
            _ => return Err(unsupported(ruby, "depth", depth, &DEPTHS)),
        };
        Ok(Self(RefCell::new(sketch)))
    }

    /// `CountMinSketch.decode(bytes)`: a sketch, or the refusal symbol.
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

    fn width(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| width_of(&**sketch))
    }

    fn depth(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| depth_of(&**sketch))
    }

    fn add(ruby: &Ruby, this: Obj<Self>, item: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let item = bytes(item, "item")?;
        let outcome = sketch!(&mut *this.0.borrow_mut(), |sketch| sketch.add(&item));
        Ok(answer(ruby, outcome, "added"))
    }

    fn add_count(ruby: &Ruby, this: Obj<Self>, item: Value, count: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let item = bytes(item, "item")?;
        let count: u32 = integer(count, "count")?;
        let outcome = sketch!(&mut *this.0.borrow_mut(), |sketch| sketch
            .add_count(&item, count));
        Ok(answer(ruby, outcome, "added"))
    }

    fn add_hash_count(
        ruby: &Ruby,
        this: Obj<Self>,
        hash: Value,
        count: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let hash: u64 = integer(hash, "hash")?;
        let count: u32 = integer(count, "count")?;
        let outcome = sketch!(&mut *this.0.borrow_mut(), |sketch| sketch
            .add_hash_count(hash, count));
        Ok(answer(ruby, outcome, "added"))
    }

    fn estimate(&self, item: Value) -> Result<u32, Error> {
        let item = bytes(item, "item")?;
        Ok(sketch!(&*self.0.borrow(), |sketch| sketch.estimate(&item)))
    }

    fn estimate_hash(&self, hash: Value) -> Result<u32, Error> {
        let hash = integer(hash, "hash")?;
        Ok(sketch!(&*self.0.borrow(), |sketch| sketch.estimate_hash(hash)))
    }

    fn total(&self) -> u64 {
        sketch!(&*self.0.borrow(), |sketch| sketch.total())
    }

    /// `:merged`, `:overflow`, or `:wrong_shape` for a sketch of another
    /// width or depth (in Rust that merge does not compile).
    fn merge(ruby: &Ruby, this: Obj<Self>, other: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let other: Obj<Self> = partner(ruby, &CLASS, other)?;
        let source = other.0.borrow().clone();
        let merged = match (&mut *this.0.borrow_mut(), &source) {
            (Sketch::W1024D4(target), Sketch::W1024D4(source)) => target.merge(source),
            (Sketch::W1024D8(target), Sketch::W1024D8(source)) => target.merge(source),
            (Sketch::W4096D4(target), Sketch::W4096D4(source)) => target.merge(source),
            (Sketch::W4096D8(target), Sketch::W4096D8(source)) => target.merge(source),
            (
                Sketch::W1024D4(_) | Sketch::W1024D8(_) | Sketch::W4096D4(_) | Sketch::W4096D8(_),
                Sketch::W1024D4(_) | Sketch::W1024D8(_) | Sketch::W4096D4(_) | Sketch::W4096D8(_),
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
            "#<ProbabilisticMachines::CountMinSketch width={} depth={} total={}>",
            self.width(),
            self.depth(),
            self.total()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = define_class(ruby, module, "CountMinSketch", &CLASS)?;
    class.const_set("WIDTHS", sizes(ruby, &WIDTHS))?;
    class.const_set("DEPTHS", sizes(ruby, &DEPTHS))?;
    class.define_singleton_method("new", function!(Wrapper::new, 2))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("decode_into", method!(Wrapper::decode_into, 1))?;
    class.define_method("width", method!(Wrapper::width, 0))?;
    class.define_method("depth", method!(Wrapper::depth, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("add_count", method!(Wrapper::add_count, 2))?;
    class.define_method("add_hash_count", method!(Wrapper::add_hash_count, 2))?;
    class.define_method("estimate", method!(Wrapper::estimate, 1))?;
    class.define_method("estimate_hash", method!(Wrapper::estimate_hash, 1))?;
    class.define_method("total", method!(Wrapper::total, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
