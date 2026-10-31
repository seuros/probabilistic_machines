//! `ProbabilisticMachines::HeavyHitters`: which keys dominate a stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RArray, RHash, RModule, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{HeavyHitters, hash64};

use crate::data::hitter;
use crate::{mismatch, optional_kwarg, required_kwarg, unsupported};

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

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let counters: usize = required_kwarg(args.keywords, "k")?;
        let table = match counters {
            16 => Table::K16(Box::new(HeavyHitters::new())),
            64 => Table::K64(Box::new(HeavyHitters::new())),
            256 => Table::K256(Box::new(HeavyHitters::new())),
            _ => return Err(unsupported(ruby, "k", counters, &COUNTERS)),
        };
        Ok(Self {
            table: RefCell::new(table),
            names: RefCell::new(HashMap::new()),
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
    class.define_method("k", method!(Wrapper::counters, 0))?;
    class.define_method("add", method!(Wrapper::add, -1))?;
    class.define_method("count", method!(Wrapper::count, 1))?;
    class.define_method("observed", method!(Wrapper::observed, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("top", method!(Wrapper::top, 1))?;
    class.define_method("ranked", method!(Wrapper::ranked, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
