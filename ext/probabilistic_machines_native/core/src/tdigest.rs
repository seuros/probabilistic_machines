//! Quantile estimation over a stream, in clusters instead of samples.

use alloc::vec::Vec;

use crate::{CodecError, TAG_TDIGEST, read8};

const HEADER_LEN: usize = 31;
const CENTROID_LEN: usize = 16;

/// Clusters the digest may hold per unit of compression.
const CLUSTERS_PER_COMPRESSION: usize = 10;

/// How far a total summed one point at a time may sit from the sum of the
/// clusters those points were folded into, relative to the total itself. Both
/// sums round once per addition, so a stream of fractional weights long enough
/// to exhaust this bound is one no `f64` total describes anyway.
const WEIGHT_DRIFT: f64 = 1e-6;

/// A weighted point: the mean of the values that collapsed into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centroid {
    pub mean: f64,
    pub weight: f64,
}

/// Estimates quantiles from clusters whose weight is capped by how close they
/// sit to the tails.
///
/// A cluster covering the quantile range around `q` holds at most
/// `4 * n * q * (1 - q) / compression` of the `n` points seen, so a quantile is
/// off by at most the width of one cluster there: about `1 / compression` of
/// the distribution in the middle, shrinking towards the extremes, where
/// clusters are single points and `quantile(0.0)` and `quantile(1.0)` are
/// exact.
///
/// The cluster count grows with `compression * ln(n)`, not with `n`:
/// compression 100 holds 266 clusters at a thousand points and 809 at a
/// million. Below `10 * compression` points nothing is collapsed and the
/// quantiles are exact.
///
/// ```
/// use probabilistic_machines::TDigest;
///
/// let mut latency = TDigest::new(100);
/// for ms in 1..=10_000 {
///     latency.add(f64::from(ms));
/// }
///
/// assert!((latency.quantile(0.5) - 5_000.0).abs() < 100.0);
/// assert!((latency.quantile(0.99) - 9_900.0).abs() < 50.0);
/// assert_eq!(latency.quantile(0.0), 1.0);
/// assert_eq!(latency.quantile(1.0), 10_000.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TDigest {
    compression: u16,
    centroids: Vec<Centroid>,
    count: f64,
    minimum: f64,
    maximum: f64,
}

impl TDigest {
    #[must_use]
    pub const fn new(compression: u16) -> Self {
        Self {
            compression,
            centroids: Vec::new(),
            count: 0.0,
            minimum: f64::INFINITY,
            maximum: f64::NEG_INFINITY,
        }
    }

    #[must_use]
    pub const fn compression(&self) -> u16 {
        self.compression
    }

    /// Total weight added, which for unweighted points is the item count.
    #[must_use]
    pub const fn count(&self) -> f64 {
        self.count
    }

    /// Smallest value seen, [`f64::INFINITY`] when empty.
    #[must_use]
    pub const fn minimum(&self) -> f64 {
        self.minimum
    }

    /// Largest value seen, [`f64::NEG_INFINITY`] when empty.
    #[must_use]
    pub const fn maximum(&self) -> f64 {
        self.maximum
    }

    /// Clusters currently held, ordered by mean.
    #[must_use]
    pub fn centroids(&self) -> &[Centroid] {
        &self.centroids
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.centroids.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.centroids.is_empty()
    }

    /// Adds one observation of `value`, under the same precondition as
    /// [`add_weighted`](Self::add_weighted).
    pub fn add(&mut self, value: f64) {
        self.add_weighted(value, 1.0);
    }

    /// Adds a value standing for `weight` observations.
    ///
    /// `value` must be finite, and `weight` finite and positive: the quantiles
    /// of a stream carrying anything else mean nothing, and the clusters it
    /// leaves are not ones [`decode`](Self::decode) reads back.
    pub fn add_weighted(&mut self, value: f64, weight: f64) {
        let at = self
            .centroids
            .partition_point(|centroid| centroid.mean < value);
        self.centroids.insert(
            at,
            Centroid {
                mean: value,
                weight,
            },
        );
        self.count += weight;
        if value < self.minimum {
            self.minimum = value;
        }
        if value > self.maximum {
            self.maximum = value;
        }
        if self.centroids.len() > self.capacity() {
            self.compress();
        }
    }

    /// Value at quantile `q`, [`f64::NAN`] when empty.
    #[must_use]
    pub fn quantile(&self, q: f64) -> f64 {
        if self.centroids.is_empty() {
            return f64::NAN;
        }

        let target = q * self.count;
        let mut processed = 0.0;
        let mut left_rank = 0.0;
        let mut left_mean = self.minimum;

        for centroid in &self.centroids {
            let half = centroid.weight * 0.5;
            let rank = processed + half;
            if target < rank {
                return lerp(target, left_rank, left_mean, rank, centroid.mean);
            }
            left_rank = rank;
            left_mean = centroid.mean;
            processed += centroid.weight;
        }

        lerp(target, left_rank, left_mean, self.count, self.maximum)
    }

    /// Fraction of the stream at or below `value`, [`f64::NAN`] when empty.
    #[must_use]
    pub fn cdf(&self, value: f64) -> f64 {
        if self.centroids.is_empty() {
            return f64::NAN;
        }
        if value < self.minimum {
            return 0.0;
        }
        if value > self.maximum {
            return 1.0;
        }

        let mut processed = 0.0;
        let mut left_rank = 0.0;
        let mut left_mean = self.minimum;

        for centroid in &self.centroids {
            let half = centroid.weight * 0.5;
            let rank = processed + half;
            if value < centroid.mean {
                return lerp(value, left_mean, left_rank, centroid.mean, rank) / self.count;
            }
            left_rank = rank;
            left_mean = centroid.mean;
            processed += centroid.weight;
        }

        lerp(value, left_mean, left_rank, self.maximum, self.count) / self.count
    }

    /// Folds another digest in. The result is the same digest you would get by
    /// feeding one instance both streams, up to the order clusters formed in.
    pub fn merge(&mut self, other: &Self) {
        self.centroids.extend_from_slice(&other.centroids);
        self.centroids
            .sort_unstable_by(|left, right| left.mean.total_cmp(&right.mean));
        self.count += other.count;
        if other.minimum < self.minimum {
            self.minimum = other.minimum;
        }
        if other.maximum > self.maximum {
            self.maximum = other.maximum;
        }
        self.compress();
    }

    /// Collapses adjacent clusters up to the weight their quantile allows.
    pub fn compress(&mut self) {
        let factor = 4.0 / f64::from(self.compression) * self.count;
        let mut merged = Vec::with_capacity(self.centroids.len());
        let mut processed = 0.0;
        let mut open: Option<Centroid> = None;

        for &next in &self.centroids {
            let Some(current) = open else {
                open = Some(next);
                continue;
            };
            let combined = current.weight + next.weight;
            let half = combined * 0.5;
            let rank = (processed + half) / self.count;
            if combined <= factor * rank * (1.0 - rank) {
                open = Some(fold(current, next));
            } else {
                processed += current.weight;
                merged.push(current);
                open = Some(next);
            }
        }

        merged.extend(open);
        self.centroids = merged;

        // A cluster holding nearly all the weight pins every later rank at 1.0,
        // where the scale function above can merge nothing.
        while self.centroids.len() > self.capacity() {
            self.centroids = self
                .centroids
                .chunks(2)
                .map(|pair| match pair {
                    [left, right] => fold(*left, *right),
                    _ => pair[0],
                })
                .collect();
        }
    }

    /// Bytes [`encode`](Self::encode) needs, which grows with the cluster count.
    #[must_use]
    pub const fn encoded_len(&self) -> usize {
        HEADER_LEN + self.centroids.len() * CENTROID_LEN
    }

    /// Writes the digest into `out`, returning the number of bytes written.
    ///
    /// The frame is a header of the tag, the compression, the cluster count,
    /// the total weight and the two extremes, then a record of mean and weight
    /// per cluster, every scalar little-endian.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`encoded_len`](Self::encoded_len).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let length = self.encoded_len();
        let Some(frame) = out.get_mut(..length) else {
            return Err(CodecError::Truncated);
        };
        self.write_frame(frame);
        Ok(length)
    }

    /// The digest's frame, exactly [`encoded_len`](Self::encoded_len) bytes.
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; self.encoded_len()].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

    /// Fills a frame of exactly [`encoded_len`](Self::encoded_len) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "capacity caps the cluster count at compression * CLUSTERS_PER_COMPRESSION"
        )]
        let clusters = self.centroids.len() as u32;
        frame[0] = TAG_TDIGEST;
        frame[1..3].copy_from_slice(&self.compression.to_le_bytes());
        frame[3..7].copy_from_slice(&clusters.to_le_bytes());
        frame[7..15].copy_from_slice(&self.count.to_le_bytes());
        frame[15..23].copy_from_slice(&self.minimum.to_le_bytes());
        frame[23..31].copy_from_slice(&self.maximum.to_le_bytes());

        let (slots, _) = frame[HEADER_LEN..].as_chunks_mut::<CENTROID_LEN>();
        for (slot, centroid) in slots.iter_mut().zip(&self.centroids) {
            slot[..8].copy_from_slice(&centroid.mean.to_le_bytes());
            slot[8..].copy_from_slice(&centroid.weight.to_le_bytes());
        }
    }

    /// Reads a digest written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is shorter than a header or than
    /// the clusters that header counts,
    /// [`CodecError::Tag`] when it holds another structure,
    /// [`CodecError::Parameters`] when the clusters, the total weight and the
    /// extremes contradict each other.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let Some(header) = bytes.get(..HEADER_LEN) else {
            return Err(CodecError::Truncated);
        };
        if header[0] != TAG_TDIGEST {
            return Err(CodecError::Tag);
        }
        let counted = u32::from_le_bytes([header[3], header[4], header[5], header[6]]);
        let Ok(clusters) = usize::try_from(counted) else {
            return Err(CodecError::Truncated);
        };
        if (bytes.len() - HEADER_LEN) / CENTROID_LEN < clusters {
            return Err(CodecError::Truncated);
        }
        let body = &bytes[HEADER_LEN..HEADER_LEN + clusters * CENTROID_LEN];

        let mut digest = Self::new(u16::from_le_bytes([header[1], header[2]]));
        digest.count = f64::from_le_bytes(read8(&header[7..15]));
        digest.minimum = f64::from_le_bytes(read8(&header[15..23]));
        digest.maximum = f64::from_le_bytes(read8(&header[23..31]));
        let (slots, _) = body.as_chunks::<CENTROID_LEN>();
        digest.centroids = slots
            .iter()
            .map(|slot| Centroid {
                mean: f64::from_le_bytes(read8(&slot[..8])),
                weight: f64::from_le_bytes(read8(&slot[8..])),
            })
            .collect();
        if digest.is_coherent() {
            Ok(digest)
        } else {
            Err(CodecError::Parameters)
        }
    }

    /// Whether the clusters carry the total weight and reach the extremes the
    /// header claims, as every digest built by adding points does.
    fn is_coherent(&self) -> bool {
        if self.centroids.len() > self.capacity() {
            return false;
        }
        let (Some(first), Some(last)) = (self.centroids.first(), self.centroids.last()) else {
            return self.count == 0.0
                && self.minimum == f64::INFINITY
                && self.maximum == f64::NEG_INFINITY;
        };
        if !self.count.is_finite() || self.count <= 0.0 {
            return false;
        }
        let weight: f64 = self.centroids.iter().map(|centroid| centroid.weight).sum();
        weight.is_finite()
            && self.centroids.iter().all(|centroid| {
                centroid.mean.is_finite() && centroid.weight.is_finite() && centroid.weight > 0.0
            })
            && self
                .centroids
                .is_sorted_by(|left, right| left.mean <= right.mean)
            && (weight - self.count).abs() <= self.count * WEIGHT_DRIFT
            && self.minimum.is_finite()
            && self.maximum.is_finite()
            && self.minimum <= first.mean
            && last.mean <= self.maximum
    }

    /// Clusters held before compression runs, and never below the one cluster
    /// folding cannot reduce further.
    fn capacity(&self) -> usize {
        (usize::from(self.compression) * CLUSTERS_PER_COMPRESSION).max(1)
    }
}

/// The weighted mean of two adjacent clusters.
///
/// Interpolating at half scale keeps the span of two opposite-sign extremes
/// inside the finite range, and doubling a mean that lies between two finite
/// means cannot leave it.
// Clippy only suggests `mul_add` when `std` provides it; `core` has no
// `f64::mul_add`, so the suggestion would break the `no_std` build.
#[cfg_attr(
    feature = "std",
    expect(clippy::suboptimal_flops, reason = "mul_add is std-only")
)]
fn fold(left: Centroid, right: Centroid) -> Centroid {
    let weight = left.weight + right.weight;
    let low = left.mean * 0.5;
    let high = right.mean * 0.5;
    Centroid {
        mean: ((high - low) * (right.weight / weight) + low) * 2.0,
        weight,
    }
}

/// Where `at` falls between two points, read off the line through them.
///
/// A span of two opposite-sign extremes is wider than [`f64::MAX`], and an
/// infinite span made every finite `at` land on the low end. Such a span is
/// halved the way [`fold`] averages, which costs nothing while the halves stay
/// normal but rounds a subnormal span to zero, so the span is only halved when
/// it does not fit.
// Clippy only suggests `mul_add` when `std` provides it; `core` has no
// `f64::mul_add`, so the suggestion would break the `no_std` build.
#[cfg_attr(
    feature = "std",
    expect(clippy::suboptimal_flops, reason = "mul_add is std-only")
)]
fn lerp(at: f64, low_x: f64, low_y: f64, high_x: f64, high_y: f64) -> f64 {
    if high_x <= low_x {
        return high_y;
    }
    let span = high_x - low_x;
    let fraction = if span.is_finite() {
        (at - low_x) / span
    } else {
        let low = low_x * 0.5;
        (at * 0.5 - low) / (high_x * 0.5 - low)
    };
    low_y * (1.0 - fraction) + high_y * fraction
}

#[cfg(test)]
mod tests;
