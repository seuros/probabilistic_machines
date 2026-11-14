//! The `Data` values the sketches hand back, `Hitter` and `Centroid`.
//!
//! Both classes are defined in Ruby (`lib/probabilistic_machines.rb`) so their
//! `initialize` runs for every instance, hand-built or returned. It calls the
//! private validation hooks below, which hold a value to the core's own
//! constructor: `Hitter::new` and `Centroid::new`.

use magnus::{Error, RArray, RClass, RModule, Ruby, Value, function, prelude::*};
use matryona_machines::ruby::{Class, Registry, finite, integer, utf8};
use probabilistic_machines::{Centroid, Hitter};

/// `Hitter` and `Centroid`, resolved once at init and pinned:
/// `lib/probabilistic_machines.rb` defines both before the extension loads.
struct Classes {
    hitter: Class,
    centroid: Class,
}

static CLASSES: Registry<Classes> = Registry::new("probabilistic_machines");

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let classes = Classes {
        hitter: Class::resolve(ruby, module, "Hitter")?,
        centroid: Class::resolve(ruby, module, "Centroid")?,
    };
    CLASSES.set(ruby, classes)?;
    module.define_singleton_method("__hitter", function!(checked_hitter, 3))?;
    module.define_singleton_method("__centroid", function!(checked_centroid, 2))?;
    Ok(())
}

/// `ProbabilisticMachines.__hitter(key, count, overestimate)`: the members of a
/// valid `Hitter`, the key a frozen UTF-8 copy so the value cannot change under
/// the caller. `TypeError` for a key that is not exactly a String or a count
/// that is not an Integer; `RangeError` for a key that is not UTF-8 text, where
/// `Hitter::new` answers `None` (an overestimate not below the count) or a
/// count is outside `0..2**64`.
fn checked_hitter(
    ruby: &Ruby,
    key: Value,
    count: Value,
    overestimate: Value,
) -> Result<RArray, Error> {
    let count: u64 = integer(count, "count")?;
    let overestimate: u64 = integer(overestimate, "overestimate")?;
    let key = utf8(key, "key")?;
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
/// `Centroid`, as Floats. `TypeError` for anything but a Float or an Integer;
/// `RangeError` for a mean or weight that is not finite, and where
/// `Centroid::new` answers `None` (a weight that is not positive).
fn checked_centroid(ruby: &Ruby, mean: Value, weight: Value) -> Result<(f64, f64), Error> {
    let (mean, weight) = (finite(mean, "mean")?, finite(weight, "weight")?);
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

/// The `Hitter` class resolved at init.
pub fn hitter_class(ruby: &Ruby) -> Result<RClass, Error> {
    Ok(CLASSES.get(ruby)?.hitter.get(ruby))
}

/// The `Centroid` class resolved at init.
pub fn centroid_class(ruby: &Ruby) -> Result<RClass, Error> {
    Ok(CLASSES.get(ruby)?.centroid.get(ruby))
}

/// A `Hitter` for a key the table tracks.
pub fn hitter(class: RClass, key: &str, entry: &Hitter<u64>) -> Result<Value, Error> {
    class.funcall("new", (key, entry.count(), entry.overestimate()))
}

/// A `Centroid` for one cluster of a digest.
pub fn centroid(class: RClass, cluster: &Centroid) -> Result<Value, Error> {
    class.funcall("new", (cluster.mean(), cluster.weight()))
}
