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
use vyrm_store::Engine;

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
    let mut execution_links = links;
    execution_links.push(TraceLink::Plan {
        plan_digest: plan.digest.clone(),
    });
    let execution_span = match DurableTraceSpan::start(
        store,
        scope,
        actor,
        execution_identity,
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
    let execution = match vyrm_mx::execute(store, &plan, budget) {
        Ok(execution) => execution,
        Err(error) => {
            let outcome = if matches!(error, vyrm_mx::Error::Budget(_)) {
                TraceOutcome::Denied
            } else {
                TraceOutcome::Error
            };
            return fail_query(
                store,
                Some(execution_span),
                root,
                "execution",
                mx_error_class(&error),
                outcome,
                error.into(),
            );
        }
    };
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
