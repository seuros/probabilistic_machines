//! The `Data` values the sketches hand back, `Hitter` and `Centroid`.
//!
//! Both classes are defined in Ruby (`lib/probabilistic_machines.rb`) so their
//! `initialize` runs for every instance, hand-built or returned. It calls the
//! private validation hooks below, which hold a value to the core's own
//! constructor: `Hitter::new` and `Centroid::new`.

use std::sync::OnceLock;

use magnus::value::Opaque;
use magnus::{Error, RArray, RClass, RModule, RString, Ruby, Value, function, prelude::*};
use probabilistic_machines::{Centroid, Hitter};

use crate::Whole;

/// `Hitter` and `Centroid`, resolved once at init: `lib/probabilistic_machines.rb`
/// defines both before the extension loads.
static CLASSES: OnceLock<[Opaque<RClass>; 2]> = OnceLock::new();

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let hitter: RClass = module.const_get("Hitter")?;
    let centroid: RClass = module.const_get("Centroid")?;
    // Held in a static, so pinned: `GC.compact` must not move them.
    ruby.gc_register_mark_object(hitter);
    ruby.gc_register_mark_object(centroid);
    // A second load keeps the first classes: they are the same constants.
    let _ = CLASSES.get_or_init(|| [hitter.into(), centroid.into()]);
    module.define_singleton_method("__hitter", function!(checked_hitter, 3))?;
    module.define_singleton_method("__centroid", function!(checked_centroid, 2))?;
    Ok(())
}

/// A key as the table hashes it: the bytes of a String, which must be UTF-8.
/// `TypeError` for anything but a String, `RangeError` for bytes that are not
/// UTF-8 (whatever encoding the String is tagged with).
pub fn utf8_key(ruby: &Ruby, key: Value) -> Result<String, Error> {
    let Some(string) = RString::from_value(key) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            format!("a key is a String, not {}", key.class()),
        ));
    };
    String::from_utf8(string.to_bytes().to_vec()).map_err(|_| {
        Error::new(
            ruby.exception_range_error(),
            "a key is UTF-8 text; these bytes are not UTF-8",
        )
    })
}

/// `ProbabilisticMachines.__hitter(key, count, overestimate)`: the members of a
/// valid `Hitter`, the key a frozen UTF-8 copy so the value cannot change under
/// the caller. `TypeError` for a key that is not a String or a count that is
/// not an Integer; `RangeError` for a key that is not UTF-8, where
/// `Hitter::new` answers `None` (an overestimate not below the count) or a
/// count is outside `0..2**64`.
fn checked_hitter(
    ruby: &Ruby,
    key: Value,
    Whole(count): Whole<u64>,
    Whole(overestimate): Whole<u64>,
) -> Result<RArray, Error> {
    let key = utf8_key(ruby, key)?;
    if Hitter::new((), count, overestimate).is_none() {
        return Err(Error::new(
            ruby.exception_range_error(),
            format!("overestimate {overestimate} is not below count {count}"),
        ));
    }
    let key = ruby.str_new(&key);
    key.freeze();
    Ok(ruby.ary_new_from_values(&[
        key.as_value(),
        ruby.into_value(count),
        ruby.into_value(overestimate),
    ]))
}

/// `ProbabilisticMachines.__centroid(mean, weight)`: the members of a valid
/// `Centroid`, as Floats. `TypeError` for a non-number; `RangeError` where
/// `Centroid::new` answers `None` (a mean that is not finite, a weight that is
/// not finite and positive).
fn checked_centroid(ruby: &Ruby, mean: f64, weight: f64) -> Result<(f64, f64), Error> {
    let Some(centroid) = Centroid::new(mean, weight) else {
        return Err(Error::new(
            ruby.exception_range_error(),
            format!(
                "a centroid needs a finite mean and a finite, positive weight: {mean}, {weight}"
            ),
        ));
    };
    Ok((centroid.mean(), centroid.weight()))
}

fn class(ruby: &Ruby, index: usize) -> Result<RClass, Error> {
    CLASSES
        .get()
        .and_then(|classes| classes.get(index))
        .map(|class| ruby.get_inner(*class))
        .ok_or_else(|| {
            Error::new(
                ruby.exception_load_error(),
                "the probabilistic_machines extension is not initialised",
            )
        })
}

/// The `Hitter` class resolved at init.
pub fn hitter_class(ruby: &Ruby) -> Result<RClass, Error> {
    class(ruby, 0)
}

/// The `Centroid` class resolved at init.
pub fn centroid_class(ruby: &Ruby) -> Result<RClass, Error> {
    class(ruby, 1)
}

/// A `Hitter` for a key the table tracks.
pub fn hitter(class: RClass, key: &str, entry: &Hitter<u64>) -> Result<Value, Error> {
    class.funcall("new", (key, entry.count(), entry.overestimate()))
}

/// A `Centroid` for one cluster of a digest.
pub fn centroid(class: RClass, cluster: &Centroid) -> Result<Value, Error> {
    class.funcall("new", (cluster.mean(), cluster.weight()))
}
