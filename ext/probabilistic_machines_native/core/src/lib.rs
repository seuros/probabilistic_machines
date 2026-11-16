//! Sketches that answer questions about a stream in less memory than the
//! stream: `HyperLogLog`, count-min, cuckoo filter, heavy hitters and t-digest.
#![no_std]
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(test)]
extern crate std;

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// Defines a fixed-size sketch's `encode` and `encoded` over its private
/// `write_frame`, which fills the header and body of a frame of exactly
/// `ENCODED_LEN` bytes. `$noun`
/// names the sketch in the docs; the `$about` pieces, joined, describe its
/// frame.
macro_rules! encoders {
    ($noun:literal $(, $($about:literal),+)?) => {
        #[doc = concat!("Writes the ", $noun, " into `out` and returns the bytes written.")]
        $(
            ///
            #[doc = concat!($($about, " "),+)]
        )?
        ///
        /// # Errors
        ///
        /// [`Refusal::Truncated`] when `out` is shorter than
        /// [`ENCODED_LEN`](Self::ENCODED_LEN).
        pub fn encode(&self, out: &mut [u8]) -> Result<usize, Refusal> {
            $crate::encode_frame(out, Self::ENCODED_LEN, |header, body| {
                self.write_frame(header, body);
            })
        }

        #[doc = concat!("The ", $noun, "'s frame, [`ENCODED_LEN`](Self::ENCODED_LEN) bytes long.")]
        #[cfg(feature = "alloc")]
        #[must_use]
        pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
            $crate::encoded_frame(Self::ENCODED_LEN, |header, body| {
                self.write_frame(header, body);
            })
        }
    };
}

pub mod countmin;
pub mod cuckoo;
pub mod hash;
pub mod hyperloglog;
pub mod topk;

#[cfg(feature = "alloc")]
pub mod tdigest;

pub use countmin::CountMinSketch;
pub use cuckoo::{CuckooFilter, Removing};
pub use hash::{hash64, mix64};
pub use hyperloglog::HyperLogLog;
pub use topk::{HeavyHitters, Hitter};

#[cfg(feature = "alloc")]
pub use tdigest::{Centroid, MIN_COMPRESSION, TDigest};

/// Why a call was not honoured. A refused call changed nothing.
///
/// This is the crate's only refusal type; every operation that can refuse
/// answers `Result<_, Refusal>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The buffer is shorter than the frame: the output of an `encode`, or the
    /// input of a `decode`.
    Truncated,
    /// The frame's leading tag byte names another structure.
    WrongStructure,
    /// The frame, or the sketch given to a `merge`, was built at another
    /// shape: other const-generic parameters, or another digest compression.
    WrongShape,
    /// The frame has the right shape but holds contents no sequence of calls
    /// could have produced.
    Malformed,
    /// A cuckoo filter found no free slot for the fingerprint, in its two
    /// buckets or after 64 evictions.
    Full,
    /// A counter or a total would pass the largest value its type holds: a
    /// `u32` count-min counter, a `u64` heavy-hitters counter or observation
    /// count, or a digest's `f64` total weight becoming infinite.
    Overflow,
    /// A count of zero observes nothing, and on a heavy-hitters table would
    /// still evict a tracked key.
    ZeroCount,
    /// A value or a weight given to a digest is NaN or infinite.
    NotFinite,
    /// A weight given to a digest is zero or negative.
    InvalidWeight,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "buffer shorter than the frame",
            Self::WrongStructure => "the frame holds another structure",
            Self::WrongShape => "the frame was written at another shape",
            Self::Malformed => "the frame holds contents no sketch produces",
            Self::Full => "no free slot for the fingerprint",
            Self::Overflow => "a counter would overflow",
            Self::ZeroCount => "a count of zero observes nothing",
            Self::NotFinite => "a value or weight is not finite",
            Self::InvalidWeight => "a weight is zero or negative",
        })
    }
}

impl core::error::Error for Refusal {}

pub(crate) const TAG_HYPERLOGLOG: u8 = 0xA1;
pub(crate) const TAG_TOPK: u8 = 0xB1;
pub(crate) const TAG_COUNTMIN: u8 = 0xC1;
#[cfg(feature = "alloc")]
pub(crate) const TAG_TDIGEST: u8 = 0xD1;
pub(crate) const TAG_CUCKOO: u8 = 0xF1;

/// Fills the first `len` bytes of `out` through `write`, handed the `H`-byte
/// header and the body after it, for the frames every sketch encodes to.
///
/// # Errors
///
/// [`Refusal::Truncated`] when `out` is shorter than `len`. Every frame is at
/// least its header long, so `len` never falls short of `H`; were it to, the
/// frame could not be written and the answer is the same refusal, with `out`
/// untouched.
pub(crate) fn encode_frame<const H: usize>(
    out: &mut [u8],
    len: usize,
    write: impl FnOnce(&mut [u8; H], &mut [u8]),
) -> Result<usize, Refusal> {
    let Some((header, body)) = out
        .get_mut(..len)
        .and_then(<[u8]>::split_first_chunk_mut::<H>)
    else {
        return Err(Refusal::Truncated);
    };
    write(header, body);
    Ok(len)
}

/// A freshly allocated `len`-byte frame filled through `write`, handed the
/// `H`-byte header and the body after it.
#[cfg(feature = "alloc")]
pub(crate) fn encoded_frame<const H: usize>(
    len: usize,
    write: impl FnOnce(&mut [u8; H], &mut [u8]),
) -> alloc::boxed::Box<[u8]> {
    let mut frame = alloc::vec![0_u8; len].into_boxed_slice();
    if let Some((header, body)) = frame.split_first_chunk_mut::<H>() {
        write(header, body);
    } else {
        // An all-zero frame carries no tag, so every decoder refuses it.
        debug_assert!(false, "every frame is at least its header long");
    }
    frame
}

/// The `H`-byte header and the body of the first `len` bytes of `bytes`,
/// once they are known to be a frame tagged `tag`.
///
/// # Errors
///
/// [`Refusal::Truncated`] when `bytes` is shorter than `len` or than the
/// header, [`Refusal::WrongStructure`] when the tag names another structure.
pub(crate) fn open_frame<const H: usize>(
    bytes: &[u8],
    len: usize,
    tag: u8,
) -> Result<(&[u8; H], &[u8]), Refusal> {
    let Some((header, body)) = bytes.get(..len).and_then(<[u8]>::split_first_chunk::<H>) else {
        return Err(Refusal::Truncated);
    };
    if header.first() != Some(&tag) {
        return Err(Refusal::WrongStructure);
    }
    Ok((header, body))
}
