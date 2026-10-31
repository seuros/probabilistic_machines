//! `ProbabilisticMachines::HeavyHitters`: which keys dominate a stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RArray, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{CodecError, HeavyHitters, hash64};

use crate::data::hitter;
use crate::{codec_error, mismatch, optional_kwarg, required_kwarg, unsupported, unsupported_blob};

/// Counter counts [`Table`] is monomorphised for.
const COUNTERS: [usize; 3] = [16, 64, 256];

#[derive(Clone)]
enum Table {
    K16(Box<HeavyHitters<u64, 16>>),
    K64(Box<HeavyHitters<u64, 64>>),
    K256(Box<HeavyHitters<u64, 256>>),
}

/// Runs `body` against whichever variant is held.
macro_rules! table {
    ($table:expr, |$inner:ident| $body:expr) => {
        match $table {
            Table::K16($inner) => $body,
            Table::K64($inner) => $body,
            Table::K256($inner) => $body,
        }
    };
}

/// Space-Saving needs `Copy + Eq` keys, so the table counts 64-bit key hashes
/// and `names` holds the string each tracked hash came from. A key the table
/// evicted leaves its name behind, so `names` is pruned back to the tracked
/// hashes once it holds more than twice the counters.
#[magnus::wrap(class = "ProbabilisticMachines::HeavyHitters", free_immediately, size)]
struct Wrapper {
    table: RefCell<Table>,
    names: RefCell<HashMap<u64, String>>,
}

const fn counters_of<const K: usize>(_: &HeavyHitters<u64, K>) -> usize {
    K
}

const fn frame_len_of<const K: usize>(_: &HeavyHitters<u64, K>) -> usize {
    HeavyHitters::<u64, K>::ENCODED_LEN
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the dispatch match at once.
#[inline(never)]
fn boxed_new<const K: usize>() -> Box<HeavyHitters<u64, K>> {
    Box::new(HeavyHitters::new())
}

#[inline(never)]
fn boxed_decode<const K: usize>(
    ruby: &Ruby,
    frame: &[u8],
) -> Result<Box<HeavyHitters<u64, K>>, Error> {
    let mut table = boxed_new::<K>();
    table
        .decode_into(frame)
        .map_err(|error| codec_error(ruby, error))?;
    Ok(table)
}

/// A `usize` as the key table's 64-bit little-endian length field.
fn length_field(length: usize) -> [u8; 8] {
    let mut field = [0_u8; 8];
    for (slot, byte) in field.iter_mut().zip(length.to_le_bytes()) {
        *slot = byte;
    }
    field
}

fn bad_key_table(ruby: &Ruby, reason: &str) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("the blob's key table {reason}"),
    )
}

/// Reads the name records that follow the core frame back into the
/// hash-to-name map.
fn key_table(ruby: &Ruby, mut records: &[u8]) -> Result<HashMap<u64, String>, Error> {
    let mut names = HashMap::new();
    while !records.is_empty() {
        let Some((field, tail)) = records.split_first_chunk::<8>() else {
            return Err(bad_key_table(ruby, "ends inside a record header"));
        };
        let Ok(length) = usize::try_from(u64::from_le_bytes(*field)) else {
            return Err(bad_key_table(ruby, "names a key this platform cannot hold"));
        };
        let Some((name, tail)) = tail.split_at_checked(length) else {
            return Err(bad_key_table(ruby, "ends inside a key"));
        };
        let Ok(name) = str::from_utf8(name) else {
            return Err(bad_key_table(ruby, "holds a key that is not UTF-8"));
        };
        names.insert(hash64(name.as_bytes()), name.to_owned());
        records = tail;
    }
    Ok(names)
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let counters: usize = required_kwarg(args.keywords, "k")?;
        let table = match counters {
            16 => Table::K16(boxed_new::<16>()),
            64 => Table::K64(boxed_new::<64>()),
            256 => Table::K256(boxed_new::<256>()),
            _ => return Err(unsupported(ruby, "k", counters, &COUNTERS)),
        };
        Ok(Self {
            table: RefCell::new(table),
            names: RefCell::new(HashMap::new()),
        })
    }

    /// Reads a blob written by [`dump`](Self::dump).
    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let Some(shape) = blob.get(1..3) else {
            return Err(codec_error(ruby, CodecError::Truncated));
        };
        let counters = u16::from_le_bytes([shape[0], shape[1]]);
        let table = match counters {
            16 => Table::K16(boxed_decode::<16>(ruby, &blob)?),
            64 => Table::K64(boxed_decode::<64>(ruby, &blob)?),
            256 => Table::K256(boxed_decode::<256>(ruby, &blob)?),
            _ => return Err(unsupported_blob(ruby, "counter count", &COUNTERS)),
        };

        let frame_len = table!(&table, |inner| frame_len_of(&**inner));
        let names = key_table(ruby, &blob[frame_len..])?;
        let unnamed = table!(&table, |inner| inner
            .hitters()
            .any(|entry| !names.contains_key(&entry.key)));
        if unnamed {
            return Err(bad_key_table(ruby, "does not name every tracked key"));
        }

        Ok(Self {
            table: RefCell::new(table),
            names: RefCell::new(names),
        })
    }

    fn counters(&self) -> usize {
        table!(&*self.table.borrow(), |table| counters_of(&**table))
    }

    fn add(rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(String,), (), (), (), RHash, ()>(args)?;
        let (key,) = args.required;
        let count: u64 = optional_kwarg(args.keywords, "count")?.unwrap_or(1);
        let hash = hash64(key.as_bytes());
        table!(&mut *rb_self.table.borrow_mut(), |table| table
            .add_count(hash, count));
        rb_self.names.borrow_mut().insert(hash, key);
        rb_self.prune();
        Ok(())
    }

    fn count(&self, key: String) -> u64 {
        let hash = hash64(key.as_bytes());
        table!(&*self.table.borrow(), |table| table.count(&hash))
    }

    fn observed(&self) -> u64 {
        table!(&*self.table.borrow(), |table| table.observed())
    }

    fn size(&self) -> usize {
        table!(&*self.table.borrow(), |table| table.len())
    }

    fn is_empty(&self) -> bool {
        table!(&*self.table.borrow(), |table| table.is_empty())
    }

    fn ranked(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        rb_self.take(ruby, usize::MAX)
    }

    fn top(ruby: &Ruby, rb_self: &Self, count: usize) -> Result<RArray, Error> {
        rb_self.take(ruby, count)
    }

    fn merge(ruby: &Ruby, rb_self: Obj<Self>, other: &Self) -> Result<Obj<Self>, Error> {
        let source = other.table.borrow().clone();
        let names = other.names.borrow().clone();
        match (&mut *rb_self.table.borrow_mut(), &source) {
            (Table::K16(target), Table::K16(source)) => target.merge(source),
            (Table::K64(target), Table::K64(source)) => target.merge(source),
            (Table::K256(target), Table::K256(source)) => target.merge(source),
            _ => return Err(mismatch(ruby, "table")),
        }
        rb_self.names.borrow_mut().extend(names);
        rb_self.prune();
        Ok(rb_self)
    }

    /// The core frame followed by the names of the keys it tracks.
    ///
    /// ```text
    /// core frame   HeavyHitters::<u64, k>::ENCODED_LEN bytes, tag 0xB1
    /// name record  u64 little-endian byte length, then that many UTF-8 bytes
    /// ```
    ///
    /// One record per tracked key, in the core frame's order, filling the rest
    /// of the blob. The table counts key hashes, so [`load`](Self::load)
    /// rehashes each name to rebuild the hash-to-name map `top` and `ranked`
    /// read.
    fn dump(ruby: &Ruby, rb_self: &Self) -> RString {
        let (frame, keys) = table!(&*rb_self.table.borrow(), |table| (
            table.encoded(),
            table.hitters().map(|entry| entry.key).collect::<Vec<u64>>()
        ));
        let names = rb_self.names.borrow();
        let mut blob = Vec::from(frame);
        for name in keys.iter().filter_map(|key| names.get(key)) {
            blob.extend_from_slice(&length_field(name.len()));
            blob.extend_from_slice(name.as_bytes());
        }
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<ProbabilisticMachines::HeavyHitters k={} tracked={} observed={}>",
            self.counters(),
            self.size(),
            self.observed()
        )
    }

    /// The `count` most frequent tracked keys, most frequent first.
    fn take(&self, ruby: &Ruby, count: usize) -> Result<RArray, Error> {
        let ranked = table!(&*self.table.borrow(), |table| table.ranked());
        let names = self.names.borrow();
        let hitters = ruby.ary_new_capa(ranked.len().min(count));
        for entry in ranked.into_iter().take(count) {
            let key = names.get(&entry.key).ok_or_else(|| {
                Error::new(
                    ruby.exception_runtime_error(),
                    "a tracked key lost its name",
                )
            })?;
            hitters.push(hitter(
                ruby,
                ruby.str_new(key).as_value(),
                entry.count,
                entry.overestimate,
            )?)?;
        }
        Ok(hitters)
    }

    fn prune(&self) {
        let mut names = self.names.borrow_mut();
        if names.len() <= 2 * self.counters() {
            return;
        }
        let tracked: HashSet<u64> = table!(&*self.table.borrow(), |table| table
            .hitters()
            .map(|entry| entry.key)
            .collect());
        names.retain(|hash, _| tracked.contains(hash));
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("HeavyHitters", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("k", method!(Wrapper::counters, 0))?;
    class.define_method("add", method!(Wrapper::add, -1))?;
    class.define_method("count", method!(Wrapper::count, 1))?;
    class.define_method("observed", method!(Wrapper::observed, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("top", method!(Wrapper::top, 1))?;
    class.define_method("ranked", method!(Wrapper::ranked, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
