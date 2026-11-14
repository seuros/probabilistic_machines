use super::CountMinSketch;
use crate::Refusal;
use crate::hash::mix64;
use matryona_machines::testing::refuses;

fn key(index: u32) -> [u8; 8] {
    mix64(u64::from(index)).to_le_bytes()
}

fn truth(index: u32) -> u32 {
    1 + index % 17
}

#[test]
fn an_estimate_is_never_below_the_truth() {
    let mut sketch = CountMinSketch::<2048, 5>::new();
    for index in 0..4_000 {
        assert_eq!(sketch.add_count(&key(index), truth(index)), Ok(()));
    }

    for index in 0..4_000 {
        let estimate = sketch.estimate(&key(index));
        assert!(estimate >= truth(index), "{index}: {estimate}");
    }
}

#[test]
fn the_overshoot_stays_inside_the_sketch_width() {
    let mut sketch = CountMinSketch::<2048, 5>::new();
    for index in 0..4_000 {
        assert_eq!(sketch.add_count(&key(index), truth(index)), Ok(()));
    }

    let total: u32 = (0..4_000).map(truth).sum();
    assert_eq!(sketch.total(), u64::from(total));

    let budget = 2.72 * f64::from(total) / 2048.0;
    let worst = (0..4_000)
        .map(|index| f64::from(sketch.estimate(&key(index)) - truth(index)))
        .fold(0.0_f64, f64::max);

    assert!(worst <= budget, "{worst} > {budget}");
}

#[test]
fn an_untouched_sketch_estimates_nothing() {
    let sketch = CountMinSketch::<64, 3>::new();
    assert_eq!(sketch.estimate(b"anything"), 0);
    assert_eq!(sketch.total(), 0);
}

#[test]
fn merging_equals_feeding_one_sketch_both_streams() {
    let mut left = CountMinSketch::<256, 4>::new();
    let mut right = CountMinSketch::<256, 4>::new();
    let mut together = CountMinSketch::<256, 4>::new();

    for index in 0..500 {
        assert_eq!(left.add_count(&key(index), truth(index)), Ok(()));
        assert_eq!(together.add_count(&key(index), truth(index)), Ok(()));
    }
    for index in 300..900 {
        assert_eq!(right.add_count(&key(index), truth(index)), Ok(()));
        assert_eq!(together.add_count(&key(index), truth(index)), Ok(()));
    }

    assert_eq!(left.merge(&right), Ok(()));
    assert_eq!(left, together);
    assert_eq!(left.total(), together.total());
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    for index in 0..200 {
        assert_eq!(sketch.add(&key(index)), Ok(()));
    }

    let mut frame = [0_u8; CountMinSketch::<32, 3>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(12 + 32 * 3 * 4));
    #[cfg(feature = "alloc")]
    assert_eq!(&*sketch.encoded(), &frame[..]);
    assert_eq!(CountMinSketch::<32, 3>::decode(&frame), Ok(sketch.clone()));

    let short = frame.len() - 1;
    assert_eq!(sketch.encode(&mut frame[..short]), Err(Refusal::Truncated));
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&frame[..short]),
        Err(Refusal::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&wrong),
        Err(Refusal::WrongStructure)
    );

    let mut lying = frame;
    lying[2] = 2;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&lying),
        Err(Refusal::WrongShape)
    );
}

// A frame whose total was edited down described counters no stream could have
// filled, and the sketch read it back as an estimate above its own total.
#[test]
fn counters_the_total_cannot_explain_are_refused() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    assert_eq!(sketch.add_count(b"x", 10), Ok(()));

    let mut frame = [0_u8; CountMinSketch::<32, 3>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(frame.len()));

    let mut emptied = frame;
    emptied[3] = 0;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&emptied),
        Err(Refusal::Malformed)
    );

    let mut inflated = frame;
    inflated[3] = 11;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&inflated),
        Err(Refusal::Malformed)
    );

    let mut flagged = frame;
    flagged[11] = 1;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&flagged),
        Err(Refusal::Malformed)
    );
    assert_eq!(CountMinSketch::<32, 3>::decode(&frame), Ok(sketch));
}

#[test]
fn a_single_row_still_counts() {
    let mut sketch = CountMinSketch::<4096, 1>::new();
    assert_eq!(sketch.add_count(b"only", 9), Ok(()));
    assert!(sketch.estimate(b"only") >= 9);
}

// Saturating silently left an estimate below the true count, so the
// never-underestimate guarantee only held behind a flag. An addition that
// would carry a counter past u32::MAX is now refused and changes nothing.
#[test]
fn an_addition_past_a_full_counter_is_refused_and_changes_nothing() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    assert_eq!(sketch.add_count(b"x", u32::MAX), Ok(()));

    refuses(&mut sketch, Refusal::Overflow, |s| s.add(b"x"));
    assert_eq!(sketch.estimate(b"x"), u32::MAX);
    assert_eq!(sketch.total(), u64::from(u32::MAX));
}

#[test]
fn a_merge_past_a_full_counter_is_refused_and_changes_nothing() {
    let mut sketch = CountMinSketch::<16, 2>::new();
    assert_eq!(sketch.add_count(b"hot", u32::MAX), Ok(()));
    let mut other = CountMinSketch::<16, 2>::new();
    assert_eq!(other.add(b"hot"), Ok(()));

    refuses(&mut sketch, Refusal::Overflow, |s| s.merge(&other));
}

#[test]
fn a_zero_count_is_refused_and_changes_nothing() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    assert_eq!(sketch.add_count(b"x", 4), Ok(()));

    refuses(&mut sketch, Refusal::ZeroCount, |s| s.add_count(b"x", 0));
    refuses(&mut sketch, Refusal::ZeroCount, |s| s.add_hash_count(7, 0));
}

// The reference model: exact per-item counts, and the sketch's guarantees
// checked against them after every call. Every refusal must leave the sketch
// equal to a clone taken before the call.
#[test]
fn a_random_stream_keeps_every_guarantee_and_refusals_change_nothing() {
    let mut sketch = CountMinSketch::<16, 3>::new();
    let mut truth = [0_u64; 24];
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    for _ in 0..4_000 {
        state = mix64(state);
        let item = usize::from(crate::hash::u64_low_u8(state)) % truth.len();
        let count = match state >> 60 {
            0 => 0,
            1 => u32::MAX / 3,
            _ => u32::from(crate::hash::u64_low_u16(state >> 16)),
        };
        let before = sketch.clone();
        let bytes = key(u32::from(crate::hash::usize_low_u8(item)));
        match sketch.add_count(&bytes, count) {
            Ok(()) => truth[item] += u64::from(count),
            Err(Refusal::ZeroCount) => {
                assert_eq!(count, 0);
                assert_eq!(sketch, before);
            }
            Err(refused) => {
                assert_eq!(refused, Refusal::Overflow);
                assert_eq!(sketch, before);
            }
        }
        assert_eq!(sketch.total(), truth.iter().sum::<u64>());
        for (index, &count) in truth.iter().enumerate() {
            let bytes = key(u32::from(crate::hash::usize_low_u8(index)));
            assert!(u64::from(sketch.estimate(&bytes)) >= count, "{index}");
        }
    }
}
