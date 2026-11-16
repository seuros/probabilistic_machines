//! Approximate membership with deletions, which a Bloom filter cannot do.

use crate::hash::{hash64, mix64, u32_low_u8, u64_low_u8, u64_low_u16};
use crate::{Refusal, TAG_CUCKOO, open_frame};

const SLOTS: usize = 4;
const EMPTY: u8 = 0;
const MAX_KICKS: usize = 64;
/// Tag and bucket-count bits.
const HEADER_LEN: usize = 2;

/// What [`CuckooFilter::remove`] did. Both answers are honoured, and both are
/// about the item's fingerprint, which is all the filter stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Removing {
    /// One copy of the item's fingerprint was cleared from one of its two
    /// buckets. Not a proof the item was inserted: the fingerprint may have
    /// been another item's. See the caveat on [`CuckooFilter::remove`].
    Cleared,
    /// Neither of the item's buckets holds its fingerprint, so the item is not
    /// in the filter: what the call asked for already holds. Nothing changed.
    NotHeld,
}

/// Answers "have I seen this?" in `B * 4` bytes, and unlike a Bloom filter it
/// can forget.
///
/// `B` is the bucket count, a power of two up to 65536; each bucket holds four
/// 8-bit fingerprints, so the filter stores up to `4 * B` items in `4 * B`
/// bytes. [`contains`](Self::contains) never says no to an item that was
/// inserted; it says yes to an item that was not with probability at most
/// `2 * 4 / 255`, about 3%.
///
/// The filter is a multiset of fingerprints: inserting an item twice stores
/// two copies, and it takes two removals to forget it. It cannot tell a repeat
/// from a collision, so an insert is never answered as a duplicate.
///
/// ```
/// use probabilistic_machines::{CuckooFilter, Removing};
///
/// let mut seen = CuckooFilter::<1024>::new();
/// assert_eq!(seen.insert(b"device-9"), Ok(()));
/// assert!(seen.contains(b"device-9"));
///
/// assert_eq!(seen.remove(b"device-9"), Removing::Cleared);
/// assert!(!seen.contains(b"device-9"));
/// assert_eq!(seen.remove(b"device-9"), Removing::NotHeld);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuckooFilter<const B: usize> {
    buckets: [[u8; SLOTS]; B],
    items: usize,
}

impl<const B: usize> CuckooFilter<B> {
    const BUCKET_MASK: usize = {
        assert!(B.is_power_of_two(), "bucket count must be a power of two");
        assert!(B >= 2, "bucket count must be at least 2");
        assert!(B <= 1 << 16, "bucket count must be at most 65536");
        B - 1
    };

    /// Fingerprint slots in the table.
    pub const CAPACITY: usize = B * SLOTS;

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = HEADER_LEN + B * SLOTS;

    /// An empty filter.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buckets: [[EMPTY; SLOTS]; B],
            items: 0,
        }
    }

    /// Items currently stored.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items
    }

    /// Whether the filter holds no item.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items == 0
    }

    /// Stores one copy of the item's fingerprint.
    ///
    /// # Errors
    ///
    /// [`Refusal::Full`] when the item's two buckets and 64 evictions all
    /// failed to find a slot: expect that past about 95% of
    /// [`CAPACITY`](Self::CAPACITY). The filter is left as it was.
    pub fn insert(&mut self, item: &[u8]) -> Result<(), Refusal> {
        self.insert_hash(hash64(item))
    }

    /// Inserts an item the caller already hashed.
    ///
    /// # Errors
    ///
    /// [`Refusal::Full`] when the item's two buckets and 64 evictions all
    /// failed to find a slot. The filter is left as it was.
    pub fn insert_hash(&mut self, hash: u64) -> Result<(), Refusal> {
        let fingerprint = fingerprint(hash);
        let primary = Self::bucket(hash);
        if self.place(primary, fingerprint) {
            return Ok(());
        }
        let alternate = Self::alternate(primary, fingerprint);
        if self.place(alternate, fingerprint) {
            return Ok(());
        }

        let mut path = [(0_usize, 0_usize); MAX_KICKS];
        let mut kicked = 0;
        let mut index = alternate;
        let mut carried = fingerprint;
        let mut walk = mix64(hash);

        for step in &mut path {
            walk = mix64(walk);
            let slot = usize::from(u64_low_u16(walk)) % SLOTS;
            let Some(cell) = self.cell_mut(index, slot) else {
                debug_assert!(false, "a bucket is masked below B and a slot below SLOTS");
                // Undone below with the kicks before it: refused, unchanged.
                break;
            };
            core::mem::swap(&mut carried, cell);
            *step = (index, slot);
            kicked += 1;
            index = Self::alternate(index, carried);
            if self.place(index, carried) {
                return Ok(());
            }
        }

        // Every cell on the path was found on the way out, so each is found
        // again on the way back.
        for &(index, slot) in path.iter().take(kicked).rev() {
            if let Some(cell) = self.cell_mut(index, slot) {
                core::mem::swap(&mut carried, cell);
            }
        }
        Err(Refusal::Full)
    }

    /// Whether `item` may be held: no false negatives, rare false positives.
    #[must_use]
    pub fn contains(&self, item: &[u8]) -> bool {
        self.contains_hash(hash64(item))
    }

    /// Queries an item the caller already hashed.
    #[must_use]
    pub fn contains_hash(&self, hash: u64) -> bool {
        let fingerprint = fingerprint(hash);
        let primary = Self::bucket(hash);
        let alternate = Self::alternate(primary, fingerprint);
        // Both buckets are masked below `B`, so both lookups land.
        let holds = |index: usize| {
            self.buckets
                .get(index)
                .is_some_and(|bucket| bucket.contains(&fingerprint))
        };
        holds(primary) || holds(alternate)
    }

    /// Drops one copy of an item.
    ///
    /// [`Removing::NotHeld`] is a proof: a filter has no false negatives, so
    /// an item whose fingerprint is in neither of its buckets was not there,
    /// and nothing changed. [`Removing::Cleared`] is not one. A fingerprint is
    /// eight bits, so about 3% of items that were never inserted match a
    /// stored one (the same false positive [`contains`](Self::contains) has),
    /// and removing such an item clears the fingerprint of the item it
    /// collides with, which the filter then denies having seen. Only remove
    /// what was inserted.
    pub fn remove(&mut self, item: &[u8]) -> Removing {
        self.remove_hash(hash64(item))
    }

    /// Drops an item the caller already hashed. As [`remove`](Self::remove).
    pub fn remove_hash(&mut self, hash: u64) -> Removing {
        let fingerprint = fingerprint(hash);
        let primary = Self::bucket(hash);
        let alternate = Self::alternate(primary, fingerprint);
        if self.clear(primary, fingerprint) || self.clear(alternate, fingerprint) {
            Removing::Cleared
        } else {
            Removing::NotHeld
        }
    }

    encoders!("filter");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, header: &mut [u8; HEADER_LEN], body: &mut [u8]) {
        *header = [TAG_CUCKOO, u32_low_u8(B.trailing_zeros())];

        let fingerprints = self.buckets.iter().flatten();
        for (slot, value) in body.iter_mut().zip(fingerprints) {
            *slot = *value;
        }
    }

    /// Reads a filter written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with a different bucket
    /// count. Every table of fingerprints is one some inserts could leave, so
    /// this decoder never answers [`Refusal::Malformed`].
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        let mut filter = Self::new();
        filter.decode_into(bytes)?;
        Ok(filter)
    }

    /// Reads a frame over this filter, for a caller that owns one already and
    /// has no room on its stack for a second.
    ///
    /// The filter is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let (&[_, bits], body) = open_frame::<HEADER_LEN>(bytes, Self::ENCODED_LEN, TAG_CUCKOO)?;
        if bits != u32_low_u8(B.trailing_zeros()) {
            return Err(Refusal::WrongShape);
        }

        self.items = 0;
        let fingerprints = self.buckets.iter_mut().flatten();
        for (value, byte) in fingerprints.zip(body) {
            *value = *byte;
            if *byte != EMPTY {
                self.items += 1;
            }
        }
        Ok(())
    }

    fn bucket(hash: u64) -> usize {
        usize::from(u64_low_u16(hash)) & Self::BUCKET_MASK
    }

    fn alternate(index: usize, fingerprint: u8) -> usize {
        let twist = usize::from(u64_low_u16(mix64(u64::from(fingerprint)))) & Self::BUCKET_MASK;
        index ^ twist
    }

    /// Slot `slot` of bucket `index`; `None` past `B` or `SLOTS`.
    fn cell_mut(&mut self, index: usize, slot: usize) -> Option<&mut u8> {
        self.buckets.get_mut(index)?.get_mut(slot)
    }

    /// Fills the first empty slot of bucket `index`. Every bucket index is
    /// masked below `B`, so the bucket is always found.
    fn place(&mut self, index: usize, fingerprint: u8) -> bool {
        let Some(bucket) = self.buckets.get_mut(index) else {
            return false;
        };
        for slot in bucket {
            if *slot == EMPTY {
                *slot = fingerprint;
                self.items += 1;
                return true;
            }
        }
        false
    }

    /// Clears one copy of `fingerprint` from bucket `index`, found as in
    /// [`place`](Self::place).
    fn clear(&mut self, index: usize, fingerprint: u8) -> bool {
        let Some(bucket) = self.buckets.get_mut(index) else {
            return false;
        };
        for slot in bucket {
            if *slot == fingerprint {
                *slot = EMPTY;
                // `items` counts the non-empty slots and this one held a
                // fingerprint (never `EMPTY`), so it is at least one.
                self.items -= 1;
                return true;
            }
        }
        false
    }
}

impl<const B: usize> Default for CuckooFilter<B> {
    fn default() -> Self {
        Self::new()
    }
}

fn fingerprint(hash: u64) -> u8 {
    u64_low_u8(mix64(hash)).max(1)
}

#[cfg(test)]
mod tests;
