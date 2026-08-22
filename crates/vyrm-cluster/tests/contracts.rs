use std::collections::{BTreeMap, BTreeSet};
use vyrm_cluster::*;
use vyrm_core::{ObjectReceipt, ObjectReference, ReadStamp, RuntimeRef, ScopeId};

fn node(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn zone(value: &str) -> ZoneId {
    ZoneId::new(value).unwrap()
}

fn stamp(index: u64, digest_byte: char) -> ShardReadStamp {
    ShardReadStamp {
        term: 1,
        commit_index: index,
        placement_epoch: 1,
        state_digest: digest_byte.to_string().repeat(64),
    }
}

#[test]
fn placement_requires_canonical_multi_zone_voters() {
    let valid = ShardPlacement {
        contract_version: CLUSTER_CONTRACT_VERSION,
        cluster: ClusterId::new("alpha").unwrap(),
        shard: ShardId(7),
        epoch: 1,
        policy: standard_three_zone_policy(),
        replicas: vec![
            ReplicaPlacement {
                node: node("a"),
                zone: zone("az-a"),
                role: ReplicaRole::Voter,
            },
            ReplicaPlacement {
                node: node("b"),
                zone: zone("az-b"),
                role: ReplicaRole::Voter,
            },
            ReplicaPlacement {
                node: node("c"),
                zone: zone("az-c"),
                role: ReplicaRole::Voter,
            },
        ],
    };
    valid.validate().unwrap();
    assert_eq!(valid.policy.quorum(), 2);
    assert_eq!(valid.policy.tolerated_failures(), 1);

    let mut collocated = valid.clone();
    collocated.replicas[1].zone = zone("az-a");
    assert!(collocated.validate().is_err());
    let mut unordered = valid;
    unordered.replicas.swap(0, 1);
    assert!(unordered.validate().is_err());
}

#[test]
fn snapshot_vectors_preserve_partial_order() {
    let left = SnapshotVector {
        contract_version: 1,
        scope: "tenant/project".into(),
        shards: BTreeMap::from([(ShardId(1), stamp(4, 'a')), (ShardId(2), stamp(8, 'b'))]),
    };
    let right = SnapshotVector {
        contract_version: 1,
        scope: "tenant/project".into(),
        shards: BTreeMap::from([(ShardId(1), stamp(5, 'c')), (ShardId(2), stamp(7, 'd'))]),
    };
    assert_eq!(left.relation(&right).unwrap(), VectorRelation::Concurrent);
    assert_eq!(left.relation(&left).unwrap(), VectorRelation::Equal);
}

#[test]
fn equal_cursor_with_different_truth_is_denied() {
    let left = SnapshotVector {
        contract_version: 1,
        scope: "scope".into(),
        shards: BTreeMap::from([(ShardId(1), stamp(4, 'a'))]),
    };
    let right = SnapshotVector {
        contract_version: 1,
        scope: "scope".into(),
        shards: BTreeMap::from([(ShardId(1), stamp(4, 'b'))]),
    };
    assert!(matches!(
        left.relation(&right),
        Err(ClusterError::Denied(_))
    ));
}

#[test]
fn cross_shard_write_is_explicitly_denied() {
    let scope = TransactionScope::CrossShard {
        shards: BTreeSet::from([ShardId(1), ShardId(2)]),
    };
    assert!(matches!(scope.enforce_m7(), Err(ClusterError::Denied(_))));
}

#[test]
fn transfer_is_grounded_snapshot_plus_contiguous_wal_delta() {
    let plan = ReplicaTransferPlan {
        contract_version: 1,
        shard: ShardId(3),
        placement_epoch: 2,
        source: node("a"),
        target: node("d"),
        grounded_snapshot: ShardReadStamp {
            placement_epoch: 2,
            ..stamp(10, 'a')
        },
        wal_from_exclusive: 10,
        wal_through_inclusive: 15,
        artifact_digests: BTreeSet::from(["b".repeat(64)]),
    };
    plan.validate().unwrap();
    let mut gap = plan;
    gap.wal_from_exclusive = 9;
    assert!(gap.validate().is_err());
}

#[test]
fn artifact_manifest_binds_project_read_plan_objects_and_receipt() {
    let scope = ScopeId::new("instance:cluster-artifacts").unwrap();
    let bytes = b"artifact";
    let sha256 = vyrm_core::digest::sha256_hex(bytes);
    let object = ObjectReference::for_bytes(
        "artifact:one",
        Some(RuntimeRef::new("document", "one").unwrap()),
        "application/octet-stream",
        bytes,
        ObjectReceipt {
            backend: "source".into(),
            key: ObjectReference::canonical_key(&sha256).unwrap(),
            version: Some("1".into()),
            etag: Some(sha256.clone()),
        },
    )
    .unwrap();
    let plan = ReplicaTransferPlan {
        contract_version: CLUSTER_CONTRACT_VERSION,
        shard: ShardId(3),
        placement_epoch: 2,
        source: node("a"),
        target: node("d"),
        grounded_snapshot: ShardReadStamp {
            placement_epoch: 2,
            ..stamp(10, 'a')
        },
        wal_from_exclusive: 10,
        wal_through_inclusive: 10,
        artifact_digests: BTreeSet::from([sha256.clone()]),
    };
    let read = ReadStamp::new(scope.clone(), None, 0, 4, Some("44".repeat(32))).unwrap();
    let manifest = ArtifactTransferManifest::new(plan, scope, read, vec![object.clone()]).unwrap();
    manifest.validate().unwrap();

    let target = ObjectReceipt {
        backend: "target".into(),
        key: ObjectReference::canonical_key(&sha256).unwrap(),
        version: None,
        etag: Some(sha256.clone()),
    };
    let receipt = ArtifactTransferReceipt::new(
        &manifest,
        vec![ArtifactReplicaObjectReceipt {
            reference: object.reference,
            sha256,
            length: bytes.len() as u64,
            target,
            transferred: true,
        }],
        20,
    )
    .unwrap();
    receipt.validate(&manifest).unwrap();
    let prepared = ArtifactTransferObservation::prepared(&manifest, 1, 18).unwrap();
    prepared.validate().unwrap();
    let progress = ArtifactTransferObservation::progress(
        &manifest,
        1,
        19,
        &ArtifactObjectProgress {
            sha256: manifest.objects[0].sha256.clone(),
            expected_length: manifest.objects[0].length,
            next_offset: manifest.objects[0].length,
            complete: true,
        },
    )
    .unwrap();
    progress.validate().unwrap();
    let completed =
        ArtifactTransferObservation::completed(&manifest, 1, 20, 1_000, &receipt).unwrap();
    completed.validate().unwrap();
    let failed =
        ArtifactTransferObservation::failed(&manifest, 2, 21, 500, "secret failure").unwrap();
    assert_eq!(
        failed.error_digest,
        Some(vyrm_core::digest::sha256_hex(b"secret failure"))
    );
    assert!(!serde_json::to_string(&failed)
        .unwrap()
        .contains("secret failure"));
    let prepared_trace = artifact_transfer_trace_event(&prepared).unwrap();
    let progress_trace = artifact_transfer_trace_event(&progress).unwrap();
    let completed_trace = artifact_transfer_trace_event(&completed).unwrap();
    let failed_trace = artifact_transfer_trace_event(&failed).unwrap();
    assert_eq!(prepared_trace.name, "cluster.artifact_transfer");
    assert_eq!(prepared_trace.trace_id, completed_trace.trace_id);
    assert_eq!(prepared_trace.span_id, completed_trace.span_id);
    assert_eq!(progress_trace.name, "cluster.artifact_chunk");
    assert_eq!(progress_trace.parent_span_id, Some(prepared_trace.span_id));
    assert_eq!(failed_trace.outcome, vyrm_core::TraceOutcome::Error);
    assert!(!serde_json::to_string(&failed_trace)
        .unwrap()
        .contains("secret failure"));

    let mut tampered = manifest.clone();
    tampered.objects[0].media_type = "application/substituted".into();
    assert!(tampered.validate().is_err());
    let mut tampered_receipt = receipt;
    tampered_receipt.transferred_bytes += 1;
    assert!(tampered_receipt.validate(&manifest).is_err());
}

#[test]
fn transport_telemetry_v1_fixture_round_trips_and_denies_shape_drift() {
    let fixture = include_str!("../fixtures/transport-telemetry-v1.json");
    let snapshot: vyrm_cluster::VyrmTransportTelemetrySnapshot =
        serde_json::from_str(fixture).unwrap();
    assert_eq!(
        snapshot.contract_version,
        vyrm_cluster::VYRM_CLUSTER_TELEMETRY_VERSION
    );
    snapshot.policy.validate().unwrap();
    snapshot.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap(),
        serde_json::from_str::<serde_json::Value>(fixture).unwrap()
    );

    let mut unknown: serde_json::Value = serde_json::from_str(fixture).unwrap();
    unknown["secret_payload"] = serde_json::Value::String("must fail closed".into());
    assert!(
        serde_json::from_value::<vyrm_cluster::VyrmTransportTelemetrySnapshot>(unknown).is_err()
    );
}

#[test]
fn raft_timing_policy_rejects_election_windows_that_cannot_tolerate_a_heartbeat() {
    let policy = vyrm_cluster::VyrmRaftTimingPolicy::default();
    policy.validate().unwrap();
    let invalid = vyrm_cluster::VyrmRaftTimingPolicy {
        heartbeat_interval_millis: 250,
        election_timeout_min_millis: 250,
        election_timeout_max_millis: 500,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn reshard_cutover_is_bound_to_exact_source_vector() {
    let target = ShardPlacement {
        contract_version: 1,
        cluster: ClusterId::new("alpha").unwrap(),
        shard: ShardId(11),
        epoch: 2,
        policy: standard_three_zone_policy(),
        replicas: vec![
            ReplicaPlacement {
                node: node("a"),
                zone: zone("az-a"),
                role: ReplicaRole::Voter,
            },
            ReplicaPlacement {
                node: node("b"),
                zone: zone("az-b"),
                role: ReplicaRole::Voter,
            },
            ReplicaPlacement {
                node: node("c"),
                zone: zone("az-c"),
                role: ReplicaRole::Voter,
            },
        ],
    };
    let mut plan = ReshardPlan {
        contract_version: 1,
        operation_id: "split-10".into(),
        metadata_index: 4,
        source_shards: BTreeSet::from([ShardId(10)]),
        targets: vec![target],
        cutover: SnapshotVector {
            contract_version: 1,
            scope: "tenant/project".into(),
            shards: BTreeMap::from([(ShardId(10), stamp(99, 'a'))]),
        },
        state: ReshardState::CaughtUp,
    };
    plan.validate().unwrap();
    plan.cutover.shards.insert(ShardId(12), stamp(1, 'b'));
    assert!(plan.validate().is_err());
}

#[test]
fn allowed_routes_require_explicitly_active_selected_replicas() {
    let mut route = RouteEvidence {
        contract_version: 1,
        shard: ShardId(4),
        placement_epoch: 1,
        requested_consistency: ReadConsistency::Linearizable,
        selected: vec![node("a")],
        replica_health: BTreeMap::from([(node("a"), ReplicaHealth::Active)]),
        observed: Some(stamp(3, 'a')),
        allowed: true,
        reason: "leader connected to quorum".into(),
    };
    route.validate().unwrap();
    route
        .replica_health
        .insert(node("a"), ReplicaHealth::Suspect);
    assert!(route.validate().is_err());
}
