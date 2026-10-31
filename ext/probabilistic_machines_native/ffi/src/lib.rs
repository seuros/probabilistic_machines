//! Ruby bindings for the `probabilistic-machines` sketches.
//!
//! Every sketch is sized by a const generic, so each class dispatches the size
//! it is given to a fixed set of monomorphised variants and raises
//! `ArgumentError` for any other size.

#![allow(clippy::needless_pass_by_value)]

mod countmin;
mod cuckoo;
mod data;
mod hyperloglog;
mod tdigest;
mod topk;

use std::sync::OnceLock;

use magnus::scan_args::get_kwargs;
use magnus::value::Opaque;
use magnus::{Error, ExceptionClass, Integer, RHash, RModule, Ruby, TryConvert, Value, prelude::*};
use probabilistic_machines::CodecError;

/// `ProbabilisticMachines::Full`, defined in [`init`].
static FULL: OnceLock<Opaque<ExceptionClass>> = OnceLock::new();

/// A whole number of things: a count, a width, a depth, a `k`, a hash.
///
/// magnus reads an integer through Ruby's `NUM2*` family, which truncates a
/// `Float` towards zero instead of refusing it, so an argument that means an
/// integer is read as one of these and anything but an `Integer` is turned
/// away. An `Integer` outside the target width still comes back as the
/// `RangeError` magnus raises for it.
pub(crate) struct Whole<T>(pub(crate) T);

impl<T: TryConvert> TryConvert for Whole<T> {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(not_whole(value));
        }
        T::try_convert(value).map(Self)
    }
}

fn not_whole(value: Value) -> Error {
    let ruby = Ruby::get_with(value);
    Error::new(
        ruby.exception_type_error(),
        format!(
            "no implicit conversion of {} into Integer: {}",
            value.class(),
            value.inspect()
        ),
    )
}

/// The one required keyword of a constructor.
pub(crate) fn required_kwarg<T: TryConvert>(keywords: RHash, name: &str) -> Result<T, Error> {
    Ok(get_kwargs::<_, (T,), (), ()>(keywords, &[name], &[])?
        .required
        .0)
}

/// A keyword with a default the caller may leave out.
pub(crate) fn optional_kwarg<T: TryConvert>(
    keywords: RHash,
    name: &str,
) -> Result<Option<T>, Error> {
    Ok(
        get_kwargs::<_, (), (Option<T>,), ()>(keywords, &[], &[name])?
            .optional
            .0,
    )
}

pub(crate) fn unsupported(ruby: &Ruby, name: &str, given: usize, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("{name} {given} is not supported, expected one of {supported:?}"),
    )
}

pub(crate) fn unsupported_blob(ruby: &Ruby, name: &str, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("blob was written with a {name} outside {supported:?}"),
    )
}

pub(crate) fn codec_error(ruby: &Ruby, error: CodecError) -> Error {
    Error::new(ruby.exception_arg_error(), format!("codec: {error}"))
}

pub(crate) fn mismatch(ruby: &Ruby, name: &str) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!("the other {name} has a different shape"),
    )
}

pub(crate) fn uninitialised(ruby: &Ruby) -> Error {
    Error::new(
        ruby.exception_runtime_error(),
        "the probabilistic_machines extension is not initialised",
    )
}

pub(crate) fn defined_twice(ruby: &Ruby, name: &str) -> Error {
    Error::new(
        ruby.exception_runtime_error(),
        format!("ProbabilisticMachines::{name} was initialised twice"),
    )
}

/// `ProbabilisticMachines::Full`, raised when a cuckoo filter cannot place an
/// item after relocating its neighbours.
pub(crate) fn full(ruby: &Ruby) -> Error {
    FULL.get().map_or_else(
        || uninitialised(ruby),
        |class| Error::new(ruby.get_inner(*class), "the filter is full"),
    )
}

fn define_errors(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let base = module.define_error("Error", ruby.exception_standard_error())?;
    let full = module.define_error("Full", base)?;
    FULL.set(full.into())
        .map_err(|_| defined_twice(ruby, "Full"))
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("ProbabilisticMachines")?;
    define_errors(ruby, module)?;
    data::define(ruby, module)?;
    hyperloglog::define(ruby, module)?;
    countmin::define(ruby, module)?;
    tdigest::define(ruby, module)?;
    cuckoo::define(ruby, module)?;
    topk::define(ruby, module)
}
