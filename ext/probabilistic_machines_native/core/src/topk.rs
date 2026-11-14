//! Which keys dominate a stream, in `K` counters instead of one per key.

use crate::hash::usize_low_u16;
use crate::{Refusal, TAG_TOPK, open_frame, read8};

const HEADER_LEN: usize = 13;
const RECORD_LEN: usize = 24;
/// Where a record's count and overestimate begin, after its key.
const COUNT_AT: usize = 8;
const OVERESTIMATE_AT: usize = 16;

/// A tracked key, its counter, and how much of that counter it may have
/// inherited from keys evicted before it or from a merged-in stream that
/// tracked it no longer.
///
/// The true count lies in `count - overestimate ..= count`. Every count a table
/// takes is at least one, so a tracked key has occurred at least once and its
/// overestimate stays below its count; [`new`](Self::new) holds a hand-built
/// hitter to that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hitter<T> {
    key: T,
    count: u64,
    overestimate: u64,
}

impl<T> Hitter<T> {
    /// A hitter, or `None` when `overestimate` is not below `count`: the
    /// interval `count - overestimate ..= count` would claim no occurrence.
    #[must_use]
    pub fn new(key: T, count: u64, overestimate: u64) -> Option<Self> {
        if overestimate >= count {
            return None;
        }
        Some(Self {
            key,
            count,
            overestimate,
        })
    }

    /// The tracked key.
    #[must_use]
    pub const fn key(&self) -> &T {
        &self.key
    }

    /// Its counter: at least its true count.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// How much of the counter it may have inherited: the true count is at
    /// least `count - overestimate`.
    #[must_use]
    pub const fn overestimate(&self) -> u64 {
        self.overestimate
    }
}

/// Space-Saving over `K` counters: whatever is frequent ends up tracked.
///
/// After `n` observations every key that occurred more than `n / K` times is in
/// the table, and no tracked key's counter is below its true count. Lookups and
/// insertions scan all `K` counters, which is the trade for needing no heap and
/// no key ordering: keep `K` in the dozens-to-hundreds range.
///
/// A key need only be [`Clone`] and [`Eq`], so owned keys work directly:
/// `HeavyHitters<String, 64>` tracks layer names without interning them first.
/// Cloning happens on insert and on [`merge`](Self::merge), never on a lookup,
/// so a cheap key is worth having but a `String` is not a mistake. Interning to
/// a `u32` id, or hashing with [`hash64`](crate::hash64) the way the Ruby
/// binding does, keeps the table [`Copy`] and the scan cache-friendly.
///
/// Counters are `u64`. An addition or a merge that would carry a counter or
/// the observation count past [`u64::MAX`] is refused with
/// [`Refusal::Overflow`] and changes nothing, so no counter saturates and the
/// bounds above always hold. `K` is at least one; a table of zero counters
/// does not compile.
///
/// ```
/// use probabilistic_machines::HeavyHitters;
///
/// let mut traffic = HeavyHitters::<u32, 8>::new();
/// for request in 0..1_000_u32 {
///     traffic.add(request % 97)?;
/// }
/// for _ in 0..5_000 {
///     traffic.add(7)?;
/// }
///
/// assert_eq!(traffic.observed(), 6_000);
/// assert!(traffic.count(&7) >= 5_000);
/// # Ok::<(), probabilistic_machines::Refusal>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeavyHitters<T, const K: usize> {
    slots: [Option<Hitter<T>>; K],
    observed: u64,
}

impl<T: Clone + Eq, const K: usize> HeavyHitters<T, K> {
    /// Evaluated by [`new`](Self::new), so a zero-counter table is a compile
    /// error rather than one that silently drops every key.
    const AT_LEAST_ONE_COUNTER: () = assert!(K >= 1, "a table needs at least one counter");

    #[must_use]
    pub const fn new() -> Self {
        let () = Self::AT_LEAST_ONE_COUNTER;
        Self {
            slots: [const { None }; K],
            observed: 0,
        }
    }

    /// Observations fed in: the sum of every count added or merged.
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

    /// Records one occurrence of `key`.
    ///
    /// # Errors
    ///
    /// As [`add_count`](Self::add_count).
    pub fn add(&mut self, key: T) -> Result<(), Refusal> {
        self.add_count(key, 1)
    }

    /// Records `count` occurrences of `key`.
    ///
    /// # Errors
    ///
    /// [`Refusal::ZeroCount`] when `count` is zero: it observes nothing, yet
    /// on a full table it would evict the weakest key. [`Refusal::Overflow`]
    /// when the observation count, the key's counter or the evicted floor plus
    /// `count` would pass [`u64::MAX`]. The table is unchanged either way.
    pub fn add_count(&mut self, key: T, count: u64) -> Result<(), Refusal> {
        if count == 0 {
            return Err(Refusal::ZeroCount);
        }
        let Some(observed) = self.observed.checked_add(count) else {
            return Err(Refusal::Overflow);
        };
        let (index, hitter) = self.absorbed(key, count, 0)?;
        self.slots[index] = Some(hitter);
        self.observed = observed;
        Ok(())
    }

    /// Folds another table in. Counts of keys tracked by both are exact sums.
    /// A key only one side tracked may have occurred in the other stream too,
    /// up to that side's eviction floor, so it takes the floor on both its
    /// bounds: the counter still covers the true count and the interval
    /// `count - overestimate ..= count` still holds it.
    ///
    /// # Errors
    ///
    /// [`Refusal::Overflow`] when any counter, overestimate or the observation
    /// count would pass [`u64::MAX`]. The merge is worked out on a copy and
    /// stored only once every step fits, so a refused merge changes nothing.
    pub fn merge(&mut self, other: &Self) -> Result<(), Refusal> {
        let Some(observed) = self.observed.checked_add(other.observed) else {
            return Err(Refusal::Overflow);
        };
        let floor = other.floor();
        let mut merged = self.clone();
        for tracked in merged.slots.iter_mut().flatten() {
            if other.hitters().all(|hitter| hitter.key != tracked.key) {
                let (Some(count), Some(overestimate)) = (
                    tracked.count.checked_add(floor),
                    tracked.overestimate.checked_add(floor),
                ) else {
                    return Err(Refusal::Overflow);
                };
                tracked.count = count;
                tracked.overestimate = overestimate;
            }
        }
        for hitter in other.hitters() {
            let (index, absorbed) =
                merged.absorbed(hitter.key.clone(), hitter.count, hitter.overestimate)?;
            merged.slots[index] = Some(absorbed);
        }
        merged.observed = observed;
        *self = merged;
        Ok(())
    }

    /// How often an untracked key may have occurred: nothing while a counter is
    /// still free, otherwise the weakest counter.
    fn floor(&self) -> u64 {
        if self.slots.iter().any(Option::is_none) {
            return 0;
        }
        self.hitters().map(|hitter| hitter.count).min().unwrap_or(0)
    }

    /// Where `key` goes and what it holds once `count` and `overestimate` are
    /// added: its own slot if tracked, else the first free one, else the
    /// weakest counter's, which it takes over along with that counter as its
    /// floor. Computes without storing, so a caller can refuse first.
    fn absorbed(
        &self,
        key: T,
        count: u64,
        overestimate: u64,
    ) -> Result<(usize, Hitter<T>), Refusal> {
        let mut free = None;
        let mut weakest = 0;
        let mut floor = u64::MAX;
        for (index, slot) in self.slots.iter().enumerate() {
            match slot {
                Some(tracked) if tracked.key == key => {
                    let base = (tracked.count, tracked.overestimate);
                    return Self::raised(index, key, base, count, overestimate);
                }
                Some(tracked) => {
                    if tracked.count < floor {
                        floor = tracked.count;
                        weakest = index;
                    }
                }
                None => {
                    if free.is_none() {
                        free = Some(index);
                    }
                }
            }
        }
        match free {
            Some(index) => Self::raised(index, key, (0, 0), count, overestimate),
            None => Self::raised(weakest, key, (floor, floor), count, overestimate),
        }
    }

    /// A hitter for `key` at `index` holding `base` (count, overestimate)
    /// raised by `count` and `overestimate`.
    fn raised(
        index: usize,
        key: T,
        base: (u64, u64),
        count: u64,
        overestimate: u64,
    ) -> Result<(usize, Hitter<T>), Refusal> {
        let (Some(count), Some(overestimate)) =
            (base.0.checked_add(count), base.1.checked_add(overestimate))
        else {
            return Err(Refusal::Overflow);
        };
        let hitter = Hitter {
            key,
            count,
            overestimate,
        };
        Ok((index, hitter))
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

    encoders!(
        "table",
        "The frame is the tag, `K`, the observation count, the number of tracked",
        "keys and `K` records of key, count and overestimate. Records past the",
        "tracked count are zero."
    );

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[0] = TAG_TOPK;
        frame[1..3].copy_from_slice(&Self::COUNTERS);
        frame[3..11].copy_from_slice(&self.observed.to_le_bytes());
        frame[11..13].copy_from_slice(&usize_low_u16(self.len()).to_le_bytes());
        frame[HEADER_LEN..].fill(0);

        let (slots, _) = frame[HEADER_LEN..].as_chunks_mut::<RECORD_LEN>();
        for (slot, hitter) in slots.iter_mut().zip(self.hitters()) {
            slot[..COUNT_AT].copy_from_slice(&hitter.key.to_le_bytes());
            slot[COUNT_AT..OVERESTIMATE_AT].copy_from_slice(&hitter.count.to_le_bytes());
            slot[OVERESTIMATE_AT..].copy_from_slice(&hitter.overestimate.to_le_bytes());
        }
    }

    /// Reads a table written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with a different counter
    /// count, [`Refusal::Malformed`] when it claims more tracked keys than `K`,
    /// holds records no stream could have left, or has bytes in the slots past
    /// the tracked records, which `encode` always zeroes.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
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
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let frame = open_frame(bytes, Self::ENCODED_LEN, TAG_TOPK)?;
        if frame[1..3] != Self::COUNTERS {
            return Err(Refusal::WrongShape);
        }
        let tracked = usize::from(u16::from_le_bytes([frame[11], frame[12]]));
        if tracked > K {
            return Err(Refusal::Malformed);
        }

        let observed = u64::from_le_bytes(read8(&frame[3..11]));
        let (slots, _) = frame[HEADER_LEN..].as_chunks::<RECORD_LEN>();
        let (records, unused) = slots.split_at(tracked);
        // `encode` zeroes every slot past the tracked records: one frame per
        // state, so anything else there is not a frame this table wrote.
        if unused.as_flattened().iter().any(|byte| *byte != 0)
            || !records_are_coherent(records, observed)
        {
            return Err(Refusal::Malformed);
        }

        self.observed = observed;
        for (slot, record) in self.slots.iter_mut().zip(records) {
            *slot = Some(hitter(record));
        }
        self.slots[tracked..].fill(None);
        Ok(())
    }
}

/// A record as the hitter it stands for.
fn hitter(record: &[u8; RECORD_LEN]) -> Hitter<u64> {
    Hitter {
        key: u64::from_le_bytes(read8(&record[..COUNT_AT])),
        count: u64::from_le_bytes(read8(&record[COUNT_AT..OVERESTIMATE_AT])),
        overestimate: u64::from_le_bytes(read8(&record[OVERESTIMATE_AT..])),
    }
}

/// Whether the records are ones Space-Saving could have tracked: a counter
/// stands for at least `count - overestimate` occurrences, at least one (every
/// count added is) and no more than the observations they came from, and a
/// key holds one counter.
///
/// The counter itself may pass the observation count: a merge lifts a key the
/// other table evicted to that table's eviction floor and widens the
/// overestimate with it, so the two tables' floors can stack above the two
/// streams' length while the occurrences the interval claims stay inside it.
fn records_are_coherent(records: &[[u8; RECORD_LEN]], observed: u64) -> bool {
    records.iter().enumerate().all(|(index, record)| {
        let tracked = hitter(record);
        tracked.overestimate < tracked.count
            && tracked.count - tracked.overestimate <= observed
            && records[..index]
                .iter()
                .all(|earlier| hitter(earlier).key != tracked.key)
    })
}

impl<T: Clone + Eq, const K: usize> Default for HeavyHitters<T, K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone + Eq, const K: usize> HeavyHitters<T, K> {
    /// Tracked keys, most frequent first.
    #[must_use]
    pub fn ranked(&self) -> alloc::vec::Vec<Hitter<T>> {
        let mut ranked: alloc::vec::Vec<Hitter<T>> = self.hitters().cloned().collect();
        ranked.sort_unstable_by_key(|hitter| core::cmp::Reverse(hitter.count));
        ranked
    }
}

#[cfg(test)]
mod tests;
