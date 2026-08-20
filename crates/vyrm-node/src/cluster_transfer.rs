//! Durable causal wrapper for cross-node immutable artifact hydration.

use crate::{active_reasoning_run, DurableTraceSpan, TraceIdentity};
use std::sync::Arc;
use vyrm_cluster::{
    artifact_transfer_trace_event, transfer_artifacts, ArtifactTransferManifest,
    ArtifactTransferObservation, ArtifactTransferObserver, ArtifactTransferReceipt, ClusterError,
};
use vyrm_core::{
    Millis, RuntimeProperties, RuntimeValue, TraceDataClass, TraceDomain, TraceLink, TraceOutcome,
};
use vyrm_store::{Engine, ImmutableObjectStore};

/// Fail-closed adapter from transport observations into Vyrm's authoritative
/// project trace log. Clustered deployments should supply an `Engine` whose
/// writes already cross their consensus boundary; this adapter never claims a
/// direct local write is a replicated commit.
#[derive(Clone)]
pub struct DurableArtifactTransferObserver<E> {
    store: Arc<E>,
    actor: String,
}

impl<E> DurableArtifactTransferObserver<E>
where
    E: Engine + Send + Sync + 'static,
{
    pub fn new(store: Arc<E>, actor: impl Into<String>) -> Result<Self, ClusterError> {
        let actor = actor.into();
        if actor.trim().is_empty() {
            return Err(ClusterError::Invalid(
                "artifact trace actor must not be empty".into(),
            ));
        }
        Ok(Self { store, actor })
    }

    pub fn observe_sync(
        &self,
        observation: ArtifactTransferObservation,
    ) -> vyrm_cluster::Result<()> {
        record_artifact_transfer_observation(self.store.as_ref(), &self.actor, observation)
    }
}

impl<E> ArtifactTransferObserver for DurableArtifactTransferObserver<E>
where
    E: Engine + Send + Sync + 'static,
{
    fn observe(
        &self,
        observation: ArtifactTransferObservation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = vyrm_cluster::Result<()>> + Send + '_>>
    {
        let result = self.observe_sync(observation);
        Box::pin(std::future::ready(result))
    }
}

pub fn record_artifact_transfer_observation<E: Engine>(
    store: &E,
    actor: &str,
    observation: ArtifactTransferObservation,
) -> vyrm_cluster::Result<()> {
    let event = artifact_transfer_trace_event(&observation)?;
    crate::record_runtime_trace(store, &observation.scope, actor, event)
        .map(|_| ())
        .map_err(cluster_trace_error)
}

fn cluster_trace_error(error: impl std::fmt::Display) -> ClusterError {
    ClusterError::Unavailable(format!("artifact transfer trace: {error}"))
}

pub fn execute_traced_artifact_transfer<E, S, T>(
    store: &E,
    source: &S,
    target: &T,
    manifest: &ArtifactTransferManifest,
    actor: &str,
    at: Millis,
) -> Result<ArtifactTransferReceipt, Box<dyn std::error::Error>>
where
    E: Engine,
    S: ImmutableObjectStore,
    T: ImmutableObjectStore,
{
    manifest.validate()?;
    let identity = TraceIdentity::derive(&[
        manifest.scope.as_str().as_bytes(),
        manifest.manifest_digest.as_bytes(),
        manifest.plan.source.as_str().as_bytes(),
        manifest.plan.target.as_str().as_bytes(),
    ])?;
    let storage_identity = identity.child(&[b"object.replicate"])?;
    let mut links = vec![TraceLink::Read {
        stamp: manifest.read.clone(),
    }];
    if let Ok(Some(run)) = active_reasoning_run(store) {
        links.push(TraceLink::ReasoningRun {
            run_id: run.id().to_owned(),
        });
    }
    let root = DurableTraceSpan::start(
        store,
        manifest.scope.clone(),
        actor,
        identity.clone(),
        None,
        TraceDomain::Cluster,
        "cluster.artifact_transfer",
        at,
        TraceDataClass::Control,
        links.clone(),
        manifest_attributes(manifest),
    )?;
    let storage = match DurableTraceSpan::start(
        store,
        manifest.scope.clone(),
        actor,
        storage_identity,
        Some(identity.span_id),
        TraceDomain::Storage,
        "object.replicate",
        root.observed_at(),
        TraceDataClass::Control,
        links,
        RuntimeProperties::from([(
            "manifest_digest".into(),
            RuntimeValue::Digest(manifest.manifest_digest.clone()),
        )]),
    ) {
        Ok(span) => span,
        Err(error) => {
            return finish_transfer_error(store, None, root, "storage_trace_start", error)
        }
    };
    let receipt = match transfer_artifacts(source, target, manifest, at) {
        Ok(receipt) => receipt,
        Err(error) => {
            return finish_transfer_error(
                store,
                Some(storage),
                root,
                "object_transfer",
                error.into(),
            )
        }
    };
    let evidence = receipt_attributes(&receipt);
    if let Err(error) = storage.finish(store, TraceOutcome::Ok, Vec::new(), evidence.clone()) {
        return finish_transfer_error(store, None, root, "storage_trace_finish", error);
    }
    root.finish(store, TraceOutcome::Ok, Vec::new(), evidence)?;
    Ok(receipt)
}

fn manifest_attributes(manifest: &ArtifactTransferManifest) -> RuntimeProperties {
    RuntimeProperties::from([
        (
            "manifest_digest".into(),
            RuntimeValue::Digest(manifest.manifest_digest.clone()),
        ),
        (
            "source_node".into(),
            RuntimeValue::String(manifest.plan.source.to_string()),
        ),
        (
            "target_node".into(),
            RuntimeValue::String(manifest.plan.target.to_string()),
        ),
        (
            "shard".into(),
            RuntimeValue::Unsigned(manifest.plan.shard.0),
        ),
        (
            "placement_epoch".into(),
            RuntimeValue::Unsigned(manifest.plan.placement_epoch),
        ),
        (
            "snapshot_term".into(),
            RuntimeValue::Unsigned(manifest.plan.grounded_snapshot.term),
        ),
        (
            "snapshot_commit_index".into(),
            RuntimeValue::Unsigned(manifest.plan.grounded_snapshot.commit_index),
        ),
        (
            "snapshot_state_digest".into(),
            RuntimeValue::Digest(manifest.plan.grounded_snapshot.state_digest.clone()),
        ),
        (
            "object_references".into(),
            RuntimeValue::Unsigned(manifest.objects.len() as u64),
        ),
        (
            "distinct_objects".into(),
            RuntimeValue::Unsigned(manifest.plan.artifact_digests.len() as u64),
        ),
    ])
}

fn receipt_attributes(receipt: &ArtifactTransferReceipt) -> RuntimeProperties {
    RuntimeProperties::from([
        (
            "receipt_digest".into(),
            RuntimeValue::Digest(receipt.receipt_digest.clone()),
        ),
        (
            "transferred_objects".into(),
            RuntimeValue::Unsigned(receipt.transferred_objects),
        ),
        (
            "transferred_bytes".into(),
            RuntimeValue::Unsigned(receipt.transferred_bytes),
        ),
        (
            "verified_references".into(),
            RuntimeValue::Unsigned(receipt.objects.len() as u64),
        ),
    ])
}

fn finish_transfer_error<E: Engine, T>(
    store: &E,
    storage: Option<DurableTraceSpan>,
    root: DurableTraceSpan,
    stage: &str,
    error: Box<dyn std::error::Error>,
) -> Result<T, Box<dyn std::error::Error>> {
    let rendered = error.to_string();
    let attributes = RuntimeProperties::from([
        ("failed_stage".into(), RuntimeValue::String(stage.into())),
        (
            "error_digest".into(),
            RuntimeValue::Digest(vyrm_core::digest::sha256_hex(rendered.as_bytes())),
        ),
    ]);
    let mut trace_errors = Vec::new();
    if let Some(storage) = storage {
        if let Err(trace_error) =
            storage.finish(store, TraceOutcome::Error, Vec::new(), attributes.clone())
        {
            trace_errors.push(format!("storage finish: {trace_error}"));
        }
    }
    if let Err(trace_error) = root.finish(store, TraceOutcome::Error, Vec::new(), attributes) {
        trace_errors.push(format!("root finish: {trace_error}"));
    }
    if trace_errors.is_empty() {
        Err(error)
    } else {
        Err(format!(
            "{rendered}; authoritative trace failures: {}",
            trace_errors.join("; ")
        )
        .into())
    }
}
