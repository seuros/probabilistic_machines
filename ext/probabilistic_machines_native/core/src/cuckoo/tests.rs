use super::{CuckooFilter, Removing};
use crate::Refusal;
use crate::hash::mix64;

fn key(index: u32) -> [u8; 8] {
    mix64(u64::from(index)).to_le_bytes()
}

#[test]
fn an_inserted_item_is_always_found() {
    let mut filter = CuckooFilter::<1024>::new();
    for index in 0..2_000 {
        assert_eq!(filter.insert(&key(index)), Ok(()), "{index}");
    }
    for index in 0..2_000 {
        assert!(filter.contains(&key(index)), "{index}");
    }
    assert_eq!(filter.len(), 2_000);
}

#[test]
fn removal_forgets_exactly_one_copy() {
    let mut filter = CuckooFilter::<256>::new();
    assert_eq!(filter.insert(b"serial-1"), Ok(()));
    assert_eq!(filter.insert(b"serial-1"), Ok(()));
    assert_eq!(filter.len(), 2);

    assert_eq!(filter.remove(b"serial-1"), Removing::Cleared);
    assert!(filter.contains(b"serial-1"));
    assert_eq!(filter.remove(b"serial-1"), Removing::Cleared);
    assert!(!filter.contains(b"serial-1"));
    assert!(filter.is_empty());
}

// Removing an item whose fingerprint is in neither bucket is an honoured
// repeat the filter can prove (no false negatives): it changes nothing.
#[test]
fn removing_what_is_not_held_is_honoured_and_changes_nothing() {
    let mut filter = CuckooFilter::<256>::new();
    assert_eq!(filter.insert(b"serial-1"), Ok(()));
    assert_eq!(filter.remove(b"serial-1"), Removing::Cleared);
    let before = filter.clone();

    assert_eq!(filter.remove(b"serial-1"), Removing::NotHeld);
    assert_eq!(filter.remove_hash(42), Removing::NotHeld);
    assert_eq!(filter, before);
}

// The documented caveat: a never-inserted item that shares a fingerprint and a
// bucket with a stored one is cleared in its place, and the stored item is then
// denied. Cleared is not proof of membership.
#[test]
fn clearing_a_colliding_item_takes_the_stored_fingerprint() {
    let mut filter = CuckooFilter::<2>::new();
    assert_eq!(filter.insert_hash(0), Ok(()));
    let collider = (1..u64::MAX)
        .find(|&hash| hash != 0 && filter.contains_hash(hash))
        .unwrap_or(0);
    assert_ne!(collider, 0);

    assert_eq!(filter.remove_hash(collider), Removing::Cleared);
    assert!(!filter.contains_hash(0));
}

#[test]
fn false_positives_stay_near_the_fingerprint_bound() {
    let mut filter = CuckooFilter::<1024>::new();
    for index in 0..2_048 {
        assert_eq!(filter.insert(&key(index)), Ok(()));
    }

    let mut hits = 0_u32;
    for index in 1_000_000..1_050_000 {
        if filter.contains(&key(index)) {
            hits += 1;
        }
    }

    let rate = f64::from(hits) / 50_000.0;
    assert!(rate < 0.05, "{rate}");
}

#[test]
fn a_failed_insert_leaves_every_other_item_in_place() {
    let mut filter = CuckooFilter::<64>::new();
    let mut next = 0_u32;
    while filter.insert(&key(next)) == Ok(()) {
        next += 1;
        assert!(next < 1_000);
    }

    let stored = filter.len();
    assert!(
        stored * 10 >= CuckooFilter::<64>::CAPACITY * 9,
        "{stored} of {}",
        CuckooFilter::<64>::CAPACITY
    );

    let before = filter.clone();
    assert_eq!(filter.insert(&key(next)), Err(Refusal::Full));
    assert_eq!(filter, before);
    assert_eq!(filter.len(), stored);
    for index in 0..next {
        assert!(filter.contains(&key(index)), "{index}");
    }
}

#[test]
fn eviction_keeps_both_buckets_reachable() {
    let mut filter = CuckooFilter::<2>::new();
    for index in 0..8 {
        assert_eq!(filter.insert(&key(index)), Ok(()), "{index}");
    }
    for index in 0..8 {
        assert!(filter.contains(&key(index)), "{index}");
    }
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut filter = CuckooFilter::<128>::new();
    for index in 0..300 {
        assert_eq!(filter.insert(&key(index)), Ok(()));
    }

    let mut frame = [0_u8; CuckooFilter::<128>::ENCODED_LEN];
    assert_eq!(filter.encode(&mut frame), Ok(2 + 128 * 4));
    #[cfg(feature = "alloc")]
    assert_eq!(&*filter.encoded(), &frame[..]);
    assert_eq!(CuckooFilter::<128>::decode(&frame), Ok(filter.clone()));

    let short = frame.len() - 1;
    assert_eq!(filter.encode(&mut frame[..short]), Err(Refusal::Truncated));
    assert_eq!(
        CuckooFilter::<128>::decode(&frame[..short]),
        Err(Refusal::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(
        CuckooFilter::<128>::decode(&wrong),
        Err(Refusal::WrongStructure)
    );

    let mut lying = frame;
    lying[1] = 3;
    assert_eq!(
        CuckooFilter::<128>::decode(&lying),
        Err(Refusal::WrongShape)
    );
}

// The reference model: a multiset of hashes. Every inserted hash is found,
// the stored count matches the model, and a refused insert leaves the filter
// equal to a clone taken before it.
#[test]
fn a_random_workload_keeps_every_item_and_refusals_change_nothing() {
    let mut filter = CuckooFilter::<16>::new();
    let mut held = std::vec::Vec::new();
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    for _ in 0..3_000 {
        state = mix64(state);
        let hash = mix64(state % 97);
        if state >> 62 == 0 && !held.is_empty() {
            let at = usize::from(crate::hash::u64_low_u16(state >> 8)) % held.len();
            let hash = held.swap_remove(at);
            assert_eq!(filter.remove_hash(hash), Removing::Cleared);
        } else {
            let before = filter.clone();
            match filter.insert_hash(hash) {
                Ok(()) => held.push(hash),
                Err(refused) => {
                    assert_eq!(refused, Refusal::Full);
                    assert_eq!(filter, before);
                }
            }
        }
        assert_eq!(filter.len(), held.len());
        for &hash in &held {
            assert!(filter.contains_hash(hash));
        }
    }
}
