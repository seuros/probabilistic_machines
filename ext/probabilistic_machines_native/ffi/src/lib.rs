//! Ruby bindings for the `probabilistic-machines` sketches.
//!
//! Every sketch is sized by a const generic, so each class dispatches the size
//! it is given to a fixed set of monomorphised variants, listed in a class
//! constant, and raises `RangeError` for any other size. Refusals come back as
//! the `snake_case` symbol of the core's `Refusal` variant and never raise.

mod countmin;
mod cuckoo;
mod data;
mod hyperloglog;
mod tdigest;
mod topk;

use magnus::{
    Error, Integer, RArray, RClass, RModule, Ruby, TryConvert, TypedData, Value, prelude::*,
};
use probabilistic_machines::Refusal;

/// A whole number of things: a count, a width, a depth, a hash.
///
/// magnus reads an integer through Ruby's `NUM2*` family, which truncates a
/// `Float` towards zero instead of refusing it, so an argument that means an
/// integer is read as one of these and anything but an `Integer` raises
/// `TypeError`. An `Integer` outside the target width still comes back as the
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

/// Ruby symbol of a refusal: the snake case of the core variant.
pub(crate) fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    ruby.to_symbol(match refused {
        Refusal::Truncated => "truncated",
        Refusal::WrongStructure => "wrong_structure",
        Refusal::WrongShape => "wrong_shape",
        Refusal::Malformed => "malformed",
        Refusal::Full => "full",
        Refusal::Overflow => "overflow",
        Refusal::ZeroCount => "zero_count",
        Refusal::NotFinite => "not_finite",
        Refusal::InvalidWeight => "invalid_weight",
    })
    .as_value()
}

/// The object a `decode` built, or the refusal symbol when its frame was
/// refused.
pub(crate) fn decoded_or_refused<T: TypedData>(ruby: &Ruby, outcome: Result<T, Refusal>) -> Value {
    match outcome {
        Ok(object) => ruby.obj_wrap(object).as_value(),
        Err(refused) => refusal(ruby, refused),
    }
}

/// `honoured` as a symbol when the call was honoured, its refusal otherwise.
pub(crate) fn answer(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &str) -> Value {
    match outcome {
        Ok(()) => ruby.to_symbol(honoured).as_value(),
        Err(refused) => refusal(ruby, refused),
    }
}

/// A size this build carries no monomorphised variant for: rejected
/// configuration, so `RangeError`.
pub(crate) fn unsupported(ruby: &Ruby, name: &str, given: usize, supported: &[usize]) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!("{name} {given} is not supported, expected one of {supported:?}"),
    )
}

/// The supported sizes as a frozen Array, for a class constant.
pub(crate) fn sizes(ruby: &Ruby, supported: &[usize]) -> RArray {
    let array = ruby.ary_from_iter(supported.iter().copied());
    array.freeze();
    array
}

/// Defines `name` under `module` with no allocator of its own, so
/// `Class#allocate` cannot hand out an object the core never built.
pub(crate) fn define_class(ruby: &Ruby, module: RModule, name: &str) -> Result<RClass, Error> {
    let class = module.define_class(name, ruby.class_object())?;
    class.undef_default_alloc_func();
    Ok(class)
}

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
