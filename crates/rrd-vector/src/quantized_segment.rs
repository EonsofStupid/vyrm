//! Immutable scalar, product, and binary quantization artifacts.
//!
//! These artifacts are candidate generators only. Canonical full-precision
//! vectors remain in the RRD change history and every planner execution exact
//! reranks the returned references before exposing results.

use crate::contract::invalid;
use crate::exact::validate_candidate_versions;
use crate::{
    AccessPathKind, CandidatePath, EmbeddingModelBinding, ScoreMetric, SearchHit, SearchRequest,
    VectorCandidate, VectorQuery,
};
use memmap2::{Mmap, MmapOptions};
use rrd_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, Result, RuntimeProperties, RuntimeRef,
    ScopeId, VectorCollectionAddress, VectorValue, DATA_RUNTIME_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

pub const QUANTIZED_SEGMENT_FORMAT_VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"RRDQNT01";
const HEADER_BYTES: usize = 128;
const DIGEST_OFFSET: usize = 72;
const DIGEST_BYTES: usize = 32;
const ALIGNMENT: usize = 64;
const MAX_ARTIFACT_BYTES: usize = 1 << 30;
const MAX_CANDIDATES: usize = 10_000_000;
const MAX_DIMENSIONS: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductCompression {
    X4,
    X8,
    X16,
    X32,
    X64,
}

impl ProductCompression {
    pub const fn ratio(self) -> usize {
        match self {
            Self::X4 => 4,
            Self::X8 => 8,
            Self::X16 => 16,
            Self::X32 => 32,
            Self::X64 => 64,
        }
    }

    const fn block_dimensions(self) -> usize {
        self.ratio() / std::mem::size_of::<f32>()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuantizationMethod {
    Scalar,
    Product { compression: ProductCompression },
    Binary,
}

impl QuantizationMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Product { .. } => "product",
            Self::Binary => "binary",
        }
    }

    pub const fn maximum_compression_ratio(self) -> usize {
        match self {
            Self::Scalar => 4,
            Self::Product { compression } => compression.ratio(),
            Self::Binary => 32,
        }
    }

    pub const fn access_path_kind(self) -> AccessPathKind {
        match self {
            Self::Scalar => AccessPathKind::ScalarQuantized,
            Self::Product { .. } => AccessPathKind::ProductQuantized,
            Self::Binary => AccessPathKind::BinaryQuantized,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizedKernel {
    Scalar,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizedMemoryPlacement {
    Owned,
    Mapped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizedSegmentConfig {
    pub id: ProjectionId,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    pub method: QuantizationMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
}

impl QuantizedSegmentConfig {
    pub fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("quantized segment field must be non-empty and contain no NUL bytes");
        }
        if self.dimensions == 0 || self.dimensions > MAX_DIMENSIONS {
            return invalid("quantized segment dimensions must be in 1..=1048576");
        }
        if self
            .filter_properties
            .iter()
            .any(|property| property.trim().is_empty() || property.as_bytes().contains(&0))
        {
            return invalid("quantized segment filter properties must be valid names");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        self.validate()?;
        encode_json(self).map(|bytes| digest::sha256_hex(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizedDescriptor {
    pub stamp: ProjectionStamp,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    pub method: QuantizationMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
    pub minimum_cursor: u64,
    pub candidate_versions: usize,
    pub packed_vector_bytes: usize,
    pub full_precision_vector_bytes: usize,
    pub auxiliary_bytes: usize,
}

impl QuantizedDescriptor {
    pub fn validate(&self) -> Result<()> {
        self.stamp.validate()?;
        let expected_full = self
            .candidate_versions
            .checked_mul(self.dimensions)
            .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
            .ok_or_else(|| runtime_error("quantized full-precision byte count overflow"))?;
        if self.minimum_cursor != 0
            || self.minimum_cursor > self.stamp.source_cursor
            || self.dimensions == 0
            || self.dimensions > MAX_DIMENSIONS
            || self.candidate_versions == 0
            || self.candidate_versions > MAX_CANDIDATES
            || self.packed_vector_bytes == 0
            || self.full_precision_vector_bytes != expected_full
            || self.full_precision_vector_bytes / self.packed_vector_bytes
                > self.method.maximum_compression_ratio()
        {
            return invalid("quantized descriptor coverage or byte accounting is invalid");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    pub fn candidate_path(&self, estimated_cost: u64) -> CandidatePath {
        CandidatePath {
            stamp: self.stamp.clone(),
            kind: self.method.access_path_kind(),
            field: self.field.clone(),
            dimensions: self.dimensions,
            metric: self.metric,
            embedding_model: self.embedding_model.clone(),
            filter_properties: self.filter_properties.clone(),
            estimated_candidates: self.candidate_versions as u64,
            estimated_cost,
            overlay_source_cursor: None,
            overlay_candidates: 0,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
enum QuantizedLayout {
    Scalar {
        scale: f32,
    },
    Product {
        block_dimensions: usize,
        subspaces: usize,
        codebook_size: usize,
        /// One flat `(codebook_size * block_dimensions)` table per subspace.
        codebooks: Vec<Vec<f32>>,
    },
    Binary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentMetadata {
    config: QuantizedSegmentConfig,
    generation: u64,
    source_cursor: u64,
    minimum_cursor: u64,
    row_bytes: usize,
    layout: QuantizedLayout,
    candidates: Vec<StoredCandidate>,
}

#[derive(Debug, Clone)]
enum QuantizedStorage {
    Owned(Arc<[u8]>),
    Mapped(Arc<Mmap>),
}

impl QuantizedStorage {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Mapped(bytes) => bytes,
        }
    }

    fn placement(&self) -> QuantizedMemoryPlacement {
        match self {
            Self::Owned(_) => QuantizedMemoryPlacement::Owned,
            Self::Mapped(_) => QuantizedMemoryPlacement::Mapped,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuantizedSegment {
    descriptor: QuantizedDescriptor,
    metadata: SegmentMetadata,
    payload_offset: usize,
    storage: QuantizedStorage,
}

impl PartialEq for QuantizedSegment {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.as_bytes() == other.as_bytes()
    }
}

impl QuantizedSegment {
    pub fn build(
        config: QuantizedSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        config.validate()?;
        if generation == 0 {
            return invalid("quantized segment generation must be greater than zero");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
            return invalid("quantized segment candidate count is outside bounds");
        }
        validate_candidate_versions(&candidates)?;
        for candidate in &candidates {
            if candidate.scope != config.scope
                || candidate.source_cursor > source_cursor
                || candidate.vector.field != config.field
                || candidate.vector.value.dimensions() != config.dimensions
                || !candidate.matches_model(config.embedding_model.as_ref())
                || !matches!(candidate.vector.value, VectorValue::Dense { .. })
            {
                return invalid("quantized candidate violates configuration or coverage");
            }
        }
        candidates.sort_by(|left, right| {
            left.vector
                .reference
                .cmp(&right.vector.reference)
                .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        });
        let values = candidates
            .iter()
            .map(|candidate| match &candidate.vector.value {
                VectorValue::Dense { values } => values.as_slice(),
                _ => unreachable!("dense shape was checked above"),
            })
            .collect::<Vec<_>>();
        let (layout, row_bytes, payload) = encode_payload(config.method, &values)?;
        let metadata = SegmentMetadata {
            config,
            generation,
            source_cursor,
            minimum_cursor: 0,
            row_bytes,
            layout,
            candidates: candidates
                .into_iter()
                .map(|candidate| StoredCandidate {
                    reference: candidate.vector.reference,
                    subject: candidate.vector.subject,
                    collection: candidate.vector.collection,
                    source_cursor: candidate.source_cursor,
                    valid_from: candidate.vector.valid_from,
                    valid_to: candidate.vector.valid_to,
                    properties: candidate.vector.properties,
                })
                .collect(),
        };
        let metadata_bytes = encode_json(&metadata)?;
        let payload_offset = align_up(
            HEADER_BYTES
                .checked_add(metadata_bytes.len())
                .ok_or_else(|| runtime_error("quantized metadata length overflow"))?,
            ALIGNMENT,
        )?;
        let total = payload_offset
            .checked_add(payload.len())
            .ok_or_else(|| runtime_error("quantized artifact length overflow"))?;
        if total > MAX_ARTIFACT_BYTES {
            return invalid("quantized artifact exceeds the 1 GiB safety limit");
        }
        let mut bytes = vec![0_u8; total];
        write_header(
            &mut bytes,
            metadata_bytes.len(),
            payload_offset,
            payload.len(),
            row_bytes,
            metadata.candidates.len(),
            metadata.config.dimensions,
        )?;
        bytes[HEADER_BYTES..HEADER_BYTES + metadata_bytes.len()].copy_from_slice(&metadata_bytes);
        bytes[payload_offset..].copy_from_slice(&payload);
        let artifact_digest = artifact_digest(&bytes)?;
        bytes[DIGEST_OFFSET..DIGEST_OFFSET + DIGEST_BYTES].copy_from_slice(&artifact_digest);
        Self::decode(QuantizedStorage::Owned(Arc::from(bytes)))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return invalid("quantized artifact exceeds the 1 GiB safety limit");
        }
        Self::decode(QuantizedStorage::Owned(Arc::from(bytes)))
    }

    pub fn open_mmap(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| {
            runtime_error(format!(
                "cannot open quantized artifact {}: {error}",
                path.display()
            ))
        })?;
        let length = file
            .metadata()
            .map_err(|error| runtime_error(format!("cannot stat quantized artifact: {error}")))?
            .len();
        if length == 0 || length > MAX_ARTIFACT_BYTES as u64 {
            return invalid("quantized artifact file length is outside safety bounds");
        }
        // SAFETY: the file is opened read-only and its bounded non-zero length
        // was checked. Published artifact files are immutable.
        let mmap = unsafe { MmapOptions::new().map(&file) }
            .map_err(|error| runtime_error(format!("cannot map quantized artifact: {error}")))?;
        Self::decode(QuantizedStorage::Mapped(Arc::new(mmap)))
    }

    pub fn descriptor(&self) -> &QuantizedDescriptor {
        &self.descriptor
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.storage.as_bytes()
    }

    pub fn memory_placement(&self) -> QuantizedMemoryPlacement {
        self.storage.placement()
    }

    pub fn search_candidates_at(
        &self,
        request: &SearchRequest,
        candidate_limit: usize,
        required_source_cursor: u64,
        kernel: QuantizedKernel,
    ) -> Result<Vec<SearchHit>> {
        request.validate()?;
        self.descriptor.validate()?;
        let VectorQuery::Dense { values: query } = &request.query else {
            return invalid("quantized artifact requires a dense query");
        };
        let required_properties: BTreeSet<String> = request
            .filter
            .as_ref()
            .map(|filter| filter.referenced_properties().into_iter().collect())
            .unwrap_or_default();
        if candidate_limit < request.top_k
            || required_source_cursor > request.read.commit_cursor
            || self.descriptor.stamp.state != ProjectionState::Ready
            || self.descriptor.scope != request.scope
            || self.descriptor.field != request.field
            || self.descriptor.metric != request.metric
            || self.descriptor.embedding_model != request.embedding_model
            || self.descriptor.dimensions != query.len()
            || self.descriptor.stamp.source_cursor < required_source_cursor
            || !required_properties.is_subset(&self.descriptor.filter_properties)
        {
            return invalid("quantized artifact does not satisfy the search contract");
        }
        let mut latest = BTreeMap::<&RuntimeRef, usize>::new();
        for (index, candidate) in self.metadata.candidates.iter().enumerate() {
            if candidate.source_cursor > request.read.commit_cursor
                || candidate.valid_from > request.valid_at
            {
                continue;
            }
            if latest.get(&candidate.reference).is_none_or(|current| {
                self.metadata.candidates[*current].source_cursor < candidate.source_cursor
            }) {
                latest.insert(&candidate.reference, index);
            }
        }
        let payload = &self.as_bytes()[self.payload_offset..];
        let mut hits = Vec::new();
        for index in latest.into_values() {
            let candidate = &self.metadata.candidates[index];
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
            let start = index * self.metadata.row_bytes;
            let row = &payload[start..start + self.metadata.row_bytes];
            let score = score_quantized(query, row, &self.metadata.layout, request.metric, kernel)?;
            hits.push(SearchHit {
                reference: candidate.reference.clone(),
                subject: candidate.subject.clone(),
                source_cursor: candidate.source_cursor,
                score,
            });
        }
        hits.sort_by(SearchHit::compare_best_first);
        hits.truncate(candidate_limit);
        Ok(hits)
    }

    fn decode(storage: QuantizedStorage) -> Result<Self> {
        let bytes = storage.as_bytes();
        let header = read_header(bytes)?;
        let metadata_end = HEADER_BYTES
            .checked_add(header.metadata_len)
            .ok_or_else(|| runtime_error("quantized metadata offset overflow"))?;
        let metadata: SegmentMetadata = serde_json::from_slice(&bytes[HEADER_BYTES..metadata_end])
            .map_err(|error| {
                runtime_error(format!("quantized metadata cannot be decoded: {error}"))
            })?;
        if encode_json(&metadata)? != bytes[HEADER_BYTES..metadata_end] {
            return invalid("quantized metadata is not in canonical encoding");
        }
        validate_decoded(&metadata, &header, bytes)?;
        let expected_digest = artifact_digest(bytes)?;
        if bytes[DIGEST_OFFSET..DIGEST_OFFSET + DIGEST_BYTES] != expected_digest {
            return invalid("quantized artifact digest does not match its bytes");
        }
        let full_precision_vector_bytes = metadata
            .candidates
            .len()
            .checked_mul(metadata.config.dimensions)
            .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
            .ok_or_else(|| runtime_error("quantized full-precision bytes overflow"))?;
        let auxiliary_bytes = match &metadata.layout {
            QuantizedLayout::Product { codebooks, .. } => codebooks
                .iter()
                .map(|codebook| codebook.len() * std::mem::size_of::<f32>())
                .sum(),
            QuantizedLayout::Scalar { .. } | QuantizedLayout::Binary => 0,
        };
        let descriptor = QuantizedDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: metadata.config.id.clone(),
                generation: metadata.generation,
                source_cursor: metadata.source_cursor,
                config_digest: metadata.config.digest()?,
                artifact_digest: hex_digest(&expected_digest),
                state: ProjectionState::Ready,
            },
            scope: metadata.config.scope.clone(),
            field: metadata.config.field.clone(),
            dimensions: metadata.config.dimensions,
            metric: metadata.config.metric,
            method: metadata.config.method,
            embedding_model: metadata.config.embedding_model.clone(),
            filter_properties: metadata.config.filter_properties.clone(),
            minimum_cursor: metadata.minimum_cursor,
            candidate_versions: metadata.candidates.len(),
            packed_vector_bytes: header.payload_len,
            full_precision_vector_bytes,
            auxiliary_bytes,
        };
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            metadata,
            payload_offset: header.payload_offset,
            storage,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Header {
    metadata_len: usize,
    payload_offset: usize,
    payload_len: usize,
    row_bytes: usize,
    rows: usize,
    dimensions: usize,
}

fn encode_payload(
    method: QuantizationMethod,
    values: &[&[f32]],
) -> Result<(QuantizedLayout, usize, Vec<u8>)> {
    match method {
        QuantizationMethod::Scalar => {
            let maximum = values
                .iter()
                .flat_map(|row| row.iter())
                .map(|value| value.abs())
                .fold(0.0_f32, f32::max);
            let scale = if maximum == 0.0 { 0.0 } else { maximum / 127.0 };
            let row_bytes = values[0].len();
            let mut payload = Vec::with_capacity(row_bytes * values.len());
            for row in values {
                payload.extend(row.iter().map(|value| {
                    if scale == 0.0 {
                        0_u8
                    } else {
                        ((value / scale).round().clamp(-127.0, 127.0) as i8) as u8
                    }
                }));
            }
            Ok((QuantizedLayout::Scalar { scale }, row_bytes, payload))
        }
        QuantizationMethod::Binary => {
            let row_bytes = values[0].len().div_ceil(8);
            let mut payload = vec![0_u8; row_bytes * values.len()];
            for (row_index, row) in values.iter().enumerate() {
                for (dimension, value) in row.iter().enumerate() {
                    if *value >= 0.0 {
                        payload[row_index * row_bytes + dimension / 8] |= 1 << (dimension % 8);
                    }
                }
            }
            Ok((QuantizedLayout::Binary, row_bytes, payload))
        }
        QuantizationMethod::Product { compression } => {
            let block_dimensions = compression.block_dimensions();
            let dimensions = values[0].len();
            let subspaces = dimensions.div_ceil(block_dimensions);
            let codebook_size = values.len().min(256);
            let mut codebooks = Vec::with_capacity(subspaces);
            for subspace in 0..subspaces {
                let mut codebook = Vec::with_capacity(codebook_size * block_dimensions);
                for code in 0..codebook_size {
                    let sample = code * values.len() / codebook_size;
                    for offset in 0..block_dimensions {
                        codebook.push(
                            values[sample]
                                .get(subspace * block_dimensions + offset)
                                .copied()
                                .unwrap_or(0.0),
                        );
                    }
                }
                codebooks.push(codebook);
            }
            let row_bytes = subspaces;
            let mut payload = Vec::with_capacity(row_bytes * values.len());
            for row in values {
                for (subspace, codebook) in codebooks.iter().enumerate() {
                    let mut best = (0_usize, f64::INFINITY);
                    for code in 0..codebook_size {
                        let mut distance = 0.0_f64;
                        for offset in 0..block_dimensions {
                            let left = row
                                .get(subspace * block_dimensions + offset)
                                .copied()
                                .unwrap_or(0.0);
                            let right = codebook[code * block_dimensions + offset];
                            distance += f64::from(left - right).powi(2);
                        }
                        if distance < best.1 {
                            best = (code, distance);
                        }
                    }
                    payload.push(best.0 as u8);
                }
            }
            Ok((
                QuantizedLayout::Product {
                    block_dimensions,
                    subspaces,
                    codebook_size,
                    codebooks,
                },
                row_bytes,
                payload,
            ))
        }
    }
}

fn score_quantized(
    query: &[f32],
    row: &[u8],
    layout: &QuantizedLayout,
    metric: ScoreMetric,
    kernel: QuantizedKernel,
) -> Result<f64> {
    match layout {
        QuantizedLayout::Scalar { scale } => {
            if row.len() != query.len() {
                return invalid("scalar quantized row dimensions differ from the query");
            }
            let decoded = row
                .iter()
                .map(|value| f32::from(*value as i8) * *scale)
                .collect::<Vec<_>>();
            score_dense(query, &decoded, metric, kernel)
        }
        QuantizedLayout::Product {
            block_dimensions,
            subspaces,
            codebook_size,
            codebooks,
        } => {
            if row.len() != *subspaces || codebooks.len() != *subspaces {
                return invalid("product quantized row differs from its codebooks");
            }
            let mut decoded = Vec::with_capacity(query.len());
            for (subspace, encoded) in row.iter().enumerate() {
                let code = usize::from(*encoded);
                if code >= *codebook_size {
                    return invalid("product quantized row contains an unknown centroid");
                }
                let start = code * *block_dimensions;
                let remaining = query.len() - decoded.len();
                decoded.extend_from_slice(
                    &codebooks[subspace][start..start + (*block_dimensions).min(remaining)],
                );
            }
            score_dense(query, &decoded, metric, kernel)
        }
        QuantizedLayout::Binary => {
            if row.len() != query.len().div_ceil(8) {
                return invalid("binary quantized row dimensions differ from the query");
            }
            let mut matches = 0_u64;
            for (dimension, value) in query.iter().enumerate() {
                let query_bit = *value >= 0.0;
                let row_bit = row[dimension / 8] & (1 << (dimension % 8)) != 0;
                matches += u64::from(query_bit == row_bit);
            }
            let signed = 2.0 * matches as f64 - query.len() as f64;
            Ok(match metric {
                ScoreMetric::Dot => signed,
                ScoreMetric::Cosine => signed / query.len() as f64,
                ScoreMetric::Euclidean => -(query.len() as f64 - matches as f64).sqrt(),
                ScoreMetric::Manhattan => -(query.len() as f64 - matches as f64),
            })
        }
    }
}

fn score_dense(
    query: &[f32],
    row: &[f32],
    metric: ScoreMetric,
    kernel: QuantizedKernel,
) -> Result<f64> {
    if query.len() != row.len()
        || query.iter().chain(row).any(|value| !value.is_finite())
        || (metric == ScoreMetric::Cosine && query.iter().all(|value| *value == 0.0))
    {
        return invalid("quantized dense score requires finite matching vectors");
    }
    #[cfg(target_arch = "x86_64")]
    if kernel == QuantizedKernel::Auto && std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 support was checked and both slices have equal lengths.
        return Ok(unsafe { score_dense_avx2(query, row, metric) });
    }
    Ok(score_dense_scalar(query, row, metric))
}

fn score_dense_scalar(query: &[f32], row: &[f32], metric: ScoreMetric) -> f64 {
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    let mut squared_distance = 0.0;
    let mut manhattan = 0.0;
    for (left, right) in query.iter().zip(row) {
        let left = f64::from(*left);
        let right = f64::from(*right);
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
        squared_distance += (left - right).powi(2);
        manhattan += (left - right).abs();
    }
    match metric {
        ScoreMetric::Dot => dot,
        ScoreMetric::Cosine if right_norm == 0.0 => 0.0,
        ScoreMetric::Cosine => dot / (left_norm.sqrt() * right_norm.sqrt()),
        ScoreMetric::Euclidean => -squared_distance.sqrt(),
        ScoreMetric::Manhattan => -manhattan,
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn score_dense_avx2(query: &[f32], row: &[f32], metric: ScoreMetric) -> f64 {
    use std::arch::x86_64::*;
    let mut dot = _mm256_setzero_ps();
    let mut left_norm = _mm256_setzero_ps();
    let mut right_norm = _mm256_setzero_ps();
    let mut distance = _mm256_setzero_ps();
    let mut manhattan = _mm256_setzero_ps();
    let sign_mask = _mm256_set1_ps(-0.0);
    let mut index = 0;
    while index + 8 <= query.len() {
        let left = _mm256_loadu_ps(query.as_ptr().add(index));
        let right = _mm256_loadu_ps(row.as_ptr().add(index));
        let delta = _mm256_sub_ps(left, right);
        dot = _mm256_add_ps(dot, _mm256_mul_ps(left, right));
        left_norm = _mm256_add_ps(left_norm, _mm256_mul_ps(left, left));
        right_norm = _mm256_add_ps(right_norm, _mm256_mul_ps(right, right));
        distance = _mm256_add_ps(distance, _mm256_mul_ps(delta, delta));
        manhattan = _mm256_add_ps(manhattan, _mm256_andnot_ps(sign_mask, delta));
        index += 8;
    }
    let mut lanes = [0.0_f32; 8];
    let reduce = |value: __m256, lanes: &mut [f32; 8]| {
        _mm256_storeu_ps(lanes.as_mut_ptr(), value);
        lanes.iter().map(|value| f64::from(*value)).sum::<f64>()
    };
    let mut dot = reduce(dot, &mut lanes);
    let mut left_norm = reduce(left_norm, &mut lanes);
    let mut right_norm = reduce(right_norm, &mut lanes);
    let mut distance = reduce(distance, &mut lanes);
    let mut manhattan = reduce(manhattan, &mut lanes);
    for (left, right) in query[index..].iter().zip(&row[index..]) {
        let left = f64::from(*left);
        let right = f64::from(*right);
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
        distance += (left - right).powi(2);
        manhattan += (left - right).abs();
    }
    match metric {
        ScoreMetric::Dot => dot,
        ScoreMetric::Cosine if right_norm == 0.0 => 0.0,
        ScoreMetric::Cosine => dot / (left_norm.sqrt() * right_norm.sqrt()),
        ScoreMetric::Euclidean => -distance.sqrt(),
        ScoreMetric::Manhattan => -manhattan,
    }
}

fn validate_decoded(metadata: &SegmentMetadata, header: &Header, bytes: &[u8]) -> Result<()> {
    metadata.config.validate()?;
    let metadata_end = HEADER_BYTES + header.metadata_len;
    if metadata.generation == 0
        || metadata.minimum_cursor != 0
        || metadata.config.dimensions != header.dimensions
        || metadata.candidates.len() != header.rows
        || metadata.row_bytes != header.row_bytes
        || bytes[metadata_end..header.payload_offset]
            .iter()
            .any(|byte| *byte != 0)
    {
        return invalid("quantized metadata differs from its physical header");
    }
    let expected_row_bytes = match &metadata.layout {
        QuantizedLayout::Scalar { scale } => {
            if !scale.is_finite() || *scale < 0.0 {
                return invalid("scalar quantization scale is invalid");
            }
            metadata.config.dimensions
        }
        QuantizedLayout::Binary => metadata.config.dimensions.div_ceil(8),
        QuantizedLayout::Product {
            block_dimensions,
            subspaces,
            codebook_size,
            codebooks,
        } => {
            let QuantizationMethod::Product { compression } = metadata.config.method else {
                return invalid("product layout differs from configured method");
            };
            if *block_dimensions != compression.block_dimensions()
                || *subspaces != metadata.config.dimensions.div_ceil(*block_dimensions)
                || *codebook_size == 0
                || *codebook_size > 256
                || codebooks.len() != *subspaces
                || codebooks.iter().any(|codebook| {
                    codebook.len() != codebook_size * block_dimensions
                        || codebook.iter().any(|value| !value.is_finite())
                })
            {
                return invalid("product quantization codebook shape is invalid");
            }
            *subspaces
        }
    };
    if expected_row_bytes != header.row_bytes
        || header.payload_len != header.rows.saturating_mul(header.row_bytes)
        || !matches!(
            (&metadata.layout, metadata.config.method),
            (QuantizedLayout::Scalar { .. }, QuantizationMethod::Scalar)
                | (
                    QuantizedLayout::Product { .. },
                    QuantizationMethod::Product { .. }
                )
                | (QuantizedLayout::Binary, QuantizationMethod::Binary)
        )
    {
        return invalid("quantized payload shape differs from configured method");
    }
    let mut versions = BTreeSet::new();
    for candidate in &metadata.candidates {
        if candidate.source_cursor == 0
            || candidate.source_cursor > metadata.source_cursor
            || !versions.insert((candidate.reference.clone(), candidate.source_cursor))
        {
            return invalid("quantized candidate metadata is invalid");
        }
    }
    if metadata.candidates.windows(2).any(|pair| {
        (&pair[0].reference, pair[0].source_cursor) >= (&pair[1].reference, pair[1].source_cursor)
    }) {
        return invalid("quantized candidates are not in canonical order");
    }
    Ok(())
}

fn write_header(
    bytes: &mut [u8],
    metadata_len: usize,
    payload_offset: usize,
    payload_len: usize,
    row_bytes: usize,
    rows: usize,
    dimensions: usize,
) -> Result<()> {
    bytes[..8].copy_from_slice(MAGIC);
    put_u16(bytes, 8, QUANTIZED_SEGMENT_FORMAT_VERSION);
    put_u16(bytes, 10, 0);
    put_u32(bytes, 12, as_u32(HEADER_BYTES)?);
    put_u64(bytes, 16, as_u64(metadata_len)?);
    put_u64(bytes, 24, as_u64(payload_offset)?);
    put_u64(bytes, 32, as_u64(payload_len)?);
    put_u64(bytes, 40, as_u64(row_bytes)?);
    put_u64(bytes, 48, as_u64(rows)?);
    put_u32(bytes, 56, as_u32(dimensions)?);
    put_u32(bytes, 60, 0);
    put_u64(bytes, 64, as_u64(HEADER_BYTES)?);
    Ok(())
}

fn read_header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_ARTIFACT_BYTES {
        return invalid("quantized artifact byte length is outside bounds");
    }
    if &bytes[..8] != MAGIC
        || read_u16(bytes, 8)? != QUANTIZED_SEGMENT_FORMAT_VERSION
        || read_u16(bytes, 10)? != 0
        || read_u32(bytes, 12)? as usize != HEADER_BYTES
        || read_u64(bytes, 64)? as usize != HEADER_BYTES
        || bytes[104..HEADER_BYTES].iter().any(|byte| *byte != 0)
    {
        return invalid("quantized artifact header magic, version, or reserved bytes are invalid");
    }
    let header = Header {
        metadata_len: as_usize(read_u64(bytes, 16)?)?,
        payload_offset: as_usize(read_u64(bytes, 24)?)?,
        payload_len: as_usize(read_u64(bytes, 32)?)?,
        row_bytes: as_usize(read_u64(bytes, 40)?)?,
        rows: as_usize(read_u64(bytes, 48)?)?,
        dimensions: read_u32(bytes, 56)? as usize,
    };
    let metadata_end = HEADER_BYTES
        .checked_add(header.metadata_len)
        .ok_or_else(|| runtime_error("quantized metadata offset overflow"))?;
    let payload_end = header
        .payload_offset
        .checked_add(header.payload_len)
        .ok_or_else(|| runtime_error("quantized payload offset overflow"))?;
    if header.metadata_len == 0
        || header.row_bytes == 0
        || header.rows == 0
        || header.rows > MAX_CANDIDATES
        || header.dimensions == 0
        || header.dimensions > MAX_DIMENSIONS
        || metadata_end > header.payload_offset
        || !header.payload_offset.is_multiple_of(ALIGNMENT)
        || payload_end != bytes.len()
    {
        return invalid("quantized artifact header offsets or dimensions are invalid");
    }
    Ok(header)
}

fn artifact_digest(bytes: &[u8]) -> Result<[u8; 32]> {
    if bytes.len() < HEADER_BYTES {
        return invalid("quantized artifact is shorter than its header");
    }
    let mut root = [0_u8; HEADER_BYTES + DIGEST_BYTES];
    root[..HEADER_BYTES].copy_from_slice(&bytes[..HEADER_BYTES]);
    root[DIGEST_OFFSET..DIGEST_OFFSET + DIGEST_BYTES].fill(0);
    root[HEADER_BYTES..].copy_from_slice(&digest::sha256(&bytes[HEADER_BYTES..]));
    Ok(digest::sha256(&root))
}

fn hex_digest(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value)
        .map_err(|error| runtime_error(format!("quantized metadata cannot be encoded: {error}")))
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| runtime_error("quantized alignment overflow"))
}

fn as_u64(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| runtime_error("quantized value exceeds u64"))
}

fn as_u32(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| runtime_error("quantized value exceeds u32"))
}

fn as_usize(value: u64) -> Result<usize> {
    usize::try_from(value).map_err(|_| runtime_error("quantized value exceeds usize"))
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    bytes
        .get(offset..offset + 2)
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| runtime_error("quantized u16 header read is out of bounds"))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| runtime_error("quantized u32 header read is out of bounds"))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    bytes
        .get(offset..offset + 8)
        .and_then(|slice| slice.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| runtime_error("quantized u64 header read is out of bounds"))
}

fn runtime_error(reason: impl Into<String>) -> rrd_core::Error {
    rrd_core::Error::InvalidRuntime {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchMode;
    use rrd_core::{ReadStamp, RuntimeVector};
    use std::io::Write;

    fn candidate(scope: &ScopeId, id: usize, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: id as u64 + 1,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", format!("v{id}")).unwrap(),
                subject: RuntimeRef::new("document", format!("v{id}")).unwrap(),
                collection: None,
                field: "body".into(),
                valid_from: 1,
                valid_to: None,
                value: VectorValue::Dense { values },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        }
    }

    fn segment(method: QuantizationMethod) -> QuantizedSegment {
        let scope = ScopeId::new("instance:quantized-segment").unwrap();
        let candidates = (0..12)
            .map(|row| {
                candidate(
                    &scope,
                    row,
                    (0..64)
                        .map(|dimension| ((row * 17 + dimension * 13) % 101) as f32 / 50.0 - 1.0)
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        QuantizedSegment::build(
            QuantizedSegmentConfig {
                id: ProjectionId::new(format!("quant:{}", method.as_str())).unwrap(),
                scope,
                field: "body".into(),
                dimensions: 64,
                metric: ScoreMetric::Cosine,
                method,
                embedding_model: None,
                filter_properties: BTreeSet::new(),
            },
            1,
            12,
            candidates,
        )
        .unwrap()
    }

    #[test]
    fn scalar_product_binary_round_trip_corruption_mmap_and_kernel_matrix() {
        for method in [
            QuantizationMethod::Scalar,
            QuantizationMethod::Product {
                compression: ProductCompression::X64,
            },
            QuantizationMethod::Binary,
        ] {
            let segment = segment(method);
            let reopened = QuantizedSegment::from_bytes(segment.as_bytes()).unwrap();
            assert_eq!(segment, reopened);
            assert!(
                reopened.descriptor().full_precision_vector_bytes
                    / reopened.descriptor().packed_vector_bytes
                    <= method.maximum_compression_ratio()
            );
            let mut corrupt = segment.as_bytes().to_vec();
            let last = corrupt.len() - 1;
            corrupt[last] ^= 1;
            assert!(QuantizedSegment::from_bytes(&corrupt).is_err());

            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(segment.as_bytes()).unwrap();
            file.flush().unwrap();
            let mapped = QuantizedSegment::open_mmap(file.path()).unwrap();
            assert_eq!(mapped.memory_placement(), QuantizedMemoryPlacement::Mapped);

            let scope = ScopeId::new("instance:quantized-segment").unwrap();
            let query = SearchRequest {
                read: ReadStamp::new(scope.clone(), None, 0, 12, Some("11".repeat(32))).unwrap(),
                scope,
                valid_at: 10,
                field: "body".into(),
                query: VectorQuery::Dense {
                    values: (0..64)
                        .map(|dimension| ((3 * 17 + dimension * 13) % 101) as f32 / 50.0 - 1.0)
                        .collect(),
                },
                metric: ScoreMetric::Cosine,
                embedding_model: None,
                top_k: 3,
                mode: SearchMode::RequireApproximate { exact_rerank: 8 },
                filter: None,
            };
            let scalar = mapped
                .search_candidates_at(&query, 8, 12, QuantizedKernel::Scalar)
                .unwrap();
            let auto = mapped
                .search_candidates_at(&query, 8, 12, QuantizedKernel::Auto)
                .unwrap();
            assert_eq!(
                scalar.iter().map(|hit| &hit.reference).collect::<Vec<_>>(),
                auto.iter().map(|hit| &hit.reference).collect::<Vec<_>>()
            );
            for (scalar, auto) in scalar.iter().zip(&auto) {
                assert!((scalar.score - auto.score).abs() < 1.0e-5);
            }
            assert!(scalar.iter().any(|hit| hit.reference.id.as_str() == "v3"));
        }
    }
}
