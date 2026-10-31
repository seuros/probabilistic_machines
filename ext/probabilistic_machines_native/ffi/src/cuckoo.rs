//! `ProbabilisticMachines::CuckooFilter`: membership that can forget.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::{Error, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{CodecError, CuckooFilter};

use crate::{Whole, codec_error, full, required_kwarg, unsupported, unsupported_blob};

/// Bucket counts [`Filter`] is monomorphised for.
const BUCKETS: [usize; 4] = [1024, 4096, 16384, 65536];

enum Filter {
    B1024(Box<CuckooFilter<1024>>),
    B4096(Box<CuckooFilter<4096>>),
    B16384(Box<CuckooFilter<16384>>),
    B65536(Box<CuckooFilter<65536>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! filter {
    ($filter:expr, |$inner:ident| $body:expr) => {
        match $filter {
            Filter::B1024($inner) => $body,
            Filter::B4096($inner) => $body,
            Filter::B16384($inner) => $body,
            Filter::B65536($inner) => $body,
        }
    };
}

#[magnus::wrap(class = "ProbabilisticMachines::CuckooFilter", free_immediately, size)]
struct Wrapper(RefCell<Filter>);

const fn buckets_of<const B: usize>(_: &CuckooFilter<B>) -> usize {
    B
}

const fn capacity_of<const B: usize>(_: &CuckooFilter<B>) -> usize {
    CuckooFilter::<B>::CAPACITY
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the match below at once.
#[inline(never)]
fn boxed_new<const N: usize>() -> Box<CuckooFilter<N>> {
    Box::new(CuckooFilter::new())
}

#[inline(never)]
fn boxed_decode<const N: usize>(ruby: &Ruby, frame: &[u8]) -> Result<Box<CuckooFilter<N>>, Error> {
    let mut filter = boxed_new::<N>();
    filter
        .decode_into(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(filter)
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let Whole(buckets): Whole<usize> = required_kwarg(args.keywords, "buckets")?;
        let filter = match buckets {
            1024 => Filter::B1024(boxed_new::<1024>()),
            4096 => Filter::B4096(boxed_new::<4096>()),
            16384 => Filter::B16384(boxed_new::<16384>()),
            65536 => Filter::B65536(boxed_new::<65536>()),
            _ => return Err(unsupported(ruby, "buckets", buckets, &BUCKETS)),
        };
        Ok(Self(RefCell::new(filter)))
    }

    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let Some(&exponent) = blob.get(1) else {
            return Err(codec_error(ruby, CodecError::Truncated));
        };
        let filter = match exponent {
            10 => Filter::B1024(boxed_decode::<1024>(ruby, &blob)?),
            12 => Filter::B4096(boxed_decode::<4096>(ruby, &blob)?),
            14 => Filter::B16384(boxed_decode::<16384>(ruby, &blob)?),
            16 => Filter::B65536(boxed_decode::<65536>(ruby, &blob)?),
            _ => return Err(unsupported_blob(ruby, "bucket count", &BUCKETS)),
        };
        Ok(Self(RefCell::new(filter)))
    }

    fn buckets(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| buckets_of(&**filter))
    }

    fn capacity(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| capacity_of(&**filter))
    }

    fn insert(ruby: &Ruby, rb_self: &Self, item: Bytes) -> Result<bool, Error> {
        filter!(&mut *rb_self.0.borrow_mut(), |filter| filter.insert(&item))
            .map(|()| true)
            .map_err(|_| full(ruby))
    }

    fn contains(&self, item: Bytes) -> bool {
        filter!(&*self.0.borrow(), |filter| filter.contains(&item))
    }

    fn remove(&self, item: Bytes) -> bool {
        filter!(&mut *self.0.borrow_mut(), |filter| filter.remove(&item))
    }

    fn size(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| filter.len())
    }

    fn is_empty(&self) -> bool {
        filter!(&*self.0.borrow(), |filter| filter.is_empty())
    }

    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let frame = filter!(&*rb_self.0.borrow(), |filter| filter.encoded());
        ruby.str_from_slice(&frame)
    }

    fn inspect(&self) -> String {
        format!(
            "#<ProbabilisticMachines::CuckooFilter buckets={} size={} capacity={}>",
            self.buckets(),
            self.size(),
            self.capacity()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("CuckooFilter", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("buckets", method!(Wrapper::buckets, 0))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("insert", method!(Wrapper::insert, 1))?;
    class.define_method("include?", method!(Wrapper::contains, 1))?;
    class.define_method("contains?", method!(Wrapper::contains, 1))?;
    class.define_method("remove", method!(Wrapper::remove, 1))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
