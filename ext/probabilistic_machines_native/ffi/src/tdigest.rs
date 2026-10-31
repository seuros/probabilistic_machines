//! `ProbabilisticMachines::TDigest`: quantiles of a stream, in clusters.

use std::cell::RefCell;

use bytes::Bytes;
use magnus::scan_args::scan_args;
use magnus::typed_data::Obj;
use magnus::{Error, RArray, RHash, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::TDigest;

use crate::data::centroid;
use crate::{codec_error, optional_kwarg, required_kwarg};

#[magnus::wrap(class = "ProbabilisticMachines::TDigest", free_immediately, size)]
struct Wrapper(RefCell<TDigest>);

impl Wrapper {
    fn new(args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let compression: u16 = required_kwarg(args.keywords, "compression")?;
        Ok(Self(RefCell::new(TDigest::new(compression))))
    }

    fn load(ruby: &Ruby, blob: Bytes) -> Result<Self, Error> {
        let digest = TDigest::decode(&blob).map_err(|error| codec_error(ruby, error))?;
        Ok(Self(RefCell::new(digest)))
    }

    fn compression(&self) -> u16 {
        self.0.borrow().compression()
    }

    fn add(rb_self: &Self, args: &[Value]) -> Result<(), Error> {
        let args = scan_args::<(f64,), (), (), (), RHash, ()>(args)?;
        let (value,) = args.required;
        let weight: f64 = optional_kwarg(args.keywords, "weight")?.unwrap_or(1.0);
        rb_self.0.borrow_mut().add_weighted(value, weight);
        Ok(())
    }

    fn quantile(&self, q: f64) -> f64 {
        self.0.borrow().quantile(q)
    }

    fn cdf(&self, value: f64) -> f64 {
        self.0.borrow().cdf(value)
    }

    fn count(&self) -> f64 {
        self.0.borrow().count()
    }

    fn minimum(&self) -> f64 {
        self.0.borrow().minimum()
    }

    fn maximum(&self) -> f64 {
        self.0.borrow().maximum()
    }

    fn size(&self) -> usize {
        self.0.borrow().len()
    }

    fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    fn centroids(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let digest = rb_self.0.borrow();
        let clusters = ruby.ary_new_capa(digest.len());
        for cluster in digest.centroids() {
            clusters.push(centroid(ruby, cluster.mean, cluster.weight)?)?;
        }
        Ok(clusters)
    }

    fn merge(rb_self: Obj<Self>, other: &Self) -> Obj<Self> {
        let source = other.0.borrow().clone();
        rb_self.0.borrow_mut().merge(&source);
        rb_self
    }

    fn dump(ruby: &Ruby, rb_self: &Self) -> Result<RString, Error> {
        let digest = rb_self.0.borrow();
        let mut frame = vec![0_u8; digest.encoded_len()];
        let length = digest
            .encode(&mut frame)
            .map_err(|error| codec_error(ruby, error))?;
        frame.truncate(length);
        Ok(ruby.str_from_slice(&frame))
    }

    fn inspect(&self) -> String {
        let digest = self.0.borrow();
        format!(
            "#<ProbabilisticMachines::TDigest compression={} count={} centroids={}>",
            digest.compression(),
            digest.count(),
            digest.len()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("TDigest", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_singleton_method("load", function!(Wrapper::load, 1))?;
    class.define_method("compression", method!(Wrapper::compression, 0))?;
    class.define_method("add", method!(Wrapper::add, -1))?;
    class.define_method("quantile", method!(Wrapper::quantile, 1))?;
    class.define_method("cdf", method!(Wrapper::cdf, 1))?;
    class.define_method("count", method!(Wrapper::count, 0))?;
    class.define_method("minimum", method!(Wrapper::minimum, 0))?;
    class.define_method("maximum", method!(Wrapper::maximum, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("centroids", method!(Wrapper::centroids, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("dump", method!(Wrapper::dump, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
