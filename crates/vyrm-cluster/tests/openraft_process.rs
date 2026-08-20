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
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use vyrm_cluster::{
    ClusterId, NodeId, PlacementPolicy, ReplicaPlacement, ReplicaRole, ShardId, ShardPlacement,
    VyrmNodeCommand, VyrmNodeConfig, VyrmNodeReply, VyrmNodeRequest, VyrmNodeResult,
    VyrmNodeStatus, VyrmRaftNode, VyrmTlsFiles, VyrmTransportBinding, ZoneId,
    CLUSTER_CONTRACT_VERSION, VYRM_NODE_CONFIG_VERSION, VYRM_NODE_CONTROL_VERSION,
};

const SHARD: ShardId = ShardId(11);

struct ProcessNode {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
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
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let ready = read_reply(&mut output);
        assert!(ready.ok, "node did not become ready: {ready:?}");
        assert!(matches!(ready.value, Some(VyrmNodeResult::Ready { .. })));
        Self {
            child,
            input,
            output,
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
        let reply = read_reply(&mut self.output);
        assert_eq!(reply.request_id.as_deref(), Some(request_id.as_str()));
        reply
    }

    fn status(&mut self) -> VyrmNodeStatus {
        match self.command(&VyrmNodeCommand::Status) {
            VyrmNodeResult::Status { status } => status,
            other => panic!("expected status, got {other:?}"),
        }
    }

    fn crash(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    fn shutdown(mut self) {
        assert_eq!(
            self.command(&VyrmNodeCommand::Shutdown),
            VyrmNodeResult::Ack
        );
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for ProcessNode {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
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
        expected_commit_index: Some(first_index),
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
    wait_applied(&mut node4, partition_index);
    assert!(
        node4.status().snapshot_index >= Some(snapshot_index),
        "post-purge learner must catch up from a physical snapshot"
    );

    node4.shutdown();
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
        timeout_millis: 10_000,
    }) {
        VyrmNodeResult::Status { status } => {
            assert!(status.last_applied_index >= Some(index));
        }
        other => panic!("expected applied status, got {other:?}"),
    }
}

fn snapshot_purge_and_add_learner(
    voters: &mut [&mut ProcessNode],
    at_least: u64,
    learner: u64,
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut active_leader = None;
    let mut snapshot_index = None;
    let mut next_trigger = Instant::now();
    loop {
        let last_statuses = voter_statuses(voters);
        let Some(leader) = quorum_agreed_leader(&last_statuses) else {
            assert!(
                Instant::now() < deadline,
                "voters did not agree on a leader while preparing snapshot recovery: {last_statuses:?}"
            );
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        if active_leader != Some(leader) {
            active_leader = Some(leader);
            snapshot_index = None;
            next_trigger = Instant::now();
        }
        let leader_status = last_statuses
            .iter()
            .find(|status| status.raft_node_id == leader)
            .expect("agreed leader must be one of the voters");
        if snapshot_index.is_none() {
            snapshot_index = leader_status
                .snapshot_index
                .filter(|index| *index >= at_least);
        }
        if Instant::now() >= next_trigger {
            let command = match snapshot_index {
                Some(index) => VyrmNodeCommand::PurgeLog { index },
                None => VyrmNodeCommand::TriggerSnapshot,
            };
            let reply = command_on_voter(voters, leader, &command);
            if !reply.ok {
                active_leader = None;
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            next_trigger = Instant::now() + Duration::from_millis(250);
        }
        if let Some(index) = snapshot_index {
            let current = voter_statuses(voters);
            if quorum_agreed_leader(&current) == Some(leader)
                && current
                    .iter()
                    .find(|status| status.raft_node_id == leader)
                    .is_some_and(|status| status.purged_index >= Some(index))
            {
                let reply = command_on_voter(
                    voters,
                    leader,
                    &VyrmNodeCommand::AddLearner { node_id: learner },
                );
                if reply.ok {
                    assert_eq!(reply.value, Some(VyrmNodeResult::Ack));
                    return index;
                }
                active_leader = None;
            }
        }
        assert!(
            Instant::now() < deadline,
            "current leader did not snapshot, purge, and add the learner: {last_statuses:?}"
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
                raft_node_id: id,
                data_root: data_root.clone(),
                raft_listen: addresses[&id].to_string(),
                nodes: nodes.clone(),
                certificate_der: certificate_path.clone(),
                private_key_der: key_path,
                trust_root_der: trust_root.clone(),
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
