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
use magnus::{Error, ExceptionClass, RHash, RModule, Ruby, TryConvert, prelude::*};
use probabilistic_machines::CodecError;

/// `ProbabilisticMachines::Full`, defined in [`init`].
static FULL: OnceLock<Opaque<ExceptionClass>> = OnceLock::new();

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
