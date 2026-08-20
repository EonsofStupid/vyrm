//! Canonical non-relational values carried by one data-runtime commit.
//!
//! These are truth-bearing values. Search, time, and spatial indexes are
//! rebuildable projections over them; object bytes are published separately
//! and become visible only when an [`ObjectReference`] is committed.

use crate::{digest, Error, Millis, Result, RuntimeId, RuntimeProperties, RuntimeRef, RuntimeType};
use serde::{Deserialize, Serialize};

const MAX_VECTOR_DIMENSIONS: usize = 1_048_576;

/// Canonical vector payload. Exact values are retained even when a later
/// projection uses quantization or an approximate index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VectorValue {
    Dense {
        values: Vec<f32>,
    },
    Sparse {
        dimensions: u32,
        indices: Vec<u32>,
        values: Vec<f32>,
    },
    MultiDense {
        dimensions: u32,
        vectors: Vec<Vec<f32>>,
    },
}

impl VectorValue {
    pub fn dimensions(&self) -> usize {
        match self {
            Self::Dense { values } => values.len(),
            Self::Sparse { dimensions, .. } | Self::MultiDense { dimensions, .. } => {
                *dimensions as usize
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        let dimensions = self.dimensions();
        if dimensions == 0 || dimensions > MAX_VECTOR_DIMENSIONS {
            return invalid(format!(
                "vector dimensions must be in 1..={MAX_VECTOR_DIMENSIONS}, got {dimensions}"
            ));
        }
        match self {
            Self::Dense { values } => validate_finite(values),
            Self::Sparse {
                dimensions,
                indices,
                values,
            } => {
                if indices.is_empty() || indices.len() != values.len() {
                    return invalid(
                        "sparse vector indices and values must have the same non-zero length",
                    );
                }
                if indices.iter().any(|index| index >= dimensions) {
                    return invalid("sparse vector index exceeds declared dimensions");
                }
                if indices.windows(2).any(|pair| pair[0] >= pair[1]) {
                    return invalid("sparse vector indices must be strictly increasing");
                }
                validate_finite(values)
            }
            Self::MultiDense {
                dimensions,
                vectors,
            } => {
                if vectors.is_empty() {
                    return invalid("multi-vector must contain at least one vector");
                }
                for vector in vectors {
                    if vector.len() != *dimensions as usize {
                        return invalid("every multi-vector row must match declared dimensions");
                    }
                    validate_finite(vector)?;
                }
                Ok(())
            }
        }
    }

    fn is_unit_l2(&self) -> bool {
        fn normalized(values: &[f32]) -> bool {
            let squared_norm = values
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>();
            (squared_norm - 1.0).abs() <= 1e-4
        }
        match self {
            Self::Dense { values } | Self::Sparse { values, .. } => normalized(values),
            Self::MultiDense { vectors, .. } => vectors.iter().all(|values| normalized(values)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorNormalization {
    None,
    UnitL2,
}

/// Evidence required when a vector was derived rather than supplied directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingProvenance {
    pub source_digest: String,
    pub model: String,
    pub model_digest: String,
    pub dimensions: u32,
    pub normalization: VectorNormalization,
    #[serde(default)]
    pub generation_parameters: RuntimeProperties,
}

impl EmbeddingProvenance {
    pub fn validate(&self, dimensions: usize) -> Result<()> {
        validate_digest("embedding source", &self.source_digest)?;
        validate_digest("embedding model", &self.model_digest)?;
        validate_text("embedding model", &self.model)?;
        if self.dimensions as usize != dimensions {
            return invalid(format!(
                "embedding provenance dimensions {} do not match vector dimensions {dimensions}",
                self.dimensions
            ));
        }
        Ok(())
    }
}

/// One immutable version of a vector attached to a canonical record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeVector {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    pub subject: RuntimeRef,
    pub field: String,
    pub valid_from: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Millis>,
    pub value: VectorValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<EmbeddingProvenance>,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

impl RuntimeVector {
    pub fn validate(&self) -> Result<()> {
        validate_text("vector field", &self.field)?;
        validate_window(self.valid_from, self.valid_to)?;
        self.value.validate()?;
        if let Some(provenance) = &self.provenance {
            provenance.validate(self.value.dimensions())?;
            if provenance.normalization == VectorNormalization::UnitL2 && !self.value.is_unit_l2() {
                return invalid("vector marked unit_l2 is not unit normalized");
            }
        }
        Ok(())
    }
}

/// A deterministic scalar for canonical time-series samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SeriesValue {
    Integer(i64),
    Unsigned(u64),
    Decimal(String),
    Bool(bool),
    String(String),
}

/// One immutable sample. The `series` record supplies identity and metadata;
/// `observed_at` is domain time and commit time remains transaction time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSeriesSample {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    pub series: RuntimeRef,
    pub observed_at: Millis,
    pub value: SeriesValue,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

impl RuntimeSeriesSample {
    pub fn validate(&self) -> Result<()> {
        if let SeriesValue::Decimal(value) = &self.value {
            validate_decimal(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub longitude: f64,
    pub latitude: f64,
}

impl GeoPoint {
    fn validate(self) -> Result<()> {
        if !self.longitude.is_finite() || !(-180.0..=180.0).contains(&self.longitude) {
            return invalid("longitude must be finite and in [-180, 180]");
        }
        if !self.latitude.is_finite() || !(-90.0..=90.0).contains(&self.latitude) {
            return invalid("latitude must be finite and in [-90, 90]");
        }
        Ok(())
    }
}

/// Canonical WGS84 geometry supported before spatial projection work begins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeoValue {
    Point {
        point: GeoPoint,
    },
    BoundingBox {
        southwest: GeoPoint,
        northeast: GeoPoint,
    },
}

impl GeoValue {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Point { point } => point.validate(),
            Self::BoundingBox {
                southwest,
                northeast,
            } => {
                southwest.validate()?;
                northeast.validate()?;
                if southwest.longitude > northeast.longitude
                    || southwest.latitude > northeast.latitude
                {
                    return invalid("geo bounding box corners are inverted");
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeGeo {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    pub subject: RuntimeRef,
    pub field: String,
    pub valid_from: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Millis>,
    pub value: GeoValue,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

impl RuntimeGeo {
    pub fn validate(&self) -> Result<()> {
        validate_text("geo field", &self.field)?;
        validate_window(self.valid_from, self.valid_to)?;
        self.value.validate()
    }
}

/// Backend evidence captured while staging immutable bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReceipt {
    pub backend: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// Canonical visibility record for already staged and verified immutable bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    #[serde(flatten)]
    pub reference: RuntimeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<RuntimeRef>,
    pub sha256: String,
    pub length: u64,
    pub media_type: String,
    pub receipt: ObjectReceipt,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

impl ObjectReference {
    pub fn for_bytes(
        id: impl Into<String>,
        subject: Option<RuntimeRef>,
        media_type: impl Into<String>,
        bytes: &[u8],
        receipt: ObjectReceipt,
    ) -> Result<Self> {
        let value = Self {
            reference: RuntimeRef {
                kind: RuntimeType::new("object")?,
                id: RuntimeId::new(id)?,
            },
            subject,
            sha256: digest::sha256_hex(bytes),
            length: bytes.len() as u64,
            media_type: media_type.into(),
            receipt,
            properties: RuntimeProperties::new(),
        };
        value.validate()?;
        Ok(value)
    }

    /// Builds a reference from independently verified streaming evidence.
    ///
    /// File/object adapters use this when the payload is intentionally too
    /// large to materialize as one byte slice. The digest and length remain
    /// subject to the same canonical-reference validation as `for_bytes`.
    pub fn for_verified(
        id: impl Into<String>,
        subject: Option<RuntimeRef>,
        media_type: impl Into<String>,
        sha256: impl Into<String>,
        length: u64,
        receipt: ObjectReceipt,
    ) -> Result<Self> {
        let value = Self {
            reference: RuntimeRef {
                kind: RuntimeType::new("object")?,
                id: RuntimeId::new(id)?,
            },
            subject,
            sha256: sha256.into(),
            length,
            media_type: media_type.into(),
            receipt,
            properties: RuntimeProperties::new(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn canonical_key(sha256: &str) -> Result<String> {
        validate_digest("object", sha256)?;
        Ok(format!("objects/sha256/{}/{sha256}", &sha256[..2]))
    }

    pub fn validate(&self) -> Result<()> {
        validate_digest("object", &self.sha256)?;
        validate_text("object media type", &self.media_type)?;
        validate_text("object backend", &self.receipt.backend)?;
        validate_text("object key", &self.receipt.key)?;
        if let Some(version) = &self.receipt.version {
            validate_text("object version", version)?;
        }
        if let Some(etag) = &self.receipt.etag {
            validate_text("object ETag", etag)?;
        }
        let expected = Self::canonical_key(&self.sha256)?;
        if self.receipt.key != expected {
            return invalid(format!(
                "object key {:?} is not canonical for digest {}",
                self.receipt.key, self.sha256
            ));
        }
        Ok(())
    }
}

fn validate_finite(values: &[f32]) -> Result<()> {
    if values.iter().any(|value| !value.is_finite()) {
        return invalid("vector values must be finite");
    }
    Ok(())
}

fn validate_text(kind: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.as_bytes().contains(&0) {
        return invalid(format!("{kind} must be non-empty and contain no NUL bytes"));
    }
    Ok(())
}

fn validate_digest(kind: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return invalid(format!(
            "{kind} digest must be 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}

fn validate_window(valid_from: Millis, valid_to: Option<Millis>) -> Result<()> {
    if valid_to.is_some_and(|end| end <= valid_from) {
        return Err(Error::InvalidValidityWindow {
            valid_from,
            valid_to: valid_to.unwrap_or(valid_from),
        });
    }
    Ok(())
}

fn validate_decimal(value: &str) -> Result<()> {
    validate_text("series decimal", value)?;
    if !value.parse::<f64>().is_ok_and(f64::is_finite) {
        return invalid("series decimal must be a finite numeric string");
    }
    Ok(())
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidRuntime {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_validation_rejects_ambiguous_sparse_payloads() {
        let value = VectorValue::Sparse {
            dimensions: 4,
            indices: vec![1, 1],
            values: vec![0.25, 0.5],
        };
        assert!(value.validate().is_err());
    }

    #[test]
    fn object_identity_is_derived_from_verified_bytes() {
        let bytes = b"vyrm object";
        let sha256 = digest::sha256_hex(bytes);
        let reference = ObjectReference::for_bytes(
            "fixture",
            None,
            "text/plain",
            bytes,
            ObjectReceipt {
                backend: "local".into(),
                key: ObjectReference::canonical_key(&sha256).unwrap(),
                version: None,
                etag: None,
            },
        )
        .unwrap();
        assert_eq!(reference.sha256, sha256);
        assert_eq!(reference.length, bytes.len() as u64);
    }

    #[test]
    fn declared_unit_normalization_and_decimal_finiteness_fail_closed() {
        let vector = RuntimeVector {
            reference: RuntimeRef::new("embedding", "not-normalized").unwrap(),
            subject: RuntimeRef::new("entity", "one").unwrap(),
            field: "body".into(),
            valid_from: 1,
            valid_to: None,
            value: VectorValue::Dense {
                values: vec![1.0, 1.0],
            },
            provenance: Some(EmbeddingProvenance {
                source_digest: "11".repeat(32),
                model: "fixture".into(),
                model_digest: "22".repeat(32),
                dimensions: 2,
                normalization: VectorNormalization::UnitL2,
                generation_parameters: RuntimeProperties::new(),
            }),
            properties: RuntimeProperties::new(),
        };
        assert!(vector.validate().is_err());
        let sample = RuntimeSeriesSample {
            reference: RuntimeRef::new("sample", "nan").unwrap(),
            series: RuntimeRef::new("series", "one").unwrap(),
            observed_at: 1,
            value: SeriesValue::Decimal("NaN".into()),
            properties: RuntimeProperties::new(),
        };
        assert!(sample.validate().is_err());
    }
}
