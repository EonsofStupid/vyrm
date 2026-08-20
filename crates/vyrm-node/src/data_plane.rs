//! Durable projection/vector/embedding execution evidence.
//!
//! High-volume physical counters are sampled around one complete logical
//! operation. Raw vectors, embedding inputs, and filter values never enter the
//! trace contract; their caller-owned requests are represented by digests.

use crate::{active_reasoning_run, DurableTraceSpan, TraceIdentity};
use serde::Serialize;
use vyrm_core::{
    digest, DataTransaction, Millis, ProjectionFamily, ReadStamp, RuntimeCommit,
    RuntimeCommitOutcome, RuntimeMutation, RuntimeProperties, RuntimeSchemaRegistry,
    RuntimeTraceEvent, RuntimeValue, TraceDataClass, TraceDomain, TraceLink, TraceOutcome,
    RUNTIME_TRACE_EVENT_TYPE,
};
use vyrm_embed::{
    EmbeddingBackend, EmbeddingBackendDescriptor, EmbeddingCoordinator, EmbeddingJob,
    EmbeddingRequest, EmbeddingSourceReader, ExecutionTarget, PreparedEmbedding,
};
use vyrm_mx::Catalog;
use vyrm_store::Engine;
use vyrm_vector::{
    AccessPathKind, PreparedVectorSearch, ScoreMetric, SearchExecution, SearchMode, SearchRequest,
    VectorArtifact, VectorProjectionDescriptor, VectorQuery, VectorRuntime,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TracedVectorSearch {
    pub prepared: PreparedVectorSearch,
    pub execution: SearchExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TracedEmbeddingExecution {
    pub prepared: PreparedEmbedding,
    pub commit: RuntimeCommitOutcome,
}

pub fn execute_traced_embedding<E, S, B>(
    store: &E,
    job: &EmbeddingJob,
    source_reader: &mut S,
    backend: &mut B,
    actor: &str,
    at: Millis,
) -> Result<TracedEmbeddingExecution, Box<dyn std::error::Error>>
where
    E: Engine,
    S: EmbeddingSourceReader,
    B: EmbeddingBackend,
{
    job.validate()?;
    let verification = store.runtime_read_changes(&job.read, job.read.commit_cursor, 1)?;
    if verification.through_cursor != job.read.commit_cursor {
        return Err("embedding read stamp did not verify at its captured cursor".into());
    }
    let descriptor = backend.descriptor().clone();
    descriptor.validate()?;
    let job_digest = job.digest()?;
    let cursor = job.read.commit_cursor.to_be_bytes();
    let at_bytes = at.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        job.scope.as_str().as_bytes(),
        job_digest.as_bytes(),
        descriptor.model.model_digest.as_bytes(),
        &cursor,
        &at_bytes,
    ])?;
    let root_identity = identity.clone();
    let infer_identity = root_identity.child(&[b"embedding.infer"])?;
    let commit_identity = root_identity.child(&[b"embedding.commit"])?;
    let mut links = vec![TraceLink::Read {
        stamp: job.read.clone(),
    }];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let root = DurableTraceSpan::start(
        store,
        job.scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Embedding,
        "embedding.run",
        at,
        TraceDataClass::Control,
        links.clone(),
        embedding_job_attributes(job, &descriptor, &job_digest),
    )?;
    let mut traced_backend = DurableEmbeddingBackend {
        store,
        backend,
        scope: job.scope.clone(),
        actor,
        at: root.observed_at(),
        parent: root_identity.clone(),
        identity: infer_identity.clone(),
        read: job.read.clone(),
    };
    let prepared = match EmbeddingCoordinator::prepare(job, source_reader, &mut traced_backend) {
        Ok(prepared) => prepared,
        Err(error) => {
            let (class, outcome) = embedding_error_class(&error.to_string());
            return fail_data_plane(
                store,
                None,
                root,
                "preparation",
                class,
                outcome,
                error.into(),
            );
        }
    };

    let commit_span = match DurableTraceSpan::start(
        store,
        job.scope.clone(),
        actor,
        commit_identity.clone(),
        Some(root_identity.span_id.clone()),
        TraceDomain::Storage,
        "embedding.commit",
        root.observed_at(),
        TraceDataClass::Control,
        links,
        RuntimeProperties::from([
            (
                "job_digest".into(),
                RuntimeValue::Digest(job_digest.clone()),
            ),
            (
                "mutation_kind".into(),
                RuntimeValue::String("vector".into()),
            ),
            (
                "durability".into(),
                RuntimeValue::String("authoritative".into()),
            ),
        ]),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_data_plane(
                store,
                None,
                root,
                "commit",
                "trace_start",
                TraceOutcome::Error,
                error,
            )
        }
    };
    let rebased = match trace_only_rebase(store, &job.read) {
        Ok(read) => read,
        Err(error) => {
            return fail_data_plane(
                store,
                Some(commit_span),
                root,
                "commit_rebase",
                "concurrent_source_state",
                TraceOutcome::Denied,
                error,
            )
        }
    };
    let transaction = DataTransaction::new(
        rebased.clone(),
        RuntimeCommit {
            scope: job.scope.clone(),
            at,
            actor: actor.into(),
            expected_cursor: rebased.commit_cursor,
            mutations: vec![RuntimeMutation::Vector {
                vector: prepared.vector.clone(),
            }],
        },
    )?;
    let commit = match store.commit_data_transaction(&transaction) {
        Ok(commit) => commit,
        Err(error) => {
            let outcome = if matches!(error, vyrm_store::Error::RuntimeConflict { .. }) {
                TraceOutcome::Denied
            } else {
                TraceOutcome::Error
            };
            return fail_data_plane(
                store,
                Some(commit_span),
                root,
                "commit",
                if outcome == TraceOutcome::Denied {
                    "cursor_conflict"
                } else {
                    "storage"
                },
                outcome,
                error.into(),
            );
        }
    };
    if let Err(error) = commit_span.finish(
        store,
        TraceOutcome::Ok,
        Vec::new(),
        RuntimeProperties::from([
            (
                "commit_id".into(),
                RuntimeValue::Digest(commit.commit_id.clone()),
            ),
            (
                "first_cursor".into(),
                RuntimeValue::Unsigned(commit.first_cursor),
            ),
            (
                "last_cursor".into(),
                RuntimeValue::Unsigned(commit.last_cursor),
            ),
            (
                "rebased_over_trace_events".into(),
                RuntimeValue::Unsigned(
                    rebased.commit_cursor.saturating_sub(job.read.commit_cursor),
                ),
            ),
        ]),
    ) {
        return fail_data_plane(
            store,
            None,
            root,
            "commit",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    let root_attributes = RuntimeProperties::from([
        (
            "commit_id".into(),
            RuntimeValue::Digest(commit.commit_id.clone()),
        ),
        (
            "outcome_cursor".into(),
            RuntimeValue::Unsigned(commit.last_cursor),
        ),
        (
            "output_dimensions".into(),
            RuntimeValue::Unsigned(prepared.vector.value.dimensions() as u64),
        ),
        (
            "output_kind".into(),
            RuntimeValue::String(vector_value_kind(&prepared.vector.value).into()),
        ),
    ]);
    root.finish(store, TraceOutcome::Ok, Vec::new(), root_attributes)
        .map_err(|error| {
            format!("embedding committed but its authoritative root finish failed: {error}")
        })?;
    Ok(TracedEmbeddingExecution { prepared, commit })
}

struct DurableEmbeddingBackend<'a, E, B> {
    store: &'a E,
    backend: &'a mut B,
    scope: vyrm_core::ScopeId,
    actor: &'a str,
    at: Millis,
    parent: TraceIdentity,
    identity: TraceIdentity,
    read: ReadStamp,
}

impl<E: Engine, B: EmbeddingBackend> EmbeddingBackend for DurableEmbeddingBackend<'_, E, B> {
    fn descriptor(&self) -> &EmbeddingBackendDescriptor {
        self.backend.descriptor()
    }

    fn embed(&mut self, request: &EmbeddingRequest) -> vyrm_core::Result<vyrm_core::VectorValue> {
        let descriptor = self.backend.descriptor().clone();
        let span = DurableTraceSpan::start(
            self.store,
            self.scope.clone(),
            self.actor,
            self.identity.clone(),
            Some(self.parent.span_id.clone()),
            TraceDomain::Embedding,
            "embedding.infer",
            self.at,
            TraceDataClass::Control,
            vec![TraceLink::Read {
                stamp: self.read.clone(),
            }],
            RuntimeProperties::from([
                (
                    "job_digest".into(),
                    RuntimeValue::Digest(request.job_digest.clone()),
                ),
                (
                    "source_digest".into(),
                    RuntimeValue::Digest(request.source_digest.clone()),
                ),
                (
                    "input_bytes".into(),
                    RuntimeValue::Unsigned(request.bytes.len() as u64),
                ),
                (
                    "media_type_digest".into(),
                    RuntimeValue::Digest(digest::sha256_hex(request.media_type.as_bytes())),
                ),
                (
                    "model_space_digest".into(),
                    RuntimeValue::Digest(descriptor.model.model_digest.clone()),
                ),
                (
                    "execution_target".into(),
                    RuntimeValue::String(execution_target_name(&descriptor.execution)),
                ),
            ]),
        )
        .map_err(core_trace_error)?;
        match self.backend.embed(request) {
            Ok(value) => {
                span.finish(
                    self.store,
                    TraceOutcome::Ok,
                    Vec::new(),
                    RuntimeProperties::from([
                        (
                            "output_dimensions".into(),
                            RuntimeValue::Unsigned(value.dimensions() as u64),
                        ),
                        (
                            "output_kind".into(),
                            RuntimeValue::String(vector_value_kind(&value).into()),
                        ),
                    ]),
                )
                .map_err(core_trace_error)?;
                Ok(value)
            }
            Err(error) => {
                let rendered = error.to_string();
                span.finish(
                    self.store,
                    TraceOutcome::Error,
                    Vec::new(),
                    error_attributes("inference", "backend", &rendered),
                )
                .map_err(core_trace_error)?;
                Err(error)
            }
        }
    }
}

pub fn execute_traced_vector_search<E: Engine>(
    store: &E,
    runtime: &VectorRuntime,
    request: &SearchRequest,
    ef_search: usize,
    actor: &str,
    at: Millis,
) -> Result<TracedVectorSearch, Box<dyn std::error::Error>> {
    request.validate()?;
    let verification = store.runtime_read_changes(&request.read, request.read.commit_cursor, 1)?;
    if verification.through_cursor != request.read.commit_cursor {
        return Err("vector read stamp did not verify at its captured cursor".into());
    }
    let required_source_cursor =
        required_projection_cursor(store, &request.read, ProjectionFamily::Vector)?;
    let request_digest = request.digest()?;
    let cursor = request.read.commit_cursor.to_be_bytes();
    let at_bytes = at.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        request.scope.as_str().as_bytes(),
        request_digest.as_bytes(),
        &cursor,
        &at_bytes,
    ])?;
    let root_identity = identity.clone();
    let mut links = vec![TraceLink::Read {
        stamp: request.read.clone(),
    }];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let root = DurableTraceSpan::start(
        store,
        request.scope.clone(),
        actor,
        identity,
        None,
        TraceDomain::Search,
        "vector.search",
        at,
        TraceDataClass::Control,
        links.clone(),
        vector_request_attributes(request, &request_digest, required_source_cursor, ef_search),
    )?;

    let planning_identity = root_identity.child(&[b"vector.plan"])?;
    let planning = match DurableTraceSpan::start(
        store,
        request.scope.clone(),
        actor,
        planning_identity,
        Some(root_identity.span_id.clone()),
        TraceDomain::Planning,
        "vector.plan",
        root.observed_at(),
        TraceDataClass::Control,
        links.clone(),
        RuntimeProperties::new(),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_data_plane(
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
    let prepared = match runtime.prepare_search_at(request, required_source_cursor, ef_search) {
        Ok(prepared) => prepared,
        Err(error) => {
            let (class, outcome) = vector_error_class(&error.to_string());
            return fail_data_plane(
                store,
                Some(planning),
                root,
                "planning",
                class,
                outcome,
                error.into(),
            );
        }
    };
    let plan_links = vec![
        TraceLink::Plan {
            plan_digest: prepared.plan_digest().into(),
        },
        TraceLink::Projection {
            stamp: prepared.selected_stamp().clone(),
        },
    ];
    if let Err(error) = planning.finish(
        store,
        TraceOutcome::Ok,
        plan_links.clone(),
        vector_plan_attributes(&prepared),
    ) {
        return fail_data_plane(
            store,
            None,
            root,
            "planning",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }

    let execution_identity = root_identity.child(&[b"vector.execute"])?;
    let mut execution_links = links;
    execution_links.extend(plan_links.clone());
    let execution_span = match DurableTraceSpan::start(
        store,
        request.scope.clone(),
        actor,
        execution_identity,
        Some(root_identity.span_id),
        TraceDomain::Search,
        "vector.execute",
        root.observed_at(),
        TraceDataClass::Control,
        execution_links,
        RuntimeProperties::new(),
    ) {
        Ok(span) => span,
        Err(error) => {
            return fail_data_plane(
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
    let execution = match runtime.execute_search(request, &prepared) {
        Ok(execution) => execution,
        Err(error) => {
            let (class, outcome) = vector_error_class(&error.to_string());
            return fail_data_plane(
                store,
                Some(execution_span),
                root,
                "execution",
                class,
                outcome,
                error.into(),
            );
        }
    };
    let execution_attributes = vector_execution_attributes(request, &prepared, &execution);
    if let Err(error) = execution_span.finish(
        store,
        TraceOutcome::Ok,
        Vec::new(),
        execution_attributes.clone(),
    ) {
        return fail_data_plane(
            store,
            None,
            root,
            "execution",
            "trace_finish",
            TraceOutcome::Error,
            error,
        );
    }
    root.finish(store, TraceOutcome::Ok, plan_links, execution_attributes)
        .map_err(|error| {
            format!("vector search completed but its authoritative root finish failed: {error}")
        })?;
    Ok(TracedVectorSearch {
        prepared,
        execution,
    })
}

pub fn publish_traced_vector_artifact<E: Engine>(
    store: &E,
    runtime: &mut VectorRuntime,
    expected_catalog_revision: u64,
    artifact: VectorArtifact,
    actor: &str,
    at: Millis,
) -> Result<u64, Box<dyn std::error::Error>> {
    let descriptor = artifact.descriptor();
    descriptor.validate()?;
    let scope = descriptor.scope().clone();
    let stamp = descriptor.stamp().clone();
    let read = store.runtime_read_stamp(&scope)?;
    let revision = expected_catalog_revision.to_be_bytes();
    let generation = stamp.generation.to_be_bytes();
    let identity = TraceIdentity::derive(&[
        scope.as_str().as_bytes(),
        stamp.id.as_str().as_bytes(),
        &generation,
        &revision,
    ])?;
    let mut links = vec![TraceLink::Read { stamp: read }];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let span = DurableTraceSpan::start(
        store,
        scope,
        actor,
        identity,
        None,
        TraceDomain::Projection,
        "vector.projection.publish",
        at,
        TraceDataClass::Control,
        links,
        projection_attributes(&descriptor, expected_catalog_revision),
    )?;
    match runtime.publish(expected_catalog_revision, artifact) {
        Ok(catalog_revision) => {
            span.finish(
                store,
                TraceOutcome::Ok,
                vec![TraceLink::Projection { stamp }],
                RuntimeProperties::from([(
                    "catalog_revision".into(),
                    RuntimeValue::Unsigned(catalog_revision),
                )]),
            )?;
            Ok(catalog_revision)
        }
        Err(error) => {
            let rendered = error.to_string();
            let (class, outcome) = vector_error_class(&rendered);
            span.finish(
                store,
                outcome,
                Vec::new(),
                error_attributes("publication", class, &rendered),
            )?;
            Err(error.into())
        }
    }
}

fn vector_request_attributes(
    request: &SearchRequest,
    request_digest: &str,
    required_source_cursor: u64,
    ef_search: usize,
) -> RuntimeProperties {
    let mut attributes = RuntimeProperties::from([
        (
            "request_digest".into(),
            RuntimeValue::Digest(request_digest.into()),
        ),
        (
            "query_kind".into(),
            RuntimeValue::String(vector_query_kind(&request.query).into()),
        ),
        (
            "dimensions".into(),
            RuntimeValue::Unsigned(request.query.dimensions() as u64),
        ),
        (
            "metric".into(),
            RuntimeValue::String(metric_name(request.metric).into()),
        ),
        (
            "mode".into(),
            RuntimeValue::String(search_mode_name(request.mode).into()),
        ),
        ("top_k".into(), RuntimeValue::Unsigned(request.top_k as u64)),
        (
            "required_source_cursor".into(),
            RuntimeValue::Unsigned(required_source_cursor),
        ),
        ("ef_search".into(), RuntimeValue::Unsigned(ef_search as u64)),
        (
            "filter_present".into(),
            RuntimeValue::Bool(request.filter.is_some()),
        ),
        (
            "filter_property_count".into(),
            RuntimeValue::Unsigned(
                request
                    .filter
                    .as_ref()
                    .map_or(0, |filter| filter.referenced_properties().len())
                    as u64,
            ),
        ),
    ]);
    if let Some(model) = &request.embedding_model {
        attributes.insert(
            "model_space_digest".into(),
            RuntimeValue::Digest(model.digest.clone()),
        );
    }
    attributes
}

fn required_projection_cursor<E: Engine>(
    store: &E,
    read: &ReadStamp,
    family: ProjectionFamily,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut after = 0;
    let mut required = 0;
    loop {
        let page = store.runtime_outbox_since(after, 4_096)?;
        if page.is_empty() {
            return Ok(required);
        }
        let mut advanced = false;
        for work in page {
            if work.source_cursor > read.commit_cursor {
                return Ok(required);
            }
            after = after.max(work.source_cursor);
            advanced = true;
            if work.scope == read.scope && work.family == family {
                required = required.max(work.source_cursor);
            }
        }
        if !advanced {
            return Err("projection outbox pagination did not advance".into());
        }
    }
}

/// Rebases an exact write read stamp only across mutations proven to be
/// observability-only. Any record/relation/vector/claim/event other than the
/// canonical trace event makes the original data transaction stale.
fn trace_only_rebase<E: Engine>(
    store: &E,
    original: &ReadStamp,
) -> Result<ReadStamp, Box<dyn std::error::Error>> {
    let rebased = store.runtime_read_stamp(&original.scope)?;
    if rebased.commit_cursor < original.commit_cursor {
        return Err("embedding rebase moved behind its original read".into());
    }
    if rebased.commit_cursor == original.commit_cursor {
        return Ok(rebased);
    }
    let catalog = Catalog::capture_at(store, original.clone())?;
    let prior_schema = catalog.schema_at(original.commit_cursor).cloned();
    let mut expected_schema = prior_schema
        .clone()
        .unwrap_or_else(|| RuntimeSchemaRegistry::empty(1, "install runtime trace contract"));
    let schema_install = RuntimeTraceEvent::register_schema(&mut expected_schema)?;
    if schema_install {
        if let Some(prior_schema) = &prior_schema {
            expected_schema.revision = prior_schema
                .revision
                .checked_add(1)
                .ok_or("runtime schema revision overflow during trace-only rebase")?;
            expected_schema.migration = "install canonical runtime trace contract".into();
        }
    }
    let gap = rebased
        .commit_cursor
        .checked_sub(original.commit_cursor)
        .ok_or("embedding rebase cursor underflow")?;
    let limit = usize::try_from(gap)
        .map_err(|_| "embedding trace-only rebase exceeds this platform's address space")?;
    let page = store.runtime_read_changes(&rebased, original.commit_cursor, limit)?;
    if page.through_cursor != rebased.commit_cursor || page.changes.len() != limit {
        return Err("embedding trace-only rebase did not replay the complete cursor gap".into());
    }
    let mut schema_seen = false;
    for change in page.changes {
        match change.mutation {
            RuntimeMutation::Schema { registry }
                if schema_install && !schema_seen && registry == expected_schema =>
            {
                schema_seen = true;
            }
            RuntimeMutation::Event { event } if event.kind.as_str() == RUNTIME_TRACE_EVENT_TYPE => {
                let trace_id = event.properties.get("trace_id");
                let span_id = event.properties.get("span_id");
                if !matches!(trace_id, Some(RuntimeValue::String(value)) if value.len() == 32)
                    || !matches!(span_id, Some(RuntimeValue::String(value)) if value.len() == 16)
                {
                    return Err("embedding rebase encountered a malformed trace event".into());
                }
            }
            _ => {
                return Err(format!(
                    "embedding source state changed at runtime cursor {}",
                    change.cursor
                )
                .into())
            }
        }
    }
    if schema_install && !schema_seen {
        return Err("embedding rebase expected the canonical trace schema installation".into());
    }
    Ok(rebased)
}

fn embedding_job_attributes(
    job: &EmbeddingJob,
    descriptor: &EmbeddingBackendDescriptor,
    job_digest: &str,
) -> RuntimeProperties {
    RuntimeProperties::from([
        ("job_digest".into(), RuntimeValue::Digest(job_digest.into())),
        (
            "source_digest".into(),
            RuntimeValue::Digest(job.expected_source_digest.clone()),
        ),
        (
            "model_space_digest".into(),
            RuntimeValue::Digest(descriptor.model.model_digest.clone()),
        ),
        (
            "requested_dimensions".into(),
            RuntimeValue::Unsigned(descriptor.model.dimensions as u64),
        ),
        (
            "backend_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(descriptor.id.as_bytes())),
        ),
        (
            "execution_target".into(),
            RuntimeValue::String(execution_target_name(&descriptor.execution)),
        ),
        (
            "network_required".into(),
            RuntimeValue::Bool(descriptor.network == vyrm_embed::NetworkRequirement::Required),
        ),
        (
            "deterministic".into(),
            RuntimeValue::Bool(descriptor.deterministic),
        ),
        (
            "target_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(
                &serde_json::to_vec(&job.target).unwrap_or_default(),
            )),
        ),
    ])
}

fn embedding_error_class(rendered: &str) -> (&'static str, TraceOutcome) {
    if rendered.contains("changed during inference")
        || rendered.contains("differs from the job")
        || rendered.contains("denies the network")
        || rendered.contains("requested model")
    {
        ("freshness_or_policy", TraceOutcome::Denied)
    } else if rendered.contains("backend") || rendered.contains("shape") {
        ("backend", TraceOutcome::Error)
    } else {
        ("contract", TraceOutcome::Error)
    }
}

fn execution_target_name(target: &ExecutionTarget) -> String {
    match target {
        ExecutionTarget::Cpu => "cpu".into(),
        ExecutionTarget::Gpu { platform, device } => format!("gpu:{platform}:{device}"),
        ExecutionTarget::Remote { provider } => format!("remote:{provider}"),
    }
}

fn vector_value_kind(value: &vyrm_core::VectorValue) -> &'static str {
    match value {
        vyrm_core::VectorValue::Dense { .. } => "dense",
        vyrm_core::VectorValue::Sparse { .. } => "sparse",
        vyrm_core::VectorValue::MultiDense { .. } => "multi_dense",
    }
}

fn core_trace_error(error: Box<dyn std::error::Error>) -> vyrm_core::Error {
    vyrm_core::Error::InvalidRuntime {
        reason: format!("authoritative embedding trace failed: {error}"),
    }
}

fn vector_plan_attributes(prepared: &PreparedVectorSearch) -> RuntimeProperties {
    let plan = prepared.plan();
    RuntimeProperties::from([
        (
            "plan_digest".into(),
            RuntimeValue::Digest(prepared.plan_digest().into()),
        ),
        (
            "catalog_revision".into(),
            RuntimeValue::Unsigned(prepared.catalog_revision()),
        ),
        (
            "selected_path".into(),
            RuntimeValue::String(access_path_name(plan.selected.kind).into()),
        ),
        (
            "selected_projection".into(),
            RuntimeValue::String(plan.selected.id.to_string()),
        ),
        (
            "source_cursor".into(),
            RuntimeValue::Unsigned(plan.selected.source_cursor),
        ),
        (
            "required_source_cursor".into(),
            RuntimeValue::Unsigned(plan.required_source_cursor),
        ),
        (
            "estimated_candidates".into(),
            RuntimeValue::Unsigned(plan.selected.estimated_candidates),
        ),
        (
            "estimated_cost".into(),
            RuntimeValue::Unsigned(plan.selected.estimated_cost),
        ),
        (
            "exact_rerank".into(),
            RuntimeValue::Unsigned(plan.selected.exact_rerank as u64),
        ),
        (
            "rejected_path_count".into(),
            RuntimeValue::Unsigned(plan.rejected.len() as u64),
        ),
        (
            "rejected_paths_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(
                &serde_json::to_vec(&plan.rejected).unwrap_or_default(),
            )),
        ),
        (
            "approximation_requested".into(),
            RuntimeValue::Bool(plan.approximation_requested),
        ),
    ])
}

fn vector_execution_attributes(
    request: &SearchRequest,
    prepared: &PreparedVectorSearch,
    execution: &SearchExecution,
) -> RuntimeProperties {
    let selected = prepared.plan().selected.kind;
    RuntimeProperties::from([
        (
            "selected_path".into(),
            RuntimeValue::String(access_path_name(selected).into()),
        ),
        (
            "hit_count".into(),
            RuntimeValue::Unsigned(execution.hits.len() as u64),
        ),
        (
            "exact_rerank".into(),
            RuntimeValue::Unsigned(prepared.plan().selected.exact_rerank as u64),
        ),
        (
            "fallback_to_exact".into(),
            RuntimeValue::Bool(
                prepared.plan().approximation_requested && selected != AccessPathKind::Hnsw,
            ),
        ),
        (
            "filter_selectivity".into(),
            RuntimeValue::String(
                if request.filter.is_some() {
                    "not_measured"
                } else {
                    "not_applicable"
                }
                .into(),
            ),
        ),
    ])
}

fn projection_attributes(
    descriptor: &VectorProjectionDescriptor,
    expected_catalog_revision: u64,
) -> RuntimeProperties {
    let stamp = descriptor.stamp();
    RuntimeProperties::from([
        (
            "projection_id".into(),
            RuntimeValue::String(stamp.id.to_string()),
        ),
        (
            "projection_kind".into(),
            RuntimeValue::String(
                match descriptor {
                    VectorProjectionDescriptor::ExactSegment { .. } => "exact_segment",
                    VectorProjectionDescriptor::Hnsw { .. } => "hnsw",
                }
                .into(),
            ),
        ),
        (
            "generation".into(),
            RuntimeValue::Unsigned(stamp.generation),
        ),
        (
            "source_cursor".into(),
            RuntimeValue::Unsigned(stamp.source_cursor),
        ),
        (
            "config_digest".into(),
            RuntimeValue::Digest(stamp.config_digest.clone()),
        ),
        (
            "artifact_digest".into(),
            RuntimeValue::Digest(stamp.artifact_digest.clone()),
        ),
        (
            "expected_catalog_revision".into(),
            RuntimeValue::Unsigned(expected_catalog_revision),
        ),
    ])
}

fn vector_error_class(rendered: &str) -> (&'static str, TraceOutcome) {
    if rendered.contains("stale")
        || rendered.contains("no vector access path")
        || rendered.contains("catalog conflict")
        || rendered.contains("quarantined")
    {
        ("freshness_or_policy", TraceOutcome::Denied)
    } else if rendered.contains("absent") || rendered.contains("differs") {
        ("integrity", TraceOutcome::Error)
    } else {
        ("contract", TraceOutcome::Error)
    }
}

fn access_path_name(kind: AccessPathKind) -> &'static str {
    match kind {
        AccessPathKind::ExactScan => "exact_scan",
        AccessPathKind::ExactSegment => "exact_segment",
        AccessPathKind::Hnsw => "hnsw",
    }
}

fn metric_name(metric: ScoreMetric) -> &'static str {
    match metric {
        ScoreMetric::Cosine => "cosine",
        ScoreMetric::Dot => "dot",
        ScoreMetric::Euclidean => "euclidean",
        ScoreMetric::Manhattan => "manhattan",
    }
}

fn search_mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Exact => "exact",
        SearchMode::AllowApproximate { .. } => "allow_approximate",
        SearchMode::RequireApproximate { .. } => "require_approximate",
    }
}

fn vector_query_kind(query: &VectorQuery) -> &'static str {
    match query {
        VectorQuery::Dense { .. } => "dense",
        VectorQuery::Sparse { .. } => "sparse",
        VectorQuery::MultiDense { .. } => "multi_dense",
    }
}

fn error_attributes(stage: &str, class: &str, rendered: &str) -> RuntimeProperties {
    RuntimeProperties::from([
        ("failed_stage".into(), RuntimeValue::String(stage.into())),
        ("error_class".into(), RuntimeValue::String(class.into())),
        (
            "error_digest".into(),
            RuntimeValue::Digest(digest::sha256_hex(rendered.as_bytes())),
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
fn fail_data_plane<E: Engine, T>(
    store: &E,
    child: Option<DurableTraceSpan>,
    root: DurableTraceSpan,
    stage: &str,
    class: &str,
    outcome: TraceOutcome,
    error: Box<dyn std::error::Error>,
) -> Result<T, Box<dyn std::error::Error>> {
    let rendered = error.to_string();
    let mut trace_errors = Vec::new();
    if let Some(child) = child {
        if let Err(trace_error) = child.finish(
            store,
            outcome,
            Vec::new(),
            error_attributes(stage, class, &rendered),
        ) {
            trace_errors.push(format!("child finish: {trace_error}"));
        }
    }
    if let Err(trace_error) = root.finish(
        store,
        outcome,
        Vec::new(),
        error_attributes(stage, class, &rendered),
    ) {
        trace_errors.push(format!("root finish: {trace_error}"));
    }
    if trace_errors.is_empty() {
        Err(error)
    } else {
        Err(format!(
            "{rendered}; authoritative data-plane trace also failed ({})",
            trace_errors.join("; ")
        )
        .into())
    }
}
