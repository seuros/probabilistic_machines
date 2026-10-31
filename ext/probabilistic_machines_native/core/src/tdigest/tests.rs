use super::TDigest;
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
        Err(CodecError::Parameters)
    );

    let mut wrong = frame.clone();
    wrong[0] = 0x00;
    assert_eq!(TDigest::decode(&wrong), Err(CodecError::Tag));
}
