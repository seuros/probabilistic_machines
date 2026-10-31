use super::{HEADER_LEN, HeavyHitters, OVERESTIMATE_AT, RECORD_LEN};
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

// A key the source evicted still occurred in its stream, so the merged
// counter used to claim an exact interval that excluded the true count.
#[test]
fn merging_widens_a_key_the_source_evicted() {
    let mut source = HeavyHitters::<u32, 16>::new();
    source.add(9_999);
    for key in 0..16_u32 {
        source.add_count(key, 100);
    }
    assert_eq!(source.count(&9_999), 0);

    let mut table = HeavyHitters::<u32, 16>::new();
    table.add_count(9_999, 1_000);
    table.merge(&source);

    assert_eq!(table.count(&9_999), 1_100);
    for hitter in table.hitters().filter(|hitter| hitter.key == 9_999) {
        assert_eq!(hitter.overestimate, 100);
        assert!(hitter.count - hitter.overestimate <= 1_001);
    }
}

#[test]
fn a_key_both_sides_track_takes_no_floor() {
    let mut source = HeavyHitters::<u32, 2>::new();
    source.add_count(1, 7);
    source.add_count(2, 5);

    let mut table = HeavyHitters::<u32, 2>::new();
    table.add_count(1, 1_000);
    table.merge(&source);

    assert_eq!(table.count(&1), 1_007);
    for hitter in table.hitters().filter(|hitter| hitter.key == 1) {
        assert_eq!(hitter.overestimate, 0);
    }
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

/// Two tables that traded floors until one counter passed the observations it
/// counts, which only merging produces.
fn stacked_floors() -> HeavyHitters<u64, 2> {
    let mut left = HeavyHitters::<u64, 2>::new();
    let mut right = HeavyHitters::<u64, 2>::new();
    right.add(0);
    right.add(2);
    left.add(1);
    left.merge(&right);
    right.merge(&left);
    right.add(0);
    left.merge(&right);
    left
}

/// A table whose weakest counter went to a key the merged-in table tracked
/// without ever seeing it, so the counter is all floor and the interval it
/// claims starts at nothing.
fn pure_floor() -> HeavyHitters<u64, 2> {
    let mut source = HeavyHitters::<u64, 2>::new();
    source.add_count(7, 0);

    let mut table = HeavyHitters::<u64, 2>::new();
    table.add_count(5, 10);
    table.add_count(6, 20);
    table.merge(&source);
    table
}

// A record's count and its overestimate were read with no bound from the
// observation count, so an edited frame described occurrences that never
// happened and merging carried them into a healthy table.
#[test]
fn records_no_stream_could_leave_are_refused() {
    let mut table = HeavyHitters::<u64, 2>::new();
    table.add_count(1, 10);
    table.add_count(2, 10);

    let mut frame = [0_u8; HeavyHitters::<u64, 2>::ENCODED_LEN];
    assert_eq!(table.encode(&mut frame), Ok(frame.len()));

    let mut unobserved = frame;
    unobserved[3] = 0;
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&unobserved),
        Err(CodecError::Parameters)
    );

    let mut inverted = frame;
    inverted[HEADER_LEN + OVERESTIMATE_AT] = u8::MAX;
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&inverted),
        Err(CodecError::Parameters)
    );

    let mut twice = frame;
    twice[HEADER_LEN + RECORD_LEN] = frame[HEADER_LEN];
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&twice),
        Err(CodecError::Parameters)
    );

    let stacked = stacked_floors();
    assert!(
        stacked
            .hitters()
            .any(|hitter| hitter.count > stacked.observed())
    );
    assert_eq!(stacked.encode(&mut frame), Ok(frame.len()));
    assert_eq!(HeavyHitters::<u64, 2>::decode(&frame), Ok(stacked));

    let inherited = pure_floor();
    assert!(
        inherited
            .hitters()
            .any(|hitter| hitter.count > 0 && hitter.overestimate == hitter.count)
    );
    assert_eq!(inherited.encode(&mut frame), Ok(frame.len()));
    assert_eq!(HeavyHitters::<u64, 2>::decode(&frame), Ok(inherited));
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
