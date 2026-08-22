//! Durable execution boundary for project-scoped external operator knowledge.

use crate::{active_reasoning_run, DurableTraceSpan, InstanceBinding, TraceIdentity};
use vyrm_core::{
    digest, Millis, RuntimeProperties, RuntimeValue, TraceDataClass, TraceDomain, TraceLink,
    TraceOutcome,
};
use vyrm_operator::{
    execute_operator_search, execute_operator_sync as apply_operator_sync, IterativeScanMode,
    OperatorAccessPath, OperatorKnowledgeAdapter, OperatorKnowledgeBinding,
    OperatorKnowledgeWriter, OperatorSearchRequest, OperatorSearchResult, OperatorSyncOperation,
    OperatorSyncReceipt, OperatorSyncWork,
};
use vyrm_store::Engine;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TracedOperatorSearch {
    pub result: OperatorSearchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TracedOperatorSync {
    pub receipt: OperatorSyncReceipt,
}

/// Applies one already-committed Vyrm outbox item to external operator
/// knowledge. The payload is caller-owned and content-verified; only its digest
/// enters the trace. A crash between external apply and trace finish is safe
/// because the same work identity must replay idempotently at the adapter.
#[allow(clippy::too_many_arguments)]
pub fn execute_traced_operator_sync<E, W>(
    store: &E,
    instance: &InstanceBinding,
    knowledge: &OperatorKnowledgeBinding,
    writer: &mut W,
    work: &OperatorSyncWork,
    payload: &[u8],
    actor: &str,
    at: Millis,
) -> Result<TracedOperatorSync, Box<dyn std::error::Error>>
where
    E: Engine,
    W: OperatorKnowledgeWriter,
{
    knowledge.validate()?;
    work.validate()?;
    let at_bytes = at.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        knowledge.project_id.as_bytes(),
        work.id.as_bytes(),
        &work.source_cursor.to_be_bytes(),
        &at_bytes,
    ])?;
    let root_identity = identity.clone();
    let implementation_digest = writer.descriptor().implementation_digest.clone();
    let links = vec![
        TraceLink::RuntimeCursor {
            cursor: work.source_cursor,
        },
        TraceLink::Projection {
            stamp: knowledge.projection.clone(),
        },
    ];
    let root = DurableTraceSpan::start(
        store,
        work.scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Adapter,
        "operator.knowledge.sync",
        at,
        TraceDataClass::Control,
        links.clone(),
        RuntimeProperties::from([
            ("work_id".into(), RuntimeValue::Digest(work.id.clone())),
            (
                "binding_digest".into(),
                RuntimeValue::Digest(work.binding_digest.clone()),
            ),
            (
                "source_change_digest".into(),
                RuntimeValue::Digest(work.source_change_digest.clone()),
            ),
            (
                "payload_digest".into(),
                RuntimeValue::Digest(work.payload_digest.clone()),
            ),
            (
                "source_cursor".into(),
                RuntimeValue::Unsigned(work.source_cursor),
            ),
            (
                "operation".into(),
                RuntimeValue::String(sync_operation_name(work.operation).into()),
            ),
        ]),
    )?;
    if let Err(error) = require_project_binding(instance, knowledge) {
        return fail_operator(
            store,
            None,
            root,
            "project_binding",
            "tenant_or_instance",
            TraceOutcome::Denied,
            error,
        );
    }
    let child_identity = root_identity.child(&[b"operator.knowledge.apply"])?;
    let child = match DurableTraceSpan::start(
        store,
        work.scope.clone(),
        actor,
        child_identity,
        Some(root_identity.span_id),
        TraceDomain::Adapter,
        "operator.knowledge.apply",
        root.observed_at(),
        TraceDataClass::Control,
        links,
        RuntimeProperties::from([("work_id".into(), RuntimeValue::Digest(work.id.clone()))]),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_operator(
                store,
                None,
                root,
                "apply",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let receipt = match apply_operator_sync(writer, knowledge, work, payload) {
        Ok(receipt) => receipt,
        Err(error) => {
            let rendered = error.to_string();
            let (class, outcome) = operator_error_class(&rendered);
            return fail_operator(
                store,
                Some(child),
                root,
                "apply",
                class,
                outcome,
                error.into(),
            );
        }
    };
    let revision = receipt.revision.digest()?;
    let result_links = vec![TraceLink::OperatorKnowledge {
        adapter: knowledge.adapter.clone(),
        project_id: knowledge.project_id.clone(),
        source_revision: revision.clone(),
    }];
    let attributes = RuntimeProperties::from([
        (
            "source_revision_digest".into(),
            RuntimeValue::Digest(revision),
        ),
        (
            "applied_now".into(),
            RuntimeValue::Bool(receipt.applied_now),
        ),
        (
            "idempotent_replay".into(),
            RuntimeValue::Bool(receipt.idempotent_replay),
        ),
        (
            "adapter_implementation_digest".into(),
            RuntimeValue::Digest(implementation_digest),
        ),
    ]);
    if let Err(error) = child.finish(
        store,
        TraceOutcome::Ok,
        result_links.clone(),
        attributes.clone(),
    ) {
        return fail_operator(
            store,
            None,
            root,
            "apply",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    root.finish(store, TraceOutcome::Ok, result_links, attributes)
        .map_err(|error| {
            format!("operator sync completed but its authoritative root finish failed: {error}")
        })?;
    Ok(TracedOperatorSync { receipt })
}

/// Executes one external search through its immutable instance binding. The
/// adapter owns its external snapshot; Vyrm persists only bounded causal
/// evidence and never claims that trace commits share the external transaction.
#[allow(clippy::too_many_arguments)]
pub fn execute_traced_operator_search<E, A>(
    store: &E,
    instance: &InstanceBinding,
    knowledge: &OperatorKnowledgeBinding,
    adapter: &mut A,
    request: &OperatorSearchRequest,
    actor: &str,
    at: Millis,
) -> Result<TracedOperatorSearch, Box<dyn std::error::Error>>
where
    E: Engine,
    A: OperatorKnowledgeAdapter,
{
    knowledge.validate()?;
    request.validate()?;
    let verification =
        store.runtime_read_changes(&request.search.read, request.search.read.commit_cursor, 1)?;
    if verification.through_cursor != request.search.read.commit_cursor {
        return Err("operator-knowledge read stamp did not verify at its captured cursor".into());
    }
    let request_digest = request.digest()?;
    let binding_digest = knowledge.digest()?;
    let cursor = request.search.read.commit_cursor.to_be_bytes();
    let at_bytes = at.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        knowledge.project_id.as_bytes(),
        request_digest.as_bytes(),
        &cursor,
        &at_bytes,
    ])?;
    let root_identity = identity.clone();
    let implementation_digest = adapter.descriptor().implementation_digest.clone();
    let mut links = vec![
        TraceLink::Read {
            stamp: request.search.read.clone(),
        },
        TraceLink::Projection {
            stamp: knowledge.projection.clone(),
        },
    ];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let root = DurableTraceSpan::start(
        store,
        request.search.scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Adapter,
        "operator.knowledge.search",
        at,
        TraceDataClass::Control,
        links.clone(),
        RuntimeProperties::from([
            (
                "request_digest".into(),
                RuntimeValue::Digest(request_digest.clone()),
            ),
            (
                "binding_digest".into(),
                RuntimeValue::Digest(binding_digest),
            ),
            (
                "adapter_digest".into(),
                RuntimeValue::Digest(digest::sha256_hex(knowledge.adapter.as_bytes())),
            ),
            (
                "project_digest".into(),
                RuntimeValue::Digest(digest::sha256_hex(knowledge.project_id.as_bytes())),
            ),
            (
                "query_dimensions".into(),
                RuntimeValue::Unsigned(request.search.query.dimensions() as u64),
            ),
            (
                "top_k".into(),
                RuntimeValue::Unsigned(request.search.top_k as u64),
            ),
            (
                "required_source_cursor".into(),
                RuntimeValue::Unsigned(request.required_source_cursor),
            ),
            (
                "expected_stable_revision".into(),
                RuntimeValue::Bool(request.expected_stable_revision.is_some()),
            ),
        ]),
    )?;

    if let Err(error) = require_project_binding(instance, knowledge) {
        return fail_operator(
            store,
            None,
            root,
            "project_binding",
            "tenant_or_instance",
            TraceOutcome::Denied,
            error,
        );
    }

    let execution_identity = root_identity.child(&[b"operator.knowledge.execute"])?;
    let execution = match DurableTraceSpan::start(
        store,
        request.search.scope.clone(),
        actor,
        execution_identity,
        Some(root_identity.span_id),
        TraceDomain::Adapter,
        "operator.knowledge.execute",
        root.observed_at(),
        TraceDataClass::Control,
        links,
        operator_control_attributes(request),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_operator(
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

    let result = match execute_operator_search(adapter, knowledge, request) {
        Ok(result) => result,
        Err(error) => {
            let rendered = error.to_string();
            let (class, outcome) = operator_error_class(&rendered);
            return fail_operator(
                store,
                Some(execution),
                root,
                "execution",
                class,
                outcome,
                error.into(),
            );
        }
    };
    let source_revision = result.revision.digest()?;
    let result_links = vec![
        TraceLink::OperatorKnowledge {
            adapter: knowledge.adapter.clone(),
            project_id: knowledge.project_id.clone(),
            source_revision: source_revision.clone(),
        },
        TraceLink::Projection {
            stamp: knowledge.projection.clone(),
        },
    ];
    let attributes = operator_result_attributes(&result, &source_revision, &implementation_digest);
    if let Err(error) = execution.finish(
        store,
        TraceOutcome::Ok,
        result_links.clone(),
        attributes.clone(),
    ) {
        return fail_operator(
            store,
            None,
            root,
            "execution",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    root.finish(store, TraceOutcome::Ok, result_links, attributes)
        .map_err(|error| {
            format!("operator search completed but its authoritative root finish failed: {error}")
        })?;
    Ok(TracedOperatorSearch { result })
}

fn require_project_binding(
    instance: &InstanceBinding,
    knowledge: &OperatorKnowledgeBinding,
) -> Result<(), Box<dyn std::error::Error>> {
    instance.require_runtime_ready()?;
    let member = instance.member.to_string_lossy();
    if knowledge.project_id != instance.manifest.id || knowledge.member != member {
        return Err(format!(
            "operator binding belongs to project {}/{}, not instance {}/{}",
            knowledge.project_id, knowledge.member, instance.manifest.id, member
        )
        .into());
    }
    Ok(())
}

fn operator_control_attributes(request: &OperatorSearchRequest) -> RuntimeProperties {
    let controls = &request.controls;
    let mut attributes = RuntimeProperties::from([
        (
            "requested_path".into(),
            RuntimeValue::String(path_name(controls.requested_path).into()),
        ),
        (
            "iterative_scan".into(),
            RuntimeValue::String(iterative_scan_name(controls.iterative_scan).into()),
        ),
    ]);
    for (name, value) in [
        ("hnsw_ef_search", controls.hnsw_ef_search),
        ("hnsw_max_scan_tuples", controls.hnsw_max_scan_tuples),
        ("ivfflat_probes", controls.ivfflat_probes),
        ("ivfflat_max_probes", controls.ivfflat_max_probes),
    ] {
        if let Some(value) = value {
            attributes.insert(name.into(), RuntimeValue::Unsigned(u64::from(value)));
        }
    }
    attributes
}

fn iterative_scan_name(mode: IterativeScanMode) -> &'static str {
    match mode {
        IterativeScanMode::Off => "off",
        IterativeScanMode::StrictOrder => "strict_order",
        IterativeScanMode::RelaxedOrder => "relaxed_order",
    }
}

fn operator_result_attributes(
    result: &OperatorSearchResult,
    source_revision: &str,
    implementation_digest: &str,
) -> RuntimeProperties {
    let mut attributes = RuntimeProperties::from([
        (
            "source_revision_digest".into(),
            RuntimeValue::Digest(source_revision.into()),
        ),
        (
            "adapter_implementation_digest".into(),
            RuntimeValue::Digest(implementation_digest.into()),
        ),
        (
            "snapshot_digest".into(),
            RuntimeValue::Digest(result.revision.snapshot_digest.clone()),
        ),
        (
            "catalog_digest".into(),
            RuntimeValue::Digest(result.revision.catalog_digest.clone()),
        ),
        (
            "stable_revision_present".into(),
            RuntimeValue::Bool(result.revision.stable_revision.is_some()),
        ),
        (
            "selected_path".into(),
            RuntimeValue::String(path_name(result.plan.selected_path).into()),
        ),
        (
            "fallback".into(),
            RuntimeValue::Bool(result.plan.fallback_reason_digest.is_some()),
        ),
        (
            "plan_digest".into(),
            RuntimeValue::Digest(result.plan.plan_digest.clone()),
        ),
        (
            "result_count".into(),
            RuntimeValue::Unsigned(result.hits.len() as u64),
        ),
        (
            "filter_applied_after_ann".into(),
            RuntimeValue::Bool(result.plan.filter_applied_after_ann),
        ),
        (
            "ordering_exact".into(),
            RuntimeValue::Bool(result.plan.ordering_exact),
        ),
        (
            "adapter_elapsed_micros".into(),
            RuntimeValue::Unsigned(result.elapsed_micros),
        ),
    ]);
    if let Some(index) = &result.plan.index_digest {
        attributes.insert("index_digest".into(), RuntimeValue::Digest(index.clone()));
    }
    if let Some(reason) = &result.plan.fallback_reason_digest {
        attributes.insert(
            "fallback_reason_digest".into(),
            RuntimeValue::Digest(reason.clone()),
        );
    }
    if let Some(candidates) = result.plan.candidates_examined {
        attributes.insert(
            "candidates_examined".into(),
            RuntimeValue::Unsigned(candidates),
        );
    }
    attributes
}

fn operator_error_class(rendered: &str) -> (&'static str, TraceOutcome) {
    if rendered.contains("stale")
        || rendered.contains("another project")
        || rendered.contains("scope differs")
        || rendered.contains("lacks")
        || rendered.contains("cannot enforce")
        || rendered.contains("model space")
        || rendered.contains("dimensions differ")
    {
        ("freshness_or_policy", TraceOutcome::Denied)
    } else {
        ("adapter_or_contract", TraceOutcome::Error)
    }
}

fn path_name(path: OperatorAccessPath) -> &'static str {
    match path {
        OperatorAccessPath::Exact => "exact",
        OperatorAccessPath::Hnsw => "hnsw",
        OperatorAccessPath::IvfFlat => "ivfflat",
    }
}

fn sync_operation_name(operation: OperatorSyncOperation) -> &'static str {
    match operation {
        OperatorSyncOperation::UpsertVector => "upsert_vector",
        OperatorSyncOperation::DeleteVector => "delete_vector",
    }
}

#[allow(clippy::too_many_arguments)]
fn fail_operator<E: Engine, T>(
    store: &E,
    child: Option<DurableTraceSpan>,
    root: DurableTraceSpan,
    stage: &str,
    class: &str,
    outcome: TraceOutcome,
    error: Box<dyn std::error::Error>,
) -> Result<T, Box<dyn std::error::Error>> {
    let rendered = error.to_string();
    let attributes = RuntimeProperties::from([
        ("failed_stage".into(), RuntimeValue::String(stage.into())),
        ("error_class".into(), RuntimeValue::String(class.into())),
        (
            "error_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(rendered.as_bytes())),
        ),
    ]);
    let mut trace_errors = Vec::new();
    if let Some(child) = child {
        if let Err(trace_error) = child.finish(store, outcome, Vec::new(), attributes.clone()) {
            trace_errors.push(format!("child finish: {trace_error}"));
        }
    }
    if let Err(trace_error) = root.finish(store, outcome, Vec::new(), attributes) {
        trace_errors.push(format!("root finish: {trace_error}"));
    }
    if trace_errors.is_empty() {
        Err(error)
    } else {
        Err(format!(
            "{rendered}; authoritative operator trace also failed ({})",
            trace_errors.join("; ")
        )
        .into())
    }
}
