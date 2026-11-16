use super::Hitter;
use super::{FIELD_LEN, HEADER_LEN, HeavyHitters, RECORD_LEN};
use crate::Refusal;
use crate::hash::mix64;
use matryona_machines::testing::refuses;

/// Where a record's overestimate begins, after its key and count.
const OVERESTIMATE_AT: usize = 2 * FIELD_LEN;

#[test]
fn a_key_above_one_over_k_is_always_tracked() {
    let mut table = HeavyHitters::<u32, 16>::new();
    for round in 0..10_000_u32 {
        assert_eq!(table.add(round % 500), Ok(()));
        if round % 3 == 0 {
            assert_eq!(table.add(7), Ok(()));
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
        assert_eq!(table.add(round % 200), Ok(()));
    }
    for _ in 0..1_000 {
        assert_eq!(table.add(42), Ok(()));
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
        assert_eq!(table.add(key), Ok(()));
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
        assert_eq!(one_at_a_time.add(9), Ok(()));
    }
    let mut in_bulk = HeavyHitters::<u8, 4>::new();
    assert_eq!(in_bulk.add_count(9, 100), Ok(()));

    assert_eq!(one_at_a_time, in_bulk);
}

#[test]
fn merging_sums_what_both_sides_tracked() {
    let mut left = HeavyHitters::<u32, 8>::new();
    let mut right = HeavyHitters::<u32, 8>::new();
    assert_eq!(left.add_count(1, 500), Ok(()));
    assert_eq!(left.add_count(2, 10), Ok(()));
    assert_eq!(right.add_count(1, 300), Ok(()));
    assert_eq!(right.add_count(3, 20), Ok(()));

    assert_eq!(left.merge(&right), Ok(()));
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
    assert_eq!(source.add(9_999), Ok(()));
    for key in 0..16_u32 {
        assert_eq!(source.add_count(key, 100), Ok(()));
    }
    assert_eq!(source.count(&9_999), 0);

    let mut table = HeavyHitters::<u32, 16>::new();
    assert_eq!(table.add_count(9_999, 1_000), Ok(()));
    assert_eq!(table.merge(&source), Ok(()));

    assert_eq!(table.count(&9_999), 1_100);
    for hitter in table.hitters().filter(|hitter| hitter.key == 9_999) {
        assert_eq!(hitter.overestimate, 100);
        assert!(hitter.count - hitter.overestimate <= 1_001);
    }
}

#[test]
fn a_key_both_sides_track_takes_no_floor() {
    let mut source = HeavyHitters::<u32, 2>::new();
    assert_eq!(source.add_count(1, 7), Ok(()));
    assert_eq!(source.add_count(2, 5), Ok(()));

    let mut table = HeavyHitters::<u32, 2>::new();
    assert_eq!(table.add_count(1, 1_000), Ok(()));
    assert_eq!(table.merge(&source), Ok(()));

    assert_eq!(table.count(&1), 1_007);
    for hitter in table.hitters().filter(|hitter| hitter.key == 1) {
        assert_eq!(hitter.overestimate, 0);
    }
}

// Saturating silently stopped the counter below the true count, so it no
// longer bounded it from above. Overflow is now refused and changes nothing.
#[test]
fn an_addition_past_u64_is_refused_and_changes_nothing() {
    let mut table = HeavyHitters::<u8, 2>::new();
    assert_eq!(table.add_count(1, u64::MAX), Ok(()));

    refuses(&mut table, Refusal::Overflow, |t| t.add_count(1, 1));
    refuses(&mut table, Refusal::Overflow, |t| t.add_count(2, 1));
    assert_eq!(table.count(&1), u64::MAX);
}

#[test]
fn an_eviction_past_u64_is_refused_and_changes_nothing() {
    let mut table = HeavyHitters::<u8, 1>::new();
    assert_eq!(table.add_count(1, u64::MAX - 1), Ok(()));
    let mut other = HeavyHitters::<u8, 1>::new();
    assert_eq!(other.add_count(2, 2), Ok(()));

    refuses(&mut table, Refusal::Overflow, |t| t.merge(&other));
}

#[test]
fn a_merge_whose_floor_overflows_is_refused_and_changes_nothing() {
    let mut table = HeavyHitters::<u8, 2>::new();
    assert_eq!(table.add_count(1, u64::MAX / 2), Ok(()));
    let mut other = HeavyHitters::<u8, 2>::new();
    assert_eq!(other.add_count(2, u64::MAX / 4), Ok(()));
    assert_eq!(other.add_count(3, u64::MAX / 4 + 2), Ok(()));
    let before = table.clone();
    let mut doubled = table.clone();
    assert_eq!(doubled.add_count(1, u64::MAX / 2), Ok(()));

    refuses(&mut doubled, Refusal::Overflow, |d| d.merge(&other));
    assert_eq!(table, before);
}

// A zero count used to evict the weakest key of a full table for a key that
// was never seen.
#[test]
fn a_zero_count_is_refused_and_evicts_nothing() {
    let mut table = HeavyHitters::<u8, 2>::new();
    assert_eq!(table.add_count(1, 3), Ok(()));
    assert_eq!(table.add_count(2, 4), Ok(()));

    refuses(&mut table, Refusal::ZeroCount, |t| t.add_count(3, 0));
}

#[test]
fn a_hand_built_hitter_claims_at_least_one_occurrence() {
    assert_eq!(
        Hitter::new(7_u8, 10, 9),
        Some(Hitter {
            key: 7,
            count: 10,
            overestimate: 9
        })
    );
    assert_eq!(Hitter::new(7_u8, 10, 10), None);
    assert_eq!(Hitter::new(7_u8, 0, 0), None);
}

// The reference model: exact counts per key. After every call every tracked
// counter brackets the truth, every key above observed / K is tracked, and a
// refusal leaves the table equal to a clone taken before the call.
#[test]
fn a_random_stream_keeps_the_bounds_and_refusals_change_nothing() {
    let mut table = HeavyHitters::<u8, 6>::new();
    let mut truth = [0_u64; 40];
    let mut state = 0xD1B5_4A32_D192_ED03_u64;
    for _ in 0..4_000 {
        state = mix64(state);
        let key = crate::hash::u64_low_u8(state) % 40;
        let count = match state >> 59 {
            0 => 0,
            1 => u64::MAX / 8,
            _ => (state >> 20) % 50,
        };
        let before = table.clone();
        match table.add_count(key, count) {
            Ok(()) => truth[usize::from(key)] += count,
            Err(refused) => {
                let expected = if count == 0 {
                    Refusal::ZeroCount
                } else {
                    Refusal::Overflow
                };
                assert_eq!(refused, expected);
                assert_eq!(table, before);
            }
        }
        assert_eq!(table.observed(), truth.iter().sum::<u64>());
        for hitter in table.hitters() {
            let exact = truth[usize::from(hitter.key)];
            assert!(hitter.count >= exact, "{hitter:?} {exact}");
            assert!(hitter.count - hitter.overestimate <= exact, "{hitter:?}");
        }
        for (key, &exact) in (0_u8..).zip(&truth) {
            if exact > table.observed() / 6 {
                assert!(table.count(&key) >= exact, "{key}");
            }
        }
    }
}

#[cfg(feature = "alloc")]
#[test]
fn ranking_puts_the_loudest_first() {
    let mut table = HeavyHitters::<u32, 8>::new();
    assert_eq!(table.add_count(1, 5), Ok(()));
    assert_eq!(table.add_count(2, 50), Ok(()));
    assert_eq!(table.add_count(3, 500), Ok(()));

    let ranked = table.ranked();
    let keys: alloc::vec::Vec<u32> = ranked.iter().map(|hitter| hitter.key).collect();
    assert_eq!(keys, [3, 2, 1]);
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut table = HeavyHitters::<u64, 8>::new();
    for key in 0..20_u64 {
        assert_eq!(table.add_count(key, key * 10 + 1), Ok(()));
    }

    let mut frame = [0_u8; HeavyHitters::<u64, 8>::ENCODED_LEN];
    assert_eq!(table.encode(&mut frame), Ok(13 + 8 * 24));
    #[cfg(feature = "alloc")]
    assert_eq!(&*table.encoded(), &frame[..]);
    assert_eq!(HeavyHitters::<u64, 8>::decode(&frame), Ok(table.clone()));
    let empty = HeavyHitters::<u64, 8>::new();
    let mut blank = [0_u8; HeavyHitters::<u64, 8>::ENCODED_LEN];
    assert_eq!(empty.encode(&mut blank), Ok(blank.len()));
    assert_eq!(HeavyHitters::<u64, 8>::decode(&blank), Ok(empty));

    let short = frame.len() - 1;
    assert_eq!(table.encode(&mut frame[..short]), Err(Refusal::Truncated));
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&frame[..short]),
        Err(Refusal::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&wrong),
        Err(Refusal::WrongStructure)
    );

    let mut lying = frame;
    lying[1] = 4;
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&lying),
        Err(Refusal::WrongShape)
    );

    let mut overfull = frame;
    overfull[11] = 9;
    assert_eq!(
        HeavyHitters::<u64, 8>::decode(&overfull),
        Err(Refusal::Malformed)
    );
}

/// Two tables that traded floors until one counter passed the observations it
/// counts, which only merging produces.
fn stacked_floors() -> HeavyHitters<u64, 2> {
    let mut left = HeavyHitters::<u64, 2>::new();
    let mut right = HeavyHitters::<u64, 2>::new();
    assert_eq!(right.add(0), Ok(()));
    assert_eq!(right.add(2), Ok(()));
    assert_eq!(left.add(1), Ok(()));
    assert_eq!(left.merge(&right), Ok(()));
    assert_eq!(right.merge(&left), Ok(()));
    assert_eq!(right.add(0), Ok(()));
    assert_eq!(left.merge(&right), Ok(()));
    left
}

// A record's count and its overestimate were read with no bound from the
// observation count, so an edited frame described occurrences that never
// happened and merging carried them into a healthy table.
#[test]
fn records_no_stream_could_leave_are_refused() {
    let mut table = HeavyHitters::<u64, 2>::new();
    assert_eq!(table.add_count(1, 10), Ok(()));
    assert_eq!(table.add_count(2, 10), Ok(()));

    let mut frame = [0_u8; HeavyHitters::<u64, 2>::ENCODED_LEN];
    assert_eq!(table.encode(&mut frame), Ok(frame.len()));

    let mut unobserved = frame;
    unobserved[3] = 0;
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&unobserved),
        Err(Refusal::Malformed)
    );

    let mut inverted = frame;
    inverted[HEADER_LEN + OVERESTIMATE_AT] = u8::MAX;
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&inverted),
        Err(Refusal::Malformed)
    );

    let mut twice = frame;
    twice[HEADER_LEN + RECORD_LEN] = frame[HEADER_LEN];
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&twice),
        Err(Refusal::Malformed)
    );

    let stacked = stacked_floors();
    assert!(
        stacked
            .hitters()
            .any(|hitter| hitter.count > stacked.observed())
    );
    assert_eq!(stacked.encode(&mut frame), Ok(frame.len()));
    assert_eq!(HeavyHitters::<u64, 2>::decode(&frame), Ok(stacked));

    // A counter that is all floor claims no occurrence of its key. Only a zero
    // count produced one, and a zero count is now refused.
    let mut table = HeavyHitters::<u64, 2>::new();
    assert_eq!(table.add_count(5, 10), Ok(()));
    assert_eq!(table.encode(&mut frame), Ok(frame.len()));
    let mut all_floor = frame;
    all_floor[HEADER_LEN + OVERESTIMATE_AT] = 10;
    assert_eq!(
        HeavyHitters::<u64, 2>::decode(&all_floor),
        Err(Refusal::Malformed)
    );
}

// The record slots past the tracked count were never read, so a frame with
// bytes there decoded to the same table as the zeroed frame `encode` writes:
// two frames for one state. Only the canonical frame is accepted.
#[test]
fn bytes_past_the_tracked_records_are_refused() {
    let mut table = HeavyHitters::<u64, 2>::new();
    assert_eq!(table.add_count(1, 3), Ok(()));
    let mut frame = [0_u8; HeavyHitters::<u64, 2>::ENCODED_LEN];
    assert_eq!(table.encode(&mut frame), Ok(frame.len()));
    assert_eq!(HeavyHitters::<u64, 2>::decode(&frame), Ok(table.clone()));

    for at in HEADER_LEN + RECORD_LEN..frame.len() {
        let mut stray = frame;
        stray[at] = 1;
        let mut held = table.clone();
        refuses(&mut held, Refusal::Malformed, |h| h.decode_into(&stray));
    }
}

/// An owned key needs no interning: the table clones on insert and compares by
/// value, so a layer name is a usable key on its own.
#[cfg(feature = "alloc")]
#[test]
fn an_owned_key_needs_no_interning() {
    use alloc::string::{String, ToString};

    let mut layers = HeavyHitters::<String, 8>::new();
    for tick in 0..900 {
        assert_eq!(
            layers.add(if tick % 3 == 0 { "horizon" } else { "stars" }.to_string()),
            Ok(())
        );
    }
    assert_eq!(layers.add("grid".to_string()), Ok(()));

    assert_eq!(layers.count(&"stars".to_string()), 600);
    assert_eq!(layers.count(&"horizon".to_string()), 300);
    assert_eq!(
        layers.top().map(|hitter| hitter.key.as_str()),
        Some("stars")
    );

    let mut merged = HeavyHitters::<String, 8>::new();
    assert_eq!(merged.add_count("grid".to_string(), 5), Ok(()));
    assert_eq!(merged.merge(&layers), Ok(()));
    assert_eq!(merged.count(&"grid".to_string()), 6);
    assert_eq!(merged.observed(), 906);
}
