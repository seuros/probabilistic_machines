//! The frozen `Data` objects the sketches hand back.

use std::sync::OnceLock;

use magnus::value::Opaque;
use magnus::{Error, RClass, RModule, RStruct, Ruby, Value, prelude::*};

use crate::{defined_twice, uninitialised};

static HITTER: OnceLock<Opaque<RClass>> = OnceLock::new();
static CENTROID: OnceLock<Opaque<RClass>> = OnceLock::new();

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let hitter = ruby.define_data(None, ("key", "count", "overestimate"))?;
    module.const_set("Hitter", hitter)?;
    HITTER
        .set(hitter.into())
        .map_err(|_| defined_twice(ruby, "Hitter"))?;

    let centroid = ruby.define_data(None, ("mean", "weight"))?;
    module.const_set("Centroid", centroid)?;
    CENTROID
        .set(centroid.into())
        .map_err(|_| defined_twice(ruby, "Centroid"))
}

/// A tracked key, its counter, and how much of that counter it inherited from
/// keys evicted before it.
pub fn hitter(ruby: &Ruby, key: Value, count: u64, overestimate: u64) -> Result<Value, Error> {
    new_data(
        ruby,
        &HITTER,
        [key, ruby.into_value(count), ruby.into_value(overestimate)],
    )
}

/// One cluster of a digest: the mean of the values that collapsed into it.
pub fn centroid(ruby: &Ruby, mean: f64, weight: f64) -> Result<Value, Error> {
    new_data(
        ruby,
        &CENTROID,
        [ruby.into_value(mean), ruby.into_value(weight)],
    )
}

/// Instantiate one of our `Data` classes from its member values, in
/// declaration order.
///
/// `Data#initialize` takes keywords only, so `new_instance` would need a
/// temporary `Hash` per call plus a temporary `String` per key while magnus
/// interns it. Allocating, filling the members by position and freezing is
/// everything `Data#initialize` does, and both classes are defined here with
/// no `initialize` override to skip.
fn new_data<const N: usize>(
    ruby: &Ruby,
    slot: &OnceLock<Opaque<RClass>>,
    members: [Value; N],
) -> Result<Value, Error> {
    let class = slot
        .get()
        .map(|class| ruby.get_inner(*class))
        .ok_or_else(|| uninitialised(ruby))?;
    let instance = RStruct::try_convert(class.obj_alloc()?)?;
    for (index, value) in members.into_iter().enumerate() {
        instance.aset(index, value)?;
    }
    instance.freeze();
    Ok(instance.as_value())
}
