use crate::contract::invalid;
use crate::exact::validate_candidate_versions;
use crate::{
    AccessPathKind, CandidatePath, EmbeddingModelBinding, ScoreMetric, SearchHit, SearchRequest,
    TurboQuantBits, TurboQuantVector, VectorCandidate, VectorQuery,
};
use rrd_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, Result, RuntimeProperties, RuntimeRef,
    ScopeId, VectorCollectionAddress, VectorValue, DATA_RUNTIME_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const TURBOQUANT_SEGMENT_FORMAT_VERSION: u16 = 1;
const TURBOQUANT_SEGMENT_MAGIC: &[u8; 8] = b"RRDTQ001";
const HEADER_PREFIX_BYTES: usize = 8 + 2 + 4 + 8 + 64;
const MAX_TURBOQUANT_SEGMENT_BYTES: usize = 1 << 30;
const MAX_TURBOQUANT_CANDIDATES: usize = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurboQuantSegmentConfig {
    pub id: ProjectionId,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    pub bits: TurboQuantBits,
    pub seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
}

impl TurboQuantSegmentConfig {
    pub fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("TurboQuant segment field must be non-empty and contain no NUL bytes");
        }
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("TurboQuant segment dimensions must be in 1..=1048576");
        }
        if self
            .filter_properties
            .iter()
            .any(|property| property.trim().is_empty() || property.as_bytes().contains(&0))
        {
            return invalid("TurboQuant filter properties must be valid names");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_vec(self)
            .map(|bytes| digest::sha256_hex(&bytes))
            .map_err(|error| rrd_core::Error::InvalidRuntime {
                reason: format!("TurboQuant configuration cannot be encoded: {error}"),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurboQuantDescriptor {
    pub stamp: ProjectionStamp,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    pub bits: TurboQuantBits,
    pub seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
    pub minimum_cursor: u64,
    pub candidate_versions: usize,
    pub packed_vector_bytes: usize,
    pub full_precision_vector_bytes: usize,
}

impl TurboQuantDescriptor {
    pub fn validate(&self) -> Result<()> {
        self.stamp.validate()?;
        if self.minimum_cursor != 0
            || self.minimum_cursor > self.stamp.source_cursor
            || self.dimensions == 0
            || self.candidate_versions > MAX_TURBOQUANT_CANDIDATES
            || self.packed_vector_bytes == 0
            || self.full_precision_vector_bytes
                != self
                    .candidate_versions
                    .saturating_mul(self.dimensions)
                    .saturating_mul(std::mem::size_of::<f32>())
        {
            return invalid("TurboQuant descriptor coverage or byte accounting is invalid");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    pub fn candidate_path(&self, estimated_cost: u64) -> CandidatePath {
        CandidatePath {
            stamp: self.stamp.clone(),
            kind: AccessPathKind::TurboQuant,
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
struct StoredCandidate {
    reference: RuntimeRef,
    subject: RuntimeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collection: Option<VectorCollectionAddress>,
    source_cursor: u64,
    valid_from: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    valid_to: Option<u64>,
    #[serde(default)]
    properties: RuntimeProperties,
    original_norm: f32,
    centroid_norm: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentHeader {
    config: TurboQuantSegmentConfig,
    generation: u64,
    source_cursor: u64,
    minimum_cursor: u64,
    packed_bytes_per_vector: usize,
    candidates: Vec<StoredCandidate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurboQuantSegment {
    descriptor: TurboQuantDescriptor,
    candidates: Vec<StoredCandidate>,
    packed_bytes_per_vector: usize,
    payload: Vec<u8>,
    bytes: Vec<u8>,
}

impl TurboQuantSegment {
    pub fn build(
        config: TurboQuantSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        config.validate()?;
        if generation == 0 {
            return invalid("TurboQuant segment generation must be greater than zero");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.is_empty() || candidates.len() > MAX_TURBOQUANT_CANDIDATES {
            return invalid("TurboQuant segment candidate count is outside bounds");
        }
        validate_candidate_versions(&candidates)?;
        candidates.sort_by(|left, right| {
            left.vector
                .reference
                .cmp(&right.vector.reference)
                .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        });
        let mut stored = Vec::with_capacity(candidates.len());
        let mut payload = Vec::new();
        let mut packed_bytes_per_vector = None;
        for candidate in candidates {
            if candidate.scope != config.scope
                || candidate.source_cursor > source_cursor
                || candidate.vector.field != config.field
                || candidate.vector.value.dimensions() != config.dimensions
                || !candidate.matches_model(config.embedding_model.as_ref())
            {
                return invalid("TurboQuant candidate violates configuration or coverage");
            }
            let VectorValue::Dense { values } = &candidate.vector.value else {
                return invalid("TurboQuant artifacts currently require dense vectors");
            };
            let encoded = TurboQuantVector::encode(values, config.bits, config.seed)?;
            match packed_bytes_per_vector {
                Some(expected) if expected != encoded.packed_vector_bytes() => {
                    return invalid("TurboQuant vectors produced inconsistent packed lengths");
                }
                None => packed_bytes_per_vector = Some(encoded.packed_vector_bytes()),
                _ => {}
            }
            payload.extend_from_slice(&encoded.packed);
            stored.push(StoredCandidate {
                reference: candidate.vector.reference,
                subject: candidate.vector.subject,
                collection: candidate.vector.collection,
                source_cursor: candidate.source_cursor,
                valid_from: candidate.vector.valid_from,
                valid_to: candidate.vector.valid_to,
                properties: candidate.vector.properties,
                original_norm: encoded.original_norm,
                centroid_norm: encoded.centroid_norm,
            });
        }
        let header = SegmentHeader {
            config,
            generation,
            source_cursor,
            minimum_cursor: 0,
            packed_bytes_per_vector: packed_bytes_per_vector.unwrap_or_default(),
            candidates: stored,
        };
        let header_bytes = encode_json(&header)?;
        let artifact_digest = artifact_digest(&header_bytes, &payload);
        let bytes = encode_artifact(&header_bytes, &payload, &artifact_digest)?;
        Self::from_parts(header, payload, bytes, artifact_digest)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_PREFIX_BYTES || bytes.len() > MAX_TURBOQUANT_SEGMENT_BYTES {
            return invalid("TurboQuant artifact byte length is outside bounds");
        }
        if &bytes[..8] != TURBOQUANT_SEGMENT_MAGIC {
            return invalid("TurboQuant artifact magic is invalid");
        }
        let version = u16::from_le_bytes(bytes[8..10].try_into().expect("fixed version slice"));
        if version != TURBOQUANT_SEGMENT_FORMAT_VERSION {
            return invalid("TurboQuant artifact format version is unsupported");
        }
        let header_len = u32::from_le_bytes(bytes[10..14].try_into().expect("fixed header slice"));
        let payload_len =
            u64::from_le_bytes(bytes[14..22].try_into().expect("fixed payload slice"));
        let header_len =
            usize::try_from(header_len).map_err(|_| rrd_core::Error::InvalidRuntime {
                reason: "TurboQuant header length overflows this host".into(),
            })?;
        let payload_len =
            usize::try_from(payload_len).map_err(|_| rrd_core::Error::InvalidRuntime {
                reason: "TurboQuant payload length overflows this host".into(),
            })?;
        let expected_len = HEADER_PREFIX_BYTES
            .checked_add(header_len)
            .and_then(|value| value.checked_add(payload_len))
            .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                reason: "TurboQuant artifact length overflows".into(),
            })?;
        if bytes.len() != expected_len {
            return invalid("TurboQuant artifact framing length is inconsistent");
        }
        let advertised =
            std::str::from_utf8(&bytes[22..86]).map_err(|_| rrd_core::Error::InvalidRuntime {
                reason: "TurboQuant digest is not ASCII".into(),
            })?;
        let header_bytes = &bytes[HEADER_PREFIX_BYTES..HEADER_PREFIX_BYTES + header_len];
        let payload = &bytes[HEADER_PREFIX_BYTES + header_len..];
        let actual = artifact_digest(header_bytes, payload);
        if advertised != actual {
            return invalid("TurboQuant artifact digest does not match its bytes");
        }
        let header: SegmentHeader = serde_json::from_slice(header_bytes).map_err(|error| {
            rrd_core::Error::InvalidRuntime {
                reason: format!("TurboQuant header cannot be decoded: {error}"),
            }
        })?;
        if encode_json(&header)? != header_bytes {
            return invalid("TurboQuant header is not in canonical encoding");
        }
        Self::from_parts(header, payload.to_vec(), bytes.to_vec(), actual)
    }

    fn from_parts(
        header: SegmentHeader,
        payload: Vec<u8>,
        bytes: Vec<u8>,
        artifact_digest: String,
    ) -> Result<Self> {
        header.config.validate()?;
        if header.generation == 0
            || header.minimum_cursor != 0
            || header.candidates.is_empty()
            || header.candidates.len() > MAX_TURBOQUANT_CANDIDATES
            || header.packed_bytes_per_vector == 0
            || payload.len()
                != header
                    .candidates
                    .len()
                    .saturating_mul(header.packed_bytes_per_vector)
        {
            return invalid("TurboQuant header coverage or payload length is invalid");
        }
        let mut versions = BTreeSet::new();
        for candidate in &header.candidates {
            if candidate.source_cursor == 0
                || candidate.source_cursor > header.source_cursor
                || !candidate.original_norm.is_finite()
                || candidate.original_norm < 0.0
                || !candidate.centroid_norm.is_finite()
                || candidate.centroid_norm <= 0.0
                || !versions.insert((candidate.reference.clone(), candidate.source_cursor))
            {
                return invalid("TurboQuant candidate metadata is invalid");
            }
        }
        let descriptor = TurboQuantDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: header.config.id.clone(),
                generation: header.generation,
                source_cursor: header.source_cursor,
                config_digest: header.config.digest()?,
                artifact_digest,
                state: ProjectionState::Ready,
            },
            scope: header.config.scope,
            field: header.config.field,
            dimensions: header.config.dimensions,
            metric: header.config.metric,
            bits: header.config.bits,
            seed: header.config.seed,
            embedding_model: header.config.embedding_model,
            filter_properties: header.config.filter_properties,
            minimum_cursor: header.minimum_cursor,
            candidate_versions: header.candidates.len(),
            packed_vector_bytes: payload.len(),
            full_precision_vector_bytes: header
                .candidates
                .len()
                .saturating_mul(header.config.dimensions)
                .saturating_mul(std::mem::size_of::<f32>()),
        };
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            candidates: header.candidates,
            packed_bytes_per_vector: header.packed_bytes_per_vector,
            payload,
            bytes,
        })
    }

    pub fn descriptor(&self) -> &TurboQuantDescriptor {
        &self.descriptor
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn search_candidates_at(
        &self,
        request: &SearchRequest,
        candidate_limit: usize,
        required_source_cursor: u64,
    ) -> Result<Vec<SearchHit>> {
        request.validate()?;
        self.descriptor.validate()?;
        let VectorQuery::Dense { values } = &request.query else {
            return invalid("TurboQuant artifact requires a dense query");
        };
        let required_properties = request
            .filter
            .as_ref()
            .map(|filter| {
                filter
                    .referenced_properties()
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if candidate_limit < request.top_k
            || required_source_cursor > request.read.commit_cursor
            || self.descriptor.stamp.state != ProjectionState::Ready
            || self.descriptor.scope != request.scope
            || self.descriptor.field != request.field
            || self.descriptor.metric != request.metric
            || self.descriptor.embedding_model != request.embedding_model
            || self.descriptor.dimensions != values.len()
            || self.descriptor.stamp.source_cursor < required_source_cursor
            || !required_properties.is_subset(&self.descriptor.filter_properties)
        {
            return invalid("TurboQuant artifact does not satisfy the search contract");
        }
        let mut latest = BTreeMap::<&RuntimeRef, usize>::new();
        for (index, candidate) in self.candidates.iter().enumerate() {
            if candidate.source_cursor > request.read.commit_cursor
                || candidate.valid_from > request.valid_at
            {
                continue;
            }
            if latest.get(&candidate.reference).is_none_or(|current| {
                self.candidates[*current].source_cursor < candidate.source_cursor
            }) {
                latest.insert(&candidate.reference, index);
            }
        }
        let mut hits = Vec::new();
        for index in latest.into_values() {
            let candidate = &self.candidates[index];
            if candidate
                .valid_to
                .is_some_and(|valid_to| request.valid_at >= valid_to)
                || request
                    .filter
                    .as_ref()
                    .is_some_and(|filter| !filter.matches(&candidate.properties))
            {
                continue;
            }
            let start = index * self.packed_bytes_per_vector;
            let encoded = TurboQuantVector {
                format_version: crate::TURBOQUANT_FORMAT_VERSION,
                bits: self.descriptor.bits,
                seed: self.descriptor.seed,
                dimensions: self.descriptor.dimensions,
                padded_dimensions: self
                    .descriptor
                    .bits
                    .padded_dimensions(self.descriptor.dimensions),
                original_norm: candidate.original_norm,
                centroid_norm: candidate.centroid_norm,
                packed: self.payload[start..start + self.packed_bytes_per_vector].to_vec(),
            };
            hits.push(SearchHit {
                reference: candidate.reference.clone(),
                subject: candidate.subject.clone(),
                source_cursor: candidate.source_cursor,
                score: encoded.score(values, request.metric)?,
            });
        }
        hits.sort_by(SearchHit::compare_best_first);
        hits.truncate(candidate_limit);
        Ok(hits)
    }
}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| rrd_core::Error::InvalidRuntime {
        reason: format!("TurboQuant artifact cannot be encoded: {error}"),
    })
}

fn artifact_digest(header: &[u8], payload: &[u8]) -> String {
    let mut bytes = b"rrflow-turboquant-segment-v1\0".to_vec();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(payload);
    digest::sha256_hex(&bytes)
}

fn encode_artifact(header: &[u8], payload: &[u8], artifact_digest: &str) -> Result<Vec<u8>> {
    let header_len = u32::try_from(header.len()).map_err(|_| rrd_core::Error::InvalidRuntime {
        reason: "TurboQuant header exceeds u32".into(),
    })?;
    let payload_len =
        u64::try_from(payload.len()).map_err(|_| rrd_core::Error::InvalidRuntime {
            reason: "TurboQuant payload exceeds u64".into(),
        })?;
    let mut bytes = Vec::with_capacity(HEADER_PREFIX_BYTES + header.len() + payload.len());
    bytes.extend_from_slice(TURBOQUANT_SEGMENT_MAGIC);
    bytes.extend_from_slice(&TURBOQUANT_SEGMENT_FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&header_len.to_le_bytes());
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.extend_from_slice(artifact_digest.as_bytes());
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(payload);
    if bytes.len() > MAX_TURBOQUANT_SEGMENT_BYTES {
        return invalid("TurboQuant artifact exceeds the 1 GiB safety limit");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterCondition, FilterExpression, FilterOperator, SearchMode};
    use rrd_core::{ReadStamp, RuntimeValue, RuntimeVector, VectorValue};

    fn config(scope: &ScopeId) -> TurboQuantSegmentConfig {
        TurboQuantSegmentConfig {
            id: ProjectionId::new("vector:turbo-body").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 64,
            metric: ScoreMetric::Cosine,
            bits: TurboQuantBits::Bits4,
            seed: 91,
            embedding_model: None,
            filter_properties: BTreeSet::from(["tenant".into()]),
        }
    }

    fn candidate(scope: &ScopeId, cursor: u64, id: &str, tenant: &str) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", id).unwrap(),
                subject: RuntimeRef::new("document", id).unwrap(),
                collection: None,
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense {
                    values: deterministic_vector(64, cursor + 10),
                },
                provenance: None,
                properties: RuntimeProperties::from([(
                    "tenant".into(),
                    RuntimeValue::String(tenant.into()),
                )]),
            },
        }
    }

    #[test]
    fn binary_artifact_round_trips_filters_and_accounts_for_compression() {
        let scope = ScopeId::new("instance:turbo-segment").unwrap();
        let candidates = (1..=32)
            .map(|cursor| {
                candidate(
                    &scope,
                    cursor,
                    &format!("point-{cursor:02}"),
                    if cursor % 2 == 0 { "a" } else { "b" },
                )
            })
            .collect::<Vec<_>>();
        let segment = TurboQuantSegment::build(config(&scope), 1, 32, candidates).unwrap();
        assert_eq!(segment.descriptor.packed_vector_bytes, 32 * 32);
        assert_eq!(segment.descriptor.full_precision_vector_bytes, 32 * 64 * 4);
        let reopened = TurboQuantSegment::from_bytes(segment.as_bytes()).unwrap();
        assert_eq!(reopened.descriptor(), segment.descriptor());

        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 32, Some("11".repeat(32))).unwrap(),
            valid_at: 100,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: deterministic_vector(64, 99),
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 4,
            mode: SearchMode::RequireApproximate { exact_rerank: 8 },
            filter: Some(FilterExpression::Condition {
                condition: FilterCondition {
                    property: "tenant".into(),
                    operator: FilterOperator::Equals {
                        value: RuntimeValue::String("a".into()),
                    },
                },
            }),
        };
        let hits = reopened.search_candidates_at(&request, 8, 32).unwrap();
        assert_eq!(hits.len(), 8);
        assert!(hits.iter().all(|hit| {
            hit.reference
                .id
                .as_str()
                .chars()
                .last()
                .and_then(|character| character.to_digit(10))
                .is_some_and(|digit| digit % 2 == 0)
        }));
    }

    #[test]
    fn artifact_corruption_and_stale_reads_fail_closed() {
        let scope = ScopeId::new("instance:turbo-corrupt").unwrap();
        let segment =
            TurboQuantSegment::build(config(&scope), 1, 1, [candidate(&scope, 1, "point", "a")])
                .unwrap();
        let mut corrupt = segment.as_bytes().to_vec();
        *corrupt.last_mut().unwrap() ^= 0x80;
        assert!(TurboQuantSegment::from_bytes(&corrupt).is_err());

        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 2, Some("11".repeat(32))).unwrap(),
            valid_at: 100,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: deterministic_vector(64, 99),
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 1,
            mode: SearchMode::RequireApproximate { exact_rerank: 1 },
            filter: None,
        };
        assert!(segment.search_candidates_at(&request, 1, 2).is_err());
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

    fn mix(mut value: u64, salt: u64) -> u64 {
        value ^= salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        value ^= value >> 30;
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}
