use super::HeavyHitters;
use crate::CodecError;

#[test]
fn a_key_above_one_over_k_is_always_tracked() {
    let mut table = HeavyHitters::<u32, 16>::new();
    for round in 0..10_000_u32 {
        table.add(round % 500);
        if round % 3 == 0 {
            table.add(7);
        }
    }

    assert_eq!(table.observed(), 10_000 + 3_334);
    assert!(table.count(&7) >= 3_334);
    assert_eq!(table.top().map(|hitter| hitter.key), Some(7));
}

#[test]
fn counters_bracket_the_truth() {
    let mut table = HeavyHitters::<u32, 8>::new();
    for round in 0..4_000_u32 {
        table.add(round % 200);
    }
    for _ in 0..1_000 {
        table.add(42);
    }

    for hitter in table.hitters() {
        let truth = if hitter.key == 42 { 1_020 } else { 20 };
        assert!(hitter.count >= truth, "{hitter:?}");
        assert!(hitter.count - hitter.overestimate <= truth, "{hitter:?}");
    }
}

#[test]
fn the_table_never_outgrows_its_counters() {
    let mut table = HeavyHitters::<u32, 4>::new();
    for key in 0..50_u32 {
        table.add(key);
    }

    assert_eq!(table.len(), 4);
    assert_eq!(table.observed(), 50);
}

#[test]
fn an_empty_table_reports_nothing() {
    let table = HeavyHitters::<u64, 4>::new();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
    assert_eq!(table.count(&1), 0);
    assert_eq!(table.top(), None);
}

#[test]
fn weighted_adds_match_repeated_ones() {
    let mut one_at_a_time = HeavyHitters::<u8, 4>::new();
    for _ in 0..100 {
        one_at_a_time.add(9);
    }
    let mut in_bulk = HeavyHitters::<u8, 4>::new();
    in_bulk.add_count(9, 100);

    assert_eq!(one_at_a_time, in_bulk);
}

#[test]
fn merging_sums_what_both_sides_tracked() {
    let mut left = HeavyHitters::<u32, 8>::new();
    let mut right = HeavyHitters::<u32, 8>::new();
    left.add_count(1, 500);
    left.add_count(2, 10);
    right.add_count(1, 300);
    right.add_count(3, 20);

    left.merge(&right);
    assert_eq!(left.count(&1), 800);
    assert_eq!(left.count(&2), 10);
    assert_eq!(left.count(&3), 20);
    assert_eq!(left.observed(), 830);
}

#[test]
fn counters_saturate_instead_of_wrapping() {
    let mut table = HeavyHitters::<u8, 2>::new();
    table.add_count(1, u64::MAX);
    table.add_count(1, u64::MAX);

    assert_eq!(table.count(&1), u64::MAX);
    assert_eq!(table.observed(), u64::MAX);
}

#[cfg(feature = "alloc")]
#[test]
fn ranking_puts_the_loudest_first() {
    let mut table = HeavyHitters::<u32, 8>::new();
    table.add_count(1, 5);
    table.add_count(2, 50);
    table.add_count(3, 500);

    let ranked = table.ranked();
    let keys: alloc::vec::Vec<u32> = ranked.iter().map(|hitter| hitter.key).collect();
    assert_eq!(keys, [3, 2, 1]);
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut table = HeavyHitters::<u64, 8>::new();
    for key in 0..20_u64 {
        table.add_count(key, key * 10);
    }

    let mut frame = [0_u8; HeavyHitters::<u64, 8>::ENCODED_LEN];
    assert_eq!(table.encode(&mut frame), Ok(13 + 8 * 24));
    #[cfg(feature = "alloc")]
    assert_eq!(&*table.encoded(), &frame[..]);
    assert_eq!(HeavyHitters::<u64, 8>::decode(&frame), Ok(table.clone()));

    let short = frame.len() - 1;
    assert_eq!(
        table.encode(&mut frame[..short]),
        Err(CodecError::Truncated)
    );
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&frame[..short]),
        Err(CodecError::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(HeavyHitters::<u64, 8>::decode(&wrong), Err(CodecError::Tag));

    let mut lying = frame;
    lying[1] = 4;
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&lying),
        Err(CodecError::Parameters)
    );

    let mut overfull = frame;
    overfull[11] = 9;
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&overfull),
        Err(CodecError::Parameters)
    );
}

#[cfg(feature = "alloc")]
#[test]
fn an_empty_table_round_trips() {
    let table = HeavyHitters::<u64, 4>::new();
    assert_eq!(HeavyHitters::<u64, 4>::decode(&table.encoded()), Ok(table));
}

#[test]
fn a_frame_from_another_structure_is_refused() {
    let counters = crate::CountMinSketch::<4, 2>::new();
    let mut frame = [0_u8; HeavyHitters::<u64, 8>::ENCODED_LEN];
    assert_eq!(counters.encode(&mut frame), Ok(12 + 4 * 2 * 4));
    assert_eq!(HeavyHitters::<u64, 8>::decode(&frame), Err(CodecError::Tag));
}
