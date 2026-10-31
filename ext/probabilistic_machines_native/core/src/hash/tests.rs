use super::{hash64, hash64_with_seed, mix64, u64_low_u16};

#[test]
fn mixing_is_injective_over_a_long_run() {
    let mut seen = [0_u64; 1024];
    let mut value = 0_u64;
    for slot in &mut seen {
        *slot = mix64(value);
        value = value.wrapping_add(0x1234_5678_9ABC_DEF1);
    }

    seen.sort_unstable();
    assert!(seen.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn length_and_order_both_change_the_hash() {
    assert_ne!(hash64(b""), hash64(b"\0"));
    assert_ne!(hash64(b"\0"), hash64(b"\0\0"));
    assert_ne!(hash64(b"ab"), hash64(b"ba"));
    assert_ne!(hash64(b"abcdefgh"), hash64(b"abcdefgh\0"));
}

#[test]
fn hashes_stay_distinct_across_the_eight_byte_lane_boundary() {
    let filler = [b'x'; 64];
    let mut hashes = [0_u64; 65];
    for (length, slot) in hashes.iter_mut().enumerate() {
        *slot = hash64(&filler[..length]);
    }

    hashes.sort_unstable();
    assert!(hashes.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn seeds_give_independent_families() {
    assert_ne!(hash64(b"key"), hash64_with_seed(b"key", 1));
    assert_ne!(hash64_with_seed(b"key", 1), hash64_with_seed(b"key", 2));
}

#[test]
fn low_bits_spread_evenly_enough_to_index_a_sketch() {
    let mut buckets = [0_u32; 1024];
    for key in 0_u32..102_400 {
        let index = usize::from(u64_low_u16(hash64(&key.to_le_bytes()))) & 1023;
        buckets[index] += 1;
    }

    let (smallest, largest) = extremes(&buckets);
    assert!(smallest > 50, "{smallest}");
    assert!(largest < 160, "{largest}");
}

#[test]
fn high_bits_spread_evenly_too() {
    let mut buckets = [0_u32; 256];
    for key in 0_u64..25_600 {
        let index = usize::from(u64_low_u16(hash64(&key.to_le_bytes()) >> 56));
        buckets[index] += 1;
    }

    let (smallest, largest) = extremes(&buckets);
    assert!(smallest > 50, "{smallest}");
    assert!(largest < 160, "{largest}");
}

fn extremes(buckets: &[u32]) -> (u32, u32) {
    buckets
        .iter()
        .fold((u32::MAX, 0), |(smallest, largest), &count| {
            (smallest.min(count), largest.max(count))
        })
}

// A zero-padded tail XORed with the length let "a" and "b\0" cancel to the
// same value.
#[test]
fn a_short_key_does_not_collide_with_a_longer_zero_padded_one() {
    assert_ne!(hash64(b"a"), hash64(b"b\0"));
    assert_ne!(hash64(b"b"), hash64(b"c\0"));
    assert_ne!(hash64(b"a"), hash64(b"c\0\0"));
    assert_ne!(hash64(b""), hash64(b"\0"));
}
