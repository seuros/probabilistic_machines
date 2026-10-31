//! Which keys dominate a stream, in `K` counters instead of one per key.

use crate::hash::usize_low_u16;
use crate::{CodecError, TAG_TOPK, read8};

const HEADER_LEN: usize = 13;
const RECORD_LEN: usize = 24;

/// A tracked key, its counter, and how much of that counter it may have
/// inherited from keys evicted before it or from a merged-in stream that
/// tracked it no longer.
///
/// The true count lies in `count - overestimate ..= count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hitter<T> {
    pub key: T,
    pub count: u64,
    pub overestimate: u64,
}

/// Space-Saving over `K` counters: whatever is frequent ends up tracked.
///
/// After `n` observations every key that occurred more than `n / K` times is in
/// the table, and no tracked key's counter is below its true count. Lookups and
/// insertions scan all `K` counters, which is the trade for needing no heap and
/// no key ordering: keep `K` in the dozens-to-hundreds range.
///
/// ```
/// use probabilistic_machines::HeavyHitters;
///
/// let mut traffic = HeavyHitters::<u32, 8>::new();
/// for request in 0..1_000_u32 {
///     traffic.add(request % 97);
/// }
/// for _ in 0..5_000 {
///     traffic.add(7);
/// }
///
/// assert_eq!(traffic.observed(), 6_000);
/// assert!(traffic.count(&7) >= 5_000);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeavyHitters<T, const K: usize> {
    slots: [Option<Hitter<T>>; K],
    observed: u64,
}

impl<T: Copy + Eq, const K: usize> HeavyHitters<T, K> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; K],
            observed: 0,
        }
    }

    /// Observations fed in, saturating at [`u64::MAX`].
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }

    /// Keys currently tracked, at most `K`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// Every tracked key, in no particular order.
    pub fn hitters(&self) -> impl Iterator<Item = &Hitter<T>> {
        self.slots.iter().flatten()
    }

    /// Upper bound on how often `key` occurred, 0 when it is not tracked.
    #[must_use]
    pub fn count(&self, key: &T) -> u64 {
        self.hitters()
            .find(|hitter| hitter.key == *key)
            .map_or(0, |hitter| hitter.count)
    }

    /// The most frequent tracked key.
    #[must_use]
    pub fn top(&self) -> Option<&Hitter<T>> {
        self.hitters().max_by_key(|hitter| hitter.count)
    }

    pub fn add(&mut self, key: T) {
        self.add_count(key, 1);
    }

    pub fn add_count(&mut self, key: T, count: u64) {
        self.observed = self.observed.saturating_add(count);
        self.absorb(key, count, 0);
    }

    /// Folds another table in. Counts of keys tracked by both are exact sums.
    /// A key only one side tracked may have occurred in the other stream too,
    /// up to that side's eviction floor, so it takes the floor on both its
    /// bounds: the counter still covers the true count and the interval
    /// `count - overestimate ..= count` still holds it.
    pub fn merge(&mut self, other: &Self) {
        let floor = other.floor();
        for tracked in self.slots.iter_mut().flatten() {
            if other.hitters().all(|hitter| hitter.key != tracked.key) {
                tracked.count = tracked.count.saturating_add(floor);
                tracked.overestimate = tracked.overestimate.saturating_add(floor);
            }
        }
        for hitter in other.hitters() {
            self.absorb(hitter.key, hitter.count, hitter.overestimate);
        }
        self.observed = self.observed.saturating_add(other.observed);
    }

    /// How often an untracked key may have occurred: nothing while a counter is
    /// still free, otherwise the weakest counter.
    fn floor(&self) -> u64 {
        if self.slots.iter().any(Option::is_none) {
            return 0;
        }
        self.hitters().map(|hitter| hitter.count).min().unwrap_or(0)
    }

    fn absorb(&mut self, key: T, count: u64, overestimate: u64) {
        if let Some(tracked) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|hitter| hitter.key == key)
        {
            tracked.count = tracked.count.saturating_add(count);
            tracked.overestimate = tracked.overestimate.saturating_add(overestimate);
            return;
        }

        if let Some(free) = self.slots.iter_mut().find(|slot| slot.is_none()) {
            *free = Some(Hitter {
                key,
                count,
                overestimate,
            });
            return;
        }

        let Some(weakest) = self
            .slots
            .iter_mut()
            .flatten()
            .min_by_key(|hitter| hitter.count)
        else {
            return;
        };
        let floor = weakest.count;
        *weakest = Hitter {
            key,
            count: floor.saturating_add(count),
            overestimate: floor.saturating_add(overestimate),
        };
    }
}

impl<const K: usize> HeavyHitters<u64, K> {
    /// `K` as the frame's shape field.
    const COUNTERS: [u8; 2] = {
        assert!(K >= 1, "a table needs at least one counter");
        assert!(K < 1 << 16, "a table needs fewer than 65536 counters");
        usize_low_u16(K).to_le_bytes()
    };

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = HEADER_LEN + K * RECORD_LEN;

    /// Writes the table into `out`, returning the number of bytes written.
    ///
    /// The frame is the tag, `K`, the observation count, the number of tracked
    /// keys and `K` records of key, count and overestimate. Records past the
    /// tracked count are zero.
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

    /// The table's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; Self::ENCODED_LEN].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[0] = TAG_TOPK;
        frame[1..3].copy_from_slice(&Self::COUNTERS);
        frame[3..11].copy_from_slice(&self.observed.to_le_bytes());
        frame[11..13].copy_from_slice(&usize_low_u16(self.len()).to_le_bytes());
        frame[HEADER_LEN..].fill(0);

        let (slots, _) = frame[HEADER_LEN..].as_chunks_mut::<RECORD_LEN>();
        for (slot, hitter) in slots.iter_mut().zip(self.hitters()) {
            slot[..8].copy_from_slice(&hitter.key.to_le_bytes());
            slot[8..16].copy_from_slice(&hitter.count.to_le_bytes());
            slot[16..].copy_from_slice(&hitter.overestimate.to_le_bytes());
        }
    }

    /// Reads a table written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when it was
    /// written with a different counter count or claims more tracked keys than
    /// `K`.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut table = Self::new();
        table.decode_into(bytes)?;
        Ok(table)
    }

    /// Reads a frame over this table, for a caller that owns one already and
    /// has no room on its stack for a second.
    ///
    /// The table is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let Some(frame) = bytes.get(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        if frame[0] != TAG_TOPK {
            return Err(CodecError::Tag);
        }
        if frame[1..3] != Self::COUNTERS {
            return Err(CodecError::Parameters);
        }
        let tracked = usize::from(u16::from_le_bytes([frame[11], frame[12]]));
        if tracked > K {
            return Err(CodecError::Parameters);
        }

        self.observed = u64::from_le_bytes(read8(&frame[3..11]));
        let (slots, _) = frame[HEADER_LEN..].as_chunks::<RECORD_LEN>();
        for (slot, record) in self.slots.iter_mut().zip(&slots[..tracked]) {
            *slot = Some(Hitter {
                key: u64::from_le_bytes(read8(&record[..8])),
                count: u64::from_le_bytes(read8(&record[8..16])),
                overestimate: u64::from_le_bytes(read8(&record[16..])),
            });
        }
        self.slots[tracked..].fill(None);
        Ok(())
    }
}

impl<T: Copy + Eq, const K: usize> Default for HeavyHitters<T, K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl<T: Copy + Eq, const K: usize> HeavyHitters<T, K> {
    /// Tracked keys, most frequent first.
    #[must_use]
    pub fn ranked(&self) -> alloc::vec::Vec<Hitter<T>> {
        let mut ranked: alloc::vec::Vec<Hitter<T>> = self.hitters().copied().collect();
        ranked.sort_unstable_by_key(|hitter| core::cmp::Reverse(hitter.count));
        ranked
    }
}

#[cfg(test)]
mod tests;
