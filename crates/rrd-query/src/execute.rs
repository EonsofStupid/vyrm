use crate::{
    execute_snapshot, index::index_artifact_name, rows_to_record_batch, Bm25Artifact, Bm25Config,
    BoundFilter, Error, IndexArtifact, IndexKind, LogicalOperator, PhysicalOperator, PhysicalPlan,
    Result,
};
use crate::{Projection, Source, TraversalDirection};
use rrd_core::{
    resolve_as_of, Claim, GeoValue, RuntimeChange, RuntimeGeo, RuntimeGraphSnapshot,
    RuntimeMutation, RuntimeReadValidation, RuntimeValue, SeriesValue,
};
use rrd_store::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBudget {
    pub max_scanned_changes: usize,
    pub max_rows: usize,
    pub max_output_bytes: usize,
    pub max_batch_rows: usize,
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_scanned_changes: 100_000,
            max_rows: 10_000,
            max_output_bytes: 8 * 1024 * 1024,
            max_batch_rows: 256,
        }
    }
}

impl ExecutionBudget {
    fn validate(&self) -> Result<()> {
        if self.max_scanned_changes == 0
            || self.max_rows == 0
            || self.max_output_bytes == 0
            || self.max_batch_rows == 0
        {
            return Err(Error::Budget(
                "all execution budgets must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryRow {
    pub identity: String,
    pub values: BTreeMap<String, RuntimeValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryBatch {
    pub ordinal: usize,
    pub rows: Vec<QueryRow>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryExecution {
    pub plan_digest: String,
    pub read_manifest: String,
    pub valid_at: u64,
    pub known_at_cursor: u64,
    /// Cursor positions requested by the selected result access path. This
    /// excludes the hash-chain replay currently used to validate `ReadStamp`.
    pub scanned_changes: usize,
    pub stamp_validation: String,
    pub stamp_validation_max_changes: usize,
    pub stamp_validation_proof_nodes: u16,
    pub returned_rows: usize,
    pub output_bytes: usize,
    pub truncated: bool,
    pub batches: Vec<QueryBatch>,
}

pub fn execute<E: Engine>(
    engine: &E,
    plan: &PhysicalPlan,
    budget: &ExecutionBudget,
) -> Result<QueryExecution> {
    plan.verify()?;
    budget.validate()?;
    let contract = &plan.explanation.contract;
    if contract.read_manifest != plan.logical.read.manifest_id {
        return Err(Error::Integrity(
            "plan explanation names a different read stamp".into(),
        ));
    }
    let shape = PlanShape::read(plan)?;
    let access = ReadPath::from_plan(plan, &shape)?;
    let requested = access.scanned_positions()?;
    if requested > budget.max_scanned_changes {
        return Err(Error::Budget(format!(
            "query requires scanning {requested} changes, budget allows {}",
            budget.max_scanned_changes
        )));
    }
    let loaded = access.load(engine, &plan.logical.read, &shape)?;
    let stamp_validation_max_changes =
        usize::try_from(loaded.validation.change_reads).map_err(|_| {
            Error::Budget("stamp-validation evidence exceeds this platform's address space".into())
        })?;
    let stamp_validation = loaded.validation.method.clone();
    let stamp_validation_proof_nodes = loaded.validation.proof_nodes;
    // BM25 corpus statistics are calculated from the complete authoritative
    // source snapshot, before relational predicates narrow the result set.
    let bm25_scores = bm25_scores(&loaded.rows, &shape, loaded.bm25)?;
    let has_match = shape
        .filters
        .iter()
        .any(|filter| filter.comparison.is_match());
    let batch = rows_to_record_batch(&loaded.rows, &plan.logical.field_types)?;
    let mut rows = execute_snapshot(
        batch,
        &shape.filters,
        &shape.projection,
        shape.limit,
        has_match,
    )?;
    if has_match {
        rows.retain(|row| bm25_scores.contains_key(&row.identity));
        rows.sort_by(|left, right| {
            bm25_scores[&right.identity]
                .total_cmp(&bm25_scores[&left.identity])
                .then_with(|| left.identity.cmp(&right.identity))
        });
        for row in &mut rows {
            row.values.insert(
                "_score".into(),
                RuntimeValue::Decimal(format!("{:.17}", bm25_scores[&row.identity])),
            );
        }
        if let Some(query_limit) = shape.limit {
            rows.truncate(query_limit);
        }
    }
    let semantic_rows = rows.len();
    rows.truncate(budget.max_rows);
    let row_truncated = rows.len() < semantic_rows;
    let mut accepted = Vec::with_capacity(rows.len());
    let mut output_bytes = 0usize;
    let mut byte_truncated = false;
    for mut row in rows {
        apply_projection(&mut row, &shape.projection);
        let bytes = serde_json::to_vec(&row)?.len();
        if output_bytes.saturating_add(bytes) > budget.max_output_bytes {
            byte_truncated = true;
            break;
        }
        output_bytes += bytes;
        accepted.push(row);
    }
    let returned_rows = accepted.len();
    let truncated = byte_truncated || row_truncated;
    let chunk_count = returned_rows.div_ceil(budget.max_batch_rows);
    let batches = accepted
        .chunks(budget.max_batch_rows)
        .enumerate()
        .map(|(ordinal, rows)| QueryBatch {
            ordinal,
            rows: rows.to_vec(),
            done: ordinal + 1 == chunk_count,
        })
        .collect();
    Ok(QueryExecution {
        plan_digest: plan.digest.clone(),
        read_manifest: plan.logical.read.manifest_id.clone(),
        valid_at: contract.valid_at,
        known_at_cursor: contract.known_at_cursor,
        scanned_changes: requested,
        stamp_validation,
        stamp_validation_max_changes,
        stamp_validation_proof_nodes,
        returned_rows,
        output_bytes,
        truncated,
        batches,
    })
}

enum ReadPath {
    LogScan {
        through_cursor: u64,
    },
    EventCursorLookup {
        cursor: u64,
        through_cursor: u64,
    },
    Index {
        id: rrd_core::ProjectionId,
        kind: IndexKind,
        generation: u64,
        source_cursor: u64,
        schema_revision: u64,
        valid_at: u64,
        config_digest: String,
        artifact_digest: String,
        artifact_rows: u64,
    },
}

struct LoadedAccess {
    rows: Vec<QueryRow>,
    bm25: Option<Bm25Artifact>,
    validation: RuntimeReadValidation,
}

impl ReadPath {
    fn from_plan(plan: &PhysicalPlan, shape: &PlanShape) -> Result<Self> {
        let contract = &plan.explanation.contract;
        if contract.read_manifest != plan.logical.read.manifest_id
            || contract.scope != plan.logical.read.scope.as_str()
            || contract.valid_at != shape.valid_at
            || contract.known_at_cursor != shape.known_at_cursor
            || contract.source_cursor != plan.logical.source_cursor
            || contract.schema_revision != plan.logical.schema_revision
            || contract.authorization_boundary != format!("scope:{}", plan.logical.read.scope)
            || contract.stamp_validation
                != if plan.logical.read.accumulator_root.is_some() {
                    "rfc9162_inclusion_or_hash_chain_fallback"
                } else {
                    "full_hash_chain_replay"
                }
            || contract.stamp_validation_max_changes != plan.logical.read.commit_cursor
        {
            return Err(Error::Integrity(
                "logical plan, read stamp, and execution contract disagree".into(),
            ));
        }
        let selected = plan
            .explanation
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect::<Vec<_>>();
        if selected.len() != 1 || !selected[0].exact {
            return Err(Error::Integrity(
                "physical plan must report exactly one exact selected path".into(),
            ));
        }
        let [access, PhysicalOperator::DataFusionEvaluate] = plan.operators.as_slice() else {
            return Err(Error::Integrity(
                "physical plan must contain one authoritative access path followed by DataFusion evaluation"
                    .into(),
            ));
        };
        let known_at_cursor = plan.explanation.contract.known_at_cursor;
        match access {
            PhysicalOperator::AuthoritativeLogScan {
                through_cursor,
                exact,
                stable_order,
            } if *through_cursor == known_at_cursor
                && *exact
                && stable_order == "global_cursor"
                && selected[0].name == "authoritative_log_scan" =>
            {
                Ok(Self::LogScan {
                    through_cursor: *through_cursor,
                })
            }
            PhysicalOperator::AuthoritativeEventCursorLookup {
                cursor,
                through_cursor,
                exact,
                stable_order,
            } if *through_cursor == known_at_cursor
                && *exact
                && stable_order == "global_cursor"
                && selected[0].name == "authoritative_event_cursor_lookup"
                && matches!(&shape.source, Source::Event { .. })
                && shape.filters.iter().any(|filter| {
                    filter.field == "cursor" && filter.value == RuntimeValue::Unsigned(*cursor)
                }) =>
            {
                Ok(Self::EventCursorLookup {
                    cursor: *cursor,
                    through_cursor: *through_cursor,
                })
            }
            PhysicalOperator::MaterializedIndex {
                id,
                kind,
                generation,
                source_cursor,
                schema_revision,
                valid_at,
                config_digest,
                artifact_digest,
                artifact_rows,
                exact,
                stable_order,
            } if *source_cursor == plan.logical.source_cursor
                && *schema_revision == plan.logical.schema_revision
                && *valid_at == shape.valid_at
                && *exact
                && stable_order
                    == if matches!(kind, IndexKind::Bm25 { .. }) {
                        "bm25_score_desc_identity_ascending"
                    } else {
                        "identity_ascending"
                    }
                && selected[0].name == format!("index:{id}") =>
            {
                Ok(Self::Index {
                    id: id.clone(),
                    kind: kind.clone(),
                    generation: *generation,
                    source_cursor: *source_cursor,
                    schema_revision: *schema_revision,
                    valid_at: *valid_at,
                    config_digest: config_digest.clone(),
                    artifact_digest: artifact_digest.clone(),
                    artifact_rows: *artifact_rows,
                })
            }
            _ => Err(Error::Integrity(
                "physical access path does not match the stamped logical query".into(),
            )),
        }
    }

    fn scanned_positions(&self) -> Result<usize> {
        match self {
            Self::LogScan { through_cursor } => usize::try_from(*through_cursor).map_err(|_| {
                Error::Budget("known cursor exceeds this platform's address space".into())
            }),
            Self::EventCursorLookup {
                cursor,
                through_cursor,
            } => Ok(usize::from(*cursor > 0 && *cursor <= *through_cursor)),
            Self::Index { artifact_rows, .. } => usize::try_from(*artifact_rows)
                .map_err(|_| Error::Budget("index artifact rows exceed usize".into())),
        }
    }

    fn load<E: Engine>(
        &self,
        engine: &E,
        stamp: &rrd_core::ReadStamp,
        shape: &PlanShape,
    ) -> Result<LoadedAccess> {
        if let Self::Index {
            id,
            kind,
            generation,
            source_cursor,
            schema_revision,
            valid_at,
            config_digest,
            artifact_digest,
            artifact_rows,
        } = self
        {
            let validation = engine.runtime_read_changes(stamp, stamp.commit_cursor, 1)?;
            if validation.through_cursor != stamp.commit_cursor
                || validation.head_cursor != stamp.commit_cursor
                || !validation.changes.is_empty()
            {
                return Err(Error::Integrity(
                    "index stamp validation did not preserve the captured head".into(),
                ));
            }
            let name = index_artifact_name(&stamp.scope, id, *generation, artifact_digest);
            let bytes = engine
                .get_projection(&name)?
                .ok_or_else(|| Error::Integrity(format!("index artifact {id} is missing")))?;
            if rrd_core::digest::sha256_hex(&bytes) != *artifact_digest {
                return Err(Error::Integrity(format!(
                    "index artifact {id} digest does not verify"
                )));
            }
            let artifact = IndexArtifact::decode(&bytes)?;
            if artifact.scope != stamp.scope
                || artifact.definition.id != *id
                || artifact.definition.kind != *kind
                || artifact.definition.source != shape.source
                || artifact.definition.config_digest()? != *config_digest
                || artifact.generation != *generation
                || artifact.source_cursor != *source_cursor
                || artifact.read_cursor > shape.known_at_cursor
                || artifact.schema_revision != *schema_revision
                || artifact.valid_at != *valid_at
                || artifact.rows.len()
                    != usize::try_from(*artifact_rows)
                        .map_err(|_| Error::Budget("index artifact rows exceed usize".into()))?
            {
                return Err(Error::Integrity(format!(
                    "index artifact {id} does not satisfy the physical plan"
                )));
            }
            return Ok(LoadedAccess {
                rows: artifact.rows,
                bm25: artifact.bm25,
                validation: validation.validation,
            });
        }
        let (after, limit, expected_through) = match self {
            Self::LogScan { through_cursor } if *through_cursor == 0 => {
                (stamp.commit_cursor, 1, stamp.commit_cursor)
            }
            Self::LogScan { through_cursor } => (0, self.scanned_positions()?, *through_cursor),
            Self::EventCursorLookup {
                cursor,
                through_cursor,
            } if *cursor == 0 || *cursor > *through_cursor => {
                (stamp.commit_cursor, 1, stamp.commit_cursor)
            }
            Self::EventCursorLookup { cursor, .. } => (cursor - 1, 1, *cursor),
            Self::Index { .. } => unreachable!("indexes return above"),
        };
        let page = engine.runtime_read_changes(stamp, after, limit)?;
        if page.through_cursor != expected_through || page.head_cursor != stamp.commit_cursor {
            return Err(Error::Integrity(format!(
                "stamped access ended at {}/{}, expected {}/{}",
                page.through_cursor, page.head_cursor, expected_through, stamp.commit_cursor
            )));
        }
        if page
            .changes
            .iter()
            .any(|change| change.cursor <= after || change.cursor > expected_through)
        {
            return Err(Error::Integrity(
                "stamped access returned a change outside its cursor interval".into(),
            ));
        }
        let rows = rows_for_source(
            &page.changes,
            &stamp.scope,
            &shape.source,
            shape.valid_at,
            shape.known_at_cursor,
        );
        Ok(LoadedAccess {
            rows,
            bm25: None,
            validation: page.validation,
        })
    }
}

struct PlanShape {
    source: Source,
    valid_at: u64,
    known_at_cursor: u64,
    filters: Vec<BoundFilter>,
    projection: Projection,
    limit: Option<usize>,
    schema_revision: u64,
    source_cursor: u64,
}

impl PlanShape {
    fn read(plan: &PhysicalPlan) -> Result<Self> {
        let mut source = None;
        let mut temporal = None;
        let mut filters = Vec::new();
        let mut projection = None;
        let mut limit = None;
        for operator in &plan.logical.operators {
            match operator {
                LogicalOperator::Scan { source: value } => {
                    if source.replace(value.clone()).is_some() {
                        return Err(Error::Integrity(
                            "logical plan contains more than one scan".into(),
                        ));
                    }
                }
                LogicalOperator::Filter { predicates } => filters.extend(predicates.clone()),
                LogicalOperator::Project { projection: value } => {
                    if projection.replace(value.clone()).is_some() {
                        return Err(Error::Integrity(
                            "logical plan contains more than one projection".into(),
                        ));
                    }
                }
                LogicalOperator::Limit { rows } => {
                    if limit.replace(*rows).is_some() {
                        return Err(Error::Integrity(
                            "logical plan contains more than one limit".into(),
                        ));
                    }
                }
                LogicalOperator::Temporal {
                    valid_at,
                    known_at_cursor,
                } => {
                    if temporal.replace((*valid_at, *known_at_cursor)).is_some() {
                        return Err(Error::Integrity(
                            "logical plan contains more than one temporal selector".into(),
                        ));
                    }
                }
            }
        }
        let (valid_at, known_at_cursor) = temporal
            .ok_or_else(|| Error::Integrity("logical plan has no temporal selector".into()))?;
        Ok(Self {
            source: source.ok_or_else(|| Error::Integrity("logical plan has no scan".into()))?,
            valid_at,
            known_at_cursor,
            filters,
            projection: projection
                .ok_or_else(|| Error::Integrity("logical plan has no projection".into()))?,
            limit,
            schema_revision: plan.logical.schema_revision,
            source_cursor: plan.logical.source_cursor,
        })
    }
}

fn rows_for_source(
    changes: &[RuntimeChange],
    scope: &rrd_core::ScopeId,
    source: &Source,
    valid_at: u64,
    known_at_cursor: u64,
) -> Vec<QueryRow> {
    match source {
        Source::Record { kind } => {
            let snapshot = RuntimeGraphSnapshot::from_changes(
                changes,
                scope.clone(),
                valid_at,
                known_at_cursor,
            );
            snapshot
                .records
                .into_iter()
                .filter(|record| &record.reference.kind == kind)
                .map(|record| {
                    let identity = format!("record:{}:{}", record.reference.kind, record.reference.id);
                    let mut values = record.properties;
                    values.insert("id".into(), string(record.reference.id.to_string()));
                    values.insert("kind".into(), string(record.reference.kind.to_string()));
                    values.insert("valid_from".into(), RuntimeValue::Unsigned(record.valid_from));
                    values.insert("valid_to".into(), optional_u64(record.valid_to));
                    QueryRow { identity, values }
                })
                .collect()
        }
        Source::Relation { kind } => {
            let snapshot = RuntimeGraphSnapshot::from_changes(
                changes,
                scope.clone(),
                valid_at,
                known_at_cursor,
            );
            snapshot
                .relations
                .into_iter()
                .filter(|relation| &relation.reference.kind == kind)
                .map(|relation| {
                    let identity = format!(
                        "relation:{}:{}",
                        relation.reference.kind, relation.reference.id
                    );
                    let mut values = relation.properties;
                    values.insert("id".into(), string(relation.reference.id.to_string()));
                    values.insert("kind".into(), string(relation.reference.kind.to_string()));
                    values.insert("from_kind".into(), string(relation.from.kind.to_string()));
                    values.insert("from_id".into(), string(relation.from.id.to_string()));
                    values.insert("to_kind".into(), string(relation.to.kind.to_string()));
                    values.insert("to_id".into(), string(relation.to.id.to_string()));
                    values.insert("valid_from".into(), RuntimeValue::Unsigned(relation.valid_from));
                    values.insert("valid_to".into(), optional_u64(relation.valid_to));
                    QueryRow { identity, values }
                })
                .collect()
        }
        Source::Event { kind } => changes
            .iter()
            .filter(|change| {
                change.cursor <= known_at_cursor
                    && &change.scope == scope
                    && change.at <= valid_at
                    && matches!(&change.mutation, RuntimeMutation::Event { event } if &event.kind == kind)
            })
            .filter_map(|change| {
                let RuntimeMutation::Event { event } = &change.mutation else {
                    return None;
                };
                let mut values = event.properties.clone();
                values.insert("cursor".into(), RuntimeValue::Unsigned(change.cursor));
                values.insert("kind".into(), string(event.kind.to_string()));
                values.insert("at".into(), RuntimeValue::Unsigned(change.at));
                values.insert("actor".into(), string(change.actor.clone()));
                values.insert(
                    "subject_kind".into(),
                    event
                        .subject
                        .as_ref()
                        .map_or(RuntimeValue::Null, |subject| string(subject.kind.to_string())),
                );
                values.insert(
                    "subject_id".into(),
                    event
                        .subject
                        .as_ref()
                        .map_or(RuntimeValue::Null, |subject| string(subject.id.to_string())),
                );
                Some(QueryRow {
                    identity: format!("event:{}:{}", event.kind, change.cursor),
                    values,
                })
            })
            .collect(),
        Source::Series { kind } => changes
            .iter()
            .filter(|change| change.cursor <= known_at_cursor && &change.scope == scope)
            .filter_map(|change| {
                let RuntimeMutation::SeriesSample { sample } = &change.mutation else {
                    return None;
                };
                if &sample.series.kind != kind || sample.observed_at > valid_at {
                    return None;
                }
                let mut values = sample.properties.clone();
                values.insert("id".into(), string(sample.reference.id.to_string()));
                values.insert("kind".into(), string(sample.reference.kind.to_string()));
                values.insert("series_kind".into(), string(sample.series.kind.to_string()));
                values.insert("series_id".into(), string(sample.series.id.to_string()));
                values.insert(
                    "observed_at".into(),
                    RuntimeValue::Unsigned(sample.observed_at),
                );
                values.insert("value".into(), series_value(&sample.value));
                Some(QueryRow {
                    identity: format!(
                        "series:{}:{}:{}:{}",
                        sample.series.kind,
                        sample.series.id,
                        sample.observed_at,
                        sample.reference.id
                    ),
                    values,
                })
            })
            .collect(),
        Source::Geo { kind } => geo_rows(changes, scope, kind, valid_at, known_at_cursor),
        Source::Traversal {
            relation,
            start,
            direction,
            max_depth,
        } => traversal_rows(
            changes,
            scope,
            relation,
            start,
            *direction,
            *max_depth,
            valid_at,
            known_at_cursor,
        ),
        Source::Claim { predicate } => claim_rows(changes, scope, predicate.as_ref(), valid_at),
    }
}

#[allow(clippy::too_many_arguments)]
fn traversal_rows(
    changes: &[RuntimeChange],
    scope: &rrd_core::ScopeId,
    relation_kind: &rrd_core::RuntimeType,
    start: &rrd_core::RuntimeRef,
    direction: TraversalDirection,
    max_depth: u16,
    valid_at: u64,
    known_at_cursor: u64,
) -> Vec<QueryRow> {
    let snapshot =
        RuntimeGraphSnapshot::from_changes(changes, scope.clone(), valid_at, known_at_cursor);
    if !snapshot
        .records
        .iter()
        .any(|record| &record.reference == start)
    {
        return Vec::new();
    }
    let mut relations = snapshot
        .relations
        .into_iter()
        .filter(|relation| &relation.reference.kind == relation_kind)
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| left.reference.cmp(&right.reference));
    let mut visited = BTreeSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start.clone(), 0_u16, vec![start.clone()])]);
    let mut rows = Vec::new();
    while let Some((current, depth, path)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for relation in &relations {
            let next = match direction {
                TraversalDirection::Outgoing if relation.from == current => Some(&relation.to),
                TraversalDirection::Incoming if relation.to == current => Some(&relation.from),
                TraversalDirection::Both if relation.from == current => Some(&relation.to),
                TraversalDirection::Both if relation.to == current => Some(&relation.from),
                _ => None,
            };
            let Some(next) = next else {
                continue;
            };
            if !visited.insert(next.clone()) {
                continue;
            }
            let next_depth = depth + 1;
            let mut next_path = path.clone();
            next_path.push(next.clone());
            let mut values = BTreeMap::new();
            values.insert(
                "depth".into(),
                RuntimeValue::Unsigned(u64::from(next_depth)),
            );
            values.insert("node_kind".into(), string(next.kind.to_string()));
            values.insert("node_id".into(), string(next.id.to_string()));
            values.insert(
                "relation_kind".into(),
                string(relation.reference.kind.to_string()),
            );
            values.insert(
                "relation_id".into(),
                string(relation.reference.id.to_string()),
            );
            values.insert("from_kind".into(), string(relation.from.kind.to_string()));
            values.insert("from_id".into(), string(relation.from.id.to_string()));
            values.insert("to_kind".into(), string(relation.to.kind.to_string()));
            values.insert("to_id".into(), string(relation.to.id.to_string()));
            values.insert(
                "path".into(),
                RuntimeValue::List(
                    next_path
                        .iter()
                        .map(|node| string(format!("{}:{}", node.kind, node.id)))
                        .collect(),
                ),
            );
            rows.push(QueryRow {
                identity: format!(
                    "traversal:{}:{}:{}:{}:{}:{}",
                    relation_kind, start.kind, start.id, next_depth, relation.reference.id, next.id
                ),
                values,
            });
            queue.push_back((next.clone(), next_depth, next_path));
        }
    }
    rows
}

fn geo_rows(
    changes: &[RuntimeChange],
    scope: &rrd_core::ScopeId,
    kind: &rrd_core::RuntimeType,
    valid_at: u64,
    known_at_cursor: u64,
) -> Vec<QueryRow> {
    let mut active = BTreeMap::<rrd_core::RuntimeRef, (u64, RuntimeGeo)>::new();
    for change in changes
        .iter()
        .filter(|change| change.cursor <= known_at_cursor && &change.scope == scope)
    {
        let RuntimeMutation::Geo { geo } = &change.mutation else {
            continue;
        };
        if &geo.reference.kind != kind
            || geo.valid_from > valid_at
            || geo.valid_to.is_some_and(|end| valid_at >= end)
        {
            continue;
        }
        let replace = active.get(&geo.reference).is_none_or(|(cursor, current)| {
            geo.valid_from > current.valid_from
                || (geo.valid_from == current.valid_from && change.cursor > *cursor)
        });
        if replace {
            active.insert(geo.reference.clone(), (change.cursor, geo.clone()));
        }
    }
    active
        .into_values()
        .map(|(_, geo)| {
            let mut values = geo.properties;
            values.insert("id".into(), string(geo.reference.id.to_string()));
            values.insert("kind".into(), string(geo.reference.kind.to_string()));
            values.insert("subject_kind".into(), string(geo.subject.kind.to_string()));
            values.insert("subject_id".into(), string(geo.subject.id.to_string()));
            values.insert("field".into(), string(geo.field));
            values.insert("valid_from".into(), RuntimeValue::Unsigned(geo.valid_from));
            values.insert("valid_to".into(), optional_u64(geo.valid_to));
            insert_geo_value(&mut values, &geo.value);
            QueryRow {
                identity: format!("geo:{}:{}", geo.reference.kind, geo.reference.id),
                values,
            }
        })
        .collect()
}

fn series_value(value: &SeriesValue) -> RuntimeValue {
    match value {
        SeriesValue::Integer(value) => RuntimeValue::Integer(*value),
        SeriesValue::Unsigned(value) => RuntimeValue::Unsigned(*value),
        SeriesValue::Decimal(value) => RuntimeValue::Decimal(value.clone()),
        SeriesValue::Bool(value) => RuntimeValue::Bool(*value),
        SeriesValue::String(value) => RuntimeValue::String(value.clone()),
    }
}

fn insert_geo_value(values: &mut BTreeMap<String, RuntimeValue>, geo: &GeoValue) {
    for field in [
        "longitude",
        "latitude",
        "southwest_longitude",
        "southwest_latitude",
        "northeast_longitude",
        "northeast_latitude",
    ] {
        values.insert(field.into(), RuntimeValue::Null);
    }
    match geo {
        GeoValue::Point { point } => {
            values.insert("geometry_kind".into(), string("point".into()));
            values.insert(
                "longitude".into(),
                RuntimeValue::Decimal(point.longitude.to_string()),
            );
            values.insert(
                "latitude".into(),
                RuntimeValue::Decimal(point.latitude.to_string()),
            );
        }
        GeoValue::BoundingBox {
            southwest,
            northeast,
        } => {
            values.insert("geometry_kind".into(), string("bounding_box".into()));
            values.insert(
                "southwest_longitude".into(),
                RuntimeValue::Decimal(southwest.longitude.to_string()),
            );
            values.insert(
                "southwest_latitude".into(),
                RuntimeValue::Decimal(southwest.latitude.to_string()),
            );
            values.insert(
                "northeast_longitude".into(),
                RuntimeValue::Decimal(northeast.longitude.to_string()),
            );
            values.insert(
                "northeast_latitude".into(),
                RuntimeValue::Decimal(northeast.latitude.to_string()),
            );
        }
    }
}

fn claim_rows(
    changes: &[RuntimeChange],
    scope: &rrd_core::ScopeId,
    predicate: Option<&rrd_core::Predicate>,
    valid_at: u64,
) -> Vec<QueryRow> {
    let mut groups = BTreeMap::<(String, String), Vec<Claim>>::new();
    for change in changes.iter().filter(|change| &change.scope == scope) {
        let RuntimeMutation::Claim { claim } = &change.mutation else {
            continue;
        };
        if predicate.is_some_and(|expected| &claim.predicate != expected) {
            continue;
        }
        groups
            .entry((claim.subject.to_string(), claim.predicate.to_string()))
            .or_default()
            .push(claim.clone());
    }
    groups
        .into_iter()
        .filter_map(|((subject, predicate), candidates)| {
            let claim = resolve_as_of(&candidates, valid_at)?;
            let mut values = BTreeMap::new();
            values.insert("subject".into(), string(subject.clone()));
            values.insert("predicate".into(), string(predicate.clone()));
            values.insert("object".into(), string(claim.object.clone()));
            values.insert(
                "valid_from".into(),
                RuntimeValue::Unsigned(claim.valid_from),
            );
            values.insert("valid_to".into(), optional_u64(claim.valid_to));
            values.insert("tx_time".into(), RuntimeValue::Unsigned(claim.tx_time));
            values.insert("actor".into(), string(claim.producer.actor.clone()));
            Some(QueryRow {
                identity: format!("claim:{subject}:{predicate}"),
                values,
            })
        })
        .collect()
}

fn bm25_scores(
    rows: &[QueryRow],
    shape: &PlanShape,
    artifact: Option<Bm25Artifact>,
) -> Result<BTreeMap<String, f64>> {
    let mut matches = shape
        .filters
        .iter()
        .filter(|filter| filter.comparison.is_match());
    let Some(filter) = matches.next() else {
        if artifact.is_some() {
            return Err(Error::Integrity(
                "a BM25 artifact was selected for a query without MATCH".into(),
            ));
        }
        return Ok(BTreeMap::new());
    };
    if matches.next().is_some() {
        return Err(Error::Binding(
            "BM25 v1 supports exactly one MATCH predicate".into(),
        ));
    }
    let RuntimeValue::String(query) = &filter.value else {
        return Err(Error::Binding("MATCH requires a string query".into()));
    };
    if rows.is_empty() {
        return Ok(BTreeMap::new());
    }
    let artifact = match artifact {
        Some(artifact) => {
            if artifact.source_cursor != shape.source_cursor || artifact.valid_at != shape.valid_at
            {
                return Err(Error::Integrity(
                    "selected BM25 artifact is not fresh for the bound query".into(),
                ));
            }
            artifact
        }
        None => {
            let documents = rows
                .iter()
                .filter_map(|row| match row.values.get(&filter.field) {
                    Some(RuntimeValue::String(text)) => {
                        Some(Ok((row.identity.clone(), text.clone())))
                    }
                    Some(RuntimeValue::Null) | None => None,
                    Some(_) => Some(Err(Error::Binding(format!(
                        "MATCH field {:?} produced a non-string value",
                        filter.field
                    )))),
                })
                .collect::<Result<Vec<_>>>()?;
            if documents.is_empty() {
                return Ok(BTreeMap::new());
            }
            Bm25Artifact::build(
                Bm25Config::default(),
                shape.source_cursor,
                shape.schema_revision,
                shape.valid_at,
                documents,
            )?
        }
    };
    Ok(artifact
        .search(query, rows.len().max(1))?
        .into_iter()
        .map(|hit| (hit.identity, hit.score))
        .collect())
}

fn apply_projection(row: &mut QueryRow, projection: &Projection) {
    if let Projection::Fields(fields) = projection {
        row.values.retain(|field, _| fields.contains(field));
    }
}

fn string(value: String) -> RuntimeValue {
    RuntimeValue::String(value)
}

fn optional_u64(value: Option<u64>) -> RuntimeValue {
    value.map_or(RuntimeValue::Null, RuntimeValue::Unsigned)
}
