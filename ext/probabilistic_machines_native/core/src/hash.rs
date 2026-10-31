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
    let mut consumed: u64 = 0;
    let mut cursor = 0;

    while cursor + 8 <= bytes.len() {
        let lane = u64::from_le_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]);
        accumulator = (accumulator ^ mix64(lane))
            .rotate_left(27)
            .wrapping_mul(PRIME);
        cursor += 8;
        consumed += 8;
    }

    let mut tail: u64 = 0;
    let mut shift = 0;
    while cursor < bytes.len() {
        tail |= (bytes[cursor] as u64) << shift;
        shift += 8;
        cursor += 1;
        consumed += 1;
    }

    mix64(accumulator ^ mix64(tail ^ consumed))
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
