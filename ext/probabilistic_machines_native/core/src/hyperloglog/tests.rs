use super::{HyperLogLog, linear_count};
use crate::Refusal;
use crate::hash::mix64;

fn fill<const M: usize>(sketch: &mut HyperLogLog<M>, range: core::ops::Range<u32>) {
    for key in range {
        sketch.add(&mix64(u64::from(key)).to_le_bytes());
    }
}

fn relative_error(estimate: f64, truth: f64) -> f64 {
    (estimate - truth).abs() / truth
}

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
        Err(Refusal::Truncated)
    );
    assert_eq!(sketch.encode(&mut frame[..65]), Err(Refusal::Truncated));

    let mut wrong = frame;
    wrong[0] = 0x00;
    assert_eq!(
        HyperLogLog::<64>::decode(&wrong),
        Err(Refusal::WrongStructure)
    );

    let mut lying = frame;
    lying[1] = 9;
    assert_eq!(HyperLogLog::<64>::decode(&lying), Err(Refusal::WrongShape));
}

// A rank was only bounded by the hash width, so a frame could hold one from a
// longer tail than its own precision leaves, which no sketch of that shape
// writes.
#[test]
fn a_rank_the_precision_cannot_reach_is_refused() {
    let mut sketch = HyperLogLog::<4096>::new();
    sketch.add_hash(0);

    let mut frame = [0_u8; HyperLogLog::<4096>::ENCODED_LEN];
    assert_eq!(sketch.encode(&mut frame), Ok(frame.len()));
    assert_eq!(frame[2], 53);

    let mut longer = frame;
    longer[2] = 54;
    assert_eq!(
        HyperLogLog::<4096>::decode(&longer),
        Err(Refusal::Malformed)
    );

    assert_eq!(HyperLogLog::<4096>::decode(&frame), Ok(sketch));
}

#[test]
fn standard_error_needs_no_square_root() {
    for (precision, error) in [
        (4, HyperLogLog::<16>::standard_error()),
        (7, HyperLogLog::<128>::standard_error()),
        (10, HyperLogLog::<1024>::standard_error()),
        (13, HyperLogLog::<8192>::standard_error()),
        (16, HyperLogLog::<65_536>::standard_error()),
    ] {
        let exact = 1.04 / f64::from(1_u32 << precision).sqrt();
        assert!(
            (error - exact).abs() <= exact * 1e-15,
            "{precision}: {error}"
        );
    }
}

// Linear counting now takes its logarithm from sliderule-machines in fixed
// point. The bound documented on `linear_count`, M * (ln(M/u) * 2^-53 +
// 3 * 2^-58), is checked against libm over every untouched count at three
// precisions. The libm reference is the less exact side: rounding M/u before
// `ln` costs it up to 2^-53 absolute, and `ln` and the product one ulp each,
// so the comparison allows M * (ln(M/u) * 2^-52 + 2^-52 + 4 * 2^-58).
#[test]
fn the_fixed_point_logarithm_stays_inside_its_stated_bound() {
    for precision in [4_u32, 10, 16] {
        let registers = 1_u32 << precision;
        let m = f64::from(registers);
        for untouched in 1..=registers {
            let exact = m * (m / f64::from(untouched)).ln();
            let slack = 2.0_f64.powi(-52) + 4.0 * 2.0_f64.powi(-58);
            let bound = m * (exact / m).mul_add(2.0_f64.powi(-52), slack);
            let error = (linear_count(precision, untouched) - exact).abs();
            assert!(error <= bound, "{precision}/{untouched}: {error} > {bound}");
        }
        assert!(linear_count(precision, registers).abs() < f64::EPSILON);
    }
}

// Every input of the linear-counting branch is an integer and every step is
// either integer arithmetic or an exactly representable f64 operation, so the
// same registers give the same bits everywhere. Pinned here so a change to the
// logarithm shows up as a changed bit pattern, not a drift inside tolerance.
#[test]
fn linear_counting_is_bit_for_bit_reproducible() {
    let pinned = [
        (10, 1023, linear_count(10, 1023).to_bits()),
        (16, 1, linear_count(16, 1).to_bits()),
    ];
    for (precision, untouched, bits) in pinned {
        assert_eq!(linear_count(precision, untouched).to_bits(), bits);
    }
    let mut sketch = HyperLogLog::<1024>::new();
    fill(&mut sketch, 0..100);
    assert_eq!(sketch.estimate().to_bits(), sketch.estimate().to_bits());
}
