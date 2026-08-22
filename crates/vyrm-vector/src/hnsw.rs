use crate::contract::invalid;
use crate::exact::{score_dense_candidate, validate_candidate_versions};
use crate::{
    search_exact, AccessPathKind, CandidatePath, EmbeddingModelBinding, ScoreMetric, SearchHit,
    SearchMode, SearchRequest, VectorCandidate, VectorQuery,
};
use serde::{Deserialize, Serialize};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use vyrm_core::{
    digest, ProjectionId, ProjectionStamp, ProjectionState, Result, ScopeId, VectorValue,
    DATA_RUNTIME_CONTRACT_VERSION,
};

pub const HNSW_FORMAT_VERSION: u16 = 1;
const HNSW_MAGIC: &str = "VYRHNS01";
const MAX_HNSW_BYTES: usize = 1 << 30;
const MAX_HNSW_NODES: usize = 10_000_000;
// A graph visit performs vector scoring plus heap/navigation work. Four
// score-equivalent units is deliberately conservative at filter crossovers;
// callers may still force ANN through `RequireApproximate`.
const HNSW_NAVIGATION_COST_MULTIPLIER: usize = 4;

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
    versions: BTreeMap<vyrm_core::RuntimeRef, Vec<usize>>,
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
            nodes,
        };
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
            serde_json::from_slice(bytes).map_err(|error| vyrm_core::Error::InvalidRuntime {
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
        };
        descriptor.validate()?;
        validate_graph(
            &envelope.body.config,
            envelope.body.source_cursor,
            envelope.body.entrypoint,
            &envelope.body.nodes,
        )?;
        let mut versions = BTreeMap::<vyrm_core::RuntimeRef, Vec<usize>>::new();
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
        self.search_at(request, ef_search, request.read.commit_cursor)
    }

    pub fn search_at(
        &self,
        request: &SearchRequest,
        ef_search: usize,
        required_source_cursor: u64,
    ) -> Result<Vec<SearchHit>> {
        request.validate()?;
        if required_source_cursor > request.read.commit_cursor {
            return invalid("HNSW source cursor exceeds the request read stamp");
        }
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
            || self.descriptor.stamp.source_cursor < required_source_cursor
        {
            return invalid("HNSW artifact does not satisfy request identity or freshness");
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
            )?;
        }
        let mut candidates = search_layer(
            &self.nodes,
            query,
            &[current],
            ef_search,
            0,
            self.config.metric,
            Some(&|id| self.is_visible(request, id)),
        )?;
        candidates.truncate(exact_rerank);
        search_exact(
            request,
            candidates
                .into_iter()
                .map(|scored| self.nodes[scored.id].candidate.clone()),
        )
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
        current = greedy_layer(nodes, query, current, layer as usize, config.metric)?;
    }
    let mut selected_by_layer = Vec::new();
    for layer in (0..=level.min(entry_level)).rev() {
        let found = search_layer(
            nodes,
            query,
            &[current],
            config.ef_construction,
            layer as usize,
            config.metric,
            None,
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
                score: score_dense_query(&query, &nodes[id].candidate, metric)?,
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
) -> Result<usize> {
    let mut current = start;
    let mut current_score = score_dense_query(query, &nodes[current].candidate, metric)?;
    loop {
        let mut improved = false;
        if let Some(neighbors) = nodes[current].neighbors.get(layer) {
            for neighbor in neighbors {
                let score = score_dense_query(query, &nodes[*neighbor].candidate, metric)?;
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

fn search_layer(
    nodes: &[HnswNode],
    query: &[f32],
    entries: &[usize],
    ef: usize,
    layer: usize,
    metric: ScoreMetric,
    eligible: Option<&dyn Fn(usize) -> bool>,
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
            score: score_dense_query(query, &nodes[*entry].candidate, metric)?,
        };
        frontier.push(scored);
        if eligible.is_none_or(|eligible| eligible(*entry)) {
            best.push(Reverse(scored));
        }
    }
    while let Some(current) = frontier.pop() {
        if best.len() >= ef
            && current.score
                < best
                    .peek()
                    .map(|value| value.0.score)
                    .unwrap_or(f64::NEG_INFINITY)
        {
            break;
        }
        if let Some(neighbors) = nodes[current.id].neighbors.get(layer) {
            for neighbor in neighbors {
                if !visited.insert(*neighbor) {
                    continue;
                }
                let scored = ScoredNode {
                    id: *neighbor,
                    score: score_dense_query(query, &nodes[*neighbor].candidate, metric)?,
                };
                if best.len() < ef
                    || scored.score
                        > best
                            .peek()
                            .map(|value| value.0.score)
                            .unwrap_or(f64::NEG_INFINITY)
                {
                    frontier.push(scored);
                    if eligible.is_none_or(|eligible| eligible(*neighbor)) {
                        best.push(Reverse(scored));
                        if best.len() > ef {
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
) -> Result<f64> {
    score_dense_candidate(query, &candidate.vector.value, metric)
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
    let mut bytes = b"vyrm-hnsw-level-v1\0".to_vec();
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
    if nodes.windows(2).any(|pair| {
        let left = &pair[0].candidate;
        let right = &pair[1].candidate;
        (left.vector.reference.clone(), left.source_cursor)
            >= (right.vector.reference.clone(), right.source_cursor)
    }) {
        return invalid("HNSW nodes are not in canonical identity/version order");
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
    serde_json::to_vec(value).map_err(|error| vyrm_core::Error::InvalidRuntime {
        reason: format!("HNSW artifact cannot be encoded: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterCondition, FilterExpression, FilterOperator};
    use vyrm_core::{ReadStamp, RuntimeProperties, RuntimeRef, RuntimeValue, RuntimeVector};

    fn candidate(scope: &ScopeId, cursor: u64, id: usize, values: Vec<f32>) -> VectorCandidate {
        VectorCandidate {
            scope: scope.clone(),
            source_cursor: cursor,
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", format!("v-{id:03}")).unwrap(),
                subject: RuntimeRef::new("document", format!("d-{id:03}")).unwrap(),
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
