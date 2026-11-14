//! Private native entry point.
use magnus::{Error, Ruby};

use crate::{countmin, cuckoo, data, hyperloglog, tdigest, topk};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("ProbabilisticMachines")?;
    data::define(ruby, module)?;
    hyperloglog::define(ruby, module)?;
    countmin::define(ruby, module)?;
    tdigest::define(ruby, module)?;
    cuckoo::define(ruby, module)?;
    topk::define(ruby, module)
}
