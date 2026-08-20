use std::sync::Arc;
use vyrm_cluster::{
    ArtifactTransferSessionInventory, ArtifactTransferSessionPolicy,
    ArtifactTransferTelemetrySnapshot, ClusterId, NodeId, ShardId,
    VyrmConsensusTraceTelemetrySnapshot, VyrmNodeStatus, VyrmNodeTelemetrySnapshot,
    VyrmTlsGeneration, VyrmTransportAdmissionPolicy, VyrmTransportOperation, VyrmTransportOutcome,
    VyrmTransportTelemetry, ARTIFACT_TRANSFER_TELEMETRY_VERSION,
};
use vyrm_core::ScopeId;
use vyrm_store::{Engine, PersistentEngine};

fn artifact_telemetry(started_at: u64, observed_at: u64) -> ArtifactTransferTelemetrySnapshot {
    ArtifactTransferTelemetrySnapshot {
        contract_version: ARTIFACT_TRANSFER_TELEMETRY_VERSION,
        started_at,
        observed_at,
        policy: ArtifactTransferSessionPolicy::default(),
        inventory: ArtifactTransferSessionInventory {
            active_sessions: 0,
            reserved_bytes: 0,
            partial_bytes: 0,
            retained_receipts: 0,
        },
        begin_requests: 0,
        chunk_requests: 0,
        complete_requests: 0,
        begin_responses: 0,
        accepted_chunks: 0,
        completed_responses: 0,
        completed_receipt_replays: 0,
        denied: 0,
        failed: 0,
        quota_denials: 0,
        gc_runs: 0,
        gc_removed_incomplete: 0,
        gc_removed_completed: 0,
        gc_reclaimed_partial_bytes: 0,
        overflowed: false,
    }
}

fn status(
    project: &ScopeId,
    transport: &VyrmTransportTelemetry,
    started_at: u64,
    observed_at: u64,
    trace_commits: u64,
) -> VyrmNodeStatus {
    VyrmNodeStatus {
        project_scope: project.clone(),
        cluster: ClusterId::new("cluster:connectome-test").unwrap(),
        shard: ShardId(7),
        raft_node_id: 1,
        canonical_node_id: NodeId::new("node:one").unwrap(),
        current_term: 3,
        current_leader: Some(1),
        last_log_index: Some(12),
        last_applied_index: Some(12),
        snapshot_index: Some(10),
        purged_index: Some(10),
        state: "leader".into(),
        credentials: VyrmTlsGeneration {
            generation: 1,
            leaf_digest: "ab".repeat(32),
        },
        telemetry: VyrmNodeTelemetrySnapshot {
            observed_at,
            transport_ingress: transport.snapshot(observed_at).unwrap(),
            artifacts: artifact_telemetry(started_at, observed_at),
            consensus_traces: VyrmConsensusTraceTelemetrySnapshot {
                started_at,
                observed_at,
                prepared_observations: trace_commits,
                chunk_observations: 0,
                completed_observations: trace_commits,
                failed_observations: 0,
                commit_acknowledgements: trace_commits,
                cursor_conflicts: 0,
                leader_changes: 0,
                leader_unavailable: 0,
                denied: 0,
                failed: 0,
                overflowed: false,
            },
        },
    }
}

#[test]
fn retained_cluster_samples_are_hash_linked_restart_aware_and_scope_bound() {
    let root = tempfile::tempdir().unwrap();
    vyrm_node::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    let binding = vyrm_node::InstanceBinding::discover(root.path()).unwrap();
    let project = ScopeId::new(binding.manifest.id.clone()).unwrap();
    let db = root.path().join(vyrm_node::STORE_DIR);
    let store = Arc::new(PersistentEngine::open(&db).unwrap());
    let recorder =
        connectome_ui::ClusterTelemetryRecorder::new(Arc::clone(&store), binding.clone());
    let transport =
        VyrmTransportTelemetry::new(VyrmTransportAdmissionPolicy::default(), 100).unwrap();
    let first = recorder
        .record(
            connectome_ui::RecordClusterTelemetry {
                status: status(&project, &transport, 100, 110, 0),
            },
            111,
        )
        .unwrap();
    assert_eq!(first.sample.sequence, 1);
    assert!(first.sample.previous_sample_digest.is_none());
    assert!(first.sample.delta.is_none());
    let cursor_after_first = store.runtime_cursor().unwrap();
    let duplicate = recorder
        .record(
            connectome_ui::RecordClusterTelemetry {
                status: status(&project, &transport, 100, 110, 0),
            },
            112,
        )
        .unwrap();
    assert_eq!(duplicate.sample.digest, first.sample.digest);
    assert_eq!(store.runtime_cursor().unwrap(), cursor_after_first);

    transport.accept_connection(64).unwrap();
    transport
        .admit(
            &NodeId::new("node:peer").unwrap(),
            VyrmTransportOperation::RuntimeCommit,
            64,
            120,
        )
        .unwrap()
        .finish(VyrmTransportOutcome::Denied, 32)
        .unwrap();
    let second = recorder
        .record(
            connectome_ui::RecordClusterTelemetry {
                status: status(&project, &transport, 100, 121, 1),
            },
            122,
        )
        .unwrap();
    assert_eq!(second.sample.sequence, 2);
    assert_eq!(
        second.sample.previous_sample_digest.as_deref(),
        Some(first.sample.digest.as_str())
    );
    assert_eq!(second.sample.delta.as_ref().unwrap().transport_denied, 1);
    assert!(second
        .sample
        .alerts
        .iter()
        .any(|alert| alert.code == "transport_denied"));

    let restarted_transport =
        VyrmTransportTelemetry::new(VyrmTransportAdmissionPolicy::default(), 200).unwrap();
    restarted_transport.accept_connection(32).unwrap();
    restarted_transport
        .admit(
            &NodeId::new("node:peer").unwrap(),
            VyrmTransportOperation::Append,
            32,
            205,
        )
        .unwrap()
        .finish(VyrmTransportOutcome::Allowed, 16)
        .unwrap();
    let third = recorder
        .record(
            connectome_ui::RecordClusterTelemetry {
                status: status(&project, &restarted_transport, 200, 210, 0),
            },
            211,
        )
        .unwrap();
    assert_eq!(third.sample.sequence, 3);
    assert!(third.sample.process_reset);
    assert!(third.sample.delta.is_none());
    assert!(third
        .sample
        .alerts
        .iter()
        .any(|alert| alert.code == "process_reset"));

    let regressed_transport =
        VyrmTransportTelemetry::new(VyrmTransportAdmissionPolicy::default(), 200).unwrap();
    assert!(recorder
        .record(
            connectome_ui::RecordClusterTelemetry {
                status: status(&project, &regressed_transport, 200, 220, 0),
            },
            221,
        )
        .unwrap_err()
        .to_string()
        .contains("regressed without a process reset"));

    let mut foreign = status(&project, &restarted_transport, 200, 220, 0);
    foreign.project_scope = ScopeId::new("instance:foreign").unwrap();
    assert!(recorder
        .record(
            connectome_ui::RecordClusterTelemetry { status: foreign },
            222,
        )
        .unwrap_err()
        .to_string()
        .contains("project scope"));

    let history = recorder.history(2).unwrap();
    assert_eq!(history.total_samples, 3);
    assert_eq!(history.samples.len(), 2);
    assert_eq!(history.baseline_samples.len(), 1);
    assert_eq!(
        history.baseline_samples[0].sample.digest,
        first.sample.digest
    );
    assert!(history.truncated_before_cursor.is_some());
    assert_eq!(history.nodes.len(), 1);
    assert_eq!(history.nodes[0].latest_sample_digest, third.sample.digest);
    assert!(history
        .samples
        .iter()
        .all(|sample| sample.audit_digest.is_some()));
    let schema = store.runtime_schema(&project).unwrap().unwrap();
    assert!(schema
        .records
        .keys()
        .any(|kind| kind.as_str() == "cluster_telemetry_sample"));

    drop(recorder);
    drop(store);
    let reopened = PersistentEngine::open(&db).unwrap();
    let reopened_history = connectome_ui::cluster_history(&reopened, &binding, 8).unwrap();
    assert_eq!(reopened_history.total_samples, 3);
    assert_eq!(
        reopened_history.samples[2].sample.digest,
        third.sample.digest
    );
}
