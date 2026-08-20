//! Compact, immutable dense-vector artifact with a zero-copy mmap read path.
//!
//! The legacy JSON segment remains a portable semantic fixture. This format
//! separates canonical JSON metadata from aligned row-major `f32` payloads so
//! production search does not deserialize or duplicate the vector corpus.

use crate::contract::invalid;
use crate::exact::validate_candidate_versions;
use crate::{ScoreMetric, SearchHit, SearchRequest, VectorCandidate, VectorSegmentConfig};
use memmap2::{Mmap, MmapOptions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use vyrm_core::{
    digest, EmbeddingProvenance, ProjectionStamp, ProjectionState, Result, RuntimeCommit,
    RuntimeMutation, RuntimeProperties, RuntimeRef, RuntimeVector, VectorValue,
    DATA_RUNTIME_CONTRACT_VERSION,
};

pub const COMPACT_DENSE_FORMAT_VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"VYRDMAP1";
const HEADER_BYTES: usize = 128;
const DIGEST_OFFSET: usize = 72;
const DIGEST_BYTES: usize = 32;
const ALIGNMENT: usize = 64;
const MAX_ARTIFACT_BYTES: usize = 1 << 30;
const MAX_CANDIDATES: usize = 10_000_000;
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenseMemoryPlacement {
    Owned,
    Mapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenseKernel {
    /// Byte-decoding scalar reference, independent of host alignment.
    Scalar,
    /// Runtime-dispatched AVX2 where available, otherwise the scalar oracle.
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DenseMetadata {
    config: VectorSegmentConfig,
    generation: u64,
    source_cursor: u64,
    minimum_cursor: u64,
    candidates: Vec<DenseCandidateMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DenseCandidateMetadata {
    scope: vyrm_core::ScopeId,
    source_cursor: u64,
    reference: RuntimeRef,
    subject: RuntimeRef,
    field: String,
    valid_from: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    valid_to: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provenance: Option<EmbeddingProvenance>,
    #[serde(default)]
    properties: RuntimeProperties,
}

#[derive(Debug, Clone)]
enum DenseStorage {
    Owned(Arc<[u8]>),
    Mapped(Arc<Mmap>),
}

impl DenseStorage {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Mapped(bytes) => bytes,
        }
    }

    fn placement(&self) -> DenseMemoryPlacement {
        match self {
            Self::Owned(_) => DenseMemoryPlacement::Owned,
            Self::Mapped(_) => DenseMemoryPlacement::Mapped,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactDenseSegment {
    descriptor: crate::SegmentDescriptor,
    metadata: DenseMetadata,
    vector_offset: usize,
    row_stride: usize,
    storage: DenseStorage,
}

impl PartialEq for CompactDenseSegment {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.as_bytes() == other.as_bytes()
    }
}

impl CompactDenseSegment {
    pub fn build(
        config: VectorSegmentConfig,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        config.validate()?;
        if generation == 0 {
            return invalid("compact dense generation must be greater than zero");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.len() > MAX_CANDIDATES {
            return invalid("compact dense candidate limit exceeded");
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
                return invalid("compact dense candidate violates configuration or coverage");
            }
        }
        candidates.sort_by(|left, right| {
            left.vector
                .reference
                .cmp(&right.vector.reference)
                .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        });
        let metadata = DenseMetadata {
            config,
            generation,
            source_cursor,
            minimum_cursor: 0,
            candidates: candidates
                .iter()
                .map(DenseCandidateMetadata::from)
                .collect(),
        };
        let metadata_bytes = encode_json(&metadata)?;
        let vector_offset = align_up(
            HEADER_BYTES
                .checked_add(metadata_bytes.len())
                .ok_or_else(|| runtime_error("compact dense metadata length overflow"))?,
            ALIGNMENT,
        )?;
        let row_bytes = metadata
            .config
            .dimensions
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| runtime_error("compact dense row length overflow"))?;
        let row_stride = align_up(row_bytes, ALIGNMENT)?;
        let vector_bytes = row_stride
            .checked_mul(candidates.len())
            .ok_or_else(|| runtime_error("compact dense payload length overflow"))?;
        let total = vector_offset
            .checked_add(vector_bytes)
            .ok_or_else(|| runtime_error("compact dense artifact length overflow"))?;
        if total > MAX_ARTIFACT_BYTES {
            return invalid("compact dense artifact exceeds the 1 GiB safety limit");
        }

        let mut bytes = vec![0; total];
        write_header(
            &mut bytes,
            metadata_bytes.len(),
            vector_offset,
            vector_bytes,
            row_stride,
            candidates.len(),
            metadata.config.dimensions,
        )?;
        bytes[HEADER_BYTES..HEADER_BYTES + metadata_bytes.len()].copy_from_slice(&metadata_bytes);
        for (row, candidate) in candidates.iter().enumerate() {
            let VectorValue::Dense { values } = &candidate.vector.value else {
                unreachable!("dense shape was checked above")
            };
            let start = vector_offset + row * row_stride;
            for (chunk, value) in bytes[start..start + row_bytes]
                .chunks_exact_mut(4)
                .zip(values)
            {
                chunk.copy_from_slice(&value.to_le_bytes());
            }
        }
        let artifact_digest = artifact_digest(&bytes)?;
        bytes[DIGEST_OFFSET..DIGEST_OFFSET + DIGEST_BYTES].copy_from_slice(&artifact_digest);
        Self::decode(DenseStorage::Owned(Arc::from(bytes)))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return invalid("compact dense artifact exceeds the 1 GiB safety limit");
        }
        Self::decode(DenseStorage::Owned(Arc::from(bytes)))
    }

    /// Opens a verified read-only memory map. The artifact must remain
    /// immutable for the lifetime of this value; publication through
    /// [`Self::write_atomic`] establishes that contract for local files.
    pub fn open_mmap(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|error| {
            runtime_error(format!(
                "cannot open compact dense artifact {}: {error}",
                path.display()
            ))
        })?;
        let length = file
            .metadata()
            .map_err(|error| runtime_error(format!("cannot stat compact dense artifact: {error}")))?
            .len();
        if length == 0 || length > MAX_ARTIFACT_BYTES as u64 {
            return invalid("compact dense artifact file length is outside safety bounds");
        }
        // SAFETY: the file is opened read-only, its non-zero bounded length was
        // checked, and the returned Mmap owns the mapping independently of File.
        // Callers must uphold the documented immutable-file contract.
        let mmap = unsafe { MmapOptions::new().map(&file) }.map_err(|error| {
            runtime_error(format!("cannot map compact dense artifact: {error}"))
        })?;
        Self::decode(DenseStorage::Mapped(Arc::new(mmap)))
    }

    /// Durably stages and atomically publishes this immutable artifact. An
    /// existing path is accepted only when it contains the same verified bytes.
    pub fn write_atomic(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_error(format!("cannot create compact artifact directory: {error}"))
        })?;
        if path.exists() {
            let existing = Self::open_mmap(path)?;
            if existing.as_bytes() == self.as_bytes() {
                return Ok(());
            }
            return invalid("compact dense publication path contains different bytes");
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| runtime_error("compact dense publication path has no UTF-8 filename"))?;
        let temporary = parent.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let publish = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(self.as_bytes())?;
            file.sync_all()?;
            drop(file);
            // A hard-link publication is atomic and fail-if-present. Unlike
            // rename on Unix, it cannot overwrite a generation that won a race.
            std::fs::hard_link(&temporary, path)?;
            std::fs::remove_file(&temporary)?;
            File::open(parent)?.sync_all()
        })();
        if let Err(error) = publish {
            let _ = std::fs::remove_file(&temporary);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                let existing = Self::open_mmap(path)?;
                if existing.as_bytes() == self.as_bytes() {
                    return Ok(());
                }
                return invalid("compact dense publication race installed different bytes");
            }
            return Err(runtime_error(format!(
                "cannot publish compact dense artifact: {error}"
            )));
        }
        let published = Self::open_mmap(path)?;
        if published.descriptor != self.descriptor {
            return invalid("published compact dense descriptor failed verification");
        }
        Ok(())
    }

    pub fn descriptor(&self) -> &crate::SegmentDescriptor {
        &self.descriptor
    }

    pub fn memory_placement(&self) -> DenseMemoryPlacement {
        self.storage.placement()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.storage.as_bytes()
    }

    pub fn vector_payload_bytes(&self) -> usize {
        self.row_stride * self.metadata.candidates.len()
    }

    pub fn search(&self, request: &SearchRequest, kernel: DenseKernel) -> Result<Vec<SearchHit>> {
        self.search_at(request, kernel, request.read.commit_cursor)
    }

    pub fn search_at(
        &self,
        request: &SearchRequest,
        kernel: DenseKernel,
        required_source_cursor: u64,
    ) -> Result<Vec<SearchHit>> {
        request.validate()?;
        self.descriptor.validate()?;
        if required_source_cursor > request.read.commit_cursor {
            return invalid("compact dense source cursor exceeds the request read stamp");
        }
        if self.descriptor.stamp.state != ProjectionState::Ready
            || self.descriptor.scope != request.scope
            || self.descriptor.field != request.field
            || self.descriptor.metric != request.metric
            || self.descriptor.embedding_model != request.embedding_model
            || self.descriptor.dimensions != request.query.dimensions()
            || self.descriptor.stamp.source_cursor < required_source_cursor
        {
            return invalid("compact dense segment does not satisfy request identity or freshness");
        }
        let crate::VectorQuery::Dense { values: query } = &request.query else {
            return invalid("compact dense segment requires a dense query");
        };
        let mut latest = BTreeMap::<&RuntimeRef, usize>::new();
        for (row, candidate) in self.metadata.candidates.iter().enumerate() {
            if candidate.scope != request.scope
                || candidate.source_cursor > request.read.commit_cursor
            {
                continue;
            }
            if candidate.valid_from > request.valid_at {
                continue;
            }
            latest
                .entry(&candidate.reference)
                .and_modify(|current| {
                    if self.metadata.candidates[*current].source_cursor < candidate.source_cursor {
                        *current = row;
                    }
                })
                .or_insert(row);
        }
        let mut hits = Vec::new();
        for row in latest.into_values() {
            let candidate = &self.metadata.candidates[row];
            if candidate.field != request.field
                || candidate
                    .valid_to
                    .is_some_and(|valid_to| request.valid_at >= valid_to)
                || request
                    .filter
                    .as_ref()
                    .is_some_and(|filter| !filter.matches(&candidate.properties))
            {
                continue;
            }
            hits.push(SearchHit {
                reference: candidate.reference.clone(),
                subject: candidate.subject.clone(),
                source_cursor: candidate.source_cursor,
                score: score_row(query, self.row(row), request.metric, kernel)?,
            });
        }
        hits.sort_by(SearchHit::compare_best_first);
        hits.truncate(request.top_k);
        Ok(hits)
    }

    fn decode(storage: DenseStorage) -> Result<Self> {
        let bytes = storage.as_bytes();
        let header = parse_header(bytes)?;
        let expected_digest = artifact_digest(bytes)?;
        if bytes[DIGEST_OFFSET..DIGEST_OFFSET + DIGEST_BYTES] != expected_digest {
            return invalid("compact dense artifact digest mismatch");
        }
        let metadata_bytes = &bytes[HEADER_BYTES..HEADER_BYTES + header.metadata_len];
        let metadata: DenseMetadata = serde_json::from_slice(metadata_bytes).map_err(|error| {
            runtime_error(format!("compact dense metadata cannot be decoded: {error}"))
        })?;
        if encode_json(&metadata)? != metadata_bytes {
            return invalid("compact dense metadata is not canonical JSON");
        }
        validate_decoded(&metadata, &header, bytes)?;
        let config_digest = metadata.config.digest()?;
        let descriptor = crate::SegmentDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: metadata.config.id.clone(),
                generation: metadata.generation,
                source_cursor: metadata.source_cursor,
                config_digest,
                artifact_digest: hex_digest(&expected_digest),
                state: ProjectionState::Ready,
            },
            scope: metadata.config.scope.clone(),
            field: metadata.config.field.clone(),
            dimensions: metadata.config.dimensions,
            metric: metadata.config.metric,
            embedding_model: metadata.config.embedding_model.clone(),
            filter_properties: metadata.config.filter_properties.clone(),
            minimum_cursor: metadata.minimum_cursor,
            candidate_versions: metadata.candidates.len(),
        };
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            metadata,
            vector_offset: header.vector_offset,
            row_stride: header.row_stride,
            storage,
        })
    }

    fn row(&self, row: usize) -> &[u8] {
        let row_bytes = self.metadata.config.dimensions * 4;
        let start = self.vector_offset + row * self.row_stride;
        &self.as_bytes()[start..start + row_bytes]
    }
}

impl From<&VectorCandidate> for DenseCandidateMetadata {
    fn from(candidate: &VectorCandidate) -> Self {
        Self {
            scope: candidate.scope.clone(),
            source_cursor: candidate.source_cursor,
            reference: candidate.vector.reference.clone(),
            subject: candidate.vector.subject.clone(),
            field: candidate.vector.field.clone(),
            valid_from: candidate.vector.valid_from,
            valid_to: candidate.vector.valid_to,
            provenance: candidate.vector.provenance.clone(),
            properties: candidate.vector.properties.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Header {
    metadata_len: usize,
    vector_offset: usize,
    vector_len: usize,
    row_stride: usize,
    rows: usize,
    dimensions: usize,
}

fn write_header(
    bytes: &mut [u8],
    metadata_len: usize,
    vector_offset: usize,
    vector_len: usize,
    row_stride: usize,
    rows: usize,
    dimensions: usize,
) -> Result<()> {
    bytes[..8].copy_from_slice(MAGIC);
    put_u16(bytes, 8, COMPACT_DENSE_FORMAT_VERSION);
    put_u16(bytes, 10, 0);
    put_u32(bytes, 12, HEADER_BYTES as u32);
    put_u64(bytes, 16, HEADER_BYTES as u64);
    put_u64(bytes, 24, as_u64(metadata_len)?);
    put_u64(bytes, 32, as_u64(vector_offset)?);
    put_u64(bytes, 40, as_u64(vector_len)?);
    put_u64(bytes, 48, as_u64(row_stride)?);
    put_u64(bytes, 56, as_u64(rows)?);
    put_u32(bytes, 64, as_u32(dimensions)?);
    put_u32(bytes, 68, 0);
    Ok(())
}

fn parse_header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_ARTIFACT_BYTES {
        return invalid("compact dense artifact length is outside safety bounds");
    }
    if &bytes[..8] != MAGIC
        || read_u16(bytes, 8)? != COMPACT_DENSE_FORMAT_VERSION
        || read_u16(bytes, 10)? != 0
        || read_u32(bytes, 12)? as usize != HEADER_BYTES
        || read_u64(bytes, 16)? as usize != HEADER_BYTES
        || read_u32(bytes, 68)? != 0
        || bytes[104..HEADER_BYTES].iter().any(|byte| *byte != 0)
    {
        return invalid(
            "compact dense header magic, version, flags, or reserved bytes are invalid",
        );
    }
    let header = Header {
        metadata_len: as_usize(read_u64(bytes, 24)?)?,
        vector_offset: as_usize(read_u64(bytes, 32)?)?,
        vector_len: as_usize(read_u64(bytes, 40)?)?,
        row_stride: as_usize(read_u64(bytes, 48)?)?,
        rows: as_usize(read_u64(bytes, 56)?)?,
        dimensions: read_u32(bytes, 64)? as usize,
    };
    let metadata_end = HEADER_BYTES
        .checked_add(header.metadata_len)
        .ok_or_else(|| runtime_error("compact dense metadata offset overflow"))?;
    let vector_end = header
        .vector_offset
        .checked_add(header.vector_len)
        .ok_or_else(|| runtime_error("compact dense vector offset overflow"))?;
    if header.metadata_len == 0
        || header.dimensions == 0
        || header.dimensions > 1_048_576
        || header.rows > MAX_CANDIDATES
        || metadata_end > header.vector_offset
        || !header.vector_offset.is_multiple_of(ALIGNMENT)
        || !header.row_stride.is_multiple_of(ALIGNMENT)
        || header.row_stride < header.dimensions.saturating_mul(4)
        || header.vector_len != header.row_stride.saturating_mul(header.rows)
        || vector_end != bytes.len()
        || bytes[metadata_end..header.vector_offset]
            .iter()
            .any(|byte| *byte != 0)
    {
        return invalid("compact dense header offsets or dimensions are invalid");
    }
    Ok(header)
}

fn validate_decoded(metadata: &DenseMetadata, header: &Header, bytes: &[u8]) -> Result<()> {
    metadata.config.validate()?;
    if metadata.generation == 0
        || metadata.minimum_cursor != 0
        || metadata.config.dimensions != header.dimensions
        || metadata.candidates.len() != header.rows
    {
        return invalid("compact dense metadata differs from its physical header");
    }
    let mut versions = Vec::with_capacity(metadata.candidates.len());
    let row_bytes = header.dimensions * 4;
    for (row, candidate) in metadata.candidates.iter().enumerate() {
        if candidate.scope != metadata.config.scope
            || candidate.source_cursor == 0
            || candidate.source_cursor > metadata.source_cursor
            || candidate.field != metadata.config.field
        {
            return invalid("compact dense candidate violates metadata coverage");
        }
        let start = header.vector_offset + row * header.row_stride;
        let payload = &bytes[start..start + row_bytes];
        let values = decode_row(payload)?;
        let vector = RuntimeVector {
            reference: candidate.reference.clone(),
            subject: candidate.subject.clone(),
            field: candidate.field.clone(),
            valid_from: candidate.valid_from,
            valid_to: candidate.valid_to,
            value: VectorValue::Dense { values },
            provenance: candidate.provenance.clone(),
            properties: candidate.properties.clone(),
        };
        RuntimeCommit {
            scope: candidate.scope.clone(),
            at: candidate.valid_from,
            actor: "vyrm:compact-validator".into(),
            expected_cursor: 0,
            mutations: vec![RuntimeMutation::Vector {
                vector: vector.clone(),
            }],
        }
        .validate()?;
        let candidate_for_model = VectorCandidate {
            scope: candidate.scope.clone(),
            source_cursor: candidate.source_cursor,
            vector: vector.clone(),
        };
        if !candidate_for_model.matches_model(metadata.config.embedding_model.as_ref()) {
            return invalid("compact dense candidate differs from its configured embedding model");
        }
        if bytes[start + row_bytes..start + header.row_stride]
            .iter()
            .any(|byte| *byte != 0)
        {
            return invalid("compact dense row padding is non-zero");
        }
        versions.push(VectorCandidate {
            scope: candidate.scope.clone(),
            source_cursor: candidate.source_cursor,
            vector: RuntimeVector {
                reference: candidate.reference.clone(),
                subject: candidate.subject.clone(),
                field: candidate.field.clone(),
                valid_from: candidate.valid_from,
                valid_to: candidate.valid_to,
                value: VectorValue::Dense {
                    values: vec![0.0; header.dimensions],
                },
                provenance: None,
                properties: candidate.properties.clone(),
            },
        });
    }
    validate_candidate_versions(&versions)?;
    if metadata.candidates.windows(2).any(|pair| {
        (&pair[0].reference, pair[0].source_cursor) >= (&pair[1].reference, pair[1].source_cursor)
    }) {
        return invalid("compact dense candidates are not in canonical order");
    }
    Ok(())
}

fn score_row(query: &[f32], row: &[u8], metric: ScoreMetric, kernel: DenseKernel) -> Result<f64> {
    if row.len() != query.len() * 4 {
        return invalid("compact dense query and row dimensions differ");
    }
    #[cfg(target_arch = "x86_64")]
    if kernel == DenseKernel::Auto && std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 was checked at runtime; the implementation uses unaligned
        // loads and row/query lengths were proven equal above.
        return Ok(unsafe { score_avx2(query, row, metric) });
    }
    score_scalar(query, row, metric)
}

fn score_scalar(query: &[f32], row: &[u8], metric: ScoreMetric) -> Result<f64> {
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    let mut squared_distance = 0.0;
    let mut manhattan = 0.0;
    for (left, chunk) in query.iter().zip(row.chunks_exact(4)) {
        let right = f32::from_le_bytes(chunk.try_into().expect("four-byte vector value"));
        let left = f64::from(*left);
        let right = f64::from(right);
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
        squared_distance += (left - right).powi(2);
        manhattan += (left - right).abs();
    }
    Ok(match metric {
        ScoreMetric::Dot => dot,
        ScoreMetric::Cosine if right_norm == 0.0 => 0.0,
        ScoreMetric::Cosine => dot / (left_norm.sqrt() * right_norm.sqrt()),
        ScoreMetric::Euclidean => -squared_distance.sqrt(),
        ScoreMetric::Manhattan => -manhattan,
    })
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn score_avx2(query: &[f32], row: &[u8], metric: ScoreMetric) -> f64 {
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
        let right = _mm256_loadu_ps(row.as_ptr().add(index * 4).cast::<f32>());
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
    for (left, chunk) in query[index..].iter().zip(row[index * 4..].chunks_exact(4)) {
        let right = f32::from_le_bytes(chunk.try_into().expect("four-byte vector value"));
        let left = f64::from(*left);
        let right = f64::from(right);
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

fn decode_row(row: &[u8]) -> Result<Vec<f32>> {
    row.chunks_exact(4)
        .map(|chunk| {
            let value = f32::from_le_bytes(chunk.try_into().expect("four-byte vector value"));
            if !value.is_finite() {
                return invalid("compact dense vector contains a non-finite value");
            }
            Ok(value)
        })
        .collect()
}

fn artifact_digest(bytes: &[u8]) -> Result<[u8; 32]> {
    if bytes.len() < HEADER_BYTES {
        return invalid("compact dense artifact is shorter than its header");
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
    serde_json::to_vec(value).map_err(|error| {
        runtime_error(format!("compact dense metadata cannot be encoded: {error}"))
    })
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| runtime_error("compact dense alignment overflow"))
}

fn as_u64(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| runtime_error("compact dense value exceeds u64"))
}

fn as_u32(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| runtime_error("compact dense value exceeds u32"))
}

fn as_usize(value: u64) -> Result<usize> {
    usize::try_from(value).map_err(|_| runtime_error("compact dense value exceeds usize"))
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
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| runtime_error("compact dense u16 header field is truncated"))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| runtime_error("compact dense u32 header field is truncated"))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| runtime_error("compact dense u64 header field is truncated"))
}

fn runtime_error(reason: impl Into<String>) -> vyrm_core::Error {
    vyrm_core::Error::InvalidRuntime {
        reason: reason.into(),
    }
}
