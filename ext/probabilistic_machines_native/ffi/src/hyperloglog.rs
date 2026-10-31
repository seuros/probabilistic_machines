//! `ProbabilisticMachines::HyperLogLog`: how many distinct items a stream held.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{CodecError, HyperLogLog};

use crate::{codec_error, mismatch, required_kwarg, unsupported, unsupported_blob};

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

#[magnus::wrap(class = "ProbabilisticMachines::HyperLogLog", free_immediately, size)]
struct Wrapper(RefCell<Sketch>);

const fn registers_of<const M: usize>(_: &HyperLogLog<M>) -> usize {
    M
}

fn standard_error_of<const M: usize>(_: &HyperLogLog<M>) -> f64 {
    HyperLogLog::<M>::standard_error()
}

fn encode_frame<const M: usize>(sketch: &HyperLogLog<M>) -> Result<Vec<u8>, CodecError> {
    let mut frame = vec![0_u8; HyperLogLog::<M>::ENCODED_LEN];
    let length = sketch.encode(&mut frame)?;
    frame.truncate(length);
    Ok(frame)
}

fn decode_frame<const M: usize>(ruby: &Ruby, frame: &[u8]) -> Result<HyperLogLog<M>, Error> {
    HyperLogLog::decode(frame).map_err(|error| codec_error(ruby, error))
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the match below at once.
#[inline(never)]
fn boxed_new<const N: usize>() -> Box<HyperLogLog<N>> {
    Box::new(HyperLogLog::new())
}

#[inline(never)]
fn boxed_decode<const N: usize>(ruby: &Ruby, frame: &[u8]) -> Result<Box<HyperLogLog<N>>, Error> {
    Ok(Box::new(decode_frame(ruby, frame)?))
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let registers: usize = required_kwarg(args.keywords, "registers")?;
        let sketch = match registers {
            1024 => Sketch::M1024(boxed_new::<1024>()),
            4096 => Sketch::M4096(boxed_new::<4096>()),
            16384 => Sketch::M16384(boxed_new::<16384>()),
            65536 => Sketch::M65536(boxed_new::<65536>()),
            _ => return Err(unsupported(ruby, "registers", registers, &REGISTERS)),
        };
        Ok(Self(RefCell::new(sketch)))
    }

    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let Some(&precision) = blob.get(1) else {
            return Err(codec_error(ruby, CodecError::Truncated));
        };
        let sketch = match precision {
            10 => Sketch::M1024(boxed_decode::<1024>(ruby, &blob)?),
            12 => Sketch::M4096(boxed_decode::<4096>(ruby, &blob)?),
            14 => Sketch::M16384(boxed_decode::<16384>(ruby, &blob)?),
            16 => Sketch::M65536(boxed_decode::<65536>(ruby, &blob)?),
            _ => return Err(unsupported_blob(ruby, "register count", &REGISTERS)),
        };
        Ok(Self(RefCell::new(sketch)))
    }

    fn registers(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| registers_of(&**sketch))
    }

    fn add(&self, item: Bytes) {
        sketch!(&mut *self.0.borrow_mut(), |sketch| sketch.add(&item));
    }

    fn add_hash(&self, hash: u64) {
        sketch!(&mut *self.0.borrow_mut(), |sketch| sketch.add_hash(hash));
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

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.0.borrow().clone();
        match (&mut *rb_self.0.borrow_mut(), &source) {
            (Sketch::M1024(target), Sketch::M1024(source)) => target.merge(source),
            (Sketch::M4096(target), Sketch::M4096(source)) => target.merge(source),
            (Sketch::M16384(target), Sketch::M16384(source)) => target.merge(source),
            (Sketch::M65536(target), Sketch::M65536(source)) => target.merge(source),
            _ => return Err(mismatch(ruby, "sketch")),
        }
        Ok(rb_self)
    }

    fn dump(ruby: &Ruby, rb_self: &Self) -> Result<RString, Error> {
        let frame = sketch!(&*rb_self.0.borrow(), |sketch| encode_frame(&**sketch))
            .map_err(|error| codec_error(ruby, error))?;
        Ok(ruby.str_from_slice(&frame))
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
    let class = module.define_class("HyperLogLog", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("registers", method!(Wrapper::registers, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("add_hash", method!(Wrapper::add_hash, 1))?;
    class.define_method("estimate", method!(Wrapper::estimate, 0))?;
    class.define_method("standard_error", method!(Wrapper::standard_error, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
