#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod countmin;
pub mod cuckoo;
pub mod hash;
pub mod hyperloglog;
pub mod topk;

#[cfg(feature = "alloc")]
pub mod tdigest;

pub use countmin::CountMinSketch;
pub use cuckoo::CuckooFilter;
pub use hash::{hash64, mix64};
pub use hyperloglog::HyperLogLog;
pub use topk::HeavyHitters;

#[cfg(feature = "alloc")]
pub use tdigest::TDigest;

/// Why a sketch could not be read from or written to a byte buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer is shorter than the encoded form needs.
    Truncated,
    /// The leading byte names a different structure.
    Tag,
    /// The encoded shape does not match the target type's.
    Parameters,
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "buffer too short",
            Self::Tag => "wrong structure tag",
            Self::Parameters => "shape mismatch",
        })
    }
}

impl core::error::Error for CodecError {}

pub(crate) const TAG_HYPERLOGLOG: u8 = 0xA1;
pub(crate) const TAG_COUNTMIN: u8 = 0xC1;
#[cfg(feature = "alloc")]
pub(crate) const TAG_TDIGEST: u8 = 0xD1;
pub(crate) const TAG_CUCKOO: u8 = 0xF1;
