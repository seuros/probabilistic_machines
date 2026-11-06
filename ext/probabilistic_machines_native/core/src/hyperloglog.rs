//! Distinct-count estimation in a fixed array of registers.

use crate::hash::{hash64, u32_low_u8, u64_low_u16};
use crate::{CodecError, TAG_HYPERLOGLOG, encode_frame, open_frame};

/// Odd terms of `2 * atanh(t) = ln((1 + t) / (1 - t))`, in Horner order.
const ATANH_COEFFICIENTS: [f64; 5] = [2.0, 2.0 / 3.0, 0.4, 2.0 / 7.0, 2.0 / 9.0];

/// Bits of hash a sketch splits into a register index and a leading-zero count.
const HASH_BITS: u32 = 64;

const POW2_NEG: [f64; 65] = {
    let mut table = [1.0; 65];
    let mut exponent = 1;
    while exponent < 65 {
        table[exponent] = table[exponent - 1] * 0.5;
        exponent += 1;
    }
    table
};

/// Counts distinct items in `M` bytes of state, however many items it sees.
///
/// `M` is the register count: a power of two between 16 and 65536. The relative
/// standard error of [`estimate`](Self::estimate) is `1.04 / sqrt(M)`, so 1024
/// registers give 3.25%, 4096 give 1.63%, 16384 give 0.81%. A register holds a
/// leading-zero count, which fits in a byte, so the sketch is `M` bytes on the
/// stack and never touches a heap.
///
/// ```
/// use probabilistic_machines::HyperLogLog;
///
/// let mut seen = HyperLogLog::<4096>::new();
/// for id in 0_u32..100_000 {
///     seen.add(&id.to_le_bytes());
/// }
///
/// let error = (seen.estimate() - 100_000.0).abs() / 100_000.0;
/// assert!(error < 0.05, "{error}");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HyperLogLog<const M: usize> {
    registers: [u8; M],
}

impl<const M: usize> HyperLogLog<M> {
    /// Index bits taken from the top of each hash.
    pub const PRECISION: u32 = {
        assert!(M.is_power_of_two(), "register count must be a power of two");
        assert!(M >= 16, "register count must be at least 16");
        assert!(M <= 1 << 16, "register count must be at most 65536");
        M.trailing_zeros()
    };

    /// Highest rank a register can hold: the index bits leave
    /// `HASH_BITS - PRECISION` bits of hash, and a rank counts their leading
    /// zeros plus one.
    const MAX_RANK: u8 = u32_low_u8(HASH_BITS - Self::PRECISION + 1);

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = M + 2;

    #[must_use]
    pub const fn new() -> Self {
        Self { registers: [0; M] }
    }

    /// Relative standard error of the estimate, `1.04 / sqrt(M)`.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn standard_error() -> f64 {
        1.04 / f64::from(1_u32 << Self::PRECISION).sqrt()
    }

    pub fn add(&mut self, item: &[u8]) {
        self.add_hash(hash64(item));
    }

    /// Records an item the caller already hashed, for keys that never existed
    /// as bytes.
    pub fn add_hash(&mut self, hash: u64) {
        let index = usize::from(u64_low_u16(hash >> (HASH_BITS - Self::PRECISION)));
        let rank = rank(hash, Self::PRECISION);
        if self.registers[index] < rank {
            self.registers[index] = rank;
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registers.iter().all(|&register| register == 0)
    }

    /// Estimated number of distinct items.
    ///
    /// Under `2.5 * M` the register histogram is sparse enough that linear
    /// counting over the untouched registers beats the harmonic-mean
    /// estimator, so this switches to it.
    #[must_use]
    pub fn estimate(&self) -> f64 {
        let mut histogram = [0_u32; 65];
        for &register in &self.registers {
            histogram[usize::from(register)] += 1;
        }

        let mut registers = 0.0;
        let mut harmonic = 0.0;
        for (zeros, &count) in histogram.iter().enumerate() {
            let bucket = f64::from(count);
            let weighted = bucket * POW2_NEG[zeros];
            registers += bucket;
            harmonic += weighted;
        }

        let alpha = match Self::PRECISION {
            4 => 0.673,
            5 => 0.697,
            6 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / registers),
        };
        let raw = alpha * registers * registers / harmonic;
        let untouched = f64::from(histogram[0]);

        if untouched > 0.0 && raw <= 2.5 * registers {
            registers * ln(registers / untouched)
        } else {
            raw
        }
    }

    /// Folds another sketch in, as if both had seen every item. Exact: the
    /// result equals a sketch built from the union of the two streams.
    pub fn merge(&mut self, other: &Self) {
        for (mine, theirs) in self.registers.iter_mut().zip(&other.registers) {
            if *theirs > *mine {
                *mine = *theirs;
            }
        }
    }

    /// Writes the sketch into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        encode_frame(out, Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// The sketch's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        crate::encoded_frame(Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[0] = TAG_HYPERLOGLOG;
        frame[1] = u32_low_u8(Self::PRECISION);
        frame[2..].copy_from_slice(&self.registers);
    }

    /// Reads a sketch written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when it was
    /// written with a different register count or holds a rank the hash tail
    /// its precision leaves cannot reach.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut sketch = Self::new();
        sketch.decode_into(bytes)?;
        Ok(sketch)
    }

    /// Reads a frame over this sketch, for a caller that owns one already and
    /// has no room on its stack for a second.
    ///
    /// The sketch is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let frame = open_frame(bytes, Self::ENCODED_LEN, TAG_HYPERLOGLOG)?;
        if frame[1] != u32_low_u8(Self::PRECISION) {
            return Err(CodecError::Parameters);
        }
        let mut index = 0;
        while index < M {
            if frame[2 + index] > Self::MAX_RANK {
                return Err(CodecError::Parameters);
            }
            index += 1;
        }
        self.registers.copy_from_slice(&frame[2..]);
        Ok(())
    }
}

impl<const M: usize> Default for HyperLogLog<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// One plus the number of leading zeros left in a hash once the index bits are
/// shifted off, capped at `64 - precision + 1`.
const fn rank(hash: u64, precision: u32) -> u8 {
    let tail = (hash << precision) | (1 << (precision - 1));
    u32_low_u8(tail.leading_zeros() + 1)
}

/// Natural logarithm of a positive finite `x`, accurate to a few parts in
/// `10^10`. `f64::ln` lives in `std`, and the linear-counting branch needs this
/// on bare metal.
fn ln(x: f64) -> f64 {
    let bits = x.to_bits();
    let mut mantissa = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | 0x3FF0_0000_0000_0000);
    let mut exponent = f64::from(u64_low_u16((bits >> 52) & 0x7FF)) - 1023.0;
    if mantissa > core::f64::consts::SQRT_2 {
        mantissa *= 0.5;
        exponent += 1.0;
    }

    let t = (mantissa - 1.0) / (mantissa + 1.0);
    let square = t * t;
    let mut series = 0.0;
    for coefficient in ATANH_COEFFICIENTS.iter().rev() {
        let higher = series * square;
        series = coefficient + higher;
    }

    let scaled = exponent * core::f64::consts::LN_2;
    let odd = series * t;
    scaled + odd
}

#[cfg(test)]
mod tests;
