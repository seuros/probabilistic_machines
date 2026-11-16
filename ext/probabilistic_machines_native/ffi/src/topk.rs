//! `ProbabilisticMachines::HeavyHitters`: which keys dominate a stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use magnus::typed_data::Obj;
use magnus::{Error, RArray, RModule, RString, Ruby, Value, function, method, prelude::*};
use matryona_machines::ruby::integer;
use probabilistic_machines::{HeavyHitters, Hitter, Refusal, hash64};

use crate::data::{bytes, hitter, hitter_class, utf8_key};
use crate::{Pinned, answer, decoded_or_refused, define_class, partner, pin, sizes, unsupported};

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

type Names = HashMap<u64, String>;

/// The table counts 64-bit key hashes and `names` holds the string each
/// tracked hash came from. A key the table evicted leaves its name behind, so
/// `names` is pruned back to the tracked hashes once it holds more than twice
/// the counters.
/// The class, pinned at init.
static CLASS: Pinned = Pinned::new("probabilistic_machines");

#[magnus::wrap(class = "ProbabilisticMachines::HeavyHitters", free_immediately, size)]
struct Wrapper {
    table: RefCell<Table>,
    names: RefCell<Names>,
}

const fn counters_of<const K: usize>(_: &HeavyHitters<u64, K>) -> usize {
    K
}

/// Each shape is constructed behind its own call frame. Inlined, a debug build
/// sizes one frame for every arm of the dispatch match at once.
#[inline(never)]
fn boxed_new<const K: usize>() -> Box<HeavyHitters<u64, K>> {
    Box::new(HeavyHitters::new())
}

/// A `usize` as the key table's 64-bit little-endian length field.
fn length_field(length: usize) -> [u8; 8] {
    let mut field = [0_u8; 8];
    for (slot, byte) in field.iter_mut().zip(length.to_le_bytes()) {
        *slot = byte;
    }
    field
}

/// Reads the name records that follow the core frame back into the
/// hash-to-name map. A record cut short, a length this platform cannot hold
/// or a key that is not UTF-8 is `Malformed`.
fn key_table(mut records: &[u8]) -> Result<Names, Refusal> {
    let mut names = HashMap::new();
    while let Some((field, tail)) = records.split_first_chunk::<8>() {
        let Ok(length) = usize::try_from(u64::from_le_bytes(*field)) else {
            return Err(Refusal::Malformed);
        };
        let Some((name, tail)) = tail.split_at_checked(length) else {
            return Err(Refusal::Malformed);
        };
        let Ok(name) = str::from_utf8(name) else {
            return Err(Refusal::Malformed);
        };
        names.insert(hash64(name.as_bytes()), name.to_owned());
        records = tail;
    }
    if records.is_empty() {
        Ok(names)
    } else {
        Err(Refusal::Malformed)
    }
}

/// A table and its names from a blob written by `encoded`, refusing a key
/// table that is malformed or does not name every tracked key.
#[inline(never)]
fn decode_table<const K: usize>(
    blob: &[u8],
) -> Result<(Box<HeavyHitters<u64, K>>, Names), Refusal> {
    let mut table = boxed_new::<K>();
    table.decode_into(blob)?;
    let names = key_table(&blob[HeavyHitters::<u64, K>::ENCODED_LEN..])?;
    if table
        .hitters()
        .any(|entry| !names.contains_key(entry.key()))
    {
        return Err(Refusal::Malformed);
    }
    Ok((table, names))
}

/// The table a blob holds. A counter count this build does not carry is read
/// as the smallest one it does, which answers exactly what the core answers a
/// type of another shape: `Truncated`, `WrongStructure` or `WrongShape`.
fn decoded(blob: &[u8]) -> Result<(Table, Names), Refusal> {
    Ok(match blob.get(1..3) {
        Some([64, 0]) => decode_table(blob).map(|(table, names)| (Table::K64(table), names))?,
        Some([0, 1]) => decode_table(blob).map(|(table, names)| (Table::K256(table), names))?,
        Some(_) | None => decode_table(blob).map(|(table, names)| (Table::K16(table), names))?,
    })
}

impl Wrapper {
    /// `HeavyHitters.new(counters)`. `RangeError` outside [`COUNTERS`].
    fn new(ruby: &Ruby, counters: Value) -> Result<Self, Error> {
        let counters: usize = integer(counters, "counters")?;
        let table = match counters {
            16 => Table::K16(boxed_new()),
            64 => Table::K64(boxed_new()),
            256 => Table::K256(boxed_new()),
            _ => return Err(unsupported(ruby, "counters", counters, &COUNTERS)),
        };
        Ok(Self {
            table: RefCell::new(table),
            names: RefCell::new(HashMap::new()),
        })
    }

    /// `HeavyHitters.decode(bytes)`: a table, or the refusal symbol.
    fn decode(ruby: &Ruby, blob: Value) -> Result<Value, Error> {
        let blob = bytes(blob, "bytes")?;
        let held = decoded(&blob).map(|(table, names)| Self {
            table: RefCell::new(table),
            names: RefCell::new(names),
        });
        Ok(decoded_or_refused(ruby, held))
    }

    /// `:decoded`, or the refusal symbol with the table and names untouched.
    fn decode_into(ruby: &Ruby, this: Obj<Self>, blob: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let blob = bytes(blob, "bytes")?;
        let read = match &*this.table.borrow() {
            Table::K16(_) => decode_table(&blob).map(|(table, names)| (Table::K16(table), names)),
            Table::K64(_) => decode_table(&blob).map(|(table, names)| (Table::K64(table), names)),
            Table::K256(_) => decode_table(&blob).map(|(table, names)| (Table::K256(table), names)),
        };
        let outcome = read.map(|(table, names)| {
            *this.table.borrow_mut() = table;
            *this.names.borrow_mut() = names;
        });
        Ok(answer(ruby, outcome, "decoded"))
    }

    fn counters(&self) -> usize {
        table!(&*self.table.borrow(), |table| counters_of(&**table))
    }

    fn add(ruby: &Ruby, this: Obj<Self>, key: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let key = utf8_key(key)?;
        Ok(Self::counted(ruby, this, key, 1))
    }

    fn add_count(ruby: &Ruby, this: Obj<Self>, key: Value, count: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let key = utf8_key(key)?;
        let count = integer(count, "count")?;
        Ok(Self::counted(ruby, this, key, count))
    }

    /// Records the name only once the core has honoured the count.
    fn counted(ruby: &Ruby, this: Obj<Self>, key: String, count: u64) -> Value {
        let hash = hash64(key.as_bytes());
        let outcome = table!(&mut *this.table.borrow_mut(), |table| table
            .add_count(hash, count));
        if outcome.is_ok() {
            this.names.borrow_mut().insert(hash, key);
            this.prune();
        }
        answer(ruby, outcome, "added")
    }

    fn count(this: &Self, key: Value) -> Result<u64, Error> {
        let hash = hash64(utf8_key(key)?.as_bytes());
        Ok(table!(&*this.table.borrow(), |table| table.count(&hash)))
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

    /// The most frequent tracked key, `nil` when nothing is tracked.
    fn top(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let top = table!(&*this.table.borrow(), |table| table.top().copied());
        this.built(ruby, top.as_slice())?.entry(0)
    }

    /// Every tracked key, most frequent first.
    fn ranked(ruby: &Ruby, this: &Self) -> Result<RArray, Error> {
        let ranked = table!(&*this.table.borrow(), |table| table.ranked());
        this.built(ruby, &ranked)
    }

    /// Every tracked key, in the table's own order.
    fn hitters(ruby: &Ruby, this: &Self) -> Result<RArray, Error> {
        let hitters: Vec<Hitter<u64>> = table!(&*this.table.borrow(), |table| table
            .hitters()
            .copied()
            .collect());
        this.built(ruby, &hitters)
    }

    /// `:merged`, `:overflow`, or `:wrong_shape` for a table of another
    /// counter count (in Rust that merge does not compile).
    fn merge(ruby: &Ruby, this: Obj<Self>, other: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let other: Obj<Self> = partner(ruby, &CLASS, other)?;
        let source = other.table.borrow().clone();
        let names = other.names.borrow().clone();
        let merged = match (&mut *this.table.borrow_mut(), &source) {
            (Table::K16(target), Table::K16(source)) => target.merge(source),
            (Table::K64(target), Table::K64(source)) => target.merge(source),
            (Table::K256(target), Table::K256(source)) => target.merge(source),
            (
                Table::K16(_) | Table::K64(_) | Table::K256(_),
                Table::K16(_) | Table::K64(_) | Table::K256(_),
            ) => Err(Refusal::WrongShape),
        };
        if merged.is_ok() {
            this.names.borrow_mut().extend(names);
            this.prune();
        }
        Ok(answer(ruby, merged, "merged"))
    }

    /// The core frame followed by the names of the keys it tracks.
    ///
    /// ```text
    /// core frame   HeavyHitters::<u64, counters>::ENCODED_LEN bytes, tag 0xB1
    /// name record  u64 little-endian byte length, then that many UTF-8 bytes
    /// ```
    ///
    /// One record per tracked key, in the core frame's order, filling the rest
    /// of the blob. The table counts key hashes, so [`decode`](Self::decode)
    /// rehashes each name to rebuild the hash-to-name map.
    fn encoded(ruby: &Ruby, this: &Self) -> RString {
        let (frame, keys) = table!(&*this.table.borrow(), |table| (
            table.encoded(),
            table
                .hitters()
                .map(|entry| *entry.key())
                .collect::<Vec<u64>>()
        ));
        let names = this.names.borrow();
        let mut blob = Vec::from(frame);
        for name in keys.iter().filter_map(|key| names.get(key)) {
            blob.extend_from_slice(&length_field(name.len()));
            blob.extend_from_slice(name.as_bytes());
        }
        ruby.str_from_slice(&blob)
    }

    fn inspect(&self) -> String {
        format!(
            "#<ProbabilisticMachines::HeavyHitters counters={} tracked={} observed={}>",
            self.counters(),
            self.size(),
            self.observed()
        )
    }

    /// `Hitter` values for `entries`, with the name each hash came from. Every
    /// tracked hash has one: names are recorded when a count is honoured and
    /// pruned only to the tracked set, so no entry is ever left out.
    fn built(&self, ruby: &Ruby, entries: &[Hitter<u64>]) -> Result<RArray, Error> {
        let class = hitter_class(ruby)?;
        let names = self.names.borrow().clone();
        let array = ruby.ary_new_capa(entries.len());
        for entry in entries {
            let Some(name) = names.get(entry.key()) else {
                // No answer names a key it cannot spell: a hash without its
                // name is left out rather than reported under another one.
                debug_assert!(false, "every tracked hash keeps its name");
                continue;
            };
            array.push(hitter(class, name, entry)?)?;
        }
        Ok(array)
    }

    fn prune(&self) {
        let mut names = self.names.borrow_mut();
        if names.len() <= 2 * self.counters() {
            return;
        }
        let tracked: HashSet<u64> = table!(&*self.table.borrow(), |table| table
            .hitters()
            .map(|entry| *entry.key())
            .collect());
        names.retain(|hash, _| tracked.contains(hash));
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = define_class(ruby, module, "HeavyHitters")?;
    class.const_set("COUNTERS", sizes(ruby, &COUNTERS))?;
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("decode_into", method!(Wrapper::decode_into, 1))?;
    class.define_method("counters", method!(Wrapper::counters, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("add_count", method!(Wrapper::add_count, 2))?;
    class.define_method("count", method!(Wrapper::count, 1))?;
    class.define_method("observed", method!(Wrapper::observed, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("top", method!(Wrapper::top, 0))?;
    class.define_method("ranked", method!(Wrapper::ranked, 0))?;
    class.define_method("hitters", method!(Wrapper::hitters, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    pin::<Wrapper>(ruby, class, &CLASS)
}
