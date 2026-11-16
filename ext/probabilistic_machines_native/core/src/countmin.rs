//! Frequency estimation for a stream nobody wants to store.

use crate::hash::{hash64, mix64, u32_low_u8, u64_low_u16, usize_low_u8};
use crate::{Refusal, TAG_COUNTMIN, open_frame};

/// Tag, width bits, depth, the eight-byte total, then one reserved byte: the
/// byte that once flagged a saturated counter. No counter saturates any more,
/// so it is written as zero and a frame carrying anything else (one an earlier
/// release wrote after a counter stopped at `u32::MAX`) is refused.
const HEADER_LEN: usize = 12;
const COUNTER_LEN: usize = 4;

/// Counts how often each item appeared, in `W * D` counters.
///
/// `W` is the row width, a power of two up to 65536; `D` is the number of rows.
/// An estimate is never below the true count, and overshoots it by more than
/// `e * total / W` with probability at most `e^-D`, where `total` is the sum of
/// every count added. 2048 columns over 5 rows hold the overshoot under 0.14%
/// of the total with probability 0.993.
///
/// A counter is a `u32`. An addition or a merge that would carry one past
/// [`u32::MAX`] is refused with [`Refusal::Overflow`] and changes nothing, so
/// no counter ever saturates and the lower bound above always holds.
///
/// ```
/// use probabilistic_machines::CountMinSketch;
///
/// let mut hits = CountMinSketch::<1024, 4>::new();
/// assert_eq!(hits.add_count(b"/login", 900), Ok(()));
/// assert_eq!(hits.add(b"/health"), Ok(()));
///
/// assert!(hits.estimate(b"/login") >= 900);
/// assert_eq!(hits.estimate(b"/never-seen"), 0);
/// assert_eq!(hits.total(), 901);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountMinSketch<const W: usize, const D: usize> {
    rows: [[u32; W]; D],
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
    pub const ENCODED_LEN: usize = HEADER_LEN + W * D * COUNTER_LEN;

    /// An empty sketch: every counter zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { rows: [[0; W]; D] }
    }

    /// Sum of every count added.
    ///
    /// Every row takes every count, so each sums to the total exactly; this
    /// reads it off the first. `W` counters below `2^32` sum below `2^48`, so
    /// the total needs no overflow check of its own: an addition whose
    /// counters fit has a total that fits.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.rows
            .iter()
            .take(1)
            .flatten()
            .copied()
            .map(u64::from)
            .sum()
    }

    /// Adds one occurrence of `item`.
    ///
    /// # Errors
    ///
    /// As [`add_hash_count`](Self::add_hash_count).
    pub fn add(&mut self, item: &[u8]) -> Result<(), Refusal> {
        self.add_count(item, 1)
    }

    /// Adds `count` occurrences of `item`.
    ///
    /// # Errors
    ///
    /// As [`add_hash_count`](Self::add_hash_count).
    pub fn add_count(&mut self, item: &[u8], count: u32) -> Result<(), Refusal> {
        self.add_hash_count(hash64(item), count)
    }

    /// Adds to an item the caller already hashed.
    ///
    /// # Errors
    ///
    /// [`Refusal::ZeroCount`] when `count` is zero, [`Refusal::Overflow`] when
    /// one of the item's counters would pass [`u32::MAX`]. The sketch is
    /// unchanged either way.
    pub fn add_hash_count(&mut self, hash: u64, count: u32) -> Result<(), Refusal> {
        if count == 0 {
            return Err(Refusal::ZeroCount);
        }
        let mut raised: [Option<(&mut u32, u32)>; D] = [const { None }; D];
        let mut cursor = hash;
        for (row, target) in self.rows.iter_mut().zip(&mut raised) {
            cursor = mix64(cursor);
            let column = usize::from(u64_low_u16(cursor)) & Self::COLUMN_MASK;
            let Some(counter) = row.get_mut(column) else {
                debug_assert!(false, "a column is masked below W");
                return Err(Refusal::Overflow);
            };
            let Some(sum) = counter.checked_add(count) else {
                return Err(Refusal::Overflow);
            };
            *target = Some((counter, sum));
        }
        for (counter, sum) in raised.into_iter().flatten() {
            *counter = sum;
        }
        Ok(())
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
            // The column is masked below `W`, so the lookup always lands.
            if let Some(&held) = row.get(column)
                && held < smallest
            {
                smallest = held;
            }
        }
        smallest
    }

    /// Adds another sketch's counters to this one, which is exact for sketches
    /// of the same shape: the result equals one sketch fed both streams.
    ///
    /// # Errors
    ///
    /// [`Refusal::Overflow`] when any counter's sum would pass [`u32::MAX`].
    /// Every sum is checked before the first is stored, so a refused merge
    /// leaves the sketch as it was.
    pub fn merge(&mut self, other: &Self) -> Result<(), Refusal> {
        let mine = self.rows.iter().flatten();
        let theirs = other.rows.iter().flatten();
        if mine
            .zip(theirs)
            .any(|(counter, addend)| counter.checked_add(*addend).is_none())
        {
            return Err(Refusal::Overflow);
        }
        let mine = self.rows.iter_mut().flatten();
        for (counter, addend) in mine.zip(other.rows.iter().flatten()) {
            // Every sum was checked above, so none of these saturates.
            *counter = counter.saturating_add(*addend);
        }
        Ok(())
    }

    encoders!("sketch");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, header: &mut [u8; HEADER_LEN], body: &mut [u8]) {
        let [tag, bits, depth, total @ .., reserved] = header;
        *tag = TAG_COUNTMIN;
        *bits = u32_low_u8(W.trailing_zeros());
        *depth = usize_low_u8(D);
        *total = self.total().to_le_bytes();
        *reserved = 0;

        let counters = self.rows.iter().flatten();
        let (slots, _) = body.as_chunks_mut::<COUNTER_LEN>();
        for (slot, counter) in slots.iter_mut().zip(counters) {
            *slot = counter.to_le_bytes();
        }
    }

    /// Reads a sketch written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when its shape differs from `W` by `D`,
    /// [`Refusal::Malformed`] when its counters contradict its total or it
    /// carries the saturation flag of an earlier release, whose counters no
    /// longer bound the true counts from above.
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
        let (header, body) = open_frame::<HEADER_LEN>(bytes, Self::ENCODED_LEN, TAG_COUNTMIN)?;
        let [_, bits, depth, total @ .., reserved] = *header;
        if bits != u32_low_u8(W.trailing_zeros()) || depth != usize_low_u8(D) {
            return Err(Refusal::WrongShape);
        }

        let total = u64::from_le_bytes(total);
        let (slots, _) = body.as_chunks::<COUNTER_LEN>();
        if reserved != 0 || !Self::rows_are_coherent(slots, total) {
            return Err(Refusal::Malformed);
        }

        let counters = self.rows.iter_mut().flatten();
        for (slot, counter) in slots.iter().zip(counters) {
            *counter = u32::from_le_bytes(*slot);
        }
        Ok(())
    }

    /// Whether the counters carry the total the frame claims, as every sketch
    /// built by adding items does: each row takes every count added, so it sums
    /// to the total exactly.
    fn rows_are_coherent(slots: &[[u8; COUNTER_LEN]], total: u64) -> bool {
        slots.chunks(W).all(|row| {
            let counted: u64 = row
                .iter()
                .map(|slot| u64::from(u32::from_le_bytes(*slot)))
                .sum();
            counted == total
        })
    }
}

impl<const W: usize, const D: usize> Default for CountMinSketch<W, D> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
