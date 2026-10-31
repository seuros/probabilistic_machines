//! `ProbabilisticMachines::CountMinSketch`: how often each item was seen.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::typed_data::Obj;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{CodecError, CountMinSketch};

use crate::{codec_error, mismatch, optional_kwarg, unsupported};

/// Row widths [`Sketch`] is monomorphised for.
///
/// Capped so the widest sketch is 128 KiB. The core returns by value, so a
/// wider shape could not be built on Ruby's 256 KiB fiber stack without an
/// in-place constructor, and that needs `unsafe`.
const WIDTHS: [usize; 2] = [1024, 4096];
/// Row counts [`Sketch`] is monomorphised for.
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

fn encode_frame<const W: usize, const D: usize>(
    sketch: &CountMinSketch<W, D>,
) -> Result<Vec<u8>, CodecError> {
    let mut frame = vec![0_u8; CountMinSketch::<W, D>::ENCODED_LEN];
    let length = sketch.encode(&mut frame)?;
    frame.truncate(length);
    Ok(frame)
}

fn decode_frame<const W: usize, const D: usize>(
    ruby: &Ruby,
    frame: &[u8],
) -> Result<CountMinSketch<W, D>, Error> {
    CountMinSketch::decode(frame).map_err(|error| codec_error(ruby, error))
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the match below at once.
#[inline(never)]
fn boxed_new<const W: usize, const D: usize>() -> Box<CountMinSketch<W, D>> {
    Box::new(CountMinSketch::new())
}

#[inline(never)]
fn boxed_decode<const W: usize, const D: usize>(
    ruby: &Ruby,
    frame: &[u8],
) -> Result<Box<CountMinSketch<W, D>>, Error> {
    Ok(Box::new(decode_frame(ruby, frame)?))
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (width, depth): (usize, usize) =
            get_kwargs::<_, (usize, usize), (), ()>(args.keywords, &["width", "depth"], &[])?
                .required;
        if !DEPTHS.contains(&depth) {
            return Err(unsupported(ruby, "depth", depth, &DEPTHS));
        }
        let sketch = match (width, depth) {
            (1024, 4) => Sketch::W1024D4(boxed_new::<1024, 4>()),
            (1024, 8) => Sketch::W1024D8(boxed_new::<1024, 8>()),
            (4096, 4) => Sketch::W4096D4(boxed_new::<4096, 4>()),
            (4096, 8) => Sketch::W4096D8(boxed_new::<4096, 8>()),
            _ => return Err(unsupported(ruby, "width", width, &WIDTHS)),
        };
        Ok(Self(RefCell::new(sketch)))
    }

    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let Some(shape) = blob.get(1..3) else {
            return Err(codec_error(ruby, CodecError::Truncated));
        };
        let sketch = match (shape[0], shape[1]) {
            (10, 4) => Sketch::W1024D4(boxed_decode::<1024, 4>(ruby, &blob)?),
            (10, 8) => Sketch::W1024D8(boxed_decode::<1024, 8>(ruby, &blob)?),
            (12, 4) => Sketch::W4096D4(boxed_decode::<4096, 4>(ruby, &blob)?),
            (12, 8) => Sketch::W4096D8(boxed_decode::<4096, 8>(ruby, &blob)?),
            _ => {
                return Err(Error::new(
                    ruby.exception_arg_error(),
                    format!("blob was written with a shape outside {WIDTHS:?} x {DEPTHS:?}"),
                ));
            }
        };
        Ok(Self(RefCell::new(sketch)))
    }

    fn width(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| width_of(&**sketch))
    }

    fn depth(&self) -> usize {
        sketch!(&*self.0.borrow(), |sketch| depth_of(&**sketch))
    }

    fn add(rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(Bytes,), (), (), (), RHash, ()>(args)?;
        let (item,) = args.required;
        let count: u32 = optional_kwarg(args.keywords, "count")?.unwrap_or(1);
        sketch!(&mut *rb_self.0.borrow_mut(), |sketch| sketch
            .add_count(&item, count));
        Ok(())
    }

    fn count(&self, item: Bytes) -> u32 {
        sketch!(&*self.0.borrow(), |sketch| sketch.estimate(&item))
    }

    fn total(&self) -> u64 {
        sketch!(&*self.0.borrow(), |sketch| sketch.total())
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.0.borrow().clone();
        match (&mut *rb_self.0.borrow_mut(), &source) {
            (Sketch::W1024D4(target), Sketch::W1024D4(source)) => target.merge(source),
            (Sketch::W1024D8(target), Sketch::W1024D8(source)) => target.merge(source),
            (Sketch::W4096D4(target), Sketch::W4096D4(source)) => target.merge(source),
            (Sketch::W4096D8(target), Sketch::W4096D8(source)) => target.merge(source),
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
            "#<ProbabilisticMachines::CountMinSketch width={} depth={} total={}>",
            self.width(),
            self.depth(),
            self.total()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("CountMinSketch", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("width", method!(Wrapper::width, 0))?;
    class.define_method("depth", method!(Wrapper::depth, 0))?;
    class.define_method("add", method!(Wrapper::add, -1))?;
    class.define_method("count", method!(Wrapper::count, 1))?;
    class.define_method("total", method!(Wrapper::total, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
