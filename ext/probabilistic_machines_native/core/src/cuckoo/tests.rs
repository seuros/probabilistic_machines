use super::{CuckooFilter, Full};
use crate::CodecError;
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

    assert!(filter.remove(b"serial-1"));
    assert!(filter.contains(b"serial-1"));
    assert!(filter.remove(b"serial-1"));
    assert!(!filter.contains(b"serial-1"));
    assert!(filter.is_empty());
    assert!(!filter.remove(b"serial-1"));
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

    assert_eq!(filter.insert(&key(next)), Err(Full));
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
    assert_eq!(
        filter.encode(&mut frame[..short]),
        Err(CodecError::Truncated)
    );
    assert_eq!(
        CuckooFilter::<128>::decode(&frame[..short]),
        Err(CodecError::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(CuckooFilter::<128>::decode(&wrong), Err(CodecError::Tag));

    let mut lying = frame;
    lying[1] = 3;
    assert_eq!(
        CuckooFilter::<128>::decode(&lying),
        Err(CodecError::Parameters)
    );
}
