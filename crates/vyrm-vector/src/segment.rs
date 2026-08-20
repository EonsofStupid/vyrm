use crate::contract::invalid;
use crate::exact::validate_candidate_versions;
use crate::{
    search_exact_ref, AccessPathKind, CandidatePath, EmbeddingModelBinding, ScoreMetric, SearchHit,
    SearchRequest, VectorCandidate,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use vyrm_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, Result, ScopeId,
    DATA_RUNTIME_CONTRACT_VERSION,
};

pub const VECTOR_SEGMENT_FORMAT_VERSION: u16 = 1;
const VECTOR_SEGMENT_MAGIC: &str = "VYRVEC01";
const MAX_VECTOR_SEGMENT_BYTES: usize = 1 << 30;
const MAX_VECTOR_SEGMENT_CANDIDATES: usize = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorSegmentConfig {
    pub id: ProjectionId,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
}

impl VectorSegmentConfig {
    pub fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("vector segment field must be non-empty and contain no NUL bytes");
        }
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("vector segment dimensions must be in 1..=1048576");
        }
        if self
            .filter_properties
            .iter()
            .any(|property| property.trim().is_empty() || property.as_bytes().contains(&0))
        {
            return invalid("vector segment filter properties must be valid names");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| vyrm_core::Error::InvalidRuntime {
            reason: format!("vector segment configuration cannot be encoded: {error}"),
        })?;
        Ok(digest::sha256_hex(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentDescriptor {
    pub stamp: ProjectionStamp,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
    pub minimum_cursor: u64,
    pub candidate_versions: usize,
}

impl SegmentDescriptor {
    pub fn validate(&self) -> Result<()> {
        self.stamp.validate()?;
        if self.minimum_cursor > self.stamp.source_cursor {
            return invalid("vector segment cursor interval is inverted");
        }
        if self.minimum_cursor != 0 {
            return invalid("exact vector segment must cover history from cursor zero");
        }
        if self.dimensions == 0 || self.candidate_versions > MAX_VECTOR_SEGMENT_CANDIDATES {
            return invalid("vector segment dimensions or candidate count is invalid");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    pub fn candidate_path(&self, estimated_cost: u64) -> CandidatePath {
        CandidatePath {
            stamp: self.stamp.clone(),
            kind: AccessPathKind::ExactSegment,
            field: self.field.clone(),
            dimensions: self.dimensions,
            metric: self.metric,
            embedding_model: self.embedding_model.clone(),
            filter_properties: self.filter_properties.clone(),
            estimated_candidates: self.candidate_versions as u64,
            estimated_cost,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentBody {
    config: VectorSegmentConfig,
    generation: u64,
    source_cursor: u64,
    minimum_cursor: u64,
    candidates: Vec<VectorCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentEnvelope {
    magic: String,
    format_version: u16,
    artifact_digest: String,
    body: SegmentBody,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImmutableVectorSegment {
    descriptor: SegmentDescriptor,
    candidates: Vec<VectorCandidate>,
    bytes: Vec<u8>,
}

impl ImmutableVectorSegment {
    pub fn build(
        config: VectorSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        config.validate()?;
        if generation == 0 {
            return invalid("vector segment generation must be greater than zero");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.len() > MAX_VECTOR_SEGMENT_CANDIDATES {
            return invalid("vector segment candidate limit exceeded");
        }
        validate_candidate_versions(&candidates)?;
        for candidate in &candidates {
            if candidate.scope != config.scope
                || candidate.source_cursor > source_cursor
                || candidate.vector.field != config.field
                || candidate.vector.value.dimensions() != config.dimensions
                || !candidate.matches_model(config.embedding_model.as_ref())
            {
                return invalid("vector segment candidate violates configuration or coverage");
            }
        }
        candidates.sort_by(|left, right| {
            left.vector
                .reference
                .cmp(&right.vector.reference)
                .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        });
        let body = SegmentBody {
            config: config.clone(),
            generation,
            source_cursor,
            minimum_cursor: 0,
            candidates,
        };
        let body_bytes = encode_json(&body)?;
        let artifact_digest = digest::sha256_hex(&body_bytes);
        let envelope = SegmentEnvelope {
            magic: VECTOR_SEGMENT_MAGIC.into(),
            format_version: VECTOR_SEGMENT_FORMAT_VERSION,
            artifact_digest: artifact_digest.clone(),
            body,
        };
        let bytes = encode_json(&envelope)?;
        if bytes.len() > MAX_VECTOR_SEGMENT_BYTES {
            return invalid("encoded vector segment exceeds the 1 GiB safety limit");
        }
        Self::from_parts(envelope, bytes, artifact_digest)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_VECTOR_SEGMENT_BYTES {
            return invalid("encoded vector segment exceeds the 1 GiB safety limit");
        }
        let envelope: SegmentEnvelope =
            serde_json::from_slice(bytes).map_err(|error| vyrm_core::Error::InvalidRuntime {
                reason: format!("vector segment cannot be decoded: {error}"),
            })?;
        if envelope.magic != VECTOR_SEGMENT_MAGIC
            || envelope.format_version != VECTOR_SEGMENT_FORMAT_VERSION
        {
            return invalid("vector segment magic or format version is unsupported");
        }
        if encode_json(&envelope)? != bytes {
            return invalid("vector segment bytes are not in canonical encoding");
        }
        let actual_digest = digest::sha256_hex(&encode_json(&envelope.body)?);
        if actual_digest != envelope.artifact_digest {
            return invalid("vector segment artifact digest does not match its body");
        }
        Self::from_parts(envelope, bytes.to_vec(), actual_digest)
    }

    fn from_parts(
        envelope: SegmentEnvelope,
        bytes: Vec<u8>,
        artifact_digest: String,
    ) -> Result<Self> {
        let config_digest = envelope.body.config.digest()?;
        let descriptor = SegmentDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: envelope.body.config.id.clone(),
                generation: envelope.body.generation,
                source_cursor: envelope.body.source_cursor,
                config_digest,
                artifact_digest,
                state: ProjectionState::Ready,
            },
            scope: envelope.body.config.scope,
            field: envelope.body.config.field,
            dimensions: envelope.body.config.dimensions,
            metric: envelope.body.config.metric,
            embedding_model: envelope.body.config.embedding_model,
            filter_properties: envelope.body.config.filter_properties,
            minimum_cursor: envelope.body.minimum_cursor,
            candidate_versions: envelope.body.candidates.len(),
        };
        descriptor.validate()?;
        validate_candidate_versions(&envelope.body.candidates)?;
        for candidate in &envelope.body.candidates {
            if candidate.scope != descriptor.scope
                || candidate.source_cursor > descriptor.stamp.source_cursor
                || candidate.vector.field != descriptor.field
                || candidate.vector.value.dimensions() != descriptor.dimensions
                || !candidate.matches_model(descriptor.embedding_model.as_ref())
            {
                return invalid("decoded vector segment candidate violates its descriptor");
            }
        }
        Ok(Self {
            descriptor,
            candidates: envelope.body.candidates,
            bytes,
        })
    }

    pub fn descriptor(&self) -> &SegmentDescriptor {
        &self.descriptor
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn search(&self, request: &SearchRequest) -> Result<Vec<SearchHit>> {
        self.search_at(request, request.read.commit_cursor)
    }

    pub fn search_at(
        &self,
        request: &SearchRequest,
        required_source_cursor: u64,
    ) -> Result<Vec<SearchHit>> {
        self.descriptor.validate()?;
        if required_source_cursor > request.read.commit_cursor {
            return invalid("vector segment source cursor exceeds the request read stamp");
        }
        if self.descriptor.stamp.state != ProjectionState::Ready
            || self.descriptor.scope != request.scope
            || self.descriptor.field != request.field
            || self.descriptor.metric != request.metric
            || self.descriptor.embedding_model != request.embedding_model
            || self.descriptor.dimensions != request.query.dimensions()
            || self.descriptor.stamp.source_cursor < required_source_cursor
        {
            return invalid("vector segment does not satisfy request identity or freshness");
        }
        search_exact_ref(request, &self.candidates)
    }
}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| vyrm_core::Error::InvalidRuntime {
        reason: format!("vector segment cannot be encoded: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SearchMode, VectorQuery};
    use vyrm_core::{ReadStamp, RuntimeProperties, RuntimeRef, RuntimeVector, VectorValue};

    fn config(scope: &ScopeId) -> VectorSegmentConfig {
        VectorSegmentConfig {
            id: ProjectionId::new("vector:body").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Dot,
            embedding_model: None,
            filter_properties: BTreeSet::new(),
        }
    }

    fn candidate(scope: &ScopeId, cursor: u64, id: &str, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        }
    }

    #[test]
    fn immutable_segment_round_trips_and_serves_exact_results() {
        let scope = ScopeId::new("instance:segment").unwrap();
        let segment = ImmutableVectorSegment::build(
            config(&scope),
            1,
            2,
            [
                candidate(&scope, 1, "a", vec![1.0, 0.0]),
                candidate(&scope, 2, "b", vec![0.5, 0.5]),
            ],
        )
        .unwrap();
        let decoded = ImmutableVectorSegment::from_bytes(segment.as_bytes()).unwrap();
        assert_eq!(segment.descriptor(), decoded.descriptor());
        let read = ReadStamp::new(scope.clone(), None, 0, 2, Some("11".repeat(32))).unwrap();
        let hits = decoded
            .search(&SearchRequest {
                scope,
                read,
                valid_at: 2,
                field: "body".into(),
                query: VectorQuery::Dense {
                    values: vec![1.0, 0.0],
                },
                metric: ScoreMetric::Dot,
                embedding_model: None,
                top_k: 2,
                mode: SearchMode::Exact,
                filter: None,
            })
            .unwrap();
        assert_eq!(hits[0].reference.id.as_str(), "a");
    }

    #[test]
    fn corruption_and_stale_coverage_fail_closed() {
        let scope = ScopeId::new("instance:segment-corrupt").unwrap();
        let segment = ImmutableVectorSegment::build(
            config(&scope),
            1,
            1,
            [candidate(&scope, 1, "a", vec![1.0, 0.0])],
        )
        .unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(segment.as_bytes()).unwrap();
        value["body"]["candidates"][0]["vector"]["value"]["values"][0] = serde_json::json!(0.0);
        let corrupt = serde_json::to_vec(&value).unwrap();
        assert!(ImmutableVectorSegment::from_bytes(&corrupt).is_err());

        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 2, Some("11".repeat(32))).unwrap(),
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Dot,
            embedding_model: None,
            top_k: 1,
            mode: SearchMode::Exact,
            filter: None,
        };
        assert!(segment.search(&request).is_err());
    }
}
