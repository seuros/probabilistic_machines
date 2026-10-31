//! Which keys dominate a stream, in `K` counters instead of one per key.

/// A tracked key, its counter, and how much of that counter it may have
/// inherited from keys evicted before it.
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

    /// Folds another table in. Counts of keys tracked by both are exact sums;
    /// a key only one side tracked keeps that side's bounds, so the merged
    /// table over-reports no more than its inputs did.
    pub fn merge(&mut self, other: &Self) {
        for hitter in other.hitters() {
            self.absorb(hitter.key, hitter.count, hitter.overestimate);
        }
        self.observed = self.observed.saturating_add(other.observed);
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
