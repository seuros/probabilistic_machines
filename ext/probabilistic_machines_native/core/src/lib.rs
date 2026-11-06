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
pub(crate) const TAG_TOPK: u8 = 0xB1;
pub(crate) const TAG_COUNTMIN: u8 = 0xC1;
#[cfg(feature = "alloc")]
pub(crate) const TAG_TDIGEST: u8 = 0xD1;
pub(crate) const TAG_CUCKOO: u8 = 0xF1;

/// The first eight bytes of a field, for the little-endian scalars the frames
/// are built from.
pub(crate) const fn read8(bytes: &[u8]) -> [u8; 8] {
    let mut buffer = [0_u8; 8];
    buffer.copy_from_slice(bytes);
    buffer
}

/// Fills the first `len` bytes of `out` through `write`, for the fixed-size
/// frames every sketch encodes to.
pub(crate) fn encode_frame(
    out: &mut [u8],
    len: usize,
    write: impl FnOnce(&mut [u8]),
) -> Result<usize, CodecError> {
    let Some(frame) = out.get_mut(..len) else {
        return Err(CodecError::Truncated);
    };
    write(frame);
    Ok(len)
}

/// A freshly allocated `len`-byte frame filled through `write`.
#[cfg(feature = "alloc")]
pub(crate) fn encoded_frame(len: usize, write: impl FnOnce(&mut [u8])) -> alloc::boxed::Box<[u8]> {
    let mut frame = alloc::vec![0_u8; len].into_boxed_slice();
    write(&mut frame);
    frame
}

/// The first `len` bytes of `bytes`, once they are known to be a frame tagged
/// `tag`.
pub(crate) fn open_frame(bytes: &[u8], len: usize, tag: u8) -> Result<&[u8], CodecError> {
    let Some(frame) = bytes.get(..len) else {
        return Err(CodecError::Truncated);
    };
    if frame[0] != tag {
        return Err(CodecError::Tag);
    }
    Ok(frame)
}
