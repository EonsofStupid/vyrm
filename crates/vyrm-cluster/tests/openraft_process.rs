#![cfg(feature = "openraft-transport")]

use rcgen::{
    date_time_ymd, BasicConstraints, Certificate, CertificateParams,
    CertificateRevocationListParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyIdMethod, KeyPair,
    KeyUsagePurpose, RevocationReason, RevokedCertParams, SanType, SerialNumber,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use vyrm_cluster::{
    ArtifactTransferManifest, ArtifactTransferReceipt, ClusterId, NodeId, PlacementPolicy,
    ReplicaPlacement, ReplicaRole, ShardId, ShardPlacement, VyrmNodeCommand, VyrmNodeConfig,
    VyrmNodeReply, VyrmNodeRequest, VyrmNodeResult, VyrmNodeStatus, VyrmRaftNode, VyrmTlsFiles,
    VyrmTransportBinding, VyrmTransportOperation, ZoneId, CLUSTER_CONTRACT_VERSION,
    VYRM_NODE_CONFIG_VERSION, VYRM_NODE_CONTROL_VERSION,
};
use vyrm_core::{
    ObjectReference, RuntimeChange, RuntimeCommit, RuntimeMutation, RuntimeRecordSchema,
    RuntimeSchemaRegistry, RuntimeType, RuntimeValue, ScopeId,
};
use vyrm_store::{Engine, LocalObjectStore, NativeEngine};

const SHARD: ShardId = ShardId(11);

fn project_scope() -> ScopeId {
    ScopeId::new("instance:process-cluster").unwrap()
}

struct ProcessNode {
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<VyrmNodeReply, String>>,
    reader: Option<JoinHandle<()>>,
    next_request: u64,
}

impl ProcessNode {
    fn start(config: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vyrm-cluster-node"))
            .arg(config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut child_output = BufReader::new(child.stdout.take().unwrap());
        let ready = read_reply(&mut child_output);
        assert!(ready.ok, "node did not become ready: {ready:?}");
        assert!(matches!(ready.value, Some(VyrmNodeResult::Ready { .. })));
        let (reply_sender, output) = mpsc::channel();
        let reader = thread::spawn(move || loop {
            let mut line = String::new();
            let reply = match child_output.read_line(&mut line) {
                Ok(0) => Err("node closed its supervisor output".into()),
                Ok(_) => serde_json::from_str(&line)
                    .map_err(|error| format!("node emitted an invalid supervisor reply: {error}")),
                Err(error) => Err(format!("read node supervisor reply: {error}")),
            };
            let terminal = reply.is_err();
            if reply_sender.send(reply).is_err() || terminal {
                break;
            }
        });
        Self {
            child,
            input,
            output,
            reader: Some(reader),
            next_request: 1,
        }
    }

    fn command(&mut self, command: &VyrmNodeCommand) -> VyrmNodeResult {
        let reply = self.command_reply(command);
        assert!(reply.ok, "node command failed: {command:?}: {reply:?}");
        reply.value.unwrap()
    }

    fn command_reply(&mut self, command: &VyrmNodeCommand) -> VyrmNodeReply {
        let request_id = format!("test-{}", self.next_request);
        self.next_request += 1;
        serde_json::to_writer(
            &mut self.input,
            &VyrmNodeRequest {
                version: VYRM_NODE_CONTROL_VERSION,
                request_id: request_id.clone(),
                command: command.clone(),
            },
        )
        .unwrap();
        self.input.write_all(b"\n").unwrap();
        self.input.flush().unwrap();
        let reply = self
            .output
            .recv_timeout(Duration::from_secs(45))
            .unwrap_or_else(|error| {
                panic!("node supervisor reply timed out for {command:?}: {error}")
            })
            .unwrap_or_else(|error| panic!("node supervisor reply failed: {error}"));
        assert_eq!(reply.request_id.as_deref(), Some(request_id.as_str()));
        reply
    }

    fn status(&mut self) -> VyrmNodeStatus {
        match self.command(&VyrmNodeCommand::Status) {
            VyrmNodeResult::Status { status } => *status,
            other => panic!("expected status, got {other:?}"),
        }
    }

    fn crash(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        self.join_reader();
    }

    fn shutdown(mut self) {
        assert_eq!(
            self.command(&VyrmNodeCommand::Shutdown),
            VyrmNodeResult::Ack
        );
        assert!(self.child.wait().unwrap().success());
        self.join_reader();
    }

    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.join().expect("node supervisor reader panicked");
        }
    }
}

impl Drop for ProcessNode {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.join_reader();
    }
}

#[test]
fn independent_processes_recover_fail_over_snapshot_and_reject_corruption() {
    let fixture = ProcessFixture::new();

    assert_startup_identity_denial(&fixture.configs[&4], &fixture.certificates[&3]);

    let mut node1 = ProcessNode::start(&fixture.configs[&1]);
    let mut node2 = ProcessNode::start(&fixture.configs[&2]);
    let mut node3 = ProcessNode::start(&fixture.configs[&3]);

    assert_eq!(
        node1.command(&VyrmNodeCommand::Initialize),
        VyrmNodeResult::Ack
    );
    assert_eq!(node1.command(&VyrmNodeCommand::Elect), VyrmNodeResult::Ack);
    wait_for_leader(&mut node1, 1);
    assert_eq!(
        node1.command(&VyrmNodeCommand::AddLearner { node_id: 2 }),
        VyrmNodeResult::Ack
    );
    assert_eq!(
        node1.command(&VyrmNodeCommand::AddLearner { node_id: 3 }),
        VyrmNodeResult::Ack
    );
    assert_eq!(
        node1.command(&VyrmNodeCommand::ChangeMembership {
            voters: BTreeSet::from([1, 2, 3]),
        }),
        VyrmNodeResult::Ack
    );
    let transition_index = write_index(node1.command(&VyrmNodeCommand::PlacementTransition {
        request_id: "process-placement-1".into(),
        placement: fixture.placement(),
        expected_commit_index: None,
    }));
    let first_index = write_index(node1.command(&VyrmNodeCommand::Probe {
        request_id: "process-probe-1".into(),
        placement_epoch: 1,
        expected_commit_index: Some(transition_index),
        payload: b"before-crash".to_vec(),
    }));
    wait_applied(&mut node2, first_index);
    wait_applied(&mut node3, first_index);

    let artifact_bytes = (0..(vyrm_cluster::ARTIFACT_TRANSFER_CHUNK_MAX_BYTES + 177_013))
        .map(|index| (index % 241) as u8)
        .collect::<Vec<_>>();
    let mut source_receipt = None;
    for id in 1..=3 {
        let objects =
            LocalObjectStore::open(fixture.data_roots[&id].join("application-objects")).unwrap();
        let stored = objects.put(&artifact_bytes).unwrap();
        if let Some(expected) = &source_receipt {
            assert_eq!(expected, &stored);
        } else {
            source_receipt = Some(stored);
        }
    }
    let stored = source_receipt.unwrap();
    let artifact = ObjectReference::for_bytes(
        "vector:hnsw:process-fixture@1:bytes",
        None,
        "application/vnd.vyrm.vector-hnsw+json",
        &artifact_bytes,
        stored.receipt,
    )
    .unwrap();
    let mut artifact_schema = RuntimeSchemaRegistry::empty(1, "process artifact fixture");
    artifact_schema.records.insert(
        RuntimeType::new("artifact_fixture").unwrap(),
        RuntimeRecordSchema::default(),
    );
    let artifact_commit = RuntimeCommit {
        scope: project_scope(),
        at: 10,
        actor: "cluster:process-test".into(),
        expected_cursor: 0,
        mutations: vec![
            RuntimeMutation::Schema {
                registry: artifact_schema,
            },
            RuntimeMutation::Object {
                object: artifact.clone(),
            },
        ],
    };
    let mut foreign_commit = artifact_commit.clone();
    foreign_commit.scope = ScopeId::new("instance:foreign-project").unwrap();
    let denied = node1.command_reply(&VyrmNodeCommand::RuntimeCommit {
        request_id: "process-runtime-foreign".into(),
        placement_epoch: 1,
        expected_commit_index: Some(first_index),
        commit: foreign_commit,
    });
    assert!(!denied.ok);
    assert!(denied.error.unwrap().contains("configured project"));
    let artifact_index = write_index(node1.command(&VyrmNodeCommand::RuntimeCommit {
        request_id: "process-runtime-artifact-1".into(),
        placement_epoch: 1,
        expected_commit_index: Some(first_index),
        commit: artifact_commit,
    }));
    wait_applied(&mut node2, artifact_index);
    wait_applied(&mut node3, artifact_index);

    let rotated = node1.command(&VyrmNodeCommand::RotateCredentials {
        expected_generation: 1,
        files: fixture.rotations[&1].clone(),
    });
    assert!(matches!(
        rotated,
        VyrmNodeResult::Credentials { credentials } if credentials.generation == 2
    ));
    let rejected = node1.command_reply(&VyrmNodeCommand::RotateCredentials {
        expected_generation: 1,
        files: fixture.rotations[&1].clone(),
    });
    assert!(!rejected.ok, "stale credential generation was accepted");
    for node in [&mut node2, &mut node3] {
        let node_id = node.status().raft_node_id;
        let rotated = node.command(&VyrmNodeCommand::RotateCredentials {
            expected_generation: 1,
            files: fixture.rotations[&node_id].clone(),
        });
        assert!(matches!(rotated, VyrmNodeResult::Credentials { .. }));
    }

    node2.crash();
    let second_index = write_index(node1.command(&VyrmNodeCommand::Probe {
        request_id: "process-probe-2".into(),
        placement_epoch: 1,
        expected_commit_index: Some(artifact_index),
        payload: b"while-node-two-is-down".to_vec(),
    }));
    node2 = ProcessNode::start(&fixture.configs[&2]);
    node2.command(&VyrmNodeCommand::RotateCredentials {
        expected_generation: 1,
        files: fixture.rotations[&2].clone(),
    });
    wait_applied(&mut node2, second_index);

    node1.crash();
    // Production nodes keep automatic elections enabled. `Elect` accelerates
    // failover but does not reserve leadership for the triggered survivor, so
    // continue through whichever eligible node the quorum actually elects.
    let (failover_index, failover_leader) = elect_and_write(
        &mut node2,
        &mut node3,
        "process-probe-failover",
        b"new-leader-write",
    );

    node1 = ProcessNode::start(&fixture.configs[&1]);
    let denied = node1.command_reply(&VyrmNodeCommand::WaitApplied {
        index: failover_index,
        timeout_millis: 500,
    });
    assert!(
        !denied.ok,
        "revoked pre-rotation leaf unexpectedly rejoined after restart"
    );
    node1.command(&VyrmNodeCommand::RotateCredentials {
        expected_generation: 1,
        files: fixture.rotations[&1].clone(),
    });
    wait_applied(&mut node1, failover_index);

    let partition_index = if failover_leader == 2 {
        isolate_then_elect_and_write(&mut node2, &mut node3, &mut node1)
    } else {
        isolate_then_elect_and_write(&mut node3, &mut node2, &mut node1)
    };

    let mut node4 = ProcessNode::start(&fixture.configs[&4]);
    let snapshot_index = {
        let mut voters = [&mut node1, &mut node2, &mut node3];
        snapshot_purge_and_add_learner(&mut voters, partition_index, 4)
    };
    let consensus_trace_tail = [&mut node1, &mut node2, &mut node3]
        .into_iter()
        .filter_map(|node| node.status().last_log_index)
        .max()
        .unwrap();
    wait_applied(&mut node4, consensus_trace_tail);
    wait_snapshot(&mut node4, snapshot_index);
    let learner_objects =
        LocalObjectStore::open(fixture.data_roots[&4].join("application-objects")).unwrap();
    assert_eq!(learner_objects.get(&artifact).unwrap(), artifact_bytes);
    let sessions = fixture.data_roots[&4]
        .join("application-objects")
        .join("transfer-sessions-v1");
    let session_directories = fs::read_dir(&sessions)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert!(!session_directories.is_empty());
    let mut receipts = Vec::new();
    for directory in session_directories {
        let manifest: ArtifactTransferManifest =
            serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.plan.target.as_str(), "process-node-4");
        assert!(manifest
            .objects
            .iter()
            .any(|object| object.sha256 == artifact.sha256));
        let receipt_path = directory.join("receipt.json");
        if receipt_path.is_file() {
            let receipt: ArtifactTransferReceipt =
                serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap();
            receipt.validate(&manifest).unwrap();
            receipts.push(receipt);
        }
    }
    assert!(!receipts.is_empty());
    assert!(receipts
        .iter()
        .all(|receipt| receipt.target.as_str() == "process-node-4"));
    let transferred_objects = receipts
        .iter()
        .map(|receipt| receipt.transferred_objects)
        .sum::<u64>();
    let transferred_bytes = receipts
        .iter()
        .map(|receipt| receipt.transferred_bytes)
        .sum::<u64>();
    assert!(transferred_objects <= 1);
    assert_eq!(
        transferred_bytes,
        if transferred_objects == 1 {
            artifact_bytes.len() as u64
        } else {
            0
        }
    );

    let learner_status = node4.status();
    assert_eq!(learner_status.project_scope, project_scope());
    assert_eq!(
        learner_status.telemetry.observed_at,
        learner_status.telemetry.transport_ingress.observed_at
    );
    assert_eq!(
        learner_status.telemetry.observed_at,
        learner_status.telemetry.artifacts.observed_at
    );
    assert!(
        learner_status.telemetry.transport_ingress.operations[&VyrmTransportOperation::Artifact]
            .allowed
            > 0
    );
    assert!(learner_status.telemetry.artifacts.completed_responses > 0);
    assert!(
        learner_status
            .telemetry
            .artifacts
            .inventory
            .retained_receipts
            > 0
    );
    assert!(!learner_status.telemetry.transport_ingress.overflowed);
    assert!(!learner_status.telemetry.artifacts.overflowed);
    let voter_telemetry = voter_statuses(&mut [&mut node1, &mut node2, &mut node3]);
    assert!(voter_telemetry.iter().any(|status| status
        .telemetry
        .consensus_traces
        .commit_acknowledgements
        > 0));
    let encoded_status = serde_json::to_vec(&learner_status).unwrap();
    assert!(!encoded_status
        .windows(64)
        .any(|window| window == &artifact_bytes[..64]));

    node4.shutdown();
    let learner_changes = project_changes(&fixture.data_roots[&4]);
    let trace_events = learner_changes
        .iter()
        .filter_map(|change| match &change.mutation {
            RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
                event.properties.get("name")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(trace_events
        .iter()
        .any(|name| **name == RuntimeValue::String("cluster.artifact_transfer".into())));
    assert!(trace_events
        .iter()
        .any(|name| **name == RuntimeValue::String("cluster.artifact_chunk".into())));
    assert!(!serde_json::to_vec(&learner_changes)
        .unwrap()
        .windows(64)
        .any(|window| window == &artifact_bytes[..64]));
    let current = fixture.data_roots[&4].join("CURRENT");
    let mut corrupt = fs::read(&current).unwrap();
    corrupt.push(0xff);
    fs::write(&current, corrupt).unwrap();
    assert_startup_failure(
        &fixture.configs[&4],
        "corrupt authenticated CURRENT pointer",
    );

    node1.shutdown();
    node2.shutdown();
    node3.shutdown();
    for id in 1..=3 {
        assert_eq!(
            project_changes(&fixture.data_roots[&id]),
            learner_changes,
            "voter {id} and the post-purge learner must retain identical consensus trace truth"
        );
    }
}

fn project_changes(root: &Path) -> Vec<RuntimeChange> {
    let engine = NativeEngine::open(root).unwrap();
    engine
        .runtime_changes_since(0, usize::MAX, Some(&project_scope()))
        .unwrap()
        .changes
}

fn wait_for_leader(node: &mut ProcessNode, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if node.status().current_leader == Some(expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "node did not observe leader {expected}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn elect_and_write(
    preferred: &mut ProcessNode,
    other: &mut ProcessNode,
    request_id: &str,
    payload: &[u8],
) -> (u64, u64) {
    let preferred_id = preferred.status().raft_node_id;
    let other_id = other.status().raft_node_id;
    assert_eq!(
        preferred.command(&VyrmNodeCommand::Elect),
        VyrmNodeResult::Ack
    );
    let leader = wait_for_agreed_leader(preferred, other, &[preferred_id, other_id]);
    let index = if leader == preferred_id {
        write_from_leader(preferred, other, request_id, payload)
    } else {
        write_from_leader(other, preferred, request_id, payload)
    };
    (index, leader)
}

fn isolate_then_elect_and_write(
    isolated: &mut ProcessNode,
    preferred: &mut ProcessNode,
    other: &mut ProcessNode,
) -> u64 {
    assert_eq!(
        isolated.command(&VyrmNodeCommand::SetTransportEnabled { enabled: false }),
        VyrmNodeResult::Ack
    );
    let (index, _) = elect_and_write(
        preferred,
        other,
        "process-probe-partition",
        b"quorum-survives-live-leader-isolation",
    );
    assert_eq!(
        isolated.command(&VyrmNodeCommand::SetTransportEnabled { enabled: true }),
        VyrmNodeResult::Ack
    );
    wait_applied(isolated, index);
    index
}

fn wait_for_agreed_leader(
    first: &mut ProcessNode,
    second: &mut ProcessNode,
    eligible: &[u64],
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let first_status = first.status();
        let second_status = second.status();
        if let Some(leader) = first_status
            .current_leader
            .filter(|leader| second_status.current_leader == Some(*leader))
            .filter(|leader| eligible.contains(leader))
        {
            return leader;
        }
        assert!(
            Instant::now() < deadline,
            "survivors did not agree on an eligible leader; first={first_status:?}; second={second_status:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn write_from_leader(
    leader: &mut ProcessNode,
    follower: &mut ProcessNode,
    request_id: &str,
    payload: &[u8],
) -> u64 {
    let leadership_log = leader
        .status()
        .last_log_index
        .expect("elected leader has a leadership log");
    wait_applied(leader, leadership_log);
    let expected = leader.status().last_applied_index;
    let index = write_index(leader.command(&VyrmNodeCommand::Probe {
        request_id: request_id.into(),
        placement_epoch: 1,
        expected_commit_index: expected,
        payload: payload.to_vec(),
    }));
    wait_applied(follower, index);
    index
}

fn wait_applied(node: &mut ProcessNode, index: u64) {
    match node.command(&VyrmNodeCommand::WaitApplied {
        index,
        timeout_millis: 30_000,
    }) {
        VyrmNodeResult::Status { status } => {
            assert!(status.last_applied_index >= Some(index));
        }
        other => panic!("expected applied status, got {other:?}"),
    }
}

fn wait_snapshot(node: &mut ProcessNode, index: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = node.status();
        if status.snapshot_index >= Some(index) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "post-purge learner did not report physical snapshot activation: {status:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn snapshot_purge_and_add_learner(
    voters: &mut [&mut ProcessNode],
    at_least: u64,
    learner: u64,
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut next_trigger = Instant::now();
    let mut last_add_reply = None;
    loop {
        let statuses = voter_statuses(voters);
        if Instant::now() >= next_trigger {
            for node_id in statuses.iter().map(|status| status.raft_node_id) {
                let _ = command_on_voter(voters, node_id, &VyrmNodeCommand::TriggerSnapshot);
            }
            next_trigger = Instant::now() + Duration::from_millis(250);
        }

        if statuses
            .iter()
            .all(|status| status.snapshot_index >= Some(at_least))
        {
            let snapshot_index = statuses
                .iter()
                .filter_map(|status| status.snapshot_index)
                .min()
                .expect("all voters reported a snapshot");
            for node_id in statuses.iter().map(|status| status.raft_node_id) {
                let reply = command_on_voter(
                    voters,
                    node_id,
                    &VyrmNodeCommand::PurgeLog {
                        index: snapshot_index,
                    },
                );
                assert!(reply.ok, "voter {node_id} failed to purge: {reply:?}");
            }
            let purged = voter_statuses(voters);
            if purged
                .iter()
                .all(|status| status.purged_index >= Some(snapshot_index))
            {
                if let Some(leader) = quorum_agreed_leader(&purged) {
                    let reply = command_on_voter(
                        voters,
                        leader,
                        &VyrmNodeCommand::AddLearner { node_id: learner },
                    );
                    if reply.ok {
                        assert_eq!(reply.value, Some(VyrmNodeResult::Ack));
                        return snapshot_index;
                    }
                    last_add_reply = Some(reply);
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "voters did not snapshot, purge, and add the learner: {statuses:?}; last add reply: {last_add_reply:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn voter_statuses(voters: &mut [&mut ProcessNode]) -> Vec<VyrmNodeStatus> {
    voters.iter_mut().map(|node| node.status()).collect()
}

fn quorum_agreed_leader(statuses: &[VyrmNodeStatus]) -> Option<u64> {
    let mut counts = BTreeMap::new();
    for leader in statuses.iter().filter_map(|status| status.current_leader) {
        *counts.entry(leader).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .find_map(|(leader, count)| (count >= 2).then_some(leader))
}

fn command_on_voter(
    voters: &mut [&mut ProcessNode],
    node_id: u64,
    command: &VyrmNodeCommand,
) -> VyrmNodeReply {
    for node in voters {
        if node.status().raft_node_id == node_id {
            return node.command_reply(command);
        }
    }
    panic!("voter inventory did not contain node {node_id}")
}

fn write_index(result: VyrmNodeResult) -> u64 {
    match result {
        VyrmNodeResult::Write {
            log_index,
            response,
        } => {
            assert!(
                response.accepted,
                "replicated command was denied: {response:?}"
            );
            log_index
        }
        other => panic!("expected write result, got {other:?}"),
    }
}

fn read_reply(reader: &mut BufReader<ChildStdout>) -> VyrmNodeReply {
    let mut line = String::new();
    assert_ne!(
        reader.read_line(&mut line).unwrap(),
        0,
        "node exited before reply"
    );
    serde_json::from_str(&line).unwrap()
}

fn assert_startup_identity_denial(valid_config: &Path, wrong_certificate: &Path) {
    let directory = tempfile::tempdir().unwrap();
    let mut config: VyrmNodeConfig =
        serde_json::from_slice(&fs::read(valid_config).unwrap()).unwrap();
    config.certificate_der = wrong_certificate.to_owned();
    config.data_root = directory.path().join("data");
    let path = directory.path().join("config.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_startup_failure(&path, "certificate/node identity confusion");
}

fn assert_startup_failure(config: &Path, scenario: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_vyrm-cluster-node"))
        .arg(config)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success(), "{scenario} unexpectedly started");
    assert!(
        output.stdout.is_empty(),
        "{scenario} emitted a ready receipt"
    );
    assert!(
        !output.stderr.is_empty(),
        "{scenario} did not explain its denial"
    );
}

struct ProcessFixture {
    _directory: TempDir,
    cluster: ClusterId,
    configs: BTreeMap<u64, PathBuf>,
    certificates: BTreeMap<u64, PathBuf>,
    data_roots: BTreeMap<u64, PathBuf>,
    rotations: BTreeMap<u64, VyrmTlsFiles>,
}

impl ProcessFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let cluster = ClusterId::new("cluster:process-evidence").unwrap();
        let trust_domain = "process.vyrm.test";
        let reservations = (1..=4)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect::<Vec<_>>();
        let addresses = reservations
            .iter()
            .enumerate()
            .map(|(index, listener)| ((index + 1) as u64, listener.local_addr().unwrap()))
            .collect::<BTreeMap<_, _>>();
        let nodes = addresses
            .iter()
            .map(|(id, address)| {
                (
                    *id,
                    VyrmRaftNode {
                        canonical_id: format!("process-node-{id}"),
                        zone: format!("az-{id}"),
                        endpoint: format!(
                            "vyrm+tls://{address}?server_name=process-node-{id}.vyrm.test"
                        ),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let (ca, issuer) = test_ca();
        let trust_root = directory.path().join("ca.der");
        fs::write(&trust_root, ca.der()).unwrap();
        let crl_path = directory.path().join("ca.crl.der");
        fs::write(&crl_path, test_crl(&issuer, 10_001)).unwrap();
        let mut configs = BTreeMap::new();
        let mut certificates = BTreeMap::new();
        let mut data_roots = BTreeMap::new();
        let mut rotations = BTreeMap::new();
        for id in 1..=4 {
            let binding = VyrmTransportBinding {
                trust_domain: trust_domain.into(),
                cluster: cluster.clone(),
                shard: SHARD,
                raft_node_id: id,
                canonical_node_id: NodeId::new(format!("process-node-{id}")).unwrap(),
            };
            let (certificate, key) = test_identity(
                &issuer,
                &format!("process-node-{id}.vyrm.test"),
                &binding.spiffe_id().unwrap(),
                10_000 + id,
            );
            let certificate_path = directory.path().join(format!("node-{id}.der"));
            let key_path = directory.path().join(format!("node-{id}.key.der"));
            fs::write(&certificate_path, certificate).unwrap();
            fs::write(&key_path, key).unwrap();
            let (rotated_certificate, rotated_key) = test_identity(
                &issuer,
                &format!("process-node-{id}.vyrm.test"),
                &binding.spiffe_id().unwrap(),
                20_000 + id,
            );
            let rotated_certificate_path = directory.path().join(format!("node-{id}.rotated.der"));
            let rotated_key_path = directory.path().join(format!("node-{id}.rotated.key.der"));
            fs::write(&rotated_certificate_path, rotated_certificate).unwrap();
            fs::write(&rotated_key_path, rotated_key).unwrap();
            let data_root = directory.path().join(format!("node-{id}-data"));
            let config = VyrmNodeConfig {
                version: VYRM_NODE_CONFIG_VERSION,
                trust_domain: trust_domain.into(),
                cluster: cluster.clone(),
                shard: SHARD,
                project_scope: project_scope(),
                raft_node_id: id,
                data_root: data_root.clone(),
                raft_listen: addresses[&id].to_string(),
                nodes: nodes.clone(),
                certificate_der: certificate_path.clone(),
                private_key_der: key_path,
                trust_root_der: trust_root.clone(),
                transport_admission: Default::default(),
                raft_timing: Default::default(),
            };
            let config_path = directory.path().join(format!("node-{id}.json"));
            fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
            configs.insert(id, config_path);
            certificates.insert(id, certificate_path);
            data_roots.insert(id, data_root);
            rotations.insert(
                id,
                VyrmTlsFiles {
                    certificate_der: rotated_certificate_path,
                    private_key_der: rotated_key_path,
                    trust_root_ders: vec![trust_root.clone()],
                    revocation_list_ders: vec![crl_path.clone()],
                },
            );
        }
        drop(reservations);
        Self {
            _directory: directory,
            cluster,
            configs,
            certificates,
            data_roots,
            rotations,
        }
    }

    fn placement(&self) -> ShardPlacement {
        ShardPlacement {
            contract_version: CLUSTER_CONTRACT_VERSION,
            cluster: self.cluster.clone(),
            shard: SHARD,
            epoch: 1,
            policy: PlacementPolicy {
                voter_count: 3,
                minimum_voter_zones: 3,
                maximum_voters_per_zone: 1,
            },
            replicas: (1..=3)
                .map(|id| ReplicaPlacement {
                    node: NodeId::new(format!("process-node-{id}")).unwrap(),
                    zone: ZoneId::new(format!("az-{id}")).unwrap(),
                    role: ReplicaRole::Voter,
                })
                .collect(),
        }
    }
}

fn test_ca() -> (Certificate, Issuer<'static, KeyPair>) {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let key = KeyPair::generate().unwrap();
    let certificate = params.self_signed(&key).unwrap();
    (certificate, Issuer::new(params, key))
}

fn test_identity(
    issuer: &Issuer<'static, KeyPair>,
    dns_name: &str,
    spiffe_id: &str,
    serial: u64,
) -> (Vec<u8>, Vec<u8>) {
    let mut params = CertificateParams::new(vec![dns_name.to_owned()]).unwrap();
    params.serial_number = Some(SerialNumber::from(serial));
    params
        .subject_alt_names
        .push(SanType::URI(spiffe_id.try_into().unwrap()));
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ClientAuth,
        ExtendedKeyUsagePurpose::ServerAuth,
    ];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let key = KeyPair::generate().unwrap();
    let certificate = params.signed_by(&key, issuer).unwrap();
    (certificate.der().to_vec(), key.serialize_der())
}

fn test_crl(issuer: &Issuer<'static, KeyPair>, revoked_serial: u64) -> Vec<u8> {
    CertificateRevocationListParams {
        this_update: date_time_ymd(2026, 1, 1),
        next_update: date_time_ymd(2030, 1, 1),
        crl_number: SerialNumber::from(1_u64),
        issuing_distribution_point: None,
        revoked_certs: vec![RevokedCertParams {
            serial_number: SerialNumber::from(revoked_serial),
            revocation_time: date_time_ymd(2026, 1, 1),
            reason_code: Some(RevocationReason::Superseded),
            invalidity_date: None,
        }],
        key_identifier_method: KeyIdMethod::Sha256,
    }
    .signed_by(issuer)
    .unwrap()
    .der()
    .to_vec()
}
