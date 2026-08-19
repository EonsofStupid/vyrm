#![cfg(feature = "openraft-transport")]

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, SanType,
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
    VyrmNodeStatus, VyrmRaftNode, VyrmTransportBinding, ZoneId, CLUSTER_CONTRACT_VERSION,
    VYRM_NODE_CONFIG_VERSION, VYRM_NODE_CONTROL_VERSION,
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
        assert!(reply.ok, "node command failed: {command:?}: {reply:?}");
        reply.value.unwrap()
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

    node2.crash();
    let second_index = write_index(node1.command(&VyrmNodeCommand::Probe {
        request_id: "process-probe-2".into(),
        placement_epoch: 1,
        expected_commit_index: Some(first_index),
        payload: b"while-node-two-is-down".to_vec(),
    }));
    node2 = ProcessNode::start(&fixture.configs[&2]);
    wait_applied(&mut node2, second_index);

    node1.crash();
    assert_eq!(node2.command(&VyrmNodeCommand::Elect), VyrmNodeResult::Ack);
    wait_for_leader(&mut node2, 2);
    let leadership_log = node2
        .status()
        .last_log_index
        .expect("elected leader has a leadership log");
    wait_applied(&mut node2, leadership_log);
    let failover_expected = node2.status().last_applied_index;
    let failover_index = write_index(node2.command(&VyrmNodeCommand::Probe {
        request_id: "process-probe-failover".into(),
        placement_epoch: 1,
        expected_commit_index: failover_expected,
        payload: b"new-leader-write".to_vec(),
    }));
    wait_applied(&mut node3, failover_index);

    node1 = ProcessNode::start(&fixture.configs[&1]);
    wait_applied(&mut node1, failover_index);

    assert_eq!(
        node2.command(&VyrmNodeCommand::SetTransportEnabled { enabled: false }),
        VyrmNodeResult::Ack
    );
    assert_eq!(node3.command(&VyrmNodeCommand::Elect), VyrmNodeResult::Ack);
    wait_for_leader(&mut node3, 3);
    let partition_leadership_log = node3
        .status()
        .last_log_index
        .expect("partition successor has a leadership log");
    wait_applied(&mut node3, partition_leadership_log);
    let partition_expected = node3.status().last_applied_index;
    let partition_index = write_index(node3.command(&VyrmNodeCommand::Probe {
        request_id: "process-probe-partition".into(),
        placement_epoch: 1,
        expected_commit_index: partition_expected,
        payload: b"quorum-survives-live-leader-isolation".to_vec(),
    }));
    wait_applied(&mut node1, partition_index);
    assert_eq!(
        node2.command(&VyrmNodeCommand::SetTransportEnabled { enabled: true }),
        VyrmNodeResult::Ack
    );
    wait_applied(&mut node2, partition_index);

    assert_eq!(
        node3.command(&VyrmNodeCommand::TriggerSnapshot),
        VyrmNodeResult::Ack
    );
    let snapshot_index = wait_for_snapshot(&mut node3, partition_index);
    assert_eq!(
        node3.command(&VyrmNodeCommand::PurgeLog {
            index: snapshot_index,
        }),
        VyrmNodeResult::Ack
    );
    wait_for_purge(&mut node3, snapshot_index);

    let mut node4 = ProcessNode::start(&fixture.configs[&4]);
    assert_eq!(
        node3.command(&VyrmNodeCommand::AddLearner { node_id: 4 }),
        VyrmNodeResult::Ack
    );
    wait_applied(&mut node4, partition_index);
    assert!(
        node4.status().snapshot_index.is_some(),
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

fn wait_for_snapshot(node: &mut ProcessNode, at_least: u64) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(index) = node
            .status()
            .snapshot_index
            .filter(|index| *index >= at_least)
        {
            return index;
        }
        assert!(Instant::now() < deadline, "snapshot was not published");
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_purge(node: &mut ProcessNode, at_least: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if node.status().purged_index >= Some(at_least) {
            return;
        }
        assert!(Instant::now() < deadline, "snapshot log was not purged");
        thread::sleep(Duration::from_millis(50));
    }
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
        let mut configs = BTreeMap::new();
        let mut certificates = BTreeMap::new();
        let mut data_roots = BTreeMap::new();
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
            );
            let certificate_path = directory.path().join(format!("node-{id}.der"));
            let key_path = directory.path().join(format!("node-{id}.key.der"));
            fs::write(&certificate_path, certificate).unwrap();
            fs::write(&key_path, key).unwrap();
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
        }
        drop(reservations);
        Self {
            _directory: directory,
            cluster,
            configs,
            certificates,
            data_roots,
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
) -> (Vec<u8>, Vec<u8>) {
    let mut params = CertificateParams::new(vec![dns_name.to_owned()]).unwrap();
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
