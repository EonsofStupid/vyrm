//! Explicit, durably traced `vyrmQL -> vyrmMX -> execution` orchestration.
//!
//! Connectome's GET query lens remains read-only. Operator and MCP execution
//! use this boundary when the query itself should become optimization evidence.
//! The read stamp is captured before the first trace write, preventing
//! observability from changing the meaning of `KNOWN HEAD`.

use crate::{active_reasoning_run, DurableTraceSpan, TraceIdentity};
use serde::Serialize;
use serde_json::Value;
use vyrm_core::{
    digest, Millis, RuntimeProperties, RuntimeValue, ScopeId, TraceDataClass, TraceDomain,
    TraceLink, TraceOutcome,
};
use vyrm_mx::{BoundQuery, Catalog, PhysicalPlan, QueryExecution};
pub use vyrm_mx::{ExecutionBudget, Parameters};
use vyrm_ql::{Query, Source, QUERY_CONTRACT_VERSION};
use vyrm_store::{Engine, PhysicalStoreEvidence};

const MAX_QUERY_BYTES: usize = 64 * 1024;
const MAX_QUERY_PARAMETERS: usize = 128;
const MAX_PARAMETER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TracedQueryExecution {
    pub canonical: String,
    pub query: Query,
    pub bound: BoundQuery,
    pub plan: PhysicalPlan,
    pub execution: QueryExecution,
}

/// Converts a JSON object of scalar values into the typed binder contract.
/// Positive JSON integers become unsigned; negative integers remain signed.
pub fn query_parameters_from_json(value: &Value) -> Result<Parameters, Box<dyn std::error::Error>> {
    let object = value
        .as_object()
        .ok_or("query parameters must be a JSON object")?;
    if object.len() > MAX_QUERY_PARAMETERS {
        return Err(format!("query parameters exceed {MAX_QUERY_PARAMETERS} entries").into());
    }
    if serde_json::to_vec(value)?.len() > MAX_PARAMETER_BYTES {
        return Err(format!("query parameters exceed {MAX_PARAMETER_BYTES} encoded bytes").into());
    }
    object
        .iter()
        .map(|(name, value)| Ok((name.clone(), scalar_parameter(name, value)?)))
        .collect()
}

fn scalar_parameter(name: &str, value: &Value) -> Result<RuntimeValue, Box<dyn std::error::Error>> {
    match value {
        Value::Null => Ok(RuntimeValue::Null),
        Value::Bool(value) => Ok(RuntimeValue::Bool(*value)),
        Value::Number(value) if value.as_u64().is_some() => {
            Ok(RuntimeValue::Unsigned(value.as_u64().expect("checked")))
        }
        Value::Number(value) if value.as_i64().is_some() => {
            Ok(RuntimeValue::Integer(value.as_i64().expect("checked")))
        }
        Value::String(value) => Ok(RuntimeValue::String(value.clone())),
        _ => Err(format!(
            "query parameter {name:?} must be null, boolean, integer, unsigned integer, or string"
        )
        .into()),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn execute_traced_query<E: Engine>(
    store: &E,
    scope: ScopeId,
    source: &str,
    parameters: &Parameters,
    budget: &ExecutionBudget,
    actor: &str,
    at: Millis,
) -> Result<TracedQueryExecution, Box<dyn std::error::Error>> {
    let read = store.runtime_read_stamp(&scope)?;
    let query_digest = digest::sha256_hex(source.as_bytes());
    let parameter_bytes = serde_json::to_vec(parameters)?;
    let parameter_digest = digest::sha256_hex(&parameter_bytes);
    let at_bytes = at.to_be_bytes();
    let cursor_bytes = read.commit_cursor.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        scope.as_str().as_bytes(),
        query_digest.as_bytes(),
        parameter_digest.as_bytes(),
        &at_bytes,
        &cursor_bytes,
    ])?;
    let mut links = vec![TraceLink::Read {
        stamp: read.clone(),
    }];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let root_attributes = RuntimeProperties::from([
        ("query_digest".into(), RuntimeValue::Digest(query_digest)),
        (
            "query_bytes".into(),
            RuntimeValue::Unsigned(source.len() as u64),
        ),
        (
            "parameter_digest".into(),
            RuntimeValue::Digest(parameter_digest),
        ),
        (
            "parameter_count".into(),
            RuntimeValue::Unsigned(parameters.len() as u64),
        ),
        (
            "contract_version".into(),
            RuntimeValue::Unsigned(u64::from(QUERY_CONTRACT_VERSION)),
        ),
        (
            "max_scanned_changes".into(),
            RuntimeValue::Unsigned(budget.max_scanned_changes as u64),
        ),
        (
            "max_rows".into(),
            RuntimeValue::Unsigned(budget.max_rows as u64),
        ),
        (
            "max_output_bytes".into(),
            RuntimeValue::Unsigned(budget.max_output_bytes as u64),
        ),
    ]);
    let root_identity = identity.clone();
    let root = DurableTraceSpan::start(
        store,
        scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Query,
        "query.run",
        at,
        TraceDataClass::Control,
        links.clone(),
        root_attributes,
    )?;

    if source.trim().is_empty() {
        return fail_query(
            store,
            None,
            root,
            "parse_bind",
            "empty_query",
            TraceOutcome::Denied,
            "vyrmQL query must not be empty".into(),
        );
    }
    if source.len() > MAX_QUERY_BYTES {
        return fail_query(
            store,
            None,
            root,
            "parse_bind",
            "contract_limit",
            TraceOutcome::Denied,
            format!("vyrmQL query exceeds {MAX_QUERY_BYTES} bytes").into(),
        );
    }

    let prepare_identity = root_identity.child(&[b"vyrmql.parse_bind"])?;
    let prepare = match DurableTraceSpan::start(
        store,
        scope.clone(),
        actor,
        prepare_identity,
        Some(root_identity.span_id.clone()),
        TraceDomain::Query,
        "vyrmql.parse_bind",
        root.observed_at(),
        TraceDataClass::Control,
        links.clone(),
        RuntimeProperties::new(),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_query(
                store,
                None,
                root,
                "parse_bind",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let query = match vyrm_ql::parse(source) {
        Ok(query) => query,
        Err(error) => {
            return fail_query(
                store,
                Some(prepare),
                root,
                "parse_bind",
                "parse",
                TraceOutcome::Error,
                error.into(),
            )
        }
    };
    let catalog = match Catalog::capture_at(store, read.clone()) {
        Ok(catalog) => catalog,
        Err(error) => {
            return fail_query(
                store,
                Some(prepare),
                root,
                "parse_bind",
                mx_error_class(&error),
                TraceOutcome::Error,
                error.into(),
            )
        }
    };
    let bound = match vyrm_mx::bind(&query, parameters, &catalog) {
        Ok(bound) => bound,
        Err(error) => {
            return fail_query(
                store,
                Some(prepare),
                root,
                "parse_bind",
                mx_error_class(&error),
                TraceOutcome::Error,
                error.into(),
            )
        }
    };
    let canonical = query.canonical();
    let (source_family, source_type) = source_identity(&query.source);
    let prepare_attributes = RuntimeProperties::from([
        (
            "canonical_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(canonical.as_bytes())),
        ),
        (
            "source_family".into(),
            RuntimeValue::String(source_family.into()),
        ),
        ("source_type".into(), RuntimeValue::String(source_type)),
        (
            "schema_revision".into(),
            RuntimeValue::Unsigned(bound.schema_revision),
        ),
        (
            "filter_count".into(),
            RuntimeValue::Unsigned(bound.filters.len() as u64),
        ),
    ]);
    if let Err(error) = prepare.finish(store, TraceOutcome::Ok, Vec::new(), prepare_attributes) {
        return fail_query(
            store,
            None,
            root,
            "parse_bind",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }

    let planning_identity = root_identity.child(&[b"vyrmmx.plan"])?;
    let planning = match DurableTraceSpan::start(
        store,
        scope.clone(),
        actor,
        planning_identity,
        Some(root_identity.span_id.clone()),
        TraceDomain::Planning,
        "vyrmmx.plan",
        root.observed_at(),
        TraceDataClass::Control,
        links.clone(),
        RuntimeProperties::new(),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_query(
                store,
                None,
                root,
                "planning",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let plan = match vyrm_mx::plan(&bound) {
        Ok(plan) => plan,
        Err(error) => {
            return fail_query(
                store,
                Some(planning),
                root,
                "planning",
                mx_error_class(&error),
                TraceOutcome::Error,
                error.into(),
            )
        }
    };
    let selected = plan
        .explanation
        .candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .map(|candidate| RuntimeValue::String(candidate.name.clone()))
        .collect::<Vec<_>>();
    let rejected = plan
        .explanation
        .candidates
        .iter()
        .filter(|candidate| !candidate.selected)
        .map(|candidate| RuntimeValue::String(candidate.name.clone()))
        .collect::<Vec<_>>();
    let planning_attributes = RuntimeProperties::from([
        (
            "exact".into(),
            RuntimeValue::Bool(plan.explanation.contract.exact),
        ),
        (
            "logical_operator_count".into(),
            RuntimeValue::Unsigned(plan.logical.operators.len() as u64),
        ),
        (
            "physical_operator_count".into(),
            RuntimeValue::Unsigned(plan.operators.len() as u64),
        ),
        ("selected_paths".into(), RuntimeValue::List(selected)),
        ("rejected_paths".into(), RuntimeValue::List(rejected)),
    ]);
    if let Err(error) = planning.finish(
        store,
        TraceOutcome::Ok,
        vec![TraceLink::Plan {
            plan_digest: plan.digest.clone(),
        }],
        planning_attributes,
    ) {
        return fail_query(
            store,
            None,
            root,
            "planning",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }

    let execution_identity = root_identity.child(&[b"vyrmmx.execute"])?;
    let storage_identity = execution_identity.child(&[b"vyrmkv.runtime_read"])?;
    let mut execution_links = links;
    execution_links.push(TraceLink::Plan {
        plan_digest: plan.digest.clone(),
    });
    let execution_span = match DurableTraceSpan::start(
        store,
        scope.clone(),
        actor,
        execution_identity.clone(),
        Some(root_identity.span_id),
        TraceDomain::Query,
        "vyrmmx.execute",
        root.observed_at(),
        TraceDataClass::Control,
        execution_links,
        RuntimeProperties::new(),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_query(
                store,
                None,
                root,
                "execution",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let storage_span = match DurableTraceSpan::start(
        store,
        scope,
        actor,
        storage_identity,
        Some(execution_identity.span_id),
        TraceDomain::Storage,
        "vyrmkv.runtime_read",
        root.observed_at(),
        TraceDataClass::Control,
        execution_links_for_storage(&read, &plan),
        RuntimeProperties::from([(
            "evidence_policy".into(),
            RuntimeValue::String("complete_logical_bounded_physical".into()),
        )]),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_query(
                store,
                Some(execution_span),
                root,
                "storage",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let physical_before = store.physical_store_evidence();
    let execution_result = vyrm_mx::execute(store, &plan, budget);
    let physical_after = store.physical_store_evidence();
    let execution = match execution_result {
        Ok(execution) => execution,
        Err(error) => {
            let outcome = if matches!(error, vyrm_mx::Error::Budget(_)) {
                TraceOutcome::Denied
            } else {
                TraceOutcome::Error
            };
            let error_class = mx_error_class(&error);
            let rendered = error.to_string();
            let mut storage_attributes =
                physical_storage_attributes(&physical_before, &physical_after, None);
            storage_attributes.insert(
                "error_class".into(),
                RuntimeValue::String(error_class.into()),
            );
            storage_attributes.insert(
                "error_digest".into(),
                RuntimeValue::Digest(digest::sha256_hex(rendered.as_bytes())),
            );
            let error: Box<dyn std::error::Error> = error.into();
            if let Err(trace_error) =
                storage_span.finish(store, outcome, Vec::new(), storage_attributes)
            {
                return fail_query(
                    store,
                    Some(execution_span),
                    root,
                    "storage",
                    "trace_finish",
                    TraceOutcome::Error,
                    format!("{rendered}; storage trace finish failed: {trace_error}").into(),
                );
            }
            return fail_query(
                store,
                Some(execution_span),
                root,
                "execution",
                error_class,
                outcome,
                error,
            );
        }
    };
    if let Err(error) = storage_span.finish(
        store,
        TraceOutcome::Ok,
        Vec::new(),
        physical_storage_attributes(&physical_before, &physical_after, Some(&execution)),
    ) {
        return fail_query(
            store,
            Some(execution_span),
            root,
            "storage",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    let execution_attributes = execution_attributes(&execution);
    if let Err(error) = execution_span.finish(
        store,
        TraceOutcome::Ok,
        Vec::new(),
        execution_attributes.clone(),
    ) {
        return fail_query(
            store,
            None,
            root,
            "execution",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    root.finish(
        store,
        TraceOutcome::Ok,
        vec![TraceLink::Plan {
            plan_digest: plan.digest.clone(),
        }],
        execution_attributes,
    )
    .map_err(|error| {
        format!("query completed but its authoritative root finish was not durable: {error}")
    })?;

    Ok(TracedQueryExecution {
        canonical,
        query,
        bound,
        plan,
        execution,
    })
}

fn execution_links_for_storage(read: &vyrm_core::ReadStamp, plan: &PhysicalPlan) -> Vec<TraceLink> {
    vec![
        TraceLink::Read {
            stamp: read.clone(),
        },
        TraceLink::Plan {
            plan_digest: plan.digest.clone(),
        },
    ]
}

fn physical_storage_attributes(
    before: &Result<PhysicalStoreEvidence, vyrm_store::Error>,
    after: &Result<PhysicalStoreEvidence, vyrm_store::Error>,
    execution: Option<&QueryExecution>,
) -> RuntimeProperties {
    let mut attributes = RuntimeProperties::new();
    match (before, after) {
        (Ok(before), Ok(after)) => {
            attributes.insert(
                "backend".into(),
                RuntimeValue::String(after.backend.clone()),
            );
            attributes.insert(
                "physical_evidence".into(),
                RuntimeValue::String(after.evidence_level.clone()),
            );
            attributes.insert(
                "physical_evidence_consistent".into(),
                RuntimeValue::Bool(
                    before.backend == after.backend
                        && before.evidence_level == after.evidence_level,
                ),
            );
            insert_counter(
                &mut attributes,
                "physical_sequence",
                after.physical_sequence,
            );
            insert_counter(
                &mut attributes,
                "manifest_generation",
                after.manifest_generation,
            );
            insert_counter(&mut attributes, "durable_sequence", after.durable_sequence);
            insert_counter(
                &mut attributes,
                "memtable_versions",
                after.memtable_versions,
            );
            insert_counter(&mut attributes, "memtable_bytes", after.memtable_bytes);
            insert_counter(
                &mut attributes,
                "memtable_max_versions",
                after.memtable_max_versions,
            );
            insert_counter(
                &mut attributes,
                "wal_payload_bytes",
                after.wal_payload_bytes,
            );
            insert_counter(
                &mut attributes,
                "wal_payload_max_bytes",
                after.wal_payload_max_bytes,
            );
            insert_delta(
                &mut attributes,
                "automatic_flushes_delta",
                before.automatic_flushes,
                after.automatic_flushes,
            );
            insert_delta(
                &mut attributes,
                "maintenance_write_stalls_delta",
                before.maintenance_write_stalls,
                after.maintenance_write_stalls,
            );
            insert_delta(
                &mut attributes,
                "failed_maintenance_flushes_delta",
                before.failed_maintenance_flushes,
                after.failed_maintenance_flushes,
            );
            insert_delta(
                &mut attributes,
                "oversized_batches_delta",
                before.oversized_batches,
                after.oversized_batches,
            );
            insert_delta(
                &mut attributes,
                "automatic_compactions_delta",
                before.automatic_compactions,
                after.automatic_compactions,
            );
            insert_delta(
                &mut attributes,
                "failed_compactions_delta",
                before.failed_compactions,
                after.failed_compactions,
            );
            insert_delta(
                &mut attributes,
                "compaction_input_bytes_delta",
                before.compaction_input_bytes,
                after.compaction_input_bytes,
            );
            insert_delta(
                &mut attributes,
                "compaction_output_bytes_delta",
                before.compaction_output_bytes,
                after.compaction_output_bytes,
            );
            insert_counter(
                &mut attributes,
                "peak_compaction_buffer_bytes",
                after.peak_compaction_buffer_bytes,
            );
            insert_counter(&mut attributes, "l0_segment_count", after.l0_segment_count);
            insert_counter(
                &mut attributes,
                "l0_compaction_trigger",
                after.l0_compaction_trigger,
            );
            insert_counter(
                &mut attributes,
                "compaction_debt_segments",
                after.compaction_debt_segments,
            );
            insert_counter(
                &mut attributes,
                "compaction_target_segment_bytes",
                after.compaction_target_segment_bytes,
            );
            insert_counter(&mut attributes, "segment_count", after.segment_count);
            insert_counter(&mut attributes, "segment_bytes", after.segment_bytes);
            insert_counter(
                &mut attributes,
                "cache_capacity_bytes",
                after.cache_capacity_bytes,
            );
            insert_counter(
                &mut attributes,
                "cache_resident_bytes",
                after.cache_resident_bytes,
            );
            insert_counter(&mut attributes, "cache_entries", after.cache_entries);
            insert_delta(
                &mut attributes,
                "cache_hits_delta",
                before.cache_hits,
                after.cache_hits,
            );
            insert_delta(
                &mut attributes,
                "cache_misses_delta",
                before.cache_misses,
                after.cache_misses,
            );
            insert_delta(
                &mut attributes,
                "cache_evictions_delta",
                before.cache_evictions,
                after.cache_evictions,
            );
            insert_delta(
                &mut attributes,
                "block_loads_delta",
                before.block_loads,
                after.block_loads,
            );
            insert_delta(
                &mut attributes,
                "block_bytes_loaded_delta",
                before.block_bytes_loaded,
                after.block_bytes_loaded,
            );
            insert_delta(
                &mut attributes,
                "block_bytes_decoded_delta",
                before.block_bytes_decoded,
                after.block_bytes_decoded,
            );
            insert_delta(
                &mut attributes,
                "filter_checks_delta",
                before.filter_checks,
                after.filter_checks,
            );
            insert_delta(
                &mut attributes,
                "filter_negatives_delta",
                before.filter_negatives,
                after.filter_negatives,
            );
        }
        (Err(before_error), Err(after_error)) => {
            attributes.insert(
                "physical_evidence".into(),
                RuntimeValue::String("unavailable".into()),
            );
            attributes.insert(
                "physical_error_digest".into(),
                RuntimeValue::Digest(digest::sha256_hex(
                    format!("{before_error}; {after_error}").as_bytes(),
                )),
            );
        }
        (Err(error), Ok(after)) | (Ok(after), Err(error)) => {
            attributes.insert(
                "backend".into(),
                RuntimeValue::String(after.backend.clone()),
            );
            attributes.insert(
                "physical_evidence".into(),
                RuntimeValue::String("partial".into()),
            );
            attributes.insert(
                "physical_error_digest".into(),
                RuntimeValue::Digest(digest::sha256_hex(error.to_string().as_bytes())),
            );
        }
    }
    if let Some(execution) = execution {
        attributes.insert(
            "logical_scanned_changes".into(),
            RuntimeValue::Unsigned(execution.scanned_changes as u64),
        );
        attributes.insert(
            "stamp_validation".into(),
            RuntimeValue::String(execution.stamp_validation.clone()),
        );
        attributes.insert(
            "stamp_validation_max_changes".into(),
            RuntimeValue::Unsigned(execution.stamp_validation_max_changes as u64),
        );
        attributes.insert(
            "stamp_validation_proof_nodes".into(),
            RuntimeValue::Unsigned(execution.stamp_validation_proof_nodes as u64),
        );
        attributes.insert(
            "logical_output_bytes".into(),
            RuntimeValue::Unsigned(execution.output_bytes as u64),
        );
    }
    attributes
}

fn insert_counter(attributes: &mut RuntimeProperties, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        attributes.insert(name.into(), RuntimeValue::Unsigned(value));
    }
}

fn insert_delta(
    attributes: &mut RuntimeProperties,
    name: &str,
    before: Option<u64>,
    after: Option<u64>,
) {
    if let (Some(before), Some(after)) = (before, after) {
        attributes.insert(
            name.into(),
            RuntimeValue::Unsigned(after.saturating_sub(before)),
        );
    }
}

fn execution_attributes(execution: &QueryExecution) -> RuntimeProperties {
    RuntimeProperties::from([
        (
            "scanned_changes".into(),
            RuntimeValue::Unsigned(execution.scanned_changes as u64),
        ),
        (
            "returned_rows".into(),
            RuntimeValue::Unsigned(execution.returned_rows as u64),
        ),
        (
            "stamp_validation".into(),
            RuntimeValue::String(execution.stamp_validation.clone()),
        ),
        (
            "stamp_validation_max_changes".into(),
            RuntimeValue::Unsigned(execution.stamp_validation_max_changes as u64),
        ),
        (
            "stamp_validation_proof_nodes".into(),
            RuntimeValue::Unsigned(execution.stamp_validation_proof_nodes as u64),
        ),
        (
            "output_bytes".into(),
            RuntimeValue::Unsigned(execution.output_bytes as u64),
        ),
        (
            "batch_count".into(),
            RuntimeValue::Unsigned(execution.batches.len() as u64),
        ),
        ("truncated".into(), RuntimeValue::Bool(execution.truncated)),
    ])
}

fn source_identity(source: &Source) -> (&'static str, String) {
    match source {
        Source::Record { kind } => ("record", kind.to_string()),
        Source::Relation { kind } => ("relation", kind.to_string()),
        Source::Event { kind } => ("event", kind.to_string()),
        Source::Claim { predicate } => (
            "claim",
            predicate
                .as_ref()
                .map_or_else(|| "*".into(), ToString::to_string),
        ),
    }
}

fn mx_error_class(error: &vyrm_mx::Error) -> &'static str {
    match error {
        vyrm_mx::Error::Catalog(_) => "catalog",
        vyrm_mx::Error::Binding(_) => "binding",
        vyrm_mx::Error::Budget(_) => "budget",
        vyrm_mx::Error::Execution(_) => "execution",
        vyrm_mx::Error::Integrity(_) => "integrity",
    }
}

#[allow(clippy::too_many_arguments)]
fn fail_query<E: Engine, T>(
    store: &E,
    stage: Option<DurableTraceSpan>,
    root: DurableTraceSpan,
    failed_stage: &str,
    error_class: &str,
    outcome: TraceOutcome,
    error: Box<dyn std::error::Error>,
) -> Result<T, Box<dyn std::error::Error>> {
    let rendered = error.to_string();
    let error_digest = digest::sha256_hex(rendered.as_bytes());
    let evidence = || {
        RuntimeProperties::from([
            (
                "error_class".into(),
                RuntimeValue::String(error_class.into()),
            ),
            (
                "error_digest".into(),
                RuntimeValue::Digest(error_digest.clone()),
            ),
            (
                "failed_stage".into(),
                RuntimeValue::String(failed_stage.into()),
            ),
        ])
    };
    let mut trace_errors = Vec::new();
    if let Some(stage) = stage {
        if let Err(trace_error) = stage.finish(store, outcome, Vec::new(), evidence()) {
            trace_errors.push(format!("stage finish: {trace_error}"));
        }
    }
    if let Err(trace_error) = root.finish(store, outcome, Vec::new(), evidence()) {
        trace_errors.push(format!("root finish: {trace_error}"));
    }
    if trace_errors.is_empty() {
        Err(error)
    } else {
        Err(format!(
            "{rendered}; authoritative query trace also failed ({})",
            trace_errors.join("; ")
        )
        .into())
    }
}
