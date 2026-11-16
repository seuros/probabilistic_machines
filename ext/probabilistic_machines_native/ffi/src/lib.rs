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
mod init;
mod tdigest;
mod topk;

use magnus::{Error, RArray, RClass, RModule, Ruby, TypedData, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{Class, Registry, Symbols, class_name, exact_typed_with, symbol};
use probabilistic_machines::Refusal;

matryona_machines::symbols! {
    /// The core's refusals, each the snake case of its variant.
    Refusals for Refusal as "a refusal" {
        Truncated => "truncated",
        WrongStructure => "wrong_structure",
        WrongShape => "wrong_shape",
        Malformed => "malformed",
        Full => "full",
        Overflow => "overflow",
        ZeroCount => "zero_count",
        NotFinite => "not_finite",
        InvalidWeight => "invalid_weight",
    }
}

/// Ruby symbol of a refusal: the snake case of the core variant.
pub(crate) fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    Refusals::symbol(ruby, refused).as_value()
}

/// A sketch class the binding defined, pinned at init so a merge can check
/// its partner's exact class without looking it up by name.
pub(crate) type Pinned = Registry<Class>;

/// The other operand of a merge: a `T` that is an instance of exactly the
/// class pinned in `held`, never a subclass instance. `TypeError` otherwise,
/// naming that class and the one received.
pub(crate) fn partner<T: TypedData>(
    ruby: &Ruby,
    held: &Pinned,
    other: Value,
) -> Result<Obj<T>, Error> {
    let class = held.get(ruby)?.get(ruby);
    exact_typed_with(ruby, other, class, || {
        format!("other must be a {class}, not {}", class_name(other))
    })
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
        Ok(()) => symbol(ruby, honoured).as_value(),
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

/// Pins `class` into `held` (no lookup back by name) and fixes magnus's class
/// for the wrapped `T` to it: building a `T` never runs `Object.const_get`
/// later. Called from init once the class and its methods are defined.
pub(crate) fn pin<T: TypedData>(ruby: &Ruby, class: RClass, held: &Pinned) -> Result<(), Error> {
    held.set(ruby, Class::pin_wrapped::<T>(ruby, class)?)
        .map(drop)
}
