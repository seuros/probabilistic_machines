//! Frequency estimation for a stream nobody wants to store.

use crate::hash::{hash64, mix64, u32_low_u8, u64_low_u16, usize_low_u8};
use crate::{CodecError, TAG_COUNTMIN, read8};

/// Counts how often each item appeared, in `W * D` counters.
///
/// `W` is the row width, a power of two up to 65536; `D` is the number of rows.
/// An estimate is never below the true count, and overshoots it by more than
/// `e * total / W` with probability at most `e^-D`, where `total` is the sum of
/// every count added. 2048 columns over 5 rows hold the overshoot under 0.14%
/// of the total with probability 0.993.
///
/// ```
/// use probabilistic_machines::CountMinSketch;
///
/// let mut hits = CountMinSketch::<1024, 4>::new();
/// hits.add_count(b"/login", 900);
/// hits.add(b"/health");
///
/// assert!(hits.estimate(b"/login") >= 900);
/// assert_eq!(hits.estimate(b"/never-seen"), 0);
/// assert_eq!(hits.total(), 901);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountMinSketch<const W: usize, const D: usize> {
    rows: [[u32; W]; D],
    total: u64,
}

impl<const W: usize, const D: usize> CountMinSketch<W, D> {
    const COLUMN_MASK: usize = {
        assert!(W.is_power_of_two(), "row width must be a power of two");
        assert!(W >= 2, "row width must be at least 2");
        assert!(W <= 1 << 16, "row width must be at most 65536");
        assert!(D >= 1, "a sketch needs at least one row");
        assert!(D <= 64, "a sketch needs at most 64 rows");
        W - 1
    };

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = 11 + W * D * 4;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            rows: [[0; W]; D],
            total: 0,
        }
    }

    /// Sum of every count added, saturating at [`u64::MAX`].
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    pub fn add(&mut self, item: &[u8]) {
        self.add_count(item, 1);
    }

    pub fn add_count(&mut self, item: &[u8], count: u32) {
        self.add_hash_count(hash64(item), count);
    }

    /// Adds to an item the caller already hashed.
    pub fn add_hash_count(&mut self, hash: u64, count: u32) {
        let mut cursor = hash;
        for row in &mut self.rows {
            cursor = mix64(cursor);
            let column = usize::from(u64_low_u16(cursor)) & Self::COLUMN_MASK;
            row[column] = row[column].saturating_add(count);
        }
        self.total = self.total.saturating_add(u64::from(count));
    }

    /// Upper bound on how often `item` was added.
    #[must_use]
    pub fn estimate(&self, item: &[u8]) -> u32 {
        self.estimate_hash(hash64(item))
    }

    /// Upper bound for an item the caller already hashed.
    #[must_use]
    pub fn estimate_hash(&self, hash: u64) -> u32 {
        let mut cursor = hash;
        let mut smallest = u32::MAX;
        for row in &self.rows {
            cursor = mix64(cursor);
            let column = usize::from(u64_low_u16(cursor)) & Self::COLUMN_MASK;
            if row[column] < smallest {
                smallest = row[column];
            }
        }
        smallest
    }

    /// Adds another sketch's counters to this one, which is exact for sketches
    /// of the same shape: the result equals one sketch fed both streams.
    pub fn merge(&mut self, other: &Self) {
        for (mine, theirs) in self.rows.iter_mut().zip(&other.rows) {
            for (counter, addend) in mine.iter_mut().zip(theirs) {
                *counter = counter.saturating_add(*addend);
            }
        }
        self.total = self.total.saturating_add(other.total);
    }

    /// Writes the sketch into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(frame) = out.get_mut(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        self.write_frame(frame);
        Ok(Self::ENCODED_LEN)
    }

    /// The sketch's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; Self::ENCODED_LEN].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[0] = TAG_COUNTMIN;
        frame[1] = u32_low_u8(W.trailing_zeros());
        frame[2] = usize_low_u8(D);
        frame[3..11].copy_from_slice(&self.total.to_le_bytes());

        let counters = self.rows.iter().flatten();
        let (slots, _) = frame[11..].as_chunks_mut::<4>();
        for (slot, counter) in slots.iter_mut().zip(counters) {
            *slot = counter.to_le_bytes();
        }
    }

    /// Reads a sketch written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when its
    /// shape differs from `W` by `D`.
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
        let Some(frame) = bytes.get(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        if frame[0] != TAG_COUNTMIN {
            return Err(CodecError::Tag);
        }
        if frame[1] != u32_low_u8(W.trailing_zeros()) || frame[2] != usize_low_u8(D) {
            return Err(CodecError::Parameters);
        }

        self.total = u64::from_le_bytes(read8(&frame[3..11]));
        let counters = self.rows.iter_mut().flatten();
        let (slots, _) = frame[11..].as_chunks::<4>();
        for (slot, counter) in slots.iter().zip(counters) {
            *counter = u32::from_le_bytes(*slot);
        }
        Ok(())
    }
}

impl<const W: usize, const D: usize> Default for CountMinSketch<W, D> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
