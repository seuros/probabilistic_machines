//! The crate's only hash. Sketches index by bit slices of a 64-bit value, so
//! they need avalanche behaviour, not cryptography.

const PRIME: u64 = 0x9E37_79B9_7F4A_7C15;

/// `splitmix64`'s finalizer: a bijection on [`u64`].
///
/// Every output bit depends on every input bit, and iterating it yields
/// decorrelated values, which is how the sketches derive one index per row
/// from a single hash.
#[must_use]
pub const fn mix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(PRIME);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hashes bytes with the crate's default seed.
#[must_use]
pub const fn hash64(bytes: &[u8]) -> u64 {
    hash64_with_seed(bytes, 0)
}

/// Hashes bytes, folding eight at a time through [`mix64`].
///
/// Two seeds give two independent hash families, which is what a filter needs
/// when it wants fingerprints uncorrelated with bucket indices.
#[must_use]
pub const fn hash64_with_seed(bytes: &[u8], seed: u64) -> u64 {
    let mut accumulator = seed ^ PRIME;
    // Counts at most `bytes.len()`, which fits a `u64`.
    let mut consumed: u64 = 0;
    let mut rest = bytes;

    while let Some((lane, after)) = rest.split_first_chunk::<8>() {
        accumulator = (accumulator ^ mix64(u64::from_le_bytes(*lane)))
            .rotate_left(27)
            .wrapping_mul(PRIME);
        rest = after;
        consumed += 8;
    }

    // Fewer than eight bytes are left, so `shift` stays below 64.
    let mut tail: u64 = 0;
    let mut shift = 0;
    while let [byte, after @ ..] = rest {
        tail |= (*byte as u64) << shift;
        shift += 8;
        rest = after;
        consumed += 1;
    }

    mix64(mix64(accumulator ^ consumed.wrapping_mul(PRIME)) ^ mix64(tail))
}

pub(crate) const fn u64_low_u16(value: u64) -> u16 {
    let [low, high, ..] = value.to_le_bytes();
    u16::from_le_bytes([low, high])
}

pub(crate) const fn u64_low_u8(value: u64) -> u8 {
    value.to_le_bytes()[0]
}

pub(crate) const fn u32_low_u8(value: u32) -> u8 {
    value.to_le_bytes()[0]
}

pub(crate) const fn usize_low_u8(value: usize) -> u8 {
    value.to_le_bytes()[0]
}

pub(crate) const fn usize_low_u16(value: usize) -> u16 {
    let [low, high, ..] = value.to_le_bytes();
    u16::from_le_bytes([low, high])
}

#[cfg(test)]
mod tests;
