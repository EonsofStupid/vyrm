#![cfg(feature = "openraft-transport")]

use openraft::metrics::Metric;
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::{Config, Raft, SnapshotPolicy};
use rcgen::{
    date_time_ymd, BasicConstraints, Certificate, CertificateParams,
    CertificateRevocationListParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyIdMethod, KeyPair,
    KeyUsagePurpose, RevocationReason, RevokedCertParams, SanType, SerialNumber,
};
use rustls::pki_types::{CertificateRevocationListDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::RootCertStore;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use vyrm_cluster::{
    build_vyrm_tls_configs, ClusterId, NodeId, PlacementPolicy, ReplicaPlacement, ReplicaRole,
    ShardId, ShardPlacement, VyrmRaftCommand, VyrmRaftNetworkFactory, VyrmRaftNode, VyrmRaftStore,
    VyrmRaftTlsServer, VyrmTlsMaterial, VyrmTlsReloader, VyrmTransportBinding, VyrmTransportGate,
    VyrmTransportTrust, ZoneId, CLUSTER_CONTRACT_VERSION,
};

type VyrmRaft = Raft<vyrm_cluster::VyrmRaftTypeConfig>;

struct TestNode {
    _directory: TempDir,
    raft: VyrmRaft,
    server: JoinHandle<()>,
}

#[test]
fn mutual_tls_transport_replicates_and_denies_identity_confusion() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let cluster = ClusterId::new("cluster:tls-transport").unwrap();
        let trust_domain = "vyrm.test";
        let (ca, issuer) = test_ca();
        let trust = VyrmTransportTrust::new(
            (1..=4).map(|id| (id, NodeId::new(format!("node-{id}")).unwrap())),
        )
        .unwrap();

        let mut listeners = BTreeMap::new();
        let mut nodes = BTreeMap::new();
        for id in 1..=4 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            listeners.insert(id, listener);
            nodes.insert(
                id,
                VyrmRaftNode {
                    canonical_id: format!("node-{id}"),
                    zone: format!("az-{id}"),
                    endpoint: format!("vyrm+tls://{address}?server_name=node-{id}.vyrm.test"),
                },
            );
        }

        let mut materials = BTreeMap::new();
        for id in 1..=4 {
            let binding = binding(trust_domain, &cluster, id);
            materials.insert(
                id,
                test_tls_material(
                    &ca,
                    &issuer,
                    &format!("node-{id}.vyrm.test"),
                    &binding.spiffe_id().unwrap(),
                ),
            );
        }

        let config = Arc::new(
            Config {
                snapshot_policy: SnapshotPolicy::Never,
                max_in_snapshot_log_to_keep: 0,
                purge_batch_size: 1,
                ..Config::default()
            }
            .validate()
            .unwrap(),
        );
        let mut running = BTreeMap::new();
        let mut client_configs = BTreeMap::new();
        let mut reloaders = BTreeMap::new();
        for id in 1..=4 {
            let directory = tempfile::tempdir().unwrap();
            let binding = binding(trust_domain, &cluster, id);
            let reloader =
                VyrmTlsReloader::new(binding.clone(), 1, materials.remove(&id).unwrap()).unwrap();
            let confusion_material = test_tls_material(
                &ca,
                &issuer,
                &format!("node-{id}.vyrm.test"),
                &binding.spiffe_id().unwrap(),
            );
            let (client, _) = build_vyrm_tls_configs(confusion_material).unwrap();
            client_configs.insert(id, client);
            let gate = VyrmTransportGate::enabled();
            let network = VyrmRaftNetworkFactory::new_reloadable(
                binding.clone(),
                reloader.clone(),
                gate.clone(),
            )
            .unwrap();
            let (log, state_machine) = VyrmRaftStore::open(directory.path(), ShardId(7)).unwrap();
            let raft = Raft::new(id, Arc::clone(&config), network, log, state_machine)
                .await
                .unwrap();
            let tls_server = VyrmRaftTlsServer::new_reloadable(
                binding,
                trust.clone(),
                raft.clone(),
                reloader.clone(),
                gate,
            )
            .unwrap();
            let listener = listeners.remove(&id).unwrap();
            let server = tokio::spawn(async move {
                tls_server.serve(listener).await.unwrap();
            });
            running.insert(
                id,
                TestNode {
                    _directory: directory,
                    raft,
                    server,
                },
            );
            reloaders.insert(id, reloader);
        }

        running[&1]
            .raft
            .initialize(BTreeMap::from([(1, nodes[&1].clone())]))
            .await
            .unwrap();
        running[&1].raft.trigger().elect().await.unwrap();
        running[&1]
            .raft
            .wait(Some(Duration::from_secs(5)))
            .current_leader(1, "TLS leader election")
            .await
            .unwrap();
        running[&1]
            .raft
            .add_learner(2, nodes[&2].clone(), true)
            .await
            .unwrap();
        running[&1]
            .raft
            .add_learner(3, nodes[&3].clone(), true)
            .await
            .unwrap();
        running[&1]
            .raft
            .change_membership(BTreeSet::from([1, 2, 3]), false)
            .await
            .unwrap();
        let transition = running[&1]
            .raft
            .client_write(
                VyrmRaftCommand::placement_transition(
                    "tls-placement-1",
                    test_placement(&cluster),
                    None,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(transition.data.accepted);
        let response = running[&1]
            .raft
            .client_write(
                VyrmRaftCommand::new(
                    "tls-probe-1",
                    ShardId(7),
                    1,
                    None,
                    b"authenticated-replication".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.data.accepted);
        for id in 1..=3 {
            running[&id]
                .raft
                .wait(Some(Duration::from_secs(5)))
                .ge(
                    Metric::AppliedIndex(Some(response.log_id.index)),
                    "authenticated apply on every voter",
                )
                .await
                .unwrap();
        }

        let rotated_node_one = test_tls_material(
            &ca,
            &issuer,
            "node-1.vyrm.test",
            &binding(trust_domain, &cluster, 1).spiffe_id().unwrap(),
        );
        let rotated = reloaders[&1].rotate(1, rotated_node_one).unwrap();
        assert_eq!(rotated.generation, 2);
        let stale_generation = reloaders[&1].rotate(
            1,
            test_tls_material(
                &ca,
                &issuer,
                "node-1.vyrm.test",
                &binding(trust_domain, &cluster, 1).spiffe_id().unwrap(),
            ),
        );
        assert!(stale_generation.is_err());
        let after_rotation = running[&1]
            .raft
            .client_write(
                VyrmRaftCommand::new(
                    "tls-probe-after-hot-rotation",
                    ShardId(7),
                    1,
                    Some(response.log_id.index),
                    b"hot-rotation-without-raft-restart".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(after_rotation.data.accepted);
        running[&2]
            .raft
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::AppliedIndex(Some(after_rotation.log_id.index)),
                "hot-rotated leader still replicates",
            )
            .await
            .unwrap();

        let revoked_serial = 90_001_u64;
        let (revoked_client, _) = build_vyrm_tls_configs(test_tls_material_with(
            &ca,
            &issuer,
            "node-1.vyrm.test",
            &binding(trust_domain, &cluster, 1).spiffe_id().unwrap(),
            Some(revoked_serial),
            Vec::new(),
        ))
        .unwrap();
        let crl = test_crl(&issuer, revoked_serial);
        reloaders[&2]
            .rotate(
                1,
                test_tls_material_with(
                    &ca,
                    &issuer,
                    "node-2.vyrm.test",
                    &binding(trust_domain, &cluster, 2).spiffe_id().unwrap(),
                    Some(90_002),
                    vec![crl],
                ),
            )
            .unwrap();
        let mut revoked_factory =
            VyrmRaftNetworkFactory::new(binding(trust_domain, &cluster, 1), revoked_client)
                .unwrap();
        let mut revoked = revoked_factory.new_client(2, &nodes[&2]).await;
        let denied = revoked
            .vote(
                openraft::raft::VoteRequest::new(openraft::Vote::new(101, 1), None),
                RPCOption::new(Duration::from_secs(5)),
            )
            .await;
        assert!(denied.is_err(), "revoked leaf must fail the TLS handshake");

        let (ca_two, issuer_two) = test_ca();
        let ca_one_crl = test_crl(&issuer, revoked_serial);
        let ca_two_crl = test_crl_with(&issuer_two, 2, Vec::new());
        let mut generations = BTreeMap::from([(1, 2), (2, 2), (3, 1), (4, 1)]);
        for id in 1..=4 {
            let expected = generations[&id];
            reloaders[&id]
                .rotate(
                    expected,
                    test_tls_material_with_roots(
                        &[&ca, &ca_two],
                        &issuer,
                        &format!("node-{id}.vyrm.test"),
                        &binding(trust_domain, &cluster, id).spiffe_id().unwrap(),
                        Some(91_000 + id),
                        vec![ca_one_crl.clone(), ca_two_crl.clone()],
                    ),
                )
                .unwrap();
            generations.insert(id, expected + 1);
        }
        for id in 1..=4 {
            let expected = generations[&id];
            reloaders[&id]
                .rotate(
                    expected,
                    test_tls_material_with_roots(
                        &[&ca, &ca_two],
                        &issuer_two,
                        &format!("node-{id}.vyrm.test"),
                        &binding(trust_domain, &cluster, id).spiffe_id().unwrap(),
                        Some(92_000 + id),
                        vec![ca_one_crl.clone(), ca_two_crl.clone()],
                    ),
                )
                .unwrap();
            generations.insert(id, expected + 1);
        }
        for id in 1..=4 {
            let expected = generations[&id];
            reloaders[&id]
                .rotate(
                    expected,
                    test_tls_material_with_roots(
                        &[&ca_two],
                        &issuer_two,
                        &format!("node-{id}.vyrm.test"),
                        &binding(trust_domain, &cluster, id).spiffe_id().unwrap(),
                        Some(93_000 + id),
                        vec![ca_two_crl.clone()],
                    ),
                )
                .unwrap();
            generations.insert(id, expected + 1);
        }
        let after_root_cutover = running[&1]
            .raft
            .client_write(
                VyrmRaftCommand::new(
                    "tls-probe-after-root-cutover",
                    ShardId(7),
                    1,
                    Some(after_rotation.log_id.index),
                    b"root-overlap-and-retirement".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(after_root_cutover.data.accepted);
        for id in 1..=3 {
            running[&id]
                .raft
                .wait(Some(Duration::from_secs(5)))
                .ge(
                    Metric::AppliedIndex(Some(after_root_cutover.log_id.index)),
                    "root-cutover replication",
                )
                .await
                .unwrap();
        }
        let (retired_root_client, _) = build_vyrm_tls_configs(test_tls_material_with_roots(
            &[&ca_two],
            &issuer,
            "node-1.vyrm.test",
            &binding(trust_domain, &cluster, 1).spiffe_id().unwrap(),
            Some(94_001),
            Vec::new(),
        ))
        .unwrap();
        let mut retired_root_factory =
            VyrmRaftNetworkFactory::new(binding(trust_domain, &cluster, 1), retired_root_client)
                .unwrap();
        let mut retired_root = retired_root_factory.new_client(2, &nodes[&2]).await;
        let denied = retired_root
            .vote(
                openraft::raft::VoteRequest::new(openraft::Vote::new(102, 1), None),
                RPCOption::new(Duration::from_secs(5)),
            )
            .await;
        assert!(
            denied.is_err(),
            "retired CA leaf must fail client authentication"
        );

        running[&1].raft.trigger().snapshot().await.unwrap();
        let snapshot_metrics = running[&1]
            .raft
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::Snapshot(Some(response.log_id)),
                "authenticated snapshot publication",
            )
            .await
            .unwrap();
        let snapshot_log = snapshot_metrics.snapshot.unwrap();
        running[&1]
            .raft
            .trigger()
            .purge_log(snapshot_log.index)
            .await
            .unwrap();
        running[&1]
            .raft
            .wait(Some(Duration::from_secs(5)))
            .purged(Some(snapshot_log), "authenticated snapshot log purge")
            .await
            .unwrap();
        running[&1]
            .raft
            .add_learner(4, nodes[&4].clone(), true)
            .await
            .unwrap();
        running[&4]
            .raft
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::AppliedIndex(Some(response.log_id.index)),
                "TLS learner receives snapshot after log purge",
            )
            .await
            .unwrap();

        let mut confused_factory = VyrmRaftNetworkFactory::new(
            binding(trust_domain, &cluster, 1),
            Arc::clone(&client_configs[&3]),
        )
        .unwrap();
        let mut confused = confused_factory.new_client(2, &nodes[&2]).await;
        let vote = openraft::Vote::new(99, 1);
        let denied = confused
            .vote(
                openraft::raft::VoteRequest::new(vote, None),
                RPCOption::new(Duration::from_secs(5)),
            )
            .await;
        assert!(
            denied.is_err(),
            "node-3 certificate must not impersonate node-1"
        );

        let mut forged_vote_factory = VyrmRaftNetworkFactory::new(
            binding(trust_domain, &cluster, 3),
            Arc::clone(&client_configs[&3]),
        )
        .unwrap();
        let mut forged_vote = forged_vote_factory.new_client(2, &nodes[&2]).await;
        let denied = forged_vote
            .vote(
                openraft::raft::VoteRequest::new(openraft::Vote::new(100, 1), None),
                RPCOption::new(Duration::from_secs(5)),
            )
            .await;
        assert!(
            denied.is_err(),
            "authenticated node-3 must not send a Raft vote claiming node-1"
        );

        for node in running.values() {
            node.raft.shutdown().await.unwrap();
            node.server.abort();
        }
    });
}

fn binding(trust_domain: &str, cluster: &ClusterId, id: u64) -> VyrmTransportBinding {
    VyrmTransportBinding {
        trust_domain: trust_domain.into(),
        cluster: cluster.clone(),
        shard: ShardId(7),
        raft_node_id: id,
        canonical_node_id: NodeId::new(format!("node-{id}")).unwrap(),
    }
}

fn test_placement(cluster: &ClusterId) -> ShardPlacement {
    ShardPlacement {
        contract_version: CLUSTER_CONTRACT_VERSION,
        cluster: cluster.clone(),
        shard: ShardId(7),
        epoch: 1,
        policy: PlacementPolicy {
            voter_count: 3,
            minimum_voter_zones: 3,
            maximum_voters_per_zone: 1,
        },
        replicas: (1..=3)
            .map(|id| ReplicaPlacement {
                node: NodeId::new(format!("node-{id}")).unwrap(),
                zone: ZoneId::new(format!("az-{id}")).unwrap(),
                role: ReplicaRole::Voter,
            })
            .collect(),
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

fn test_tls_material(
    ca: &Certificate,
    issuer: &Issuer<'static, KeyPair>,
    dns_name: &str,
    spiffe_id: &str,
) -> VyrmTlsMaterial {
    test_tls_material_with(ca, issuer, dns_name, spiffe_id, None, Vec::new())
}

fn test_tls_material_with(
    ca: &Certificate,
    issuer: &Issuer<'static, KeyPair>,
    dns_name: &str,
    spiffe_id: &str,
    serial: Option<u64>,
    revocation_lists: Vec<CertificateRevocationListDer<'static>>,
) -> VyrmTlsMaterial {
    test_tls_material_with_roots(&[ca], issuer, dns_name, spiffe_id, serial, revocation_lists)
}

fn test_tls_material_with_roots(
    roots_to_add: &[&Certificate],
    issuer: &Issuer<'static, KeyPair>,
    dns_name: &str,
    spiffe_id: &str,
    serial: Option<u64>,
    revocation_lists: Vec<CertificateRevocationListDer<'static>>,
) -> VyrmTlsMaterial {
    let mut params = CertificateParams::new(vec![dns_name.to_owned()]).unwrap();
    params.serial_number = serial.map(SerialNumber::from);
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
    let mut roots = RootCertStore::empty();
    for root in roots_to_add {
        roots.add(root.der().clone()).unwrap();
    }
    VyrmTlsMaterial {
        certificate_chain: vec![certificate.der().clone()],
        private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        trust_roots: roots,
        revocation_lists,
    }
}

fn test_crl(
    issuer: &Issuer<'static, KeyPair>,
    revoked_serial: u64,
) -> CertificateRevocationListDer<'static> {
    test_crl_with(issuer, 1, vec![revoked_serial])
}

fn test_crl_with(
    issuer: &Issuer<'static, KeyPair>,
    crl_number: u64,
    revoked_serials: Vec<u64>,
) -> CertificateRevocationListDer<'static> {
    CertificateRevocationListParams {
        this_update: date_time_ymd(2026, 1, 1),
        next_update: date_time_ymd(2030, 1, 1),
        crl_number: SerialNumber::from(crl_number),
        issuing_distribution_point: None,
        revoked_certs: revoked_serials
            .into_iter()
            .map(|serial| RevokedCertParams {
                serial_number: SerialNumber::from(serial),
                revocation_time: date_time_ymd(2026, 1, 1),
                reason_code: Some(RevocationReason::KeyCompromise),
                invalidity_date: None,
            })
            .collect(),
        key_identifier_method: KeyIdMethod::Sha256,
    }
    .signed_by(issuer)
    .unwrap()
    .into()
}
