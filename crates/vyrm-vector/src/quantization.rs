use crate::contract::invalid;
use crate::ScoreMetric;
use serde::{Deserialize, Serialize};
use vyrm_core::Result;

/// Experimental per-vector symmetric 8-bit encoding.
///
/// This is deliberately not a planner-visible projection yet. It establishes
/// a deterministic size/error baseline while exact vectors remain available
/// for final reranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarQuantizedVector {
    pub scale: f32,
    pub values: Vec<i8>,
}

impl ScalarQuantizedVector {
    pub fn encode(values: &[f32]) -> Result<Self> {
        if values.is_empty() || values.len() > 1_048_576 {
            return invalid("quantized vector dimensions must be in 1..=1048576");
        }
        if values.iter().any(|value| !value.is_finite()) {
            return invalid("quantized vector input must be finite");
        }
        let maximum = values
            .iter()
            .map(|value| value.abs())
            .fold(0.0_f32, f32::max);
        if maximum == 0.0 {
            return Ok(Self {
                scale: 0.0,
                values: vec![0; values.len()],
            });
        }
        let scale = maximum / 127.0;
        let encoded = values
            .iter()
            .map(|value| (value / scale).round().clamp(-127.0, 127.0) as i8)
            .collect();
        Ok(Self {
            scale,
            values: encoded,
        })
    }

    pub fn dimensions(&self) -> usize {
        self.values.len()
    }

    pub fn estimated_payload_bytes(&self) -> usize {
        std::mem::size_of::<f32>() + self.values.len()
    }

    pub fn score(&self, query: &[f32], metric: ScoreMetric) -> Result<f64> {
        if query.len() != self.values.len() || query.iter().any(|value| !value.is_finite()) {
            return invalid("quantized score requires finite vectors with matching dimensions");
        }
        let decoded = self
            .values
            .iter()
            .map(|value| f64::from(*value) * f64::from(self.scale));
        match metric {
            ScoreMetric::Dot => Ok(query
                .iter()
                .map(|value| f64::from(*value))
                .zip(decoded)
                .map(|(left, right)| left * right)
                .sum()),
            ScoreMetric::Cosine => {
                let left_norm = query
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let decoded = self
                    .values
                    .iter()
                    .map(|value| f64::from(*value) * f64::from(self.scale))
                    .collect::<Vec<_>>();
                let right_norm = decoded
                    .iter()
                    .map(|value| value.powi(2))
                    .sum::<f64>()
                    .sqrt();
                if left_norm == 0.0 {
                    return invalid("cosine query must have non-zero norm");
                }
                if right_norm == 0.0 {
                    Ok(0.0)
                } else {
                    Ok(query
                        .iter()
                        .map(|value| f64::from(*value))
                        .zip(decoded)
                        .map(|(left, right)| left * right)
                        .sum::<f64>()
                        / (left_norm * right_norm))
                }
            }
            ScoreMetric::Euclidean => Ok(-query
                .iter()
                .map(|value| f64::from(*value))
                .zip(decoded)
                .map(|(left, right)| (left - right).powi(2))
                .sum::<f64>()
                .sqrt()),
            ScoreMetric::Manhattan => Ok(-query
                .iter()
                .map(|value| f64::from(*value))
                .zip(decoded)
                .map(|(left, right)| (left - right).abs())
                .sum::<f64>()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_quantization_is_deterministic_compact_and_bounded() {
        let values = [-1.0, -0.25, 0.0, 0.5, 1.0];
        let encoded = ScalarQuantizedVector::encode(&values).unwrap();
        assert_eq!(encoded, ScalarQuantizedVector::encode(&values).unwrap());
        assert!(encoded.estimated_payload_bytes() < values.len() * std::mem::size_of::<f32>());
        let exact = values
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>();
        let approximate = encoded.score(&values, ScoreMetric::Dot).unwrap();
        assert!((exact - approximate).abs() < 0.02);
    }
}
