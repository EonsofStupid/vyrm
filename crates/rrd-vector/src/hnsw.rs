use crate::contract::invalid;
use crate::exact::{score_dense, validate_candidate_versions};
use crate::{
    search_exact, AccessPathKind, CandidatePath, EmbeddingModelBinding, ScoreMetric, SearchHit,
    SearchMode, SearchRequest, VectorCandidate, VectorQuery,
};
use rrd_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, Result, ScopeId, VectorValue,
    DATA_RUNTIME_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

pub const HNSW_FORMAT_VERSION: u16 = 2;
const HNSW_MAGIC: &str = "RRDHNS02";
const MAX_HNSW_BYTES: usize = 1 << 30;
const MAX_HNSW_NODES: usize = 10_000_000;
// A graph visit performs vector scoring plus heap/navigation work. Four
// score-equivalent units is deliberately conservative at filter crossovers;
// callers may still force ANN through `RequireApproximate`.
const HNSW_NAVIGATION_COST_MULTIPLIER: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HnswMaintenanceKind {
    #[default]
    FullBuild,
    Incremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HnswKernel {
    Scalar,
    /// Runtime-dispatched AVX2 on supported x86_64 hosts, otherwise scalar.
    Auto,
}

fn is_full_build(value: &HnswMaintenanceKind) -> bool {
    *value == HnswMaintenanceKind::FullBuild
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HnswConfig {
    pub id: ProjectionId,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    pub m: usize,
    pub ef_construction: usize,
    pub max_level: u8,
    pub seed: u64,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
}

impl HnswConfig {
    pub fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() || self.field.as_bytes().contains(&0) {
            return invalid("HNSW field must be non-empty and contain no NUL bytes");
        }
        if self.dimensions == 0 || self.dimensions > 1_048_576 {
            return invalid("HNSW dimensions must be in 1..=1048576");
        }
        if !(2..=128).contains(&self.m) {
            return invalid("HNSW m must be in 2..=128");
        }
        if self.ef_construction < self.m || self.ef_construction > 1_000_000 {
            return invalid("HNSW ef_construction must be in m..=1000000");
        }
        if self.max_level == 0 || self.max_level > 32 {
            return invalid("HNSW max_level must be in 1..=32");
        }
        if self
            .filter_properties
            .iter()
            .any(|property| property.trim().is_empty() || property.as_bytes().contains(&0))
        {
            return invalid("HNSW filter properties must be valid names");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(digest::sha256_hex(&encode_json(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HnswDescriptor {
    pub stamp: ProjectionStamp,
    pub scope: ScopeId,
    pub field: String,
    pub dimensions: usize,
    pub metric: ScoreMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<EmbeddingModelBinding>,
    pub m: usize,
    pub ef_construction: usize,
    pub max_level: u8,
    pub nodes: usize,
    #[serde(default)]
    pub filter_properties: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "is_full_build")]
    pub maintenance: HnswMaintenanceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub indexed_delta_vectors: usize,
}

impl HnswDescriptor {
    pub fn validate(&self) -> Result<()> {
        self.stamp.validate()?;
        if self.field.trim().is_empty()
            || self.field.as_bytes().contains(&0)
            || self.dimensions == 0
            || !(2..=128).contains(&self.m)
            || self.ef_construction < self.m
            || self.ef_construction > 1_000_000
            || self.max_level == 0
            || self.max_level > 32
            || self.nodes > MAX_HNSW_NODES
            || self
                .filter_properties
                .iter()
                .any(|property| property.trim().is_empty() || property.as_bytes().contains(&0))
        {
            return invalid("HNSW descriptor contains invalid build parameters");
        }
        if let Some(model) = &self.embedding_model {
            model.validate()?;
        }
        match self.maintenance {
            HnswMaintenanceKind::FullBuild if self.previous_generation.is_some() => {
                return invalid("full-build HNSW descriptor cannot name a previous generation");
            }
            HnswMaintenanceKind::Incremental
                if self.previous_generation != self.stamp.generation.checked_sub(1)
                    || self.indexed_delta_vectors == 0
                    || self.indexed_delta_vectors > self.nodes =>
            {
                return invalid("incremental HNSW descriptor has incoherent maintenance evidence");
            }
            HnswMaintenanceKind::FullBuild | HnswMaintenanceKind::Incremental => {}
        }
        Ok(())
    }

    pub fn candidate_path(&self, estimated_cost: u64) -> CandidatePath {
        CandidatePath {
            stamp: self.stamp.clone(),
            kind: AccessPathKind::Hnsw,
            field: self.field.clone(),
            dimensions: self.dimensions,
            metric: self.metric,
            embedding_model: self.embedding_model.clone(),
            filter_properties: self.filter_properties.clone(),
            estimated_candidates: self.nodes as u64,
            estimated_cost,
            overlay_source_cursor: None,
            overlay_candidates: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HnswNode {
    candidate: VectorCandidate,
    level: u8,
    /// Neighbor ids by layer, layer zero first.
    neighbors: Vec<Vec<usize>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HnswBody {
    config: HnswConfig,
    generation: u64,
    source_cursor: u64,
    entrypoint: Option<usize>,
    nodes: Vec<HnswNode>,
    #[serde(default, skip_serializing_if = "is_full_build")]
    maintenance: HnswMaintenanceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    indexed_delta_vectors: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HnswEnvelope {
    magic: String,
    format_version: u16,
    artifact_digest: String,
    body: HnswBody,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HnswIndex {
    descriptor: HnswDescriptor,
    config: HnswConfig,
    entrypoint: Option<usize>,
    nodes: Vec<HnswNode>,
    versions: BTreeMap<rrd_core::RuntimeRef, Vec<usize>>,
    bytes: Vec<u8>,
}

impl HnswIndex {
    pub fn build(
        config: HnswConfig,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        config.validate()?;
        if generation == 0 {
            return invalid("HNSW generation must be greater than zero");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.len() > MAX_HNSW_NODES {
            return invalid("HNSW node limit exceeded");
        }
        validate_candidate_versions(&candidates)?;
        candidates.sort_by(|left, right| {
            left.vector
                .reference
                .cmp(&right.vector.reference)
                .then_with(|| left.source_cursor.cmp(&right.source_cursor))
        });
        let mut nodes = Vec::with_capacity(candidates.len());
        let mut entrypoint = None;
        for candidate in candidates {
            if candidate.scope != config.scope
                || candidate.source_cursor > source_cursor
                || candidate.vector.field != config.field
                || candidate.vector.value.dimensions() != config.dimensions
                || !candidate.matches_model(config.embedding_model.as_ref())
                || !matches!(candidate.vector.value, VectorValue::Dense { .. })
            {
                return invalid("HNSW candidate violates configuration or coverage");
            }
            insert_node(&config, &mut nodes, &mut entrypoint, candidate)?;
        }
        let body = HnswBody {
            config: config.clone(),
            generation,
            source_cursor,
            entrypoint,
            indexed_delta_vectors: nodes.len(),
            nodes,
            maintenance: HnswMaintenanceKind::FullBuild,
            previous_generation: None,
        };
        Self::from_body(body)
    }

    /// Appends only vector versions newer than this immutable generation.
    ///
    /// The returned generation is a new content-addressed artifact, so readers
    /// continue serving the prior graph while insertion work runs. Publication
    /// remains one catalogue CAS; no authoritative commit waits for graph work.
    pub fn advance(
        &self,
        generation: u64,
        source_cursor: u64,
        candidates: impl IntoIterator<Item = VectorCandidate>,
    ) -> Result<Self> {
        let expected_generation =
            self.descriptor
                .stamp
                .generation
                .checked_add(1)
                .ok_or_else(|| rrd_core::Error::InvalidRuntime {
                    reason: "HNSW generation overflowed".into(),
                })?;
        if generation != expected_generation {
            return invalid("incremental HNSW generation must immediately follow the active one");
        }
        if source_cursor <= self.descriptor.stamp.source_cursor {
            return invalid("incremental HNSW source cursor must advance");
        }
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        if candidates.is_empty() {
            return invalid("incremental HNSW update must contain at least one vector version");
        }
        if self
            .nodes
            .len()
            .checked_add(candidates.len())
            .is_none_or(|total| total > MAX_HNSW_NODES)
        {
            return invalid("HNSW node limit exceeded");
        }
        validate_candidate_versions(&candidates)?;
        let existing = self
            .nodes
            .iter()
            .map(|node| {
                (
                    node.candidate.vector.reference.clone(),
                    node.candidate.source_cursor,
                )
            })
            .collect::<BTreeSet<_>>();
        candidates.sort_by(|left, right| {
            left.source_cursor
                .cmp(&right.source_cursor)
                .then_with(|| left.vector.reference.cmp(&right.vector.reference))
        });
        for candidate in &candidates {
            if candidate.source_cursor <= self.descriptor.stamp.source_cursor
                || candidate.source_cursor > source_cursor
                || existing.contains(&(candidate.vector.reference.clone(), candidate.source_cursor))
                || candidate.scope != self.config.scope
                || candidate.vector.field != self.config.field
                || candidate.vector.value.dimensions() != self.config.dimensions
                || !candidate.matches_model(self.config.embedding_model.as_ref())
                || !matches!(candidate.vector.value, VectorValue::Dense { .. })
            {
                return invalid("incremental HNSW candidate violates configuration or coverage");
            }
        }
        let indexed_delta_vectors = candidates.len();
        let mut nodes = self.nodes.clone();
        let mut entrypoint = self.entrypoint;
        for candidate in candidates {
            insert_node(&self.config, &mut nodes, &mut entrypoint, candidate)?;
        }
        Self::from_body(HnswBody {
            config: self.config.clone(),
            generation,
            source_cursor,
            entrypoint,
            nodes,
            maintenance: HnswMaintenanceKind::Incremental,
            previous_generation: Some(self.descriptor.stamp.generation),
            indexed_delta_vectors,
        })
    }

    fn from_body(body: HnswBody) -> Result<Self> {
        let artifact_digest = digest::sha256_hex(&encode_json(&body)?);
        let envelope = HnswEnvelope {
            magic: HNSW_MAGIC.into(),
            format_version: HNSW_FORMAT_VERSION,
            artifact_digest: artifact_digest.clone(),
            body,
        };
        let bytes = encode_json(&envelope)?;
        if bytes.len() > MAX_HNSW_BYTES {
            return invalid("encoded HNSW artifact exceeds the 1 GiB safety limit");
        }
        Self::from_parts(envelope, artifact_digest, bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_HNSW_BYTES {
            return invalid("encoded HNSW artifact exceeds the 1 GiB safety limit");
        }
        let envelope: HnswEnvelope =
            serde_json::from_slice(bytes).map_err(|error| rrd_core::Error::InvalidRuntime {
                reason: format!("HNSW artifact cannot be decoded: {error}"),
            })?;
        if envelope.magic != HNSW_MAGIC || envelope.format_version != HNSW_FORMAT_VERSION {
            return invalid("HNSW magic or format version is unsupported");
        }
        if encode_json(&envelope)? != bytes {
            return invalid("HNSW bytes are not in canonical encoding");
        }
        let actual_digest = digest::sha256_hex(&encode_json(&envelope.body)?);
        if actual_digest != envelope.artifact_digest {
            return invalid("HNSW artifact digest does not match its body");
        }
        Self::from_parts(envelope, actual_digest, bytes.to_vec())
    }

    fn from_parts(envelope: HnswEnvelope, artifact_digest: String, bytes: Vec<u8>) -> Result<Self> {
        let config_digest = envelope.body.config.digest()?;
        let descriptor = HnswDescriptor {
            stamp: ProjectionStamp {
                contract_version: DATA_RUNTIME_CONTRACT_VERSION,
                id: envelope.body.config.id.clone(),
                generation: envelope.body.generation,
                source_cursor: envelope.body.source_cursor,
                config_digest,
                artifact_digest,
                state: ProjectionState::Ready,
            },
            scope: envelope.body.config.scope.clone(),
            field: envelope.body.config.field.clone(),
            dimensions: envelope.body.config.dimensions,
            metric: envelope.body.config.metric,
            embedding_model: envelope.body.config.embedding_model.clone(),
            m: envelope.body.config.m,
            ef_construction: envelope.body.config.ef_construction,
            max_level: envelope.body.config.max_level,
            nodes: envelope.body.nodes.len(),
            filter_properties: envelope.body.config.filter_properties.clone(),
            maintenance: envelope.body.maintenance,
            previous_generation: envelope.body.previous_generation,
            indexed_delta_vectors: envelope.body.indexed_delta_vectors,
        };
        descriptor.validate()?;
        validate_graph(
            &envelope.body.config,
            envelope.body.source_cursor,
            envelope.body.entrypoint,
            &envelope.body.nodes,
        )?;
        let mut versions = BTreeMap::<rrd_core::RuntimeRef, Vec<usize>>::new();
        for (id, node) in envelope.body.nodes.iter().enumerate() {
            versions
                .entry(node.candidate.vector.reference.clone())
                .or_default()
                .push(id);
        }
        for ids in versions.values_mut() {
            ids.sort_by_key(|id| envelope.body.nodes[*id].candidate.source_cursor);
        }
        Ok(Self {
            descriptor,
            config: envelope.body.config,
            entrypoint: envelope.body.entrypoint,
            nodes: envelope.body.nodes,
            versions,
            bytes,
        })
    }

    pub fn descriptor(&self) -> &HnswDescriptor {
        &self.descriptor
    }

    pub fn config(&self) -> &HnswConfig {
        &self.config
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Estimates graph work from the current filter/visibility cardinality.
    /// This scans compact metadata, not vector dimensions, so the planner can
    /// choose the exact path when a highly selective filter would force HNSW
    /// to traverse most of the graph.
    pub fn estimated_search_cost(&self, request: &SearchRequest, ef_search: usize) -> Result<u64> {
        request.validate()?;
        if ef_search == 0 || ef_search > 1_000_000 {
            return invalid("HNSW ef_search must be in 1..=1000000");
        }
        if request.filter.is_none()
            && request.read.commit_cursor == self.descriptor.stamp.source_cursor
        {
            return Ok(ef_search
                .min(self.nodes.len())
                .max(1)
                .saturating_mul(HNSW_NAVIGATION_COST_MULTIPLIER) as u64);
        }
        let eligible = self
            .versions
            .values()
            .filter_map(|versions| {
                versions.iter().rev().find(|version| {
                    let candidate = &self.nodes[**version].candidate;
                    candidate.source_cursor <= request.read.commit_cursor
                        && candidate.vector.valid_from <= request.valid_at
                })
            })
            .filter(|id| self.is_visible(request, **id))
            .count();
        if eligible == 0 {
            return Ok(self
                .nodes
                .len()
                .max(1)
                .saturating_mul(HNSW_NAVIGATION_COST_MULTIPLIER) as u64);
        }
        let estimated = ef_search
            .saturating_mul(self.versions.len())
            .div_ceil(eligible)
            .min(self.nodes.len())
            .max(1);
        Ok(estimated.saturating_mul(HNSW_NAVIGATION_COST_MULTIPLIER) as u64)
    }

    /// Generates HNSW candidates, then delegates final scoring, filtering, and
    /// deterministic ordering to the exact oracle.
    pub fn search(&self, request: &SearchRequest, ef_search: usize) -> Result<Vec<SearchHit>> {
        self.search_at_with_kernel(
            request,
            ef_search,
            request.read.commit_cursor,
            HnswKernel::Auto,
        )
    }

    pub fn search_with_kernel(
        &self,
        request: &SearchRequest,
        ef_search: usize,
        kernel: HnswKernel,
    ) -> Result<Vec<SearchHit>> {
        self.search_at_with_kernel(request, ef_search, request.read.commit_cursor, kernel)
    }

    pub fn search_at(
        &self,
        request: &SearchRequest,
        ef_search: usize,
        required_source_cursor: u64,
    ) -> Result<Vec<SearchHit>> {
        self.search_at_with_kernel(request, ef_search, required_source_cursor, HnswKernel::Auto)
    }

    fn search_at_with_kernel(
        &self,
        request: &SearchRequest,
        ef_search: usize,
        required_source_cursor: u64,
        kernel: HnswKernel,
    ) -> Result<Vec<SearchHit>> {
        request.validate()?;
        if required_source_cursor > request.read.commit_cursor {
            return invalid("HNSW source cursor exceeds the request read stamp");
        }
        if self.descriptor.stamp.source_cursor < required_source_cursor {
            return invalid("HNSW artifact does not satisfy request identity or freshness");
        }
        let candidates = self.search_candidates_with_kernel(request, ef_search, kernel)?;
        search_exact(request, candidates)
    }

    pub(crate) fn search_candidates_with_kernel(
        &self,
        request: &SearchRequest,
        ef_search: usize,
        kernel: HnswKernel,
    ) -> Result<Vec<VectorCandidate>> {
        request.validate()?;
        let exact_rerank = match request.mode {
            SearchMode::Exact => return invalid("HNSW cannot serve an exact-only request"),
            SearchMode::AllowApproximate { exact_rerank }
            | SearchMode::RequireApproximate { exact_rerank } => exact_rerank,
        };
        if ef_search < exact_rerank || ef_search > 1_000_000 {
            return invalid("HNSW ef_search must be in exact_rerank..=1000000");
        }
        if self.descriptor.stamp.state != ProjectionState::Ready
            || self.descriptor.scope != request.scope
            || self.descriptor.field != request.field
            || self.descriptor.metric != request.metric
            || self.descriptor.embedding_model != request.embedding_model
            || self.descriptor.dimensions != request.query.dimensions()
        {
            return invalid("HNSW artifact does not satisfy request identity");
        }
        let required = request
            .filter
            .as_ref()
            .map(|filter| {
                filter
                    .referenced_properties()
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if !required.is_subset(&self.descriptor.filter_properties) {
            return invalid("HNSW artifact does not cover every filter property");
        }
        let Some(entrypoint) = self.entrypoint else {
            return Ok(Vec::new());
        };
        let mut current = entrypoint;
        let query = dense_query(&request.query)?;
        for layer in (1..=self.nodes[entrypoint].level).rev() {
            current = greedy_layer(
                &self.nodes,
                query,
                current,
                layer as usize,
                self.config.metric,
                kernel,
            )?;
        }
        let mut candidates = search_layer(
            &self.nodes,
            query,
            &[current],
            SearchLayerOptions {
                ef: ef_search,
                layer: 0,
                metric: self.config.metric,
                kernel,
                eligible: Some(&|id| self.is_visible(request, id)),
            },
        )?;
        candidates.truncate(exact_rerank);
        Ok(candidates
            .into_iter()
            .map(|scored| self.nodes[scored.id].candidate.clone())
            .collect())
    }

    fn is_visible(&self, request: &SearchRequest, id: usize) -> bool {
        let candidate = &self.nodes[id].candidate;
        let latest = self
            .versions
            .get(&candidate.vector.reference)
            .and_then(|versions| {
                versions.iter().rev().find(|version| {
                    let version = &self.nodes[**version].candidate;
                    version.source_cursor <= request.read.commit_cursor
                        && version.vector.valid_from <= request.valid_at
                })
            });
        if latest.copied() != Some(id)
            || candidate
                .vector
                .valid_to
                .is_some_and(|valid_to| request.valid_at >= valid_to)
        {
            return false;
        }
        request
            .filter
            .as_ref()
            .is_none_or(|filter| filter.matches(candidate.filter_properties()))
    }
}

#[derive(Debug, Clone, Copy)]
struct ScoredNode {
    id: usize,
    score: f64,
}

impl PartialEq for ScoredNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.score.total_cmp(&other.score) == Ordering::Equal
    }
}

impl Eq for ScoredNode {}

impl PartialOrd for ScoredNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

fn insert_node(
    config: &HnswConfig,
    nodes: &mut Vec<HnswNode>,
    entrypoint: &mut Option<usize>,
    candidate: VectorCandidate,
) -> Result<()> {
    let level = deterministic_level(config, &candidate);
    let new_id = nodes.len();
    let Some(mut current) = *entrypoint else {
        nodes.push(HnswNode {
            candidate,
            level,
            neighbors: vec![Vec::new(); level as usize + 1],
        });
        *entrypoint = Some(0);
        return Ok(());
    };
    let query = dense_candidate(&candidate)?;
    let entry_level = nodes[current].level;
    for layer in ((level + 1)..=entry_level).rev() {
        current = greedy_layer(
            nodes,
            query,
            current,
            layer as usize,
            config.metric,
            HnswKernel::Scalar,
        )?;
    }
    let mut selected_by_layer = Vec::new();
    for layer in (0..=level.min(entry_level)).rev() {
        let found = search_layer(
            nodes,
            query,
            &[current],
            SearchLayerOptions {
                ef: config.ef_construction,
                layer: layer as usize,
                metric: config.metric,
                kernel: HnswKernel::Scalar,
                eligible: None,
            },
        )?;
        let selected = select_neighbors(found, layer_limit(config.m, layer as usize));
        if let Some(best) = selected.first() {
            current = best.id;
        }
        selected_by_layer.push((layer as usize, selected));
    }
    let mut neighbors = vec![Vec::new(); level as usize + 1];
    for (layer, selected) in &selected_by_layer {
        neighbors[*layer] = selected.iter().map(|scored| scored.id).collect();
    }
    nodes.push(HnswNode {
        candidate,
        level,
        neighbors,
    });
    for (layer, selected) in selected_by_layer {
        for scored in selected {
            if !nodes[scored.id].neighbors[layer].contains(&new_id) {
                nodes[scored.id].neighbors[layer].push(new_id);
            }
            prune_neighbors(
                nodes,
                scored.id,
                layer,
                layer_limit(config.m, layer),
                config.metric,
            )?;
        }
    }
    if level > entry_level {
        *entrypoint = Some(new_id);
    }
    Ok(())
}

fn prune_neighbors(
    nodes: &mut [HnswNode],
    node: usize,
    layer: usize,
    m: usize,
    metric: ScoreMetric,
) -> Result<()> {
    let query = dense_candidate(&nodes[node].candidate)?.to_vec();
    let mut neighbors = nodes[node].neighbors[layer].clone();
    let scored = neighbors
        .drain(..)
        .map(|id| {
            Ok(ScoredNode {
                id,
                score: score_dense_query(&query, &nodes[id].candidate, metric, HnswKernel::Scalar)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    nodes[node].neighbors[layer] = select_neighbors(scored, m)
        .into_iter()
        .map(|value| value.id)
        .collect();
    Ok(())
}

fn select_neighbors(mut candidates: Vec<ScoredNode>, limit: usize) -> Vec<ScoredNode> {
    sort_scored(&mut candidates);
    candidates.truncate(limit);
    candidates
}

fn layer_limit(m: usize, layer: usize) -> usize {
    if layer == 0 {
        m.saturating_mul(2)
    } else {
        m
    }
}

fn greedy_layer(
    nodes: &[HnswNode],
    query: &[f32],
    start: usize,
    layer: usize,
    metric: ScoreMetric,
    kernel: HnswKernel,
) -> Result<usize> {
    let mut current = start;
    let mut current_score = score_dense_query(query, &nodes[current].candidate, metric, kernel)?;
    loop {
        let mut improved = false;
        if let Some(neighbors) = nodes[current].neighbors.get(layer) {
            for neighbor in neighbors {
                let score = score_dense_query(query, &nodes[*neighbor].candidate, metric, kernel)?;
                if score > current_score || (score == current_score && *neighbor < current) {
                    current = *neighbor;
                    current_score = score;
                    improved = true;
                }
            }
        }
        if !improved {
            return Ok(current);
        }
    }
}

struct SearchLayerOptions<'a> {
    ef: usize,
    layer: usize,
    metric: ScoreMetric,
    kernel: HnswKernel,
    eligible: Option<&'a dyn Fn(usize) -> bool>,
}

fn search_layer(
    nodes: &[HnswNode],
    query: &[f32],
    entries: &[usize],
    options: SearchLayerOptions<'_>,
) -> Result<Vec<ScoredNode>> {
    let mut visited = BTreeSet::new();
    let mut frontier = BinaryHeap::new();
    let mut best = BinaryHeap::<Reverse<ScoredNode>>::new();
    for entry in entries {
        if *entry >= nodes.len() || !visited.insert(*entry) {
            continue;
        }
        let scored = ScoredNode {
            id: *entry,
            score: score_dense_query(
                query,
                &nodes[*entry].candidate,
                options.metric,
                options.kernel,
            )?,
        };
        frontier.push(scored);
        if options.eligible.is_none_or(|eligible| eligible(*entry)) {
            best.push(Reverse(scored));
        }
    }
    while let Some(current) = frontier.pop() {
        if best.len() >= options.ef
            && current.score
                < best
                    .peek()
                    .map(|value| value.0.score)
                    .unwrap_or(f64::NEG_INFINITY)
        {
            break;
        }
        if let Some(neighbors) = nodes[current.id].neighbors.get(options.layer) {
            for neighbor in neighbors {
                if !visited.insert(*neighbor) {
                    continue;
                }
                let scored = ScoredNode {
                    id: *neighbor,
                    score: score_dense_query(
                        query,
                        &nodes[*neighbor].candidate,
                        options.metric,
                        options.kernel,
                    )?,
                };
                if best.len() < options.ef
                    || scored.score
                        > best
                            .peek()
                            .map(|value| value.0.score)
                            .unwrap_or(f64::NEG_INFINITY)
                {
                    frontier.push(scored);
                    if options.eligible.is_none_or(|eligible| eligible(*neighbor)) {
                        best.push(Reverse(scored));
                        if best.len() > options.ef {
                            best.pop();
                        }
                    }
                }
            }
        }
    }
    let mut best = best.into_iter().map(|value| value.0).collect::<Vec<_>>();
    sort_scored(&mut best);
    Ok(best)
}

fn sort_scored(values: &mut [ScoredNode]) {
    values.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn score_dense_query(
    query: &[f32],
    candidate: &VectorCandidate,
    metric: ScoreMetric,
    kernel: HnswKernel,
) -> Result<f64> {
    let values = dense_candidate(candidate)?;
    score_dense_values(query, values, metric, kernel)
}

fn score_dense_values(
    query: &[f32],
    candidate: &[f32],
    metric: ScoreMetric,
    kernel: HnswKernel,
) -> Result<f64> {
    if query.len() != candidate.len() {
        return invalid("dense query and candidate dimensions differ");
    }
    #[cfg(target_arch = "x86_64")]
    if kernel == HnswKernel::Auto && std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 was detected at runtime and both slices have the same
        // proven length; the kernel performs only unaligned in-bounds loads.
        return Ok(unsafe { score_dense_avx2(query, candidate, metric) });
    }
    score_dense(query, candidate, metric)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn score_dense_avx2(left: &[f32], right: &[f32], metric: ScoreMetric) -> f64 {
    use std::arch::x86_64::*;
    let mut dot = _mm256_setzero_ps();
    let mut left_norm = _mm256_setzero_ps();
    let mut right_norm = _mm256_setzero_ps();
    let mut distance = _mm256_setzero_ps();
    let mut manhattan = _mm256_setzero_ps();
    let sign_mask = _mm256_set1_ps(-0.0);
    let mut index = 0;
    while index + 8 <= left.len() {
        let lhs = _mm256_loadu_ps(left.as_ptr().add(index));
        let rhs = _mm256_loadu_ps(right.as_ptr().add(index));
        let delta = _mm256_sub_ps(lhs, rhs);
        dot = _mm256_add_ps(dot, _mm256_mul_ps(lhs, rhs));
        left_norm = _mm256_add_ps(left_norm, _mm256_mul_ps(lhs, lhs));
        right_norm = _mm256_add_ps(right_norm, _mm256_mul_ps(rhs, rhs));
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
    for (lhs, rhs) in left[index..].iter().zip(&right[index..]) {
        let lhs = f64::from(*lhs);
        let rhs = f64::from(*rhs);
        dot += lhs * rhs;
        left_norm += lhs * lhs;
        right_norm += rhs * rhs;
        distance += (lhs - rhs).powi(2);
        manhattan += (lhs - rhs).abs();
    }
    match metric {
        ScoreMetric::Dot => dot,
        ScoreMetric::Cosine if right_norm == 0.0 => 0.0,
        ScoreMetric::Cosine => dot / (left_norm.sqrt() * right_norm.sqrt()),
        ScoreMetric::Euclidean => -distance.sqrt(),
        ScoreMetric::Manhattan => -manhattan,
    }
}

fn dense_query(query: &VectorQuery) -> Result<&[f32]> {
    match query {
        VectorQuery::Dense { values } => Ok(values),
        _ => invalid("HNSW currently supports dense queries only"),
    }
}

fn dense_candidate(candidate: &VectorCandidate) -> Result<&[f32]> {
    match &candidate.vector.value {
        VectorValue::Dense { values } => Ok(values),
        _ => invalid("HNSW currently supports dense candidates only"),
    }
}

fn deterministic_level(config: &HnswConfig, candidate: &VectorCandidate) -> u8 {
    let mut bytes = b"rrflow-hnsw-level-v1\0".to_vec();
    bytes.extend_from_slice(&config.seed.to_be_bytes());
    bytes.extend_from_slice(candidate.vector.reference.kind.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(candidate.vector.reference.id.as_str().as_bytes());
    bytes.extend_from_slice(&candidate.source_cursor.to_be_bytes());
    let hash = digest::sha256(&bytes);
    let mut random = u64::from_be_bytes(hash[..8].try_into().expect("eight-byte hash prefix"));
    let mut level = 0;
    while level < config.max_level && random % config.m as u64 == 0 {
        level += 1;
        random /= config.m as u64;
    }
    level
}

fn validate_graph(
    config: &HnswConfig,
    source_cursor: u64,
    entrypoint: Option<usize>,
    nodes: &[HnswNode],
) -> Result<()> {
    config.validate()?;
    validate_candidate_versions(nodes.iter().map(|node| &node.candidate))?;
    if entrypoint.is_some_and(|entrypoint| entrypoint >= nodes.len())
        || (nodes.is_empty() != entrypoint.is_none())
    {
        return invalid("HNSW entrypoint is inconsistent with its nodes");
    }
    if let Some(entrypoint) = entrypoint {
        let maximum_level = nodes.iter().map(|node| node.level).max().unwrap_or(0);
        if nodes[entrypoint].level != maximum_level {
            return invalid("HNSW entrypoint does not own the maximum level");
        }
    }
    for (id, node) in nodes.iter().enumerate() {
        if node.candidate.scope != config.scope
            || node.candidate.source_cursor > source_cursor
            || node.candidate.vector.field != config.field
            || node.candidate.vector.value.dimensions() != config.dimensions
            || !node
                .candidate
                .matches_model(config.embedding_model.as_ref())
            || !matches!(node.candidate.vector.value, VectorValue::Dense { .. })
            || node.level > config.max_level
            || node.neighbors.len() != node.level as usize + 1
        {
            return invalid("HNSW node violates configuration or coverage");
        }
        for (layer, neighbors) in node.neighbors.iter().enumerate() {
            if neighbors.len() > layer_limit(config.m, layer) {
                return invalid("HNSW node exceeds configured degree");
            }
            let mut unique = BTreeSet::new();
            for neighbor in neighbors {
                if *neighbor >= nodes.len()
                    || *neighbor == id
                    || !unique.insert(*neighbor)
                    || nodes[*neighbor].level < layer as u8
                {
                    return invalid("HNSW neighbor reference is invalid");
                }
            }
        }
    }
    if let Some(entrypoint) = entrypoint {
        let mut reachable = vec![false; nodes.len()];
        let mut pending = vec![entrypoint];
        reachable[entrypoint] = true;
        while let Some(node) = pending.pop() {
            for neighbor in &nodes[node].neighbors[0] {
                if !reachable[*neighbor] {
                    reachable[*neighbor] = true;
                    pending.push(*neighbor);
                }
            }
        }
        if reachable.iter().any(|reachable| !reachable) {
            return invalid("HNSW layer zero is not reachable from the entrypoint");
        }
    }
    Ok(())
}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| rrd_core::Error::InvalidRuntime {
        reason: format!("HNSW artifact cannot be encoded: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterCondition, FilterExpression, FilterOperator};
    use rrd_core::{ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector};

    fn candidate(scope: &ScopeId, cursor: u64, id: usize, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", format!("v-{id:03}")).unwrap(),
                subject: RuntimeRef::new("document", format!("d-{id:03}")).unwrap(),
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

    fn config(scope: &ScopeId) -> HnswConfig {
        HnswConfig {
            id: ProjectionId::new("vector:hnsw:body").unwrap(),
            scope: scope.clone(),
            field: "body".into(),
            dimensions: 2,
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            m: 8,
            ef_construction: 32,
            max_level: 8,
            seed: 7,
            filter_properties: BTreeSet::new(),
        }
    }

    #[test]
    fn hnsw_round_trip_is_deterministic_and_exact_reranks_candidates() {
        let scope = ScopeId::new("instance:hnsw").unwrap();
        let values = (0..64)
            .map(|index| {
                let angle = index as f32 * std::f32::consts::TAU / 64.0;
                candidate(
                    &scope,
                    index + 1,
                    index as usize,
                    vec![angle.cos(), angle.sin()],
                )
            })
            .collect::<Vec<_>>();
        let index = HnswIndex::build(config(&scope), 1, 64, values).unwrap();
        let decoded = HnswIndex::from_bytes(index.as_bytes()).unwrap();
        assert_eq!(index.descriptor(), decoded.descriptor());
        assert_eq!(index.as_bytes(), decoded.as_bytes());
        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 64, Some("11".repeat(32))).unwrap(),
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 5,
            mode: SearchMode::RequireApproximate { exact_rerank: 20 },
            filter: None,
        };
        let hits = decoded.search(&request, 32).unwrap();
        assert_eq!(hits.len(), 5);
        assert_eq!(hits[0].reference.id.as_str(), "v-000");
        assert_eq!(hits[0].score, 1.0);
    }

    #[test]
    fn stale_or_corrupt_hnsw_fails_closed() {
        let scope = ScopeId::new("instance:hnsw-corrupt").unwrap();
        let index = HnswIndex::build(
            config(&scope),
            1,
            1,
            [candidate(&scope, 1, 0, vec![1.0, 0.0])],
        )
        .unwrap();
        let mut bytes = index.as_bytes().to_vec();
        let position = bytes.len() / 2;
        bytes[position] ^= 1;
        assert!(HnswIndex::from_bytes(&bytes).is_err());
        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 2, Some("11".repeat(32))).unwrap(),
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 1,
            mode: SearchMode::RequireApproximate { exact_rerank: 1 },
            filter: None,
        };
        assert!(index.search(&request, 1).is_err());
    }

    #[test]
    fn selective_filter_admits_candidates_during_traversal_and_signals_crossover() {
        let scope = ScopeId::new("instance:hnsw-filter").unwrap();
        let mut values = (0..100)
            .map(|index| {
                let angle = index as f32 * std::f32::consts::TAU / 100.0;
                let mut value = candidate(
                    &scope,
                    index + 1,
                    index as usize,
                    vec![angle.cos(), angle.sin()],
                );
                value
                    .vector
                    .properties
                    .insert("selected".into(), RuntimeValue::Bool(index == 37));
                value
            })
            .collect::<Vec<_>>();
        let mut build = config(&scope);
        build.filter_properties.insert("selected".into());
        let index = HnswIndex::build(build, 1, 100, values.drain(..)).unwrap();
        let request = SearchRequest {
            scope: scope.clone(),
            read: ReadStamp::new(scope, None, 0, 100, Some("11".repeat(32))).unwrap(),
            valid_at: 2,
            field: "body".into(),
            query: VectorQuery::Dense {
                values: vec![1.0, 0.0],
            },
            metric: ScoreMetric::Cosine,
            embedding_model: None,
            top_k: 1,
            mode: SearchMode::RequireApproximate { exact_rerank: 10 },
            filter: Some(FilterExpression::Condition {
                condition: FilterCondition {
                    property: "selected".into(),
                    operator: FilterOperator::Equals {
                        value: RuntimeValue::Bool(true),
                    },
                },
            }),
        };
        assert_eq!(index.estimated_search_cost(&request, 10).unwrap(), 400);
        let hits = index.search(&request, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reference.id.as_str(), "v-037");
    }
}
