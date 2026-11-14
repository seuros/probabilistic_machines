//! Distinct-count estimation in a fixed array of registers.

use crate::hash::{hash64, u32_low_u8, u64_low_u16};
use crate::{Refusal, TAG_HYPERLOGLOG, open_frame};
use sliderule_machines::ln_ratio;

/// Fraction bits the linear-counting logarithm is taken with.
const LN_BITS: u32 = sliderule_machines::MAX_FRACTION_BITS;

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
/// Nothing here can refuse: [`add`](Self::add) and [`merge`](Self::merge) only
/// raise registers, so both are idempotent and neither has a limit to hit.
/// Only the codec answers a [`Refusal`].
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

    /// An empty sketch: every register zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { registers: [0; M] }
    }

    /// Relative standard error of the estimate, `1.04 / sqrt(M)`.
    ///
    /// `M` is a power of two, so the root is a power of two as well, times
    /// `sqrt(2)` when the precision is odd: no `std` square root is needed.
    #[must_use]
    pub const fn standard_error() -> f64 {
        let halved = 1.04 * POW2_NEG[(Self::PRECISION / 2) as usize];
        if Self::PRECISION % 2 == 1 {
            halved * core::f64::consts::FRAC_1_SQRT_2
        } else {
            halved
        }
    }

    /// Counts `item`, hashed with [`hash64`].
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

    /// Whether nothing was ever added: every register zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registers.iter().all(|&register| register == 0)
    }

    /// Estimated number of distinct items.
    ///
    /// Under `2.5 * M` the register histogram is sparse enough that linear
    /// counting over the untouched registers beats the harmonic-mean
    /// estimator, so this switches to it.
    ///
    /// Linear counting is `M * ln(M / untouched)` over two integers, taken
    /// with `sliderule_machines::ln_ratio` at 58 fraction bits: the logarithm
    /// is within `3 * 2^-58` of the exact one before its one rounding to an
    /// `f64`, and involves no floating-point series, so the same registers
    /// give the same bits on every target. The harmonic-mean branch is sums,
    /// products and one division of `f64`s, each correctly rounded by IEEE 754.
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
            linear_count(Self::PRECISION, histogram[0])
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

    encoders!("sketch");

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
    /// [`Refusal::Truncated`] when `bytes` is too short, [`Refusal::WrongStructure`]
    /// when it holds another structure, [`Refusal::WrongShape`] when it was
    /// written with a different register count, [`Refusal::Malformed`] when
    /// it holds a rank the hash tail its precision leaves cannot reach.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
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
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let frame = open_frame(bytes, Self::ENCODED_LEN, TAG_HYPERLOGLOG)?;
        if frame[1] != u32_low_u8(Self::PRECISION) {
            return Err(Refusal::WrongShape);
        }
        let mut index = 0;
        while index < M {
            if frame[2 + index] > Self::MAX_RANK {
                return Err(Refusal::Malformed);
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

/// `M * ln(M / untouched)` for `M = 2^precision` registers of which
/// `untouched` (`1..=M`) still hold zero.
///
/// `ln_ratio` answers an interval around the exact `ln(M) - ln(untouched)`:
/// each factor's logarithm falls short by less than `3 * 2^-58`, so the
/// working value sits within `3 * 2^-58` of the exact one and the interval is
/// that value widened by three units on each side. Its midpoint is the
/// working value, at most `3 * 2^-58` off.
///
/// The ratio is at least one, so the midpoint is never negative, and below
/// `ln(65536) * 2^58 < 2^62`, so its low 64 bits are all of it. Those split
/// into two `u32` halves that convert to `f64` exactly; their sum rounds once
/// (relative `2^-53`), and scaling by `2^-58` and by the power of two `M` is
/// exact. The result is within `M * (ln(M / untouched) * 2^-53 + 3 * 2^-58)`
/// of the real value: under `10^-10` items at 65536 registers, against a
/// documented error of `1.04 / sqrt(M)` of the estimate.
fn linear_count(precision: u32, untouched: u32) -> f64 {
    let registers = 1_u64 << precision;
    let ln = ln_ratio::<LN_BITS>(&[registers], &[u64::from(untouched)]);
    let midpoint = i128::midpoint(ln.lo, ln.hi).max(0).cast_unsigned();
    let [b0, b1, b2, b3, b4, b5, b6, b7, ..] = midpoint.to_le_bytes();
    let low = f64::from(u32::from_le_bytes([b0, b1, b2, b3]));
    let high = f64::from(u32::from_le_bytes([b4, b5, b6, b7]));
    let scaled = (high * 4_294_967_296.0 + low) * POW2_NEG[LN_BITS as usize];
    scaled * f64::from(1_u32 << precision)
}

#[cfg(test)]
mod tests;
