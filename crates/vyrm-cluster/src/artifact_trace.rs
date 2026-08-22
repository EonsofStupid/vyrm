//! Canonical privacy-bounded trace projection for artifact transport events.

use crate::{ArtifactTransferObservation, ArtifactTransferObservationPhase, ClusterError, Result};
use vyrm_core::{
    digest::sha256_hex, RuntimeProperties, RuntimeTraceEvent, RuntimeValue, SnapshotId, SpanId,
    TraceDataClass, TraceDomain, TraceId, TraceLink, TraceOutcome,
};

/// Converts one validated transport observation into the portable runtime
/// trace event committed by both local and consensus-backed observers.
pub fn artifact_transfer_trace_event(
    observation: &ArtifactTransferObservation,
) -> Result<RuntimeTraceEvent> {
    observation.validate()?;
    let attempt = observation.attempt.to_be_bytes();
    let parts = [
        observation.scope.as_str().as_bytes(),
        observation.manifest_digest.as_bytes(),
        observation.source.as_str().as_bytes(),
        observation.target.as_str().as_bytes(),
        attempt.as_slice(),
    ];
    let trace_id = trace_id(&parts)?;
    let span_id = span_id(&parts)?;
    let links = vec![
        TraceLink::Read {
            stamp: observation.read.clone(),
        },
        TraceLink::Snapshot {
            snapshot_id: SnapshotId::new(format!(
                "raft:{}:{}:{}:{}",
                observation.shard.0,
                observation.grounded_snapshot.term,
                observation.grounded_snapshot.commit_index,
                observation.grounded_snapshot.state_digest
            ))
            .map_err(core_error)?,
            cursor: observation.read.commit_cursor,
        },
    ];
    let mut attributes = RuntimeProperties::from([
        (
            "manifest_digest".into(),
            RuntimeValue::Digest(observation.manifest_digest.clone()),
        ),
        (
            "source_node".into(),
            RuntimeValue::String(observation.source.to_string()),
        ),
        (
            "target_node".into(),
            RuntimeValue::String(observation.target.to_string()),
        ),
        (
            "attempt".into(),
            RuntimeValue::Unsigned(observation.attempt),
        ),
        ("shard".into(), RuntimeValue::Unsigned(observation.shard.0)),
        (
            "placement_epoch".into(),
            RuntimeValue::Unsigned(observation.placement_epoch),
        ),
        (
            "snapshot_term".into(),
            RuntimeValue::Unsigned(observation.grounded_snapshot.term),
        ),
        (
            "snapshot_commit_index".into(),
            RuntimeValue::Unsigned(observation.grounded_snapshot.commit_index),
        ),
        (
            "snapshot_state_digest".into(),
            RuntimeValue::Digest(observation.grounded_snapshot.state_digest.clone()),
        ),
        (
            "object_references".into(),
            RuntimeValue::Unsigned(observation.object_references),
        ),
        (
            "distinct_objects".into(),
            RuntimeValue::Unsigned(observation.distinct_objects),
        ),
    ]);
    for (name, value) in [
        observation
            .object_digest
            .clone()
            .map(|value| ("object_digest", RuntimeValue::Digest(value))),
        observation
            .next_offset
            .map(|value| ("next_offset", RuntimeValue::Unsigned(value))),
        observation
            .expected_length
            .map(|value| ("expected_length", RuntimeValue::Unsigned(value))),
        observation
            .receipt_digest
            .clone()
            .map(|value| ("receipt_digest", RuntimeValue::Digest(value))),
        observation
            .error_digest
            .clone()
            .map(|value| ("error_digest", RuntimeValue::Digest(value))),
    ]
    .into_iter()
    .flatten()
    {
        attributes.insert(name.into(), value);
    }
    if observation.phase == ArtifactTransferObservationPhase::Completed {
        attributes.insert(
            "transferred_objects".into(),
            RuntimeValue::Unsigned(observation.transferred_objects),
        );
        attributes.insert(
            "transferred_bytes".into(),
            RuntimeValue::Unsigned(observation.transferred_bytes),
        );
    }

    match observation.phase {
        ArtifactTransferObservationPhase::Prepared => RuntimeTraceEvent::start(
            trace_id,
            span_id,
            None,
            TraceDomain::Cluster,
            "cluster.artifact_transfer",
            observation.at,
            TraceDataClass::Control,
            links,
            attributes,
        ),
        ArtifactTransferObservationPhase::ChunkAccepted => {
            let offset = observation.next_offset.unwrap_or_default().to_be_bytes();
            let child = child_span_id(
                &trace_id,
                &span_id,
                &[
                    b"cluster.artifact_chunk",
                    observation
                        .object_digest
                        .as_deref()
                        .unwrap_or_default()
                        .as_bytes(),
                    offset.as_slice(),
                ],
            )?;
            RuntimeTraceEvent::annotation(
                trace_id,
                child,
                Some(span_id),
                TraceDomain::Storage,
                "cluster.artifact_chunk",
                observation.at,
                TraceOutcome::Ok,
                TraceDataClass::Control,
                links,
                attributes,
            )
        }
        ArtifactTransferObservationPhase::Completed => RuntimeTraceEvent::finish(
            trace_id,
            span_id,
            None,
            TraceDomain::Cluster,
            "cluster.artifact_transfer",
            observation.at,
            observation.duration_micros.unwrap_or_default(),
            TraceOutcome::Ok,
            TraceDataClass::Control,
            links,
            attributes,
        ),
        ArtifactTransferObservationPhase::Failed => RuntimeTraceEvent::finish(
            trace_id,
            span_id,
            None,
            TraceDomain::Cluster,
            "cluster.artifact_transfer",
            observation.at,
            observation.duration_micros.unwrap_or_default(),
            TraceOutcome::Error,
            TraceDataClass::Control,
            links,
            attributes,
        ),
    }
    .map_err(core_error)
}

fn trace_id(parts: &[&[u8]]) -> Result<TraceId> {
    let digest = identity_digest(b"vyrm-runtime-trace-id-v1\0", parts);
    TraceId::new(&digest[..32]).map_err(core_error)
}

fn span_id(parts: &[&[u8]]) -> Result<SpanId> {
    let digest = identity_digest(b"vyrm-runtime-span-id-v1\0", parts);
    SpanId::new(&digest[..16]).map_err(core_error)
}

fn child_span_id(trace: &TraceId, parent: &SpanId, parts: &[&[u8]]) -> Result<SpanId> {
    let mut coordinates = Vec::with_capacity(parts.len() + 2);
    coordinates.push(trace.as_str().as_bytes());
    coordinates.push(parent.as_str().as_bytes());
    coordinates.extend_from_slice(parts);
    let digest = identity_digest(b"vyrm-runtime-child-span-id-v1\0", &coordinates);
    SpanId::new(&digest[..16]).map_err(core_error)
}

fn identity_digest(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut bytes = Vec::with_capacity(
        domain.len()
            + parts
                .iter()
                .map(|part| std::mem::size_of::<u64>() + part.len())
                .sum::<usize>(),
    );
    bytes.extend_from_slice(domain);
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    sha256_hex(&bytes)
}

fn core_error(error: impl std::fmt::Display) -> ClusterError {
    ClusterError::Invalid(format!("artifact transfer trace: {error}"))
}
