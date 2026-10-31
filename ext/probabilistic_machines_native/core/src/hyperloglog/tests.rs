use super::{HyperLogLog, ln};
use crate::CodecError;
use crate::hash::mix64;

fn fill<const M: usize>(sketch: &mut HyperLogLog<M>, range: core::ops::Range<u32>) {
    for key in range {
        sketch.add(&mix64(u64::from(key)).to_le_bytes());
    }
}

fn relative_error(estimate: f64, truth: f64) -> f64 {
    (estimate - truth).abs() / truth
}

#[cfg(feature = "std")]
#[test]
fn estimates_land_inside_three_standard_errors() {
    let tolerance = 3.0 * HyperLogLog::<4096>::standard_error();
    for &truth in &[1_000_u32, 10_000, 100_000] {
        let mut sketch = HyperLogLog::<4096>::new();
        fill(&mut sketch, 0..truth);

        let error = relative_error(sketch.estimate(), f64::from(truth));
        assert!(error < tolerance, "{truth}: {error} >= {tolerance}");
    }
}

#[test]
fn small_cardinalities_fall_back_to_linear_counting() {
    for &truth in &[1_u32, 7, 50, 100] {
        let mut sketch = HyperLogLog::<1024>::new();
        fill(&mut sketch, 0..truth);

        let error = relative_error(sketch.estimate(), f64::from(truth));
        assert!(
            error < HyperLogLog::<1024>::standard_error(),
            "{truth}: {error}"
        );
    }
}

#[cfg(feature = "std")]
#[test]
fn the_switch_between_estimators_leaves_no_discontinuity() {
    let tolerance = 3.0 * HyperLogLog::<1024>::standard_error();
    for &truth in &[400_u32, 800, 1_500, 3_000, 6_000] {
        let mut sketch = HyperLogLog::<1024>::new();
        fill(&mut sketch, 0..truth);

        let error = relative_error(sketch.estimate(), f64::from(truth));
        assert!(error < tolerance, "{truth}: {error} >= {tolerance}");
    }
}

#[test]
fn an_empty_sketch_estimates_nothing() {
    let sketch = HyperLogLog::<256>::new();
    assert!(sketch.is_empty());
    assert!(sketch.estimate().abs() < 1e-9);
}

#[test]
fn repeats_leave_the_registers_alone() {
    let mut once = HyperLogLog::<512>::new();
    fill(&mut once, 0..300);

    let mut twice = once;
    fill(&mut twice, 0..300);
    fill(&mut twice, 0..300);

    assert_eq!(once, twice);
}

#[test]
fn merging_equals_counting_the_union() {
    let mut left = HyperLogLog::<2048>::new();
    fill(&mut left, 0..5_000);
    let mut right = HyperLogLog::<2048>::new();
    fill(&mut right, 2_500..7_500);

    let mut union = HyperLogLog::<2048>::new();
    fill(&mut union, 0..7_500);

    left.merge(&right);
    assert_eq!(left, union);
}

#[test]
fn merging_is_idempotent_and_order_free() {
    let mut left = HyperLogLog::<256>::new();
    fill(&mut left, 0..900);
    let mut right = HyperLogLog::<256>::new();
    fill(&mut right, 500..1_400);

    let mut one_way = left;
    one_way.merge(&right);
    let mut other_way = right;
    other_way.merge(&left);
    assert_eq!(one_way, other_way);

    let mut again = one_way;
    again.merge(&right);
    assert_eq!(again, one_way);
}

#[test]
fn the_longest_run_of_zeros_saturates_at_the_register_bound() {
    let mut sketch = HyperLogLog::<16>::new();
    sketch.add_hash(0);

    let mut frame = [0_u8; HyperLogLog::<16>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(frame.len()));
    assert_eq!(frame[1], 4);
    assert_eq!(frame[2], 61);
}

#[test]
fn every_hash_lands_in_a_register() {
    let mut sketch = HyperLogLog::<65_536>::new();
    sketch.add_hash(u64::MAX);
    sketch.add_hash(0);
    assert!(!sketch.is_empty());
}

#[test]
fn frames_round_trip_and_reject_foreign_bytes() {
    let mut sketch = HyperLogLog::<64>::new();
    fill(&mut sketch, 0..200);

    let mut frame = [0_u8; HyperLogLog::<64>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(66));
    #[cfg(feature = "alloc")]
    assert_eq!(&*sketch.encoded(), &frame[..]);
    assert_eq!(HyperLogLog::<64>::decode(&frame), Ok(sketch));

    assert_eq!(
        HyperLogLog::<64>::decode(&frame[..65]),
        Err(CodecError::Truncated)
    );
    assert_eq!(sketch.encode(&mut frame[..65]), Err(CodecError::Truncated));

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(HyperLogLog::<64>::decode(&wrong), Err(CodecError::Tag));

    let mut lying = frame;
    lying[1] = 9;
    assert_eq!(
        HyperLogLog::<64>::decode(&lying),
        Err(CodecError::Parameters)
    );
}

#[cfg(feature = "std")]
#[test]
fn standard_error_follows_the_register_count() {
    assert!((HyperLogLog::<1024>::standard_error() - 0.0325).abs() < 0.0001);
    assert!((HyperLogLog::<4096>::standard_error() - 0.01625).abs() < 0.0001);
}

#[test]
fn the_hand_rolled_logarithm_matches_the_libm_one() {
    for step in 1..=20_000_u32 {
        let argument = f64::from(step) * 3.3;
        let error = (ln(argument) - argument.ln()).abs();
        assert!(error < 1e-9, "{argument}: {error}");
    }
}

// Linear counting evaluates ln(m / zeros), which sits just above 1.0 for a
// sparse sketch. That is the hot domain and the series' worst case.
#[test]
fn the_hand_rolled_logarithm_is_accurate_just_above_one() {
    assert!(ln(1.0).abs() < 1e-12);

    for step in 0..=20_000_u32 {
        let argument = f64::mul_add(f64::from(step), 0.000_115, 1.0);
        let error = (ln(argument) - argument.ln()).abs();
        assert!(error < 1e-9, "{argument}: {error}");
    }
}
