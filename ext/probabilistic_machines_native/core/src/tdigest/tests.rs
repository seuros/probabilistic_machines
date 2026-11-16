use core::assert_matches;

use super::{CENTROID_LEN, Centroid, HEADER_LEN, MIN_COMPRESSION, TDigest};
use crate::Refusal;
use matryona_machines::testing::refuses;

fn built(compression: u16) -> TDigest {
    TDigest::new(compression).unwrap_or_else(|| panic!("compression {compression}"))
}

fn uniform(points: u32, compression: u16) -> TDigest {
    let mut digest = built(compression);
    for point in 1..=points {
        assert_eq!(digest.add(f64::from(point)), Ok(()));
    }
    digest
}

#[test]
fn quantiles_of_a_uniform_stream_land_within_one_percent() {
    let digest = uniform(10_000, 100);
    for &(quantile, truth) in &[
        (0.01, 100.0),
        (0.1, 1_000.0),
        (0.5, 5_000.0),
        (0.9, 9_000.0),
        (0.99, 9_900.0),
    ] {
        let estimate = digest.quantile(quantile);
        let error = (estimate - truth).abs() / 10_000.0;
        assert!(error < 0.01, "q{quantile}: {estimate} vs {truth}");
    }
}

#[test]
fn the_extremes_are_exact() {
    let digest = uniform(10_000, 100);
    assert!((digest.quantile(0.0) - 1.0).abs() < f64::EPSILON);
    assert!((digest.quantile(1.0) - 10_000.0).abs() < f64::EPSILON);
    assert!((digest.minimum() - 1.0).abs() < f64::EPSILON);
    assert!((digest.maximum() - 10_000.0).abs() < f64::EPSILON);
    assert!((digest.count() - 10_000.0).abs() < f64::EPSILON);
}

#[test]
fn the_tails_are_sharper_than_the_middle() {
    let digest = uniform(100_000, 100);
    let tail = (digest.quantile(0.999) - 99_900.0).abs() / 100_000.0;
    let middle = (digest.quantile(0.5) - 50_000.0).abs() / 100_000.0;
    assert!(tail < 0.001, "{tail}");
    assert!(middle < 0.01, "{middle}");
}

#[test]
fn the_cluster_count_stays_bounded_whatever_the_stream_length() {
    let short = uniform(10_000, 100);
    let long = uniform(200_000, 100);
    assert!(short.len() <= 1_000, "{}", short.len());
    assert!(long.len() <= 1_000, "{}", long.len());
}

#[test]
fn an_empty_digest_has_no_quantiles() {
    let digest = built(100);
    assert!(digest.is_empty());
    assert!(digest.quantile(0.5).is_nan());
    assert!(digest.cdf(0.5).is_nan());
}

#[test]
fn a_quantile_outside_zero_to_one_is_nan() {
    let digest = uniform(1_000, 100);
    for q in [-0.0, 0.0, 0.5, 1.0] {
        assert!(!digest.quantile(q).is_nan(), "q{q}");
    }
    for q in [
        -1e-12,
        1.0 + f64::EPSILON,
        2.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert!(digest.quantile(q).is_nan(), "q{q}");
    }
}

#[test]
fn a_single_point_is_every_quantile() {
    let mut digest = built(100);
    assert_eq!(digest.add(42.0), Ok(()));
    for &quantile in &[0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!((digest.quantile(quantile) - 42.0).abs() < f64::EPSILON);
    }
}

#[test]
fn a_thousand_copies_of_one_value_do_not_divide_by_zero() {
    let mut digest = built(50);
    for _ in 0..1_000 {
        assert_eq!(digest.add(42.0), Ok(()));
    }

    assert!((digest.quantile(0.5) - 42.0).abs() < f64::EPSILON);
    assert!(digest.cdf(42.0).is_finite());
    assert!((digest.count() - 1_000.0).abs() < f64::EPSILON);
}

#[test]
fn a_weighted_point_stands_for_repeats_of_it() {
    let mut weighted = built(200);
    let mut repeated = built(200);
    for value in 1..=100_u32 {
        assert_eq!(weighted.add_weighted(f64::from(value), 10.0), Ok(()));
        for _ in 0..10 {
            assert_eq!(repeated.add(f64::from(value)), Ok(()));
        }
    }

    assert!((weighted.count() - repeated.count()).abs() < f64::EPSILON);
    for &quantile in &[0.05, 0.5, 0.95] {
        let drift = (weighted.quantile(quantile) - repeated.quantile(quantile)).abs();
        assert!(drift < 1.0, "q{quantile}: {drift}");
    }
}

#[test]
fn the_cdf_inverts_the_quantile() {
    let digest = uniform(10_000, 200);
    for &quantile in &[0.1, 0.25, 0.5, 0.75, 0.9] {
        let value = digest.quantile(quantile);
        let back = digest.cdf(value);
        assert!((back - quantile).abs() < 0.01, "q{quantile}: {back}");
    }
    assert!(digest.cdf(-5.0).abs() < f64::EPSILON);
    assert!((digest.cdf(50_000.0) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn merging_agrees_with_one_digest_fed_both_streams() {
    let mut left = built(100);
    let mut right = built(100);
    let mut together = built(100);

    for point in 1..=20_000_u32 {
        let value = f64::from(point);
        if point % 2 == 0 {
            assert_eq!(left.add(value), Ok(()));
        } else {
            assert_eq!(right.add(value), Ok(()));
        }
        assert_eq!(together.add(value), Ok(()));
    }

    assert_eq!(left.merge(&right), Ok(()));
    assert!((left.count() - together.count()).abs() < f64::EPSILON);
    assert!((left.minimum() - 1.0).abs() < f64::EPSILON);
    assert!((left.maximum() - 20_000.0).abs() < f64::EPSILON);

    for &quantile in &[0.01, 0.25, 0.5, 0.75, 0.99] {
        let drift = (left.quantile(quantile) - together.quantile(quantile)).abs() / 20_000.0;
        assert!(drift < 0.01, "q{quantile}: {drift}");
    }
}

#[test]
fn merging_an_empty_digest_keeps_the_distribution() {
    let mut digest = uniform(1_000, 100);
    let before = digest.clone();
    assert_eq!(digest.merge(&built(100)), Ok(()));

    assert!((digest.count() - before.count()).abs() < f64::EPSILON);
    assert!((digest.minimum() - before.minimum()).abs() < f64::EPSILON);
    assert!((digest.maximum() - before.maximum()).abs() < f64::EPSILON);
    for &quantile in &[0.1, 0.5, 0.9] {
        let drift = (digest.quantile(quantile) - before.quantile(quantile)).abs();
        assert!(drift < 10.0, "q{quantile}: {drift}");
    }
}

#[test]
fn frames_round_trip_every_bit_of_every_cluster() {
    let digest = uniform(5_000, 100);
    let written = digest.encoded_len();
    let mut frame = alloc::vec![0_u8; written];
    assert_eq!(digest.encode(&mut frame), Ok(written));
    assert_eq!(&*digest.encoded(), &frame[..]);
    assert_eq!(TDigest::decode(&frame), Ok(digest));
}

#[test]
fn frames_reject_foreign_bytes() {
    let digest = uniform(100, 100);
    let written = digest.encoded_len();
    let mut frame = alloc::vec![0_u8; written];
    assert_eq!(digest.encode(&mut frame), Ok(written));

    assert_eq!(digest.encode(&mut frame[..10]), Err(Refusal::Truncated));
    assert_eq!(TDigest::decode(&frame[..10]), Err(Refusal::Truncated));
    assert_eq!(
        TDigest::decode(&frame[..written - 1]),
        Err(Refusal::Truncated)
    );

    let mut wrong = frame.clone();
    wrong[0] = 0x00;
    assert_eq!(TDigest::decode(&wrong), Err(Refusal::WrongStructure));
}

// A body of whole cluster records passed for a whole digest, so dropping
// records loaded a digest that kept the header's count: dropping all of them
// left an empty digest reporting a positive count and answering NaN.
#[test]
fn a_frame_that_lost_whole_clusters_is_not_a_digest() {
    let digest = uniform(3, 10);
    let frame = digest.encoded();
    let count = digest.count();

    assert_eq!(
        TDigest::decode(&frame[..HEADER_LEN]),
        Err(Refusal::Truncated)
    );
    assert_eq!(
        TDigest::decode(&frame[..frame.len() - CENTROID_LEN]),
        Err(Refusal::Truncated)
    );
    assert_eq!(TDigest::decode(&frame), Ok(digest));

    let mut heavier = frame.clone();
    heavier[7..15].copy_from_slice(&(count * (1.0 + 1e-3)).to_le_bytes());
    assert_eq!(TDigest::decode(&heavier), Err(Refusal::Malformed));

    let mut drifted = frame;
    drifted[7..15].copy_from_slice(&(count * (1.0 + 1e-9)).to_le_bytes());
    assert_eq!(TDigest::decode(&drifted).map(|digest| digest.len()), Ok(3));
}

// The drift tolerance scaled with the stored total, so an infinite total sat
// within infinity of the clusters' finite sum and passed: a frame the add path
// refuses to produce loaded into a digest counting infinity and answering NaN.
#[test]
fn a_frame_claiming_an_impossible_total_is_not_a_digest() {
    let digest = uniform(3, 10);
    let frame = digest.encoded();

    for total in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 0.0, -3.0] {
        let mut tampered = frame.clone();
        tampered[7..15].copy_from_slice(&total.to_le_bytes());
        assert_eq!(
            TDigest::decode(&tampered),
            Err(Refusal::Malformed),
            "total {total}"
        );
    }

    let mut overflowing = frame.clone();
    overflowing[7..15].copy_from_slice(&f64::MAX.to_le_bytes());
    for slot in 0..digest.len() {
        let at = HEADER_LEN + slot * CENTROID_LEN + 8;
        overflowing[at..at + 8].copy_from_slice(&f64::MAX.to_le_bytes());
    }
    assert_eq!(TDigest::decode(&overflowing), Err(Refusal::Malformed));

    assert_eq!(TDigest::decode(&frame), Ok(digest));
    assert_eq!(TDigest::decode(&built(10).encoded()), Ok(built(10)));
}

// Subtracting opposite-sign extremes overflowed before the multiply, so a
// finite population produced an infinite quantile and q75 fell below q50.
#[test]
fn opposite_sign_extremes_interpolate_to_a_finite_monotonic_answer() {
    let mut digest = built(100);
    assert_eq!(digest.add(-1e308), Ok(()));
    assert_eq!(digest.add(1e308), Ok(()));

    let q50 = digest.quantile(0.5);
    let q75 = digest.quantile(0.75);

    assert!(q50.is_finite(), "q50 was {q50}");
    assert!(q75.is_finite(), "q75 was {q75}");
    assert!(q75 >= q50, "q75 {q75} fell below q50 {q50}");
}

// Folding a cluster of one sign into a cluster of the other overflowed the
// span before weighting it, collapsing a finite population into a NaN cluster
// that gave NaN quantiles and lost the exact maximum.
#[test]
fn collapsing_opposite_sign_extremes_keeps_every_cluster_finite() {
    let mut digest = built(MIN_COMPRESSION);
    for _ in 0..60 {
        assert_eq!(digest.add(-1e308), Ok(()));
    }
    for _ in 0..60 {
        assert_eq!(digest.add(1e308), Ok(()));
    }

    assert!(
        digest
            .centroids()
            .iter()
            .all(|centroid| centroid.mean.is_finite()),
        "{:?}",
        digest.centroids()
    );
    assert!(
        digest
            .centroids()
            .is_sorted_by(|left, right| left.mean <= right.mean),
        "{:?}",
        digest.centroids()
    );

    let q50 = digest.quantile(0.5);
    assert!(q50.is_finite(), "q50 was {q50}");
    assert!((digest.quantile(0.0) + 1e308).abs() < f64::EPSILON);
    assert!((digest.quantile(1.0) - 1e308).abs() < f64::EPSILON);
}

// Interpolating between two endpoints divided by their span, which overflows
// to infinity for opposite-sign extremes, so every finite value landed on the
// low endpoint and the midpoint of a symmetric pair read as a quarter of the
// stream instead of half.
#[test]
fn the_midpoint_of_opposite_sign_extremes_is_half_the_stream() {
    let mut wide = built(100);
    assert_eq!(wide.add(-f64::MAX), Ok(()));
    assert_eq!(wide.add(f64::MAX), Ok(()));

    let middle = wide.cdf(0.0);
    assert!((middle - 0.5).abs() < f64::EPSILON, "cdf(0) was {middle}");
    assert!((wide.quantile(0.0) + f64::MAX).abs() < f64::EPSILON);
    assert!((wide.quantile(1.0) - f64::MAX).abs() < f64::EPSILON);

    let step = 2.0 * f64::EPSILON;
    let mut narrow = built(100);
    assert_eq!(narrow.add(1.0), Ok(()));
    assert_eq!(narrow.add(1.0 + step + step), Ok(()));

    let halfway = narrow.cdf(1.0 + step);
    assert!((halfway - 0.5).abs() < f64::EPSILON, "cdf was {halfway}");
}

// Halving the span to keep an opposite-sign pair finite rounded a subnormal
// span to zero, so two neighbouring subnormals divided zero by zero and the
// cdf of the lower one came back NaN.
#[test]
fn a_subnormal_span_interpolates_without_rounding_to_zero() {
    let smallest = f64::MIN_POSITIVE * f64::EPSILON;
    let mut tiny = built(100);
    assert_eq!(tiny.add(0.0), Ok(()));
    assert_eq!(tiny.add(smallest), Ok(()));

    let low = tiny.cdf(0.0);
    assert!((low - 0.25).abs() < f64::EPSILON, "cdf(0) was {low}");
    assert!((tiny.cdf(smallest) - 1.0).abs() < f64::EPSILON);
    assert!(tiny.quantile(0.0).total_cmp(&0.0).is_eq());
    assert!(tiny.quantile(1.0).total_cmp(&smallest).is_eq());

    let mut neighbours = built(100);
    assert_eq!(neighbours.add(smallest), Ok(()));
    assert_eq!(neighbours.add(smallest * 3.0), Ok(()));
    let between = neighbours.cdf(smallest * 2.0);
    assert!((between - 0.5).abs() < f64::EPSILON, "cdf was {between}");

    let mut wide = built(100);
    assert_eq!(wide.add(-f64::MAX), Ok(()));
    assert_eq!(wide.add(f64::MAX), Ok(()));
    let middle = wide.cdf(0.0);
    assert!((middle - 0.5).abs() < f64::EPSILON, "cdf(0) was {middle}");
}

#[test]
fn a_compression_below_the_minimum_builds_nothing() {
    assert_eq!(TDigest::new(0), None);
    assert_eq!(TDigest::new(MIN_COMPRESSION - 1), None);
    assert_eq!(built(MIN_COMPRESSION).compression(), MIN_COMPRESSION);

    let mut frame = built(MIN_COMPRESSION).encoded();
    frame[1..3].copy_from_slice(&(MIN_COMPRESSION - 1).to_le_bytes());
    assert_eq!(TDigest::decode(&frame), Err(Refusal::Malformed));
}

// add_weighted documented its precondition and checked none of it: a NaN
// value, an infinite or negative weight, or a total overflowing to infinity
// went straight into the clusters and left a digest decode refuses.
#[test]
fn an_add_outside_the_precondition_is_refused_and_changes_nothing() {
    let mut digest = uniform(50, 100);
    for (value, weight, refusal) in [
        (f64::NAN, 1.0, Refusal::NotFinite),
        (f64::INFINITY, 1.0, Refusal::NotFinite),
        (1.0, f64::NAN, Refusal::NotFinite),
        (1.0, f64::INFINITY, Refusal::NotFinite),
        (1.0, 0.0, Refusal::InvalidWeight),
        (1.0, -2.0, Refusal::InvalidWeight),
        (1.0, f64::MAX, Refusal::Overflow),
    ] {
        let mut huge = digest.clone();
        if refusal == Refusal::Overflow {
            assert_eq!(huge.add_weighted(2.0, f64::MAX), Ok(()));
            refuses(&mut huge, refusal, |d| d.add_weighted(value, weight));
        } else {
            refuses(&mut digest, refusal, |d| d.add_weighted(value, weight));
        }
    }
    refuses(&mut digest, Refusal::NotFinite, |d| {
        d.add(f64::NEG_INFINITY)
    });
}

#[test]
fn a_merge_of_another_shape_or_an_infinite_total_is_refused_and_changes_nothing() {
    let mut digest = uniform(50, 100);
    refuses(&mut digest, Refusal::WrongShape, |d| {
        d.merge(&uniform(50, 200))
    });

    let mut heavy = built(100);
    assert_eq!(heavy.add_weighted(1.0, f64::MAX), Ok(()));
    let mut target = heavy.clone();
    refuses(&mut target, Refusal::Overflow, |t| t.merge(&heavy));
}

#[test]
fn a_hand_built_centroid_is_finite_with_a_positive_weight() {
    assert_eq!(
        Centroid::new(1.5, 2.0),
        Some(Centroid {
            mean: 1.5,
            weight: 2.0
        })
    );
    for (mean, weight) in [
        (f64::NAN, 1.0),
        (1.0, 0.0),
        (1.0, -1.0),
        (1.0, f64::INFINITY),
    ] {
        assert_eq!(Centroid::new(mean, weight), None, "{mean} {weight}");
    }
}

// The reference model: the exact total weight and extremes. Every refusal
// leaves the digest equal to a clone taken before the call.
#[test]
fn a_random_stream_tracks_its_total_and_refusals_change_nothing() {
    let mut digest = built(20);
    let mut total = 0.0_f64;
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    for step in 0..3_000_u32 {
        state = crate::hash::mix64(state);
        let value = match state >> 61 {
            0 => f64::NAN,
            _ => f64::from(crate::hash::u64_low_u16(state)) - 30_000.0,
        };
        let weight = match (state >> 56) & 7 {
            0 => 0.0,
            1 => -1.0,
            _ => f64::from(crate::hash::u64_low_u8(state >> 8)) + 1.0,
        };
        let before = digest.clone();
        match digest.add_weighted(value, weight) {
            Ok(()) => total += weight,
            Err(refused) => {
                assert_matches!(
                    refused,
                    Refusal::NotFinite | Refusal::InvalidWeight,
                    "{step}"
                );
                assert_eq!(digest, before);
            }
        }
        assert!((digest.count() - total).abs() <= total * 1e-12, "{step}");
        assert!(digest.len() <= 200);
    }
}
