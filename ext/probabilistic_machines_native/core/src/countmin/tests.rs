use super::CountMinSketch;
use crate::CodecError;
use crate::hash::mix64;

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
        sketch.add_count(&key(index), truth(index));
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
        sketch.add_count(&key(index), truth(index));
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
fn counters_and_totals_saturate_instead_of_wrapping() {
    let mut sketch = CountMinSketch::<16, 2>::new();
    sketch.add_count(b"hot", u32::MAX);
    sketch.add_count(b"hot", u32::MAX);

    assert_eq!(sketch.estimate(b"hot"), u32::MAX);
    assert_eq!(sketch.total(), u64::from(u32::MAX) * 2);

    let mut full = CountMinSketch::<16, 2>::new();
    full.add_count(b"hot", u32::MAX);
    let mut other = full.clone();
    other.merge(&full);
    other.merge(&full);
    assert_eq!(other.estimate(b"hot"), u32::MAX);
}

#[test]
fn merging_equals_feeding_one_sketch_both_streams() {
    let mut left = CountMinSketch::<256, 4>::new();
    let mut right = CountMinSketch::<256, 4>::new();
    let mut together = CountMinSketch::<256, 4>::new();

    for index in 0..500 {
        left.add_count(&key(index), truth(index));
        together.add_count(&key(index), truth(index));
    }
    for index in 300..900 {
        right.add_count(&key(index), truth(index));
        together.add_count(&key(index), truth(index));
    }

    left.merge(&right);
    assert_eq!(left, together);
    assert_eq!(left.total(), together.total());
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    for index in 0..200 {
        sketch.add(&key(index));
    }

    let mut frame = [0_u8; CountMinSketch::<32, 3>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(12 + 32 * 3 * 4));
    #[cfg(feature = "alloc")]
    assert_eq!(&*sketch.encoded(), &frame[..]);
    assert_eq!(CountMinSketch::<32, 3>::decode(&frame), Ok(sketch.clone()));

    let short = frame.len() - 1;
    assert_eq!(
        sketch.encode(&mut frame[..short]),
        Err(CodecError::Truncated)
    );
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&frame[..short]),
        Err(CodecError::Truncated)
    );

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&wrong),
        Err(CodecError::Tag)
    );

    let mut lying = frame;
    lying[2] = 2;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&lying),
        Err(CodecError::Parameters)
    );
}

// A frame whose total was edited down described counters no stream could have
// filled, and the sketch read it back as an estimate above its own total.
#[test]
fn counters_the_total_cannot_explain_are_refused() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    sketch.add_count(b"x", 10);

    let mut frame = [0_u8; CountMinSketch::<32, 3>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(frame.len()));

    let mut emptied = frame;
    emptied[3] = 0;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&emptied),
        Err(CodecError::Parameters)
    );

    let mut inflated = frame;
    inflated[3] = 11;
    assert_eq!(
        CountMinSketch::<32, 3>::decode(&inflated),
        Err(CodecError::Parameters)
    );

    let mut saturated = CountMinSketch::<32, 3>::new();
    saturated.add_count(b"x", u32::MAX);
    saturated.add(b"x");
    assert!(saturated.saturated());
    assert!(u64::from(saturated.estimate(b"x")) < saturated.total());

    assert_eq!(saturated.encode(&mut frame), Ok(frame.len()));
    assert_eq!(CountMinSketch::<32, 3>::decode(&frame), Ok(saturated));
}

#[test]
fn a_single_row_still_counts() {
    let mut sketch = CountMinSketch::<4096, 1>::new();
    sketch.add_count(b"only", 9);
    assert!(sketch.estimate(b"only") >= 9);
}

// Saturating silently returned an estimate below the true count, so the
// never-underestimate guarantee needs a flag that says when it stops holding.
#[test]
fn saturation_is_reported_rather_than_silent() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    sketch.add_count(b"x", u32::MAX);
    assert!(!sketch.saturated());

    sketch.add(b"x");

    assert!(sketch.saturated());
    assert_eq!(sketch.estimate(b"x"), u32::MAX);
}

#[cfg(feature = "alloc")]
#[test]
fn saturation_survives_a_round_trip() {
    let mut sketch = CountMinSketch::<32, 3>::new();
    sketch.add_count(b"x", u32::MAX);
    sketch.add(b"x");
    assert!(sketch.saturated());

    let decoded = CountMinSketch::<32, 3>::decode(&sketch.encoded()).unwrap_or_default();

    assert!(decoded.saturated());
    assert!(!CountMinSketch::<32, 3>::new().saturated());
}
