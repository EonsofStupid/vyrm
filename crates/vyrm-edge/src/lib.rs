//! Minimal, offline edge profile: local embedding and mmap exact search.
//!
//! This crate intentionally has no HTTP client, model downloader, async
//! runtime, or database server dependency. A model adapter can be substituted
//! above `vyrm-embed`; the bundled feature-hash backend exists to keep the
//! complete source-provenance-to-search path executable without a network.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use vyrm_core::{
    digest, ProjectionId, ReadStamp, Result, RuntimeId, RuntimeProperties, RuntimeRef, ScopeId,
    VectorValue,
};
use vyrm_embed::{
    EmbeddingBackend, EmbeddingCoordinator, EmbeddingJob, EmbeddingSourceReader,
    EmbeddingSourceSnapshot, FeatureHashBackend, NetworkPolicy, EMBEDDING_CONTRACT_VERSION,
};
use vyrm_vector::{
    CompactDenseSegment, DenseKernel, EmbeddingModelBinding, ScoreMetric, SearchHit, SearchMode,
    SearchRequest, VectorCandidate, VectorQuery, VectorSegmentConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineEdgeConfig {
    pub scope: ScopeId,
    pub projection: ProjectionId,
    pub field: String,
    pub dimensions: u32,
    pub seed: u64,
}

impl OfflineEdgeConfig {
    pub fn standard(dimensions: u32, seed: u64) -> Result<Self> {
        Ok(Self {
            scope: ScopeId::new("instance:edge")?,
            projection: ProjectionId::new("vector:edge:body")?,
            field: "body".into(),
            dimensions,
            seed,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("offline edge field must be non-empty and contain no NUL bytes");
        }
        FeatureHashBackend::new(self.dimensions, self.seed)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineDocument {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub properties: RuntimeProperties,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineQueryResult {
    pub model: EmbeddingModelBinding,
    pub artifact_digest: String,
    pub source_cursor: u64,
    pub hits: Vec<SearchHit>,
}

pub struct OfflineEdgeIndex {
    config: OfflineEdgeConfig,
    backend: FeatureHashBackend,
    artifact: CompactDenseSegment,
}

impl OfflineEdgeIndex {
    pub fn build(
        config: OfflineEdgeConfig,
        generation: u64,
        documents: impl IntoIterator<Item = OfflineDocument>,
    ) -> Result<Self> {
        config.validate()?;
        let documents = documents.into_iter().collect::<Vec<_>>();
        if documents.is_empty() {
            return invalid("offline edge build requires at least one document");
        }
        let source_cursor = u64::try_from(documents.len())
            .map_err(|_| runtime_error("offline edge document count exceeds u64"))?;
        let corpus_digest =
            digest::sha256_hex(&serde_json::to_vec(&documents).map_err(|error| {
                runtime_error(format!("offline edge documents cannot be encoded: {error}"))
            })?);
        let mut backend = FeatureHashBackend::new(config.dimensions, config.seed)?;
        let model = model_binding(&backend);
        let mut candidates = Vec::with_capacity(documents.len());
        for (index, document) in documents.into_iter().enumerate() {
            let cursor = index as u64 + 1;
            let source = RuntimeRef::new("document", document.id.clone())?;
            let snapshot = EmbeddingSourceSnapshot::for_bytes(
                source.clone(),
                "text/plain",
                document.text.into_bytes(),
            )?;
            let job = EmbeddingJob {
                contract_version: EMBEDDING_CONTRACT_VERSION,
                id: RuntimeId::new(format!("edge-embed:{}", document.id))?,
                scope: config.scope.clone(),
                read: ReadStamp::new(
                    config.scope.clone(),
                    None,
                    0,
                    source_cursor,
                    Some(corpus_digest.clone()),
                )?,
                source: source.clone(),
                expected_source_digest: snapshot.digest.clone(),
                target: RuntimeRef::new("embedding", document.id.clone())?,
                subject: source,
                field: config.field.clone(),
                valid_from: 1,
                valid_to: None,
                model: backend.descriptor().model.clone(),
                network_policy: NetworkPolicy::Deny,
                requested_at: 1,
                properties: document.properties,
            };
            let mut reader = StableSource(snapshot);
            let prepared = EmbeddingCoordinator::prepare(&job, &mut reader, &mut backend)?;
            candidates.push(VectorCandidate {
                scope: config.scope.clone(),
                source_cursor: cursor,
                vector: prepared.vector,
            });
        }
        let artifact = CompactDenseSegment::build(
            VectorSegmentConfig {
                id: config.projection.clone(),
                scope: config.scope.clone(),
                field: config.field.clone(),
                dimensions: config.dimensions as usize,
                metric: ScoreMetric::Cosine,
                embedding_model: Some(model),
                filter_properties: BTreeSet::new(),
            },
            generation,
            source_cursor,
            candidates,
        )?;
        Ok(Self {
            config,
            backend,
            artifact,
        })
    }

    pub fn open_mmap(config: OfflineEdgeConfig, path: impl AsRef<Path>) -> Result<Self> {
        config.validate()?;
        let backend = FeatureHashBackend::new(config.dimensions, config.seed)?;
        let artifact = CompactDenseSegment::open_mmap(path)?;
        let descriptor = artifact.descriptor();
        if descriptor.scope != config.scope
            || descriptor.stamp.id != config.projection
            || descriptor.field != config.field
            || descriptor.dimensions != config.dimensions as usize
            || descriptor.metric != ScoreMetric::Cosine
            || descriptor.embedding_model.as_ref() != Some(&model_binding(&backend))
        {
            return invalid("offline edge configuration differs from the mapped artifact");
        }
        Ok(Self {
            config,
            backend,
            artifact,
        })
    }

    pub fn write_atomic(&self, path: impl AsRef<Path>) -> Result<()> {
        self.artifact.write_atomic(path)
    }

    /// Embeds and searches locally in one API call. `NetworkPolicy::Deny` is
    /// re-applied to the query job, so a future network-requiring adapter cannot
    /// silently enter this profile.
    pub fn search_text(
        &mut self,
        text: impl Into<String>,
        top_k: usize,
        valid_at: u64,
    ) -> Result<OfflineQueryResult> {
        let text = text.into();
        let source = RuntimeRef::new("query", "current")?;
        let snapshot =
            EmbeddingSourceSnapshot::for_bytes(source.clone(), "text/plain", text.into_bytes())?;
        let descriptor = self.artifact.descriptor();
        let job = EmbeddingJob {
            contract_version: EMBEDDING_CONTRACT_VERSION,
            id: RuntimeId::new("edge-query")?,
            scope: self.config.scope.clone(),
            read: ReadStamp::new(
                self.config.scope.clone(),
                None,
                0,
                descriptor.stamp.source_cursor,
                Some(descriptor.stamp.artifact_digest.clone()),
            )?,
            source: source.clone(),
            expected_source_digest: snapshot.digest.clone(),
            target: RuntimeRef::new("query_embedding", "current")?,
            subject: source,
            field: self.config.field.clone(),
            valid_from: valid_at,
            valid_to: None,
            model: self.backend.descriptor().model.clone(),
            network_policy: NetworkPolicy::Deny,
            requested_at: valid_at,
            properties: RuntimeProperties::new(),
        };
        let mut reader = StableSource(snapshot);
        let prepared = EmbeddingCoordinator::prepare(&job, &mut reader, &mut self.backend)?;
        let VectorValue::Dense { values } = prepared.vector.value else {
            return invalid("offline edge text backend returned a non-dense vector");
        };
        let model = model_binding(&self.backend);
        let request = SearchRequest {
            scope: self.config.scope.clone(),
            read: job.read,
            valid_at,
            field: self.config.field.clone(),
            query: VectorQuery::Dense { values },
            metric: ScoreMetric::Cosine,
            embedding_model: Some(model.clone()),
            top_k,
            mode: SearchMode::Exact,
            filter: None,
        };
        let hits = self.artifact.search(&request, DenseKernel::Auto)?;
        Ok(OfflineQueryResult {
            model,
            artifact_digest: descriptor.stamp.artifact_digest.clone(),
            source_cursor: descriptor.stamp.source_cursor,
            hits,
        })
    }

    pub fn artifact(&self) -> &CompactDenseSegment {
        &self.artifact
    }
}

struct StableSource(EmbeddingSourceSnapshot);

impl EmbeddingSourceReader for StableSource {
    fn read(&mut self, source: &RuntimeRef) -> Result<EmbeddingSourceSnapshot> {
        if source != &self.0.source {
            return invalid("offline edge source reader received the wrong identity");
        }
        Ok(self.0.clone())
    }
}

fn model_binding(backend: &FeatureHashBackend) -> EmbeddingModelBinding {
    EmbeddingModelBinding {
        name: backend.descriptor().model.canonical_name(),
        digest: backend.descriptor().model.model_digest.clone(),
    }
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(runtime_error(reason))
}

fn runtime_error(reason: impl Into<String>) -> vyrm_core::Error {
    vyrm_core::Error::InvalidRuntime {
        reason: reason.into(),
    }
}
