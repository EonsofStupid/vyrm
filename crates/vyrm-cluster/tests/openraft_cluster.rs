#![cfg(feature = "openraft-adapter")]

use openraft::error::{InstallSnapshotError, RPCError, RemoteError, Unreachable};
use openraft::metrics::Metric;
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{Config, Raft, SnapshotPolicy};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::RwLock;
use vyrm_cluster::{ShardId, VyrmRaftCommand, VyrmRaftNode, VyrmRaftStore, VyrmRaftTypeConfig};
use vyrm_core::{
    RuntimeCommit, RuntimeMutation, RuntimeRecordSchema, RuntimeSchemaRegistry, RuntimeType,
    ScopeId,
};
use vyrm_store::{Engine, NativeEngine};

type VyrmRaft = Raft<VyrmRaftTypeConfig>;

#[derive(Clone, Default)]
struct NetworkHub {
    nodes: Arc<RwLock<BTreeMap<u64, VyrmRaft>>>,
    blocked: Arc<RwLock<BTreeSet<(u64, u64)>>>,
}

impl NetworkHub {
    async fn register(&self, node: u64, raft: VyrmRaft) {
        self.nodes.write().await.insert(node, raft);
    }

    async fn partition(&self, left: u64, right: u64) {
        let mut blocked = self.blocked.write().await;
        blocked.insert((left, right));
        blocked.insert((right, left));
    }

    async fn target(&self, source: u64, target: u64) -> io::Result<VyrmRaft> {
        if self.blocked.read().await.contains(&(source, target)) {
            return Err(io::Error::other(format!("partition {source}->{target}")));
        }
        self.nodes
            .read()
            .await
            .get(&target)
            .cloned()
            .ok_or_else(|| io::Error::other(format!("node {target} is unavailable")))
    }
}

#[derive(Clone)]
struct TestNetworkFactory {
    source: u64,
    hub: NetworkHub,
}

impl RaftNetworkFactory<VyrmRaftTypeConfig> for TestNetworkFactory {
    type Network = TestNetwork;

    async fn new_client(&mut self, target: u64, _node: &VyrmRaftNode) -> Self::Network {
        TestNetwork {
            source: self.source,
            target,
            hub: self.hub.clone(),
        }
    }
}

struct TestNetwork {
    source: u64,
    target: u64,
    hub: NetworkHub,
}

impl RaftNetwork<VyrmRaftTypeConfig> for TestNetwork {
    async fn append_entries(
        &mut self,
        request: AppendEntriesRequest<VyrmRaftTypeConfig>,
        _option: RPCOption,
    ) -> Result<
        AppendEntriesResponse<u64>,
        RPCError<u64, VyrmRaftNode, openraft::error::RaftError<u64>>,
    > {
        self.hub
            .target(self.source, self.target)
            .await
            .map_err(|error| unreachable(error.to_string()))?
            .append_entries(request)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }

    async fn install_snapshot(
        &mut self,
        request: InstallSnapshotRequest<VyrmRaftTypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<u64>,
        RPCError<u64, VyrmRaftNode, openraft::error::RaftError<u64, InstallSnapshotError>>,
    > {
        self.hub
            .target(self.source, self.target)
            .await
            .map_err(|error| unreachable(error.to_string()))?
            .install_snapshot(request)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }

    async fn vote(
        &mut self,
        request: VoteRequest<u64>,
        _option: RPCOption,
    ) -> Result<VoteResponse<u64>, RPCError<u64, VyrmRaftNode, openraft::error::RaftError<u64>>>
    {
        self.hub
            .target(self.source, self.target)
            .await
            .map_err(|error| unreachable(error.to_string()))?
            .vote(request)
            .await
            .map_err(|error| RPCError::RemoteError(RemoteError::new(self.target, error)))
    }
}

struct TestCluster {
    directories: BTreeMap<u64, TempDir>,
    hub: NetworkHub,
    nodes: BTreeMap<u64, VyrmRaft>,
}

impl TestCluster {
    async fn start(ids: &[u64]) -> Self {
        let hub = NetworkHub::default();
        let mut directories = BTreeMap::new();
        let mut nodes = BTreeMap::new();
        for id in ids {
            let directory = tempfile::tempdir().unwrap();
            let (log, state_machine) = VyrmRaftStore::open(directory.path(), ShardId(5)).unwrap();
            let raft = Raft::new(
                *id,
                test_config(),
                TestNetworkFactory {
                    source: *id,
                    hub: hub.clone(),
                },
                log,
                state_machine,
            )
            .await
            .unwrap();
            hub.register(*id, raft.clone()).await;
            nodes.insert(*id, raft);
            directories.insert(*id, directory);
        }
        Self {
            directories,
            hub,
            nodes,
        }
    }

    fn node(&self, id: u64) -> &VyrmRaft {
        &self.nodes[&id]
    }

    fn directory(&self, id: u64) -> &std::path::Path {
        self.directories[&id].path()
    }

    async fn shutdown(&self) {
        for node in self.nodes.values() {
            node.shutdown().await.unwrap();
        }
    }
}

#[test]
fn real_consensus_elects_fails_over_installs_snapshot_and_changes_membership() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let cluster = TestCluster::start(&[1, 2, 3, 4]).await;
        cluster
            .node(1)
            .initialize(BTreeMap::from([(1, node(1))]))
            .await
            .unwrap();
        cluster.node(1).trigger().elect().await.unwrap();
        cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .current_leader(1, "node 1 election")
            .await
            .unwrap();

        cluster.node(1).add_learner(2, node(2), true).await.unwrap();
        cluster.node(1).add_learner(3, node(3), true).await.unwrap();
        cluster
            .node(1)
            .change_membership(BTreeSet::from([1, 2, 3]), false)
            .await
            .unwrap();
        cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .voter_ids([1, 2, 3], "three-voter membership")
            .await
            .unwrap();

        for index in 0..8 {
            let response = cluster
                .node(1)
                .client_write(
                    VyrmRaftCommand::new(
                        format!("request-{index}"),
                        ShardId(5),
                        1,
                        None,
                        format!("runtime-commit-{index}").into_bytes(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            assert!(response.data.accepted);
        }

        let before_snapshot = cluster.node(1).metrics().borrow().last_applied.unwrap();
        cluster.node(1).trigger().snapshot().await.unwrap();
        cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .ge(Metric::Snapshot(Some(before_snapshot)), "leader snapshot")
            .await
            .unwrap();
        cluster.node(1).add_learner(4, node(4), true).await.unwrap();
        let before_failover = cluster.node(1).metrics().borrow().last_applied.unwrap();
        cluster.node(1).trigger().heartbeat().await.unwrap();
        for id in [2, 3] {
            cluster
                .node(id)
                .wait(Some(Duration::from_secs(5)))
                .ge(
                    Metric::AppliedIndex(Some(before_failover.index)),
                    "voter caught up before failover",
                )
                .await
                .unwrap();
        }
        cluster
            .node(4)
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::AppliedIndex(Some(before_snapshot.index)),
                "snapshot learner catch-up",
            )
            .await
            .unwrap();

        cluster.hub.partition(1, 2).await;
        cluster.hub.partition(1, 3).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        cluster.node(2).trigger().elect().await.unwrap();
        let new_metrics = cluster
            .node(2)
            .wait(Some(Duration::from_secs(5)))
            .current_leader(2, "majority-side failover")
            .await
            .unwrap();
        assert!(new_metrics.current_term >= 2);

        let response = cluster
            .node(2)
            .client_write(
                VyrmRaftCommand::new(
                    "after-failover",
                    ShardId(5),
                    1,
                    None,
                    b"post-failover-runtime-commit".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.data.accepted);
        assert!(response.data.term >= 2);

        cluster
            .node(2)
            .change_membership(BTreeSet::from([2, 3, 4]), false)
            .await
            .unwrap();
        for id in [2, 3, 4] {
            cluster
                .node(id)
                .wait(Some(Duration::from_secs(5)))
                .voter_ids([2, 3, 4], "joint-to-uniform membership")
                .await
                .unwrap();
        }
        cluster.shutdown().await;
    });
}

#[test]
fn real_consensus_replicates_canonical_runtime_truth_to_every_voter() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let cluster = TestCluster::start(&[1, 2, 3, 4]).await;
        cluster
            .node(1)
            .initialize(BTreeMap::from([(1, node(1))]))
            .await
            .unwrap();
        cluster.node(1).trigger().elect().await.unwrap();
        cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .current_leader(1, "runtime leader election")
            .await
            .unwrap();
        cluster.node(1).add_learner(2, node(2), true).await.unwrap();
        cluster.node(1).add_learner(3, node(3), true).await.unwrap();
        cluster
            .node(1)
            .change_membership(BTreeSet::from([1, 2, 3]), false)
            .await
            .unwrap();

        let commit = bootstrap_runtime_commit();
        let response = cluster
            .node(1)
            .client_write(
                VyrmRaftCommand::runtime_commit(
                    "runtime-consensus-1",
                    ShardId(5),
                    1,
                    None,
                    commit.clone(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.data.accepted);
        assert_eq!(
            response.data.runtime_outcome.as_ref().unwrap().commit_id,
            commit.digest()
        );
        for id in [1, 2, 3] {
            cluster
                .node(id)
                .wait(Some(Duration::from_secs(5)))
                .ge(
                    Metric::AppliedIndex(Some(response.log_id.index)),
                    "canonical runtime apply on every voter",
                )
                .await
                .unwrap();
        }

        cluster.node(1).trigger().snapshot().await.unwrap();
        let snapshot_metrics = cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::Snapshot(Some(response.log_id)),
                "runtime-bearing snapshot publication",
            )
            .await
            .unwrap();
        let snapshot_log = snapshot_metrics.snapshot.unwrap();
        cluster
            .node(1)
            .trigger()
            .purge_log(snapshot_log.index)
            .await
            .unwrap();
        cluster
            .node(1)
            .wait(Some(Duration::from_secs(5)))
            .purged(Some(snapshot_log), "runtime-bearing log purge")
            .await
            .unwrap();
        cluster.node(1).add_learner(4, node(4), true).await.unwrap();
        cluster
            .node(4)
            .wait(Some(Duration::from_secs(5)))
            .ge(
                Metric::AppliedIndex(Some(response.log_id.index)),
                "new learner receives runtime truth from snapshot after purge",
            )
            .await
            .unwrap();
        cluster.shutdown().await;

        for id in [1, 2, 3, 4] {
            let engine = NativeEngine::open(cluster.directory(id)).unwrap();
            assert_eq!(engine.runtime_cursor().unwrap(), 1);
            assert!(engine
                .runtime_commit_outcome(&commit.digest())
                .unwrap()
                .is_some());
        }
    });
}

fn node(id: u64) -> VyrmRaftNode {
    VyrmRaftNode {
        canonical_id: format!("node-{id}"),
        zone: format!("az-{id}"),
        endpoint: format!("in-process://node-{id}"),
    }
}

fn test_config() -> Arc<Config> {
    let config = Config {
        cluster_name: "vyrm-m7-test".into(),
        enable_tick: false,
        enable_heartbeat: false,
        snapshot_policy: SnapshotPolicy::Never,
        max_in_snapshot_log_to_keep: 0,
        purge_batch_size: 1,
        replication_lag_threshold: 1,
        ..Config::default()
    };
    Arc::new(config.validate().unwrap())
}

fn unreachable<E>(message: String) -> RPCError<u64, VyrmRaftNode, E>
where
    E: std::error::Error,
{
    RPCError::Unreachable(Unreachable::new(&io::Error::other(message)))
}

fn bootstrap_runtime_commit() -> RuntimeCommit {
    let mut registry = RuntimeSchemaRegistry::empty(1, "consensus bootstrap");
    registry.records.insert(
        RuntimeType::new("reasoning_run").unwrap(),
        RuntimeRecordSchema::default(),
    );
    RuntimeCommit {
        scope: ScopeId::new("cluster:consensus").unwrap(),
        at: 1,
        actor: "agent:cluster-test".into(),
        expected_cursor: 0,
        mutations: vec![RuntimeMutation::Schema { registry }],
    }
}
