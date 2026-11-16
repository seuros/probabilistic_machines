//! `ProbabilisticMachines::CuckooFilter`: membership that can forget.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RModule, RString, Ruby, Value, function, method, prelude::*};
use matryona_machines::ruby::{Symbols, integer};
use probabilistic_machines::{CuckooFilter, Refusal, Removing};

use crate::data::bytes;
use crate::{Pinned, answer, decoded_or_refused, define_class, pin, sizes, unsupported};

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

/// The class, pinned at init.
static CLASS: Pinned = Pinned::new("probabilistic_machines");

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
fn boxed_decode<const N: usize>(frame: &[u8]) -> Result<Box<CuckooFilter<N>>, Refusal> {
    let mut filter = boxed_new::<N>();
    filter.decode_into(frame)?;
    Ok(filter)
}

/// The filter a frame holds. A bucket count this build does not carry is read
/// as the smallest one it does, which answers exactly what the core answers a
/// type of another shape: `Truncated`, `WrongStructure` or `WrongShape`.
fn decoded(frame: &[u8]) -> Result<Filter, Refusal> {
    Ok(match frame.get(1) {
        Some(12) => Filter::B4096(boxed_decode(frame)?),
        Some(14) => Filter::B16384(boxed_decode(frame)?),
        Some(16) => Filter::B65536(boxed_decode(frame)?),
        Some(_) | None => Filter::B1024(boxed_decode(frame)?),
    })
}

matryona_machines::symbols! {
    /// What an honoured removal answers.
    Removings for Removing as "a removal" {
        Cleared => "cleared",
        NotHeld => "not_held",
    }
}

/// The symbol of an honoured removal.
fn removing(ruby: &Ruby, removed: Removing) -> Value {
    Removings::symbol(ruby, removed).as_value()
}

impl Wrapper {
    /// `CuckooFilter.new(buckets)`. `RangeError` outside [`BUCKETS`].
    fn new(ruby: &Ruby, buckets: Value) -> Result<Self, Error> {
        let buckets: usize = integer(buckets, "buckets")?;
        let filter = match buckets {
            1024 => Filter::B1024(boxed_new()),
            4096 => Filter::B4096(boxed_new()),
            16384 => Filter::B16384(boxed_new()),
            65536 => Filter::B65536(boxed_new()),
            _ => return Err(unsupported(ruby, "buckets", buckets, &BUCKETS)),
        };
        Ok(Self(RefCell::new(filter)))
    }

    /// `CuckooFilter.decode(bytes)`: a filter, or the refusal symbol.
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
        let outcome = filter!(&mut *this.0.borrow_mut(), |filter| filter
            .decode_into(&bytes));
        Ok(answer(ruby, outcome, "decoded"))
    }

    fn buckets(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| buckets_of(&**filter))
    }

    fn capacity(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| capacity_of(&**filter))
    }

    fn insert(ruby: &Ruby, this: Obj<Self>, item: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let item = bytes(item, "item")?;
        let outcome = filter!(&mut *this.0.borrow_mut(), |filter| filter.insert(&item));
        Ok(answer(ruby, outcome, "inserted"))
    }

    fn insert_hash(ruby: &Ruby, this: Obj<Self>, hash: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let hash: u64 = integer(hash, "hash")?;
        let outcome = filter!(&mut *this.0.borrow_mut(), |filter| filter.insert_hash(hash));
        Ok(answer(ruby, outcome, "inserted"))
    }

    fn contains(&self, item: Value) -> Result<bool, Error> {
        let item = bytes(item, "item")?;
        Ok(filter!(&*self.0.borrow(), |filter| filter.contains(&item)))
    }

    fn contains_hash(&self, hash: Value) -> Result<bool, Error> {
        let hash = integer(hash, "hash")?;
        Ok(filter!(&*self.0.borrow(), |filter| filter.contains_hash(hash)))
    }

    fn remove(ruby: &Ruby, this: Obj<Self>, item: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let item = bytes(item, "item")?;
        let removed = filter!(&mut *this.0.borrow_mut(), |filter| filter.remove(&item));
        Ok(removing(ruby, removed))
    }

    fn remove_hash(ruby: &Ruby, this: Obj<Self>, hash: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let hash: u64 = integer(hash, "hash")?;
        let removed = filter!(&mut *this.0.borrow_mut(), |filter| filter.remove_hash(hash));
        Ok(removing(ruby, removed))
    }

    fn size(&self) -> usize {
        filter!(&*self.0.borrow(), |filter| filter.len())
    }

    fn is_empty(&self) -> bool {
        filter!(&*self.0.borrow(), |filter| filter.is_empty())
    }

    fn encoded(ruby: &Ruby, this: &Self) -> RString {
        let frame = filter!(&*this.0.borrow(), |filter| filter.encoded());
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
    let class = define_class(ruby, module, "CuckooFilter")?;
    class.const_set("BUCKETS", sizes(ruby, &BUCKETS))?;
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("decode_into", method!(Wrapper::decode_into, 1))?;
    class.define_method("buckets", method!(Wrapper::buckets, 0))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("insert", method!(Wrapper::insert, 1))?;
    class.define_method("insert_hash", method!(Wrapper::insert_hash, 1))?;
    class.define_method("contains?", method!(Wrapper::contains, 1))?;
    class.define_method("contains_hash?", method!(Wrapper::contains_hash, 1))?;
    class.define_method("remove", method!(Wrapper::remove, 1))?;
    class.define_method("remove_hash", method!(Wrapper::remove_hash, 1))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    pin::<Wrapper>(ruby, class, &CLASS)
}
