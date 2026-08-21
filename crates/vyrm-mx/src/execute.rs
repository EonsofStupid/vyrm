use crate::{BoundFilter, Error, LogicalOperator, PhysicalOperator, PhysicalPlan, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vyrm_core::{
    resolve_as_of, Claim, RuntimeChange, RuntimeGraphSnapshot, RuntimeMutation, RuntimeValue,
};
use vyrm_ql::{Projection, Source};
use vyrm_store::Engine;

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
    let stamp_validation_max_changes = usize::try_from(contract.stamp_validation_max_changes)
        .map_err(|_| {
            Error::Budget("stamp-validation bound exceeds this platform's address space".into())
        })?;
    if requested > budget.max_scanned_changes {
        return Err(Error::Budget(format!(
            "query requires scanning {requested} changes, budget allows {}",
            budget.max_scanned_changes
        )));
    }
    let changes = access.load(engine, &plan.logical.read)?;
    let mut rows = rows_for_source(
        &changes,
        &plan.logical.read.scope,
        &shape.source,
        contract.valid_at,
        contract.known_at_cursor,
    );
    rows.retain(|row| {
        shape
            .filters
            .iter()
            .all(|filter| matches_filter(row, filter))
    });
    rows.sort_by(|left, right| left.identity.cmp(&right.identity));
    if let Some(query_limit) = shape.limit {
        rows.truncate(query_limit);
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
        stamp_validation: contract.stamp_validation.clone(),
        stamp_validation_max_changes,
        returned_rows,
        output_bytes,
        truncated,
        batches,
    })
}

enum ReadPath {
    LogScan { through_cursor: u64 },
    EventCursorLookup { cursor: u64, through_cursor: u64 },
}

impl ReadPath {
    fn from_plan(plan: &PhysicalPlan, shape: &PlanShape) -> Result<Self> {
        let contract = &plan.explanation.contract;
        if contract.read_manifest != plan.logical.read.manifest_id
            || contract.scope != plan.logical.read.scope.as_str()
            || contract.valid_at != shape.valid_at
            || contract.known_at_cursor != shape.known_at_cursor
            || contract.schema_revision != plan.logical.schema_revision
            || contract.authorization_boundary != format!("scope:{}", plan.logical.read.scope)
            || contract.stamp_validation != "full_hash_chain_replay"
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
        let [access, PhysicalOperator::ReferenceEvaluate] = plan.operators.as_slice() else {
            return Err(Error::Integrity(
                "physical plan must contain one authoritative access path followed by reference evaluation"
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
        }
    }

    fn load<E: Engine>(
        &self,
        engine: &E,
        stamp: &vyrm_core::ReadStamp,
    ) -> Result<Vec<RuntimeChange>> {
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
        Ok(page.changes)
    }
}

struct PlanShape {
    source: Source,
    valid_at: u64,
    known_at_cursor: u64,
    filters: Vec<BoundFilter>,
    projection: Projection,
    limit: Option<usize>,
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
        })
    }
}

fn rows_for_source(
    changes: &[RuntimeChange],
    scope: &vyrm_core::ScopeId,
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
        Source::Claim { predicate } => claim_rows(changes, scope, predicate.as_ref(), valid_at),
    }
}

fn claim_rows(
    changes: &[RuntimeChange],
    scope: &vyrm_core::ScopeId,
    predicate: Option<&vyrm_core::Predicate>,
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

fn matches_filter(row: &QueryRow, filter: &BoundFilter) -> bool {
    row.values.get(&filter.field) == Some(&filter.value)
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
