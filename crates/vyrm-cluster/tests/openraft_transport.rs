#![cfg(feature = "openraft-transport")]

use openraft::metrics::Metric;
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::{Config, Raft, SnapshotPolicy};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
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
    VyrmRaftTlsServer, VyrmTlsMaterial, VyrmTransportBinding, VyrmTransportTrust, ZoneId,
    CLUSTER_CONTRACT_VERSION,
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
        for id in 1..=4 {
            let directory = tempfile::tempdir().unwrap();
            let binding = binding(trust_domain, &cluster, id);
            let (client, server) = build_vyrm_tls_configs(materials.remove(&id).unwrap()).unwrap();
            client_configs.insert(id, Arc::clone(&client));
            let network = VyrmRaftNetworkFactory::new(binding.clone(), client).unwrap();
            let (log, state_machine) = VyrmRaftStore::open(directory.path(), ShardId(7)).unwrap();
            let raft = Raft::new(id, Arc::clone(&config), network, log, state_machine)
                .await
                .unwrap();
            let tls_server =
                VyrmRaftTlsServer::new(binding, trust.clone(), raft.clone(), server).unwrap();
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
    let mut roots = RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    VyrmTlsMaterial {
        certificate_chain: vec![certificate.der().clone()],
        private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        trust_roots: roots,
    }
}
