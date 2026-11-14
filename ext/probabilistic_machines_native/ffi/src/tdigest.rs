//! `ProbabilisticMachines::TDigest`: quantiles of a stream, in clusters.

use std::cell::RefCell;

use magnus::typed_data::Obj;
use magnus::{Error, RArray, RModule, RString, Ruby, Value, function, method, prelude::*};
use probabilistic_machines::{MIN_COMPRESSION, TDigest};

use crate::data::{centroid, centroid_class};
use crate::{Whole, answer, decoded_or_refused, define_class};

#[magnus::wrap(class = "ProbabilisticMachines::TDigest", free_immediately, size)]
struct Wrapper(RefCell<TDigest>);

impl Wrapper {
    /// `TDigest.new(compression)`. `RangeError` where `TDigest::new` answers
    /// `None`, below [`MIN_COMPRESSION`], or past 65535.
    fn new(ruby: &Ruby, Whole(compression): Whole<u16>) -> Result<Self, Error> {
        TDigest::new(compression)
            .map(|digest| Self(RefCell::new(digest)))
            .ok_or_else(|| {
                Error::new(
                    ruby.exception_range_error(),
                    format!("compression must be at least {MIN_COMPRESSION}, got {compression}"),
                )
            })
    }

    /// `TDigest.decode(bytes)`: a digest, or the refusal symbol.
    fn decode(ruby: &Ruby, bytes: RString) -> Value {
        let bytes = bytes.to_bytes();
        decoded_or_refused(
            ruby,
            TDigest::decode(&bytes).map(|held| Self(RefCell::new(held))),
        )
    }

    fn compression(&self) -> u16 {
        self.0.borrow().compression()
    }

    fn add(ruby: &Ruby, this: Obj<Self>, value: f64) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().add(value);
        Ok(answer(ruby, outcome, "added"))
    }

    fn add_weighted(ruby: &Ruby, this: Obj<Self>, value: f64, weight: f64) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().add_weighted(value, weight);
        Ok(answer(ruby, outcome, "added"))
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

    fn centroids(ruby: &Ruby, this: &Self) -> Result<RArray, Error> {
        let class = centroid_class(ruby)?;
        let clusters: Vec<_> = this.0.borrow().centroids().to_vec();
        let array = ruby.ary_new_capa(clusters.len());
        for cluster in &clusters {
            array.push(centroid(class, cluster)?)?;
        }
        Ok(array)
    }

    fn compress(this: Obj<Self>) -> Result<(), Error> {
        this.check_frozen()?;
        this.0.borrow_mut().compress();
        Ok(())
    }

    /// `:merged`, `:wrong_shape` for a digest of another compression, or
    /// `:overflow`.
    fn merge(ruby: &Ruby, this: Obj<Self>, other: &Self) -> Result<Value, Error> {
        this.check_frozen()?;
        let source = other.0.borrow().clone();
        let merged = this.0.borrow_mut().merge(&source);
        Ok(answer(ruby, merged, "merged"))
    }

    fn encoded(ruby: &Ruby, this: &Self) -> RString {
        ruby.str_from_slice(&this.0.borrow().encoded())
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
    let class = define_class(ruby, module, "TDigest")?;
    class.const_set("MIN_COMPRESSION", MIN_COMPRESSION)?;
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("compression", method!(Wrapper::compression, 0))?;
    class.define_method("add", method!(Wrapper::add, 1))?;
    class.define_method("add_weighted", method!(Wrapper::add_weighted, 2))?;
    class.define_method("quantile", method!(Wrapper::quantile, 1))?;
    class.define_method("cdf", method!(Wrapper::cdf, 1))?;
    class.define_method("count", method!(Wrapper::count, 0))?;
    class.define_method("minimum", method!(Wrapper::minimum, 0))?;
    class.define_method("maximum", method!(Wrapper::maximum, 0))?;
    class.define_method("size", method!(Wrapper::size, 0))?;
    class.define_method("empty?", method!(Wrapper::is_empty, 0))?;
    class.define_method("centroids", method!(Wrapper::centroids, 0))?;
    class.define_method("compress", method!(Wrapper::compress, 0))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("encoded", method!(Wrapper::encoded, 0))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
