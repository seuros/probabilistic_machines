use super::{CENTROID_LEN, HEADER_LEN, TDigest};
use crate::CodecError;

fn uniform(points: u32, compression: u16) -> TDigest {
    let mut digest = TDigest::new(compression);
    for point in 1..=points {
        digest.add(f64::from(point));
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
    let digest = TDigest::new(100);
    assert!(digest.is_empty());
    assert!(digest.quantile(0.5).is_nan());
    assert!(digest.cdf(0.5).is_nan());
}

#[test]
fn a_single_point_is_every_quantile() {
    let mut digest = TDigest::new(100);
    digest.add(42.0);
    for &quantile in &[0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!((digest.quantile(quantile) - 42.0).abs() < f64::EPSILON);
    }
}

#[test]
fn a_thousand_copies_of_one_value_do_not_divide_by_zero() {
    let mut digest = TDigest::new(50);
    for _ in 0..1_000 {
        digest.add(42.0);
    }

    assert!((digest.quantile(0.5) - 42.0).abs() < f64::EPSILON);
    assert!(digest.cdf(42.0).is_finite());
    assert!((digest.count() - 1_000.0).abs() < f64::EPSILON);
}

#[test]
fn a_weighted_point_stands_for_repeats_of_it() {
    let mut weighted = TDigest::new(200);
    let mut repeated = TDigest::new(200);
    for value in 1..=100_u32 {
        weighted.add_weighted(f64::from(value), 10.0);
        for _ in 0..10 {
            repeated.add(f64::from(value));
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
    let mut left = TDigest::new(100);
    let mut right = TDigest::new(100);
    let mut together = TDigest::new(100);

    for point in 1..=20_000_u32 {
        let value = f64::from(point);
        if point % 2 == 0 {
            left.add(value);
        } else {
            right.add(value);
        }
        together.add(value);
    }

    left.merge(&right);
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
    digest.merge(&TDigest::new(100));

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

    assert_eq!(digest.encode(&mut frame[..10]), Err(CodecError::Truncated));
    assert_eq!(TDigest::decode(&frame[..10]), Err(CodecError::Truncated));
    assert_eq!(
        TDigest::decode(&frame[..written - 1]),
        Err(CodecError::Truncated)
    );

    let mut wrong = frame.clone();
    wrong[0] = 0x00;
    assert_eq!(TDigest::decode(&wrong), Err(CodecError::Tag));
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
        Err(CodecError::Truncated)
    );
    assert_eq!(
        TDigest::decode(&frame[..frame.len() - CENTROID_LEN]),
        Err(CodecError::Truncated)
    );
    assert_eq!(TDigest::decode(&frame), Ok(digest));

    let mut heavier = frame.clone();
    heavier[7..15].copy_from_slice(&(count * (1.0 + 1e-3)).to_le_bytes());
    assert_eq!(TDigest::decode(&heavier), Err(CodecError::Parameters));

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
            Err(CodecError::Parameters),
            "total {total}"
        );
    }

    let mut overflowing = frame.clone();
    overflowing[7..15].copy_from_slice(&f64::MAX.to_le_bytes());
    for slot in 0..digest.len() {
        let at = HEADER_LEN + slot * CENTROID_LEN + 8;
        overflowing[at..at + 8].copy_from_slice(&f64::MAX.to_le_bytes());
    }
    assert_eq!(TDigest::decode(&overflowing), Err(CodecError::Parameters));

    assert_eq!(TDigest::decode(&frame), Ok(digest));
    assert_eq!(
        TDigest::decode(&TDigest::new(10).encoded()),
        Ok(TDigest::new(10))
    );
}

// Subtracting opposite-sign extremes overflowed before the multiply, so a
// finite population produced an infinite quantile and q75 fell below q50.
#[test]
fn opposite_sign_extremes_interpolate_to_a_finite_monotonic_answer() {
    let mut digest = TDigest::new(100);
    digest.add(-1e308);
    digest.add(1e308);

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
    let mut digest = TDigest::new(1);
    for _ in 0..6 {
        digest.add(-1e308);
    }
    for _ in 0..6 {
        digest.add(1e308);
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
    let mut wide = TDigest::new(100);
    wide.add(-f64::MAX);
    wide.add(f64::MAX);

    let middle = wide.cdf(0.0);
    assert!((middle - 0.5).abs() < f64::EPSILON, "cdf(0) was {middle}");
    assert!((wide.quantile(0.0) + f64::MAX).abs() < f64::EPSILON);
    assert!((wide.quantile(1.0) - f64::MAX).abs() < f64::EPSILON);

    let step = 2.0 * f64::EPSILON;
    let mut narrow = TDigest::new(100);
    narrow.add(1.0);
    narrow.add(1.0 + step + step);

    let halfway = narrow.cdf(1.0 + step);
    assert!((halfway - 0.5).abs() < f64::EPSILON, "cdf was {halfway}");
}
