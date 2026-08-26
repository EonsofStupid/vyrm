//! Deterministic MSE TurboQuant codec for dense vector artifacts.
//!
//! This follows the production-relevant MSE path: randomized orthogonal
//! Hadamard rotation, per-vector length normalization onto a fixed standard
//! normal Lloyd-Max codebook, packed low-bit centroids, asymmetric full-
//! precision query scoring, and per-vector centroid-norm correction. It is a
//! codec/oracle component; planner-visible persisted serving lands separately.

use crate::contract::invalid;
use crate::ScoreMetric;
use rrd_core::Result;
use serde::{Deserialize, Serialize};

pub const TURBOQUANT_FORMAT_VERSION: u16 = 1;
const MAX_DIMENSIONS: usize = 1_048_576;
const ROTATION_ROUNDS: usize = 3;

const CENTROIDS_1: [f32; 2] = [-0.797_884_6, 0.797_884_6];
const CENTROIDS_2: [f32; 4] = [-1.510, -0.4528, 0.4528, 1.510];
const CENTROIDS_4: [f32; 16] = [
    -2.733, -2.069, -1.618, -1.256, -0.9424, -0.6568, -0.3881, -0.1284, 0.1284, 0.3881, 0.6568,
    0.9424, 1.256, 1.618, 2.069, 2.733,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurboQuantBits {
    Bits4,
    Bits2,
    Bits1_5,
    Bits1,
}

impl TurboQuantBits {
    const fn stored_bits(self) -> u8 {
        match self {
            Self::Bits4 => 4,
            Self::Bits2 => 2,
            Self::Bits1_5 | Self::Bits1 => 1,
        }
    }

    const fn centroids(self) -> &'static [f32] {
        match self {
            Self::Bits4 => &CENTROIDS_4,
            Self::Bits2 => &CENTROIDS_2,
            Self::Bits1_5 | Self::Bits1 => &CENTROIDS_1,
        }
    }

    pub(crate) fn padded_dimensions(self, dimensions: usize) -> usize {
        match self {
            Self::Bits4 => dimensions.next_multiple_of(2),
            Self::Bits2 => dimensions.next_multiple_of(4),
            Self::Bits1_5 => dimensions
                .checked_mul(3)
                .and_then(|value| value.checked_add(1))
                .map(|value| value / 2)
                .unwrap_or(usize::MAX)
                .next_multiple_of(8),
            Self::Bits1 => dimensions.next_multiple_of(8),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurboQuantVector {
    pub format_version: u16,
    pub bits: TurboQuantBits,
    pub seed: u64,
    pub dimensions: usize,
    pub padded_dimensions: usize,
    pub original_norm: f32,
    pub centroid_norm: f32,
    pub packed: Vec<u8>,
}

impl TurboQuantVector {
    pub fn encode(values: &[f32], bits: TurboQuantBits, seed: u64) -> Result<Self> {
        validate_input(values)?;
        let padded_dimensions = bits.padded_dimensions(values.len());
        if padded_dimensions > MAX_DIMENSIONS.saturating_mul(2) {
            return invalid("TurboQuant padded dimensions exceed the codec bound");
        }
        let original_norm = l2_norm(values);
        let mut rotated = vec![0.0_f64; padded_dimensions];
        for (target, source) in rotated.iter_mut().zip(values) {
            *target = f64::from(*source);
        }
        Rotation::new(padded_dimensions, seed).apply(&mut rotated);
        if original_norm > 0.0 {
            let scale = (padded_dimensions as f64).sqrt() / f64::from(original_norm);
            for value in &mut rotated {
                *value *= scale;
            }
        }
        let indices = rotated
            .iter()
            .map(|value| nearest_centroid(bits.centroids(), *value))
            .collect::<Vec<_>>();
        let centroid_norm = indices
            .iter()
            .map(|index| f64::from(bits.centroids()[usize::from(*index)]).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        let vector = Self {
            format_version: TURBOQUANT_FORMAT_VERSION,
            bits,
            seed,
            dimensions: values.len(),
            padded_dimensions,
            original_norm,
            centroid_norm,
            packed: pack(&indices, bits.stored_bits()),
        };
        vector.validate()?;
        Ok(vector)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format_version != TURBOQUANT_FORMAT_VERSION
            || self.dimensions == 0
            || self.dimensions > MAX_DIMENSIONS
            || self.padded_dimensions != self.bits.padded_dimensions(self.dimensions)
        {
            return invalid("TurboQuant vector format or dimensions are invalid");
        }
        if !self.original_norm.is_finite()
            || self.original_norm < 0.0
            || !self.centroid_norm.is_finite()
            || self.centroid_norm <= 0.0
        {
            return invalid("TurboQuant vector norms are invalid");
        }
        let expected = packed_len(self.padded_dimensions, self.bits.stored_bits());
        if self.packed.len() != expected {
            return invalid("TurboQuant packed byte length differs from its dimensions");
        }
        let levels = self.bits.centroids().len();
        if unpack(
            &self.packed,
            self.padded_dimensions,
            self.bits.stored_bits(),
        )
        .iter()
        .any(|index| usize::from(*index) >= levels)
        {
            return invalid("TurboQuant packed centroid index is out of range");
        }
        Ok(())
    }

    pub fn estimated_payload_bytes(&self) -> usize {
        self.packed.len()
            + std::mem::size_of::<u16>()
            + std::mem::size_of::<u64>()
            + 2 * std::mem::size_of::<usize>()
            + 2 * std::mem::size_of::<f32>()
    }

    pub fn packed_vector_bytes(&self) -> usize {
        self.packed.len()
    }

    pub fn score(&self, query: &[f32], metric: ScoreMetric) -> Result<f64> {
        self.validate()?;
        if query.len() != self.dimensions || query.iter().any(|value| !value.is_finite()) {
            return invalid("TurboQuant score requires finite vectors with matching dimensions");
        }
        let query_norm = l2_norm(query);
        if metric == ScoreMetric::Cosine && query_norm == 0.0 {
            return invalid("cosine query must have non-zero norm");
        }
        let mut rotated_query = vec![0.0_f64; self.padded_dimensions];
        for (target, source) in rotated_query.iter_mut().zip(query) {
            *target = f64::from(*source);
        }
        let rotation = Rotation::new(self.padded_dimensions, self.seed);
        rotation.apply(&mut rotated_query);
        let centroids = self.decoded_centroids();
        let raw_dot = rotated_query
            .iter()
            .zip(&centroids)
            .map(|(query, stored)| query * stored)
            .sum::<f64>();
        let scale = if self.original_norm == 0.0 {
            0.0
        } else {
            f64::from(self.original_norm) / f64::from(self.centroid_norm)
        };
        let dot = raw_dot * scale;
        match metric {
            ScoreMetric::Dot => Ok(dot),
            ScoreMetric::Cosine => {
                if self.original_norm == 0.0 {
                    Ok(0.0)
                } else {
                    Ok(raw_dot / (f64::from(query_norm) * f64::from(self.centroid_norm)))
                }
            }
            ScoreMetric::Euclidean => {
                let squared = f64::from(query_norm).powi(2) + f64::from(self.original_norm).powi(2)
                    - 2.0 * dot;
                Ok(-squared.max(0.0).sqrt())
            }
            ScoreMetric::Manhattan => {
                let mut reconstructed = centroids
                    .into_iter()
                    .map(|value| value * scale)
                    .collect::<Vec<_>>();
                rotation.apply_inverse(&mut reconstructed);
                Ok(-query
                    .iter()
                    .map(|value| f64::from(*value))
                    .zip(reconstructed)
                    .take(self.dimensions)
                    .map(|(left, right)| (left - right).abs())
                    .sum::<f64>())
            }
        }
    }

    pub fn reconstruct(&self) -> Result<Vec<f32>> {
        self.validate()?;
        let scale = if self.original_norm == 0.0 {
            0.0
        } else {
            f64::from(self.original_norm) / f64::from(self.centroid_norm)
        };
        let mut reconstructed = self
            .decoded_centroids()
            .into_iter()
            .map(|value| value * scale)
            .collect::<Vec<_>>();
        Rotation::new(self.padded_dimensions, self.seed).apply_inverse(&mut reconstructed);
        Ok(reconstructed
            .into_iter()
            .take(self.dimensions)
            .map(|value| value as f32)
            .collect())
    }

    fn decoded_centroids(&self) -> Vec<f64> {
        let centroids = self.bits.centroids();
        unpack(
            &self.packed,
            self.padded_dimensions,
            self.bits.stored_bits(),
        )
        .into_iter()
        .map(|index| f64::from(centroids[usize::from(index)]))
        .collect()
    }
}

fn validate_input(values: &[f32]) -> Result<()> {
    if values.is_empty() || values.len() > MAX_DIMENSIONS {
        return invalid("TurboQuant dimensions must be in 1..=1048576");
    }
    if values.iter().any(|value| !value.is_finite()) {
        return invalid("TurboQuant input must be finite");
    }
    Ok(())
}

fn l2_norm(values: &[f32]) -> f32 {
    values
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt() as f32
}

fn nearest_centroid(centroids: &[f32], value: f64) -> u8 {
    centroids
        .windows(2)
        .position(|pair| value <= f64::from((pair[0] + pair[1]) / 2.0))
        .unwrap_or(centroids.len() - 1) as u8
}

fn packed_len(dimensions: usize, bits: u8) -> usize {
    dimensions.saturating_mul(usize::from(bits)).div_ceil(8)
}

fn pack(indices: &[u8], bits: u8) -> Vec<u8> {
    let mut packed = vec![0_u8; packed_len(indices.len(), bits)];
    let mut bit_offset = 0usize;
    for index in indices {
        let byte = bit_offset / 8;
        let shift = bit_offset % 8;
        packed[byte] |= *index << shift;
        if shift + usize::from(bits) > 8 {
            packed[byte + 1] |= *index >> (8 - shift);
        }
        bit_offset += usize::from(bits);
    }
    packed
}

fn unpack(packed: &[u8], dimensions: usize, bits: u8) -> Vec<u8> {
    let mask = (1_u16 << bits) - 1;
    (0..dimensions)
        .map(|index| {
            let bit_offset = index * usize::from(bits);
            let byte = bit_offset / 8;
            let shift = bit_offset % 8;
            let lower = u16::from(packed[byte]);
            let upper = packed.get(byte + 1).copied().map(u16::from).unwrap_or(0);
            (((lower | (upper << 8)) >> shift) & mask) as u8
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Rotation {
    permutations: [Vec<usize>; ROTATION_ROUNDS],
}

impl Rotation {
    fn new(dimensions: usize, seed: u64) -> Self {
        Self {
            permutations: std::array::from_fn(|round| {
                permutation(dimensions, mix(seed, round as u64 + 1))
            }),
        }
    }

    fn apply(&self, values: &mut [f64]) {
        hadamard_chunks(values);
        let mut scratch = vec![0.0; values.len()];
        for permutation in &self.permutations {
            for (target, source) in scratch.iter_mut().zip(permutation) {
                *target = values[*source];
            }
            values.copy_from_slice(&scratch);
            hadamard_chunks(values);
        }
    }

    fn apply_inverse(&self, values: &mut [f64]) {
        hadamard_chunks(values);
        let mut scratch = vec![0.0; values.len()];
        for permutation in self.permutations.iter().rev() {
            for (target, source) in permutation.iter().enumerate() {
                scratch[*source] = values[target];
            }
            values.copy_from_slice(&scratch);
            hadamard_chunks(values);
        }
    }
}

fn hadamard_chunks(values: &mut [f64]) {
    let mut remaining = values.len();
    let mut offset = 0usize;
    while remaining > 0 {
        let size = 1usize << remaining.ilog2();
        let chunk = &mut values[offset..offset + size];
        hadamard(chunk);
        let normalization = 1.0 / (size as f64).sqrt();
        for value in chunk {
            *value *= normalization;
        }
        offset += size;
        remaining -= size;
    }
}

fn hadamard(values: &mut [f64]) {
    let mut width = 1usize;
    while width < values.len() {
        for start in (0..values.len()).step_by(width * 2) {
            for offset in 0..width {
                let left = values[start + offset];
                let right = values[start + offset + width];
                values[start + offset] = left + right;
                values[start + offset + width] = left - right;
            }
        }
        width *= 2;
    }
}

fn permutation(dimensions: usize, seed: u64) -> Vec<usize> {
    let mut values = (0..dimensions).collect::<Vec<_>>();
    let mut state = seed;
    for upper in (1..dimensions).rev() {
        state = mix(state, upper as u64);
        let selected = (state as usize) % (upper + 1);
        values.swap(upper, selected);
    }
    values
}

fn mix(mut value: u64, salt: u64) -> u64 {
    value ^= salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_norm_preserving_and_invertible_for_irregular_dimensions() {
        for dimensions in [1, 3, 63, 384, 769] {
            let rotation = Rotation::new(dimensions, 42);
            let original = deterministic_vector(dimensions, dimensions as u64);
            let original = original.into_iter().map(f64::from).collect::<Vec<_>>();
            let mut rotated = original.clone();
            rotation.apply(&mut rotated);
            let before = original.iter().map(|value| value * value).sum::<f64>();
            let after = rotated.iter().map(|value| value * value).sum::<f64>();
            assert!((before - after).abs() < 1e-8 * before.max(1.0));
            rotation.apply_inverse(&mut rotated);
            for (expected, actual) in original.iter().zip(rotated) {
                assert!((expected - actual).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn all_bit_modes_are_deterministic_packed_and_score_asymmetrically() {
        let values = deterministic_vector(384, 7);
        let query = deterministic_vector(384, 9);
        for (bits, expected_bytes) in [
            (TurboQuantBits::Bits4, 192),
            (TurboQuantBits::Bits2, 96),
            (TurboQuantBits::Bits1_5, 72),
            (TurboQuantBits::Bits1, 48),
        ] {
            let encoded = TurboQuantVector::encode(&values, bits, 11).unwrap();
            assert_eq!(encoded.packed_vector_bytes(), expected_bytes);
            assert_eq!(
                encoded,
                TurboQuantVector::encode(&values, bits, 11).unwrap()
            );
            assert!(encoded.score(&query, ScoreMetric::Dot).unwrap().is_finite());
            assert!(encoded
                .score(&query, ScoreMetric::Cosine)
                .unwrap()
                .is_finite());
            assert!(encoded
                .score(&query, ScoreMetric::Euclidean)
                .unwrap()
                .is_finite());
            assert!(encoded
                .score(&query, ScoreMetric::Manhattan)
                .unwrap()
                .is_finite());
        }
    }

    #[test]
    fn four_bit_reconstruction_and_top_k_recall_are_oracle_gated() {
        let dimensions = 128;
        let corpus = (0..256)
            .map(|index| deterministic_vector(dimensions, index + 100))
            .collect::<Vec<_>>();
        let encoded = corpus
            .iter()
            .map(|vector| TurboQuantVector::encode(vector, TurboQuantBits::Bits4, 91).unwrap())
            .collect::<Vec<_>>();
        let mut recall_total = 0.0;
        for query_seed in 0..16 {
            let query = deterministic_vector(dimensions, query_seed + 10_000);
            let exact = top_k(&corpus, &query, 10, |vector, query| {
                vector
                    .iter()
                    .zip(query)
                    .map(|(left, right)| f64::from(*left) * f64::from(*right))
                    .sum()
            });
            let approximate = top_k(&encoded, &query, 10, |vector, query| {
                vector.score(query, ScoreMetric::Dot).unwrap()
            });
            let overlap = exact
                .iter()
                .filter(|identity| approximate.contains(identity))
                .count();
            recall_total += overlap as f64 / 10.0;
        }
        let mean_recall = recall_total / 16.0;
        assert!(mean_recall >= 0.80, "four-bit mean recall@10={mean_recall}");

        let reconstructed = encoded[0].reconstruct().unwrap();
        let original_norm = f64::from(l2_norm(&corpus[0]));
        let reconstructed_norm = f64::from(l2_norm(&reconstructed));
        assert!((original_norm - reconstructed_norm).abs() < 1e-4);
    }

    #[test]
    fn malformed_and_non_finite_inputs_fail_closed() {
        assert!(TurboQuantVector::encode(&[], TurboQuantBits::Bits4, 1).is_err());
        assert!(TurboQuantVector::encode(&[f32::NAN], TurboQuantBits::Bits4, 1).is_err());
        let mut encoded = TurboQuantVector::encode(&[1.0, 2.0], TurboQuantBits::Bits2, 1).unwrap();
        encoded.packed.clear();
        assert!(encoded.validate().is_err());
    }

    fn deterministic_vector(dimensions: usize, seed: u64) -> Vec<f32> {
        let mut state = seed;
        (0..dimensions)
            .map(|index| {
                state = mix(state, index as u64 + 1);
                let unit = (state >> 40) as f32 / ((1_u32 << 24) - 1) as f32;
                unit * 2.0 - 1.0
            })
            .collect()
    }

    fn top_k<T>(
        corpus: &[T],
        query: &[f32],
        limit: usize,
        score: impl Fn(&T, &[f32]) -> f64,
    ) -> Vec<usize> {
        let mut scores = corpus
            .iter()
            .enumerate()
            .map(|(identity, vector)| (identity, score(vector, query)))
            .collect::<Vec<_>>();
        scores.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        scores
            .into_iter()
            .take(limit)
            .map(|(identity, _)| identity)
            .collect()
    }
}
