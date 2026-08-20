//! Process boundary for one durable Vyrm Raft node.
//!
//! Raft traffic uses the authenticated transport. Administrative lifecycle
//! commands use a bounded, versioned JSON-lines protocol over the process's
//! inherited stdin/stdout. Keeping this surface off the network makes the
//! executable safe to supervise while Clyffy grows a separately authenticated
//! management plane.

use crate::transport::VyrmConsensusCommitError;
use crate::{
    artifact_transfer_trace_event, ArtifactTransferObservation, ArtifactTransferObserver,
    ArtifactTransferReceiver, ClusterError, ClusterId, NodeId, Result as ClusterResult, ShardId,
    ShardPlacement, VyrmRaftCommand, VyrmRaftNetworkFactory, VyrmRaftNode, VyrmRaftResponse,
    VyrmRaftStateMachine, VyrmRaftStore, VyrmRaftTlsServer, VyrmTlsGeneration, VyrmTlsMaterial,
    VyrmTlsReloader, VyrmTransportAdmissionPolicy, VyrmTransportBinding, VyrmTransportGate,
    VyrmTransportTelemetrySnapshot, VyrmTransportTrust,
};
use openraft::metrics::Metric;
use openraft::{Config, Raft, SnapshotPolicy};
use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivateKeyDer, PrivatePkcs8KeyDer,
};
use rustls::RootCertStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use vyrm_core::{RuntimeCommit, ScopeId};

pub const VYRM_NODE_CONFIG_VERSION: u16 = 2;
pub const VYRM_NODE_CONTROL_VERSION: u16 = 4;
pub const VYRM_NODE_MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const VYRM_NODE_MAX_CONTROL_LINE_BYTES: usize = 1024 * 1024;
const LEARNER_CATCH_UP_TIMEOUT: Duration = Duration::from_secs(10);
const CONSENSUS_TRACE_COMMIT_RETRIES: usize = 16;
const CONSENSUS_TRACE_ROUTE_RETRIES: usize = 32;

type VyrmRaft = Raft<crate::VyrmRaftTypeConfig>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmRaftTimingPolicy {
    pub heartbeat_interval_millis: u64,
    pub election_timeout_min_millis: u64,
    pub election_timeout_max_millis: u64,
}

impl Default for VyrmRaftTimingPolicy {
    fn default() -> Self {
        Self {
            heartbeat_interval_millis: 250,
            election_timeout_min_millis: 1_000,
            election_timeout_max_millis: 2_000,
        }
    }
}

impl VyrmRaftTimingPolicy {
    pub fn validate(&self) -> ClusterResult<()> {
        if self.heartbeat_interval_millis == 0
            || self.heartbeat_interval_millis > 60_000
            || self.election_timeout_min_millis <= self.heartbeat_interval_millis
            || self.election_timeout_max_millis <= self.election_timeout_min_millis
            || self.election_timeout_max_millis > 300_000
        {
            return Err(ClusterError::Invalid(
                "Raft timing policy is outside its bounded contract".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmConsensusTraceTelemetrySnapshot {
    pub started_at: u64,
    pub observed_at: u64,
    pub prepared_observations: u64,
    pub chunk_observations: u64,
    pub completed_observations: u64,
    pub failed_observations: u64,
    pub commit_acknowledgements: u64,
    pub cursor_conflicts: u64,
    pub leader_changes: u64,
    pub leader_unavailable: u64,
    pub denied: u64,
    pub failed: u64,
    pub overflowed: bool,
}

#[derive(Debug)]
struct ConsensusTraceTelemetry {
    started_at: u64,
    prepared_observations: AtomicU64,
    chunk_observations: AtomicU64,
    completed_observations: AtomicU64,
    failed_observations: AtomicU64,
    commit_acknowledgements: AtomicU64,
    cursor_conflicts: AtomicU64,
    leader_changes: AtomicU64,
    leader_unavailable: AtomicU64,
    denied: AtomicU64,
    failed: AtomicU64,
    overflowed: AtomicBool,
}

impl ConsensusTraceTelemetry {
    fn new(started_at: u64) -> Self {
        Self {
            started_at,
            prepared_observations: AtomicU64::new(0),
            chunk_observations: AtomicU64::new(0),
            completed_observations: AtomicU64::new(0),
            failed_observations: AtomicU64::new(0),
            commit_acknowledgements: AtomicU64::new(0),
            cursor_conflicts: AtomicU64::new(0),
            leader_changes: AtomicU64::new(0),
            leader_unavailable: AtomicU64::new(0),
            denied: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            overflowed: AtomicBool::new(false),
        }
    }

    fn record_observation(&self, observation: &ArtifactTransferObservation) {
        use crate::ArtifactTransferObservationPhase as Phase;
        let counter = match observation.phase {
            Phase::Prepared => &self.prepared_observations,
            Phase::ChunkAccepted => &self.chunk_observations,
            Phase::Completed => &self.completed_observations,
            Phase::Failed => &self.failed_observations,
        };
        telemetry_increment(counter, &self.overflowed);
    }

    fn snapshot(&self, observed_at: u64) -> ClusterResult<VyrmConsensusTraceTelemetrySnapshot> {
        if observed_at < self.started_at {
            return Err(ClusterError::Invalid(
                "consensus trace telemetry observation predates this process".into(),
            ));
        }
        Ok(VyrmConsensusTraceTelemetrySnapshot {
            started_at: self.started_at,
            observed_at,
            prepared_observations: self.prepared_observations.load(Ordering::Relaxed),
            chunk_observations: self.chunk_observations.load(Ordering::Relaxed),
            completed_observations: self.completed_observations.load(Ordering::Relaxed),
            failed_observations: self.failed_observations.load(Ordering::Relaxed),
            commit_acknowledgements: self.commit_acknowledgements.load(Ordering::Relaxed),
            cursor_conflicts: self.cursor_conflicts.load(Ordering::Relaxed),
            leader_changes: self.leader_changes.load(Ordering::Relaxed),
            leader_unavailable: self.leader_unavailable.load(Ordering::Relaxed),
            denied: self.denied.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            overflowed: self.overflowed.load(Ordering::Relaxed),
        })
    }
}

#[derive(Clone)]
struct VyrmNodeTelemetrySources {
    transport: VyrmRaftTlsServer,
    artifacts: ArtifactTransferReceiver,
    traces: Arc<ConsensusTraceTelemetry>,
}

struct ConsensusArtifactTransferObserver {
    raft: Arc<OnceLock<VyrmRaft>>,
    state_machine: VyrmRaftStateMachine,
    network: VyrmRaftNetworkFactory,
    nodes: BTreeMap<u64, VyrmRaftNode>,
    local_node_id: u64,
    project_scope: ScopeId,
    actor: String,
    telemetry: Arc<ConsensusTraceTelemetry>,
}

impl ConsensusArtifactTransferObserver {
    async fn submit(
        &self,
        raft: &VyrmRaft,
        command: VyrmRaftCommand,
    ) -> ClusterResult<openraft::raft::ClientWriteResponse<crate::VyrmRaftTypeConfig>> {
        let mut last_error: Option<String> = None;
        for _ in 0..CONSENSUS_TRACE_ROUTE_RETRIES {
            let metrics = raft.metrics().borrow().clone();
            let Some(leader) = metrics.current_leader else {
                telemetry_increment(
                    &self.telemetry.leader_unavailable,
                    &self.telemetry.overflowed,
                );
                last_error = Some("no current leader".into());
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            };
            let result = if leader == self.local_node_id {
                match raft.client_write(command.clone()).await {
                    Ok(response) => Ok(response),
                    Err(error)
                        if matches!(
                            error.api_error(),
                            Some(openraft::error::ClientWriteError::ForwardToLeader(_))
                        ) =>
                    {
                        Err(VyrmConsensusCommitError::ForwardToLeader)
                    }
                    Err(error) if error.fatal().is_some() => {
                        Err(VyrmConsensusCommitError::Unavailable(error.to_string()))
                    }
                    Err(error) => Err(VyrmConsensusCommitError::Rejected(error.to_string())),
                }
            } else {
                let node = self.nodes.get(&leader).ok_or_else(|| {
                    ClusterError::Unavailable(format!(
                        "current leader {leader} is absent from the node inventory"
                    ))
                })?;
                self.network
                    .submit_runtime_commit(leader, node, command.clone())
                    .await
            };
            match result {
                Ok(response) => return Ok(response),
                Err(VyrmConsensusCommitError::ForwardToLeader) => {
                    telemetry_increment(&self.telemetry.leader_changes, &self.telemetry.overflowed);
                    last_error = Some("leader changed while routing".into());
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(VyrmConsensusCommitError::Rejected(error)) => {
                    return Err(ClusterError::Denied(error))
                }
                Err(VyrmConsensusCommitError::Unavailable(error)) => {
                    return Err(ClusterError::Unavailable(error))
                }
            }
        }
        Err(ClusterError::Unavailable(format!(
            "consensus trace could not route to a stable leader after {CONSENSUS_TRACE_ROUTE_RETRIES} attempts: {}",
            last_error.unwrap_or_else(|| "no routing evidence".into())
        )))
    }

    async fn commit(&self, observation: ArtifactTransferObservation) -> ClusterResult<()> {
        observation.validate()?;
        if observation.scope != self.project_scope {
            return Err(ClusterError::Denied(
                "artifact trace scope differs from the configured project".into(),
            ));
        }
        let event = artifact_transfer_trace_event(&observation)?;
        let event_digest = vyrm_core::digest::sha256_hex(
            &serde_json::to_vec(&event)
                .map_err(|error| ClusterError::Invalid(error.to_string()))?,
        );
        let raft = self.raft.get().cloned().ok_or_else(|| {
            ClusterError::Unavailable("consensus trace writer is not attached to Raft".into())
        })?;
        let mut last_conflict = None;
        for _ in 0..CONSENSUS_TRACE_COMMIT_RETRIES {
            let (read, schema) = self
                .state_machine
                .runtime_commit_context(&observation.scope)?;
            let commit = event
                .prepare_commit(&read, schema.as_ref(), &self.actor)
                .map_err(|error| ClusterError::Invalid(error.to_string()))?;
            let request_id = format!("artifact-trace:{event_digest}:{}", commit.expected_cursor);
            let command = VyrmRaftCommand::runtime_commit(
                request_id,
                observation.shard,
                observation.placement_epoch,
                None,
                commit,
            )?;
            let response = self.submit(&raft, command).await?;
            if response.data.accepted && response.data.runtime_outcome.is_some() {
                telemetry_increment(
                    &self.telemetry.commit_acknowledgements,
                    &self.telemetry.overflowed,
                );
                return Ok(());
            }
            if response
                .data
                .reason
                .contains("runtime commit conflict: expected cursor")
            {
                telemetry_increment(&self.telemetry.cursor_conflicts, &self.telemetry.overflowed);
                last_conflict = Some(response.data.reason);
                continue;
            }
            return Err(ClusterError::Denied(format!(
                "consensus artifact trace commit was rejected: {}",
                response.data.reason
            )));
        }
        Err(ClusterError::Unavailable(format!(
            "consensus artifact trace exhausted {CONSENSUS_TRACE_COMMIT_RETRIES} cursor retries: {}",
            last_conflict.unwrap_or_else(|| "no conflict evidence".into())
        )))
    }
}

impl ArtifactTransferObserver for ConsensusArtifactTransferObserver {
    fn observe(
        &self,
        observation: ArtifactTransferObservation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ClusterResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.telemetry.record_observation(&observation);
            let result = self.commit(observation).await;
            if let Err(error) = &result {
                match error {
                    ClusterError::Denied(_) | ClusterError::Invalid(_) => {
                        telemetry_increment(&self.telemetry.denied, &self.telemetry.overflowed)
                    }
                    ClusterError::Unavailable(_) | ClusterError::NotFound(_) => {
                        telemetry_increment(&self.telemetry.failed, &self.telemetry.overflowed)
                    }
                }
            }
            result
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmNodeConfig {
    pub version: u16,
    pub trust_domain: String,
    pub cluster: ClusterId,
    pub shard: ShardId,
    pub project_scope: ScopeId,
    pub raft_node_id: u64,
    pub data_root: PathBuf,
    pub raft_listen: String,
    pub nodes: BTreeMap<u64, VyrmRaftNode>,
    pub certificate_der: PathBuf,
    pub private_key_der: PathBuf,
    pub trust_root_der: PathBuf,
    #[serde(default)]
    pub transport_admission: VyrmTransportAdmissionPolicy,
    #[serde(default)]
    pub raft_timing: VyrmRaftTimingPolicy,
}

impl VyrmNodeConfig {
    pub fn load(path: &Path) -> ClusterResult<Self> {
        let metadata = fs::metadata(path)
            .map_err(|error| ClusterError::Unavailable(format!("node config metadata: {error}")))?;
        if metadata.len() == 0 || metadata.len() > VYRM_NODE_MAX_CONFIG_BYTES {
            return Err(ClusterError::Invalid(
                "node config must contain 1..=1048576 bytes".into(),
            ));
        }
        let bytes = fs::read(path)
            .map_err(|error| ClusterError::Unavailable(format!("read node config: {error}")))?;
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|error| ClusterError::Invalid(format!("decode node config: {error}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> ClusterResult<()> {
        if self.version != VYRM_NODE_CONFIG_VERSION
            || self.raft_node_id == 0
            || self.nodes.is_empty()
            || self.nodes.len() > 1024
            || self.data_root.as_os_str().is_empty()
        {
            return Err(ClusterError::Invalid(
                "node config version, identity, inventory, or data root is invalid".into(),
            ));
        }
        let local = self.nodes.get(&self.raft_node_id).ok_or_else(|| {
            ClusterError::Invalid("node inventory does not contain the local Raft id".into())
        })?;
        for node in self.nodes.values() {
            node.validate()?;
        }
        self.raft_listen
            .parse::<std::net::SocketAddr>()
            .map_err(|error| {
                ClusterError::Invalid(format!("invalid Raft listen address: {error}"))
            })?;
        VyrmTransportBinding {
            trust_domain: self.trust_domain.clone(),
            cluster: self.cluster.clone(),
            shard: self.shard,
            raft_node_id: self.raft_node_id,
            canonical_node_id: NodeId::new(local.canonical_id.clone())?,
        }
        .validate()?;
        self.transport_admission.validate()?;
        self.raft_timing.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum VyrmNodeCommand {
    Status,
    Initialize,
    Elect,
    AddLearner {
        node_id: u64,
    },
    ChangeMembership {
        voters: BTreeSet<u64>,
    },
    PlacementTransition {
        request_id: String,
        placement: ShardPlacement,
        expected_commit_index: Option<u64>,
    },
    Probe {
        request_id: String,
        placement_epoch: u64,
        expected_commit_index: Option<u64>,
        payload: Vec<u8>,
    },
    RuntimeCommit {
        request_id: String,
        placement_epoch: u64,
        expected_commit_index: Option<u64>,
        commit: RuntimeCommit,
    },
    TriggerSnapshot,
    PurgeLog {
        index: u64,
    },
    WaitApplied {
        index: u64,
        timeout_millis: u64,
    },
    SetTransportEnabled {
        enabled: bool,
    },
    RotateCredentials {
        expected_generation: u64,
        files: VyrmTlsFiles,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTlsFiles {
    pub certificate_der: PathBuf,
    pub private_key_der: PathBuf,
    pub trust_root_ders: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revocation_list_ders: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmNodeRequest {
    pub version: u16,
    pub request_id: String,
    pub command: VyrmNodeCommand,
}

impl VyrmNodeRequest {
    pub fn validate(&self) -> ClusterResult<()> {
        if self.version != VYRM_NODE_CONTROL_VERSION
            || self.request_id.is_empty()
            || self.request_id.len() > 128
            || self.request_id.as_bytes().contains(&0)
        {
            return Err(ClusterError::Invalid(
                "control request version or identity is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmNodeStatus {
    pub project_scope: ScopeId,
    pub cluster: ClusterId,
    pub shard: ShardId,
    pub raft_node_id: u64,
    pub canonical_node_id: NodeId,
    pub current_term: u64,
    pub current_leader: Option<u64>,
    pub last_log_index: Option<u64>,
    pub last_applied_index: Option<u64>,
    pub snapshot_index: Option<u64>,
    pub purged_index: Option<u64>,
    pub state: String,
    pub credentials: VyrmTlsGeneration,
    pub telemetry: VyrmNodeTelemetrySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmNodeTelemetrySnapshot {
    pub observed_at: u64,
    pub transport_ingress: VyrmTransportTelemetrySnapshot,
    pub artifacts: crate::ArtifactTransferTelemetrySnapshot,
    pub consensus_traces: VyrmConsensusTraceTelemetrySnapshot,
}

impl VyrmConsensusTraceTelemetrySnapshot {
    pub fn validate(&self) -> ClusterResult<()> {
        if self.started_at > self.observed_at {
            return Err(ClusterError::Invalid(
                "consensus trace telemetry observation predates its process".into(),
            ));
        }
        Ok(())
    }
}

impl VyrmNodeTelemetrySnapshot {
    pub fn validate(&self) -> ClusterResult<()> {
        if self.transport_ingress.observed_at != self.observed_at
            || self.artifacts.observed_at != self.observed_at
            || self.consensus_traces.observed_at != self.observed_at
        {
            return Err(ClusterError::Invalid(
                "node telemetry sections do not share one observation time".into(),
            ));
        }
        self.transport_ingress.validate()?;
        self.artifacts.validate()?;
        self.consensus_traces.validate()
    }
}

impl VyrmNodeStatus {
    pub fn validate(&self) -> ClusterResult<()> {
        if self.raft_node_id == 0
            || self.state.trim().is_empty()
            || self.state.len() > 64
            || self.credentials.generation == 0
            || self.credentials.leaf_digest.len() != 64
            || !self
                .credentials
                .leaf_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.last_applied_index > self.last_log_index
            || self.purged_index > self.last_log_index
            || self.snapshot_index > self.last_log_index
        {
            return Err(ClusterError::Invalid(
                "node status is outside its bounded contract".into(),
            ));
        }
        self.telemetry.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum VyrmNodeResult {
    Ready {
        raft_node_id: u64,
    },
    Ack,
    Status {
        status: Box<VyrmNodeStatus>,
    },
    Write {
        log_index: u64,
        response: VyrmRaftResponse,
    },
    Credentials {
        credentials: VyrmTlsGeneration,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmNodeReply {
    pub version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<VyrmNodeResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl VyrmNodeReply {
    fn success(request_id: Option<String>, value: VyrmNodeResult) -> Self {
        Self {
            version: VYRM_NODE_CONTROL_VERSION,
            request_id,
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    fn failure(request_id: Option<String>, error: impl ToString) -> Self {
        Self {
            version: VYRM_NODE_CONTROL_VERSION,
            request_id,
            ok: false,
            value: None,
            error: Some(error.to_string()),
        }
    }
}

pub async fn run_vyrm_node(config: VyrmNodeConfig) -> ClusterResult<()> {
    config.validate()?;
    let local = config.nodes[&config.raft_node_id].clone();
    let binding = VyrmTransportBinding {
        trust_domain: config.trust_domain.clone(),
        cluster: config.cluster.clone(),
        shard: config.shard,
        raft_node_id: config.raft_node_id,
        canonical_node_id: NodeId::new(local.canonical_id)?,
    };
    let trust = VyrmTransportTrust::new(config.nodes.iter().map(|(id, node)| {
        (
            *id,
            NodeId::new(node.canonical_id.clone()).expect("validated node identity"),
        )
    }))?;
    let material = load_tls_material(&VyrmTlsFiles {
        certificate_der: config.certificate_der.clone(),
        private_key_der: config.private_key_der.clone(),
        trust_root_ders: vec![config.trust_root_der.clone()],
        revocation_list_ders: Vec::new(),
    })?;
    let credentials = VyrmTlsReloader::new(binding.clone(), 1, material)?;
    let transport_gate = VyrmTransportGate::enabled();
    let (log, state_machine) = VyrmRaftStore::open(&config.data_root, config.shard)?;
    let application_objects = state_machine.application_objects();
    let artifact_receiver = ArtifactTransferReceiver::open(application_objects.clone())?;
    let artifact_status = artifact_receiver.clone();
    let trace_telemetry = Arc::new(ConsensusTraceTelemetry::new(node_now_millis()));
    let raft_slot = Arc::new(OnceLock::new());
    let network = VyrmRaftNetworkFactory::new_reloadable_with_artifacts(
        binding.clone(),
        credentials.clone(),
        transport_gate.clone(),
        state_machine.clone(),
        application_objects,
        config.project_scope.clone(),
    )?;
    let trace_observer = Arc::new(ConsensusArtifactTransferObserver {
        raft: Arc::clone(&raft_slot),
        state_machine: state_machine.clone(),
        network: network.clone(),
        nodes: config.nodes.clone(),
        local_node_id: config.raft_node_id,
        project_scope: config.project_scope.clone(),
        actor: format!("cluster:artifact-transfer:{}", config.raft_node_id),
        telemetry: Arc::clone(&trace_telemetry),
    });
    let network = network.with_artifact_observer(trace_observer);
    let raft_config = Arc::new(
        Config {
            heartbeat_interval: config.raft_timing.heartbeat_interval_millis,
            election_timeout_min: config.raft_timing.election_timeout_min_millis,
            election_timeout_max: config.raft_timing.election_timeout_max_millis,
            snapshot_policy: SnapshotPolicy::Never,
            max_in_snapshot_log_to_keep: 0,
            purge_batch_size: 1,
            ..Config::default()
        }
        .validate()
        .map_err(|error| ClusterError::Invalid(error.to_string()))?,
    );
    let raft = Raft::new(
        config.raft_node_id,
        raft_config,
        network,
        log,
        state_machine.clone(),
    )
    .await
    .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
    raft_slot.set(raft.clone()).map_err(|_| {
        ClusterError::Unavailable("consensus trace writer was attached more than once".into())
    })?;
    let listener = TcpListener::bind(&config.raft_listen)
        .await
        .map_err(|error| ClusterError::Unavailable(format!("bind Raft transport: {error}")))?;
    let server = VyrmRaftTlsServer::new_reloadable_with_artifacts(
        binding,
        trust,
        raft.clone(),
        credentials.clone(),
        transport_gate.clone(),
        artifact_receiver,
        config.project_scope.clone(),
    )?
    .with_admission_policy(config.transport_admission.clone())?;
    let telemetry_sources = VyrmNodeTelemetrySources {
        transport: server.clone(),
        artifacts: artifact_status,
        traces: trace_telemetry,
    };
    let server_task = tokio::spawn(async move { server.serve(listener).await });

    write_reply(&VyrmNodeReply::success(
        None,
        VyrmNodeResult::Ready {
            raft_node_id: config.raft_node_id,
        },
    ))
    .await?;

    let mut input = BufReader::new(tokio::io::stdin());
    while let Some(line) = read_control_line(&mut input)
        .await
        .map_err(|error| ClusterError::Unavailable(format!("read node control: {error}")))?
    {
        let reply = match line {
            Err(()) => VyrmNodeReply::failure(None, "control command exceeds 1048576 bytes"),
            Ok(line) if line.is_empty() => {
                VyrmNodeReply::failure(None, "control command must not be empty")
            }
            Ok(line) => match serde_json::from_slice::<VyrmNodeRequest>(&line) {
                Ok(request) if request.validate().is_err() => VyrmNodeReply::failure(
                    Some(request.request_id),
                    "control request version or identity is invalid",
                ),
                Ok(VyrmNodeRequest {
                    request_id,
                    command: VyrmNodeCommand::Shutdown,
                    ..
                }) => {
                    write_reply(&VyrmNodeReply::success(
                        Some(request_id),
                        VyrmNodeResult::Ack,
                    ))
                    .await?;
                    break;
                }
                Ok(VyrmNodeRequest {
                    request_id,
                    command,
                    ..
                }) => match execute_command(
                    &raft,
                    &state_machine,
                    &config,
                    &transport_gate,
                    &credentials,
                    &telemetry_sources,
                    command,
                )
                .await
                {
                    Ok(value) => VyrmNodeReply::success(Some(request_id), value),
                    Err(error) => VyrmNodeReply::failure(Some(request_id), error),
                },
                Err(error) => {
                    VyrmNodeReply::failure(None, format!("decode control request: {error}"))
                }
            },
        };
        write_reply(&reply).await?;
    }

    raft.shutdown()
        .await
        .map_err(|error| ClusterError::Unavailable(format!("shutdown Raft: {error}")))?;
    server_task.abort();
    Ok(())
}

async fn read_control_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Result<Vec<u8>, ()>>> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let (consumed, newline, eof) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                (0, false, true)
            } else {
                let newline = available.iter().position(|byte| *byte == b'\n');
                let consumed = newline.map_or(available.len(), |index| index + 1);
                let content = if newline.is_some() {
                    &available[..consumed - 1]
                } else {
                    &available[..consumed]
                };
                if !oversized {
                    if line.len().saturating_add(content.len()) > VYRM_NODE_MAX_CONTROL_LINE_BYTES {
                        oversized = true;
                        line.clear();
                    } else {
                        line.extend_from_slice(content);
                    }
                }
                (consumed, newline.is_some(), false)
            }
        };
        if eof {
            return if line.is_empty() && !oversized {
                Ok(None)
            } else if oversized {
                Ok(Some(Err(())))
            } else {
                Ok(Some(Ok(line)))
            };
        }
        reader.consume(consumed);
        if newline {
            return if oversized {
                Ok(Some(Err(())))
            } else {
                Ok(Some(Ok(line)))
            };
        }
    }
}

async fn execute_command(
    raft: &VyrmRaft,
    state_machine: &crate::VyrmRaftStateMachine,
    config: &VyrmNodeConfig,
    transport_gate: &VyrmTransportGate,
    credentials: &VyrmTlsReloader,
    telemetry: &VyrmNodeTelemetrySources,
    command: VyrmNodeCommand,
) -> Result<VyrmNodeResult, String> {
    match command {
        VyrmNodeCommand::Status => Ok(VyrmNodeResult::Status {
            status: Box::new(node_status(
                raft,
                state_machine,
                credentials,
                config,
                telemetry,
            )?),
        }),
        VyrmNodeCommand::Initialize => raft
            .initialize(BTreeMap::from([(
                config.raft_node_id,
                config.nodes[&config.raft_node_id].clone(),
            )]))
            .await
            .map(|_| VyrmNodeResult::Ack)
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::Elect => raft
            .trigger()
            .elect()
            .await
            .map(|_| VyrmNodeResult::Ack)
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::AddLearner { node_id } => {
            let node = config
                .nodes
                .get(&node_id)
                .cloned()
                .ok_or_else(|| format!("node inventory has no node {node_id}"))?;
            let response = raft
                .add_learner(node_id, node, false)
                .await
                .map_err(|error| error.to_string())?;
            let membership_log_id = response.log_id;
            let leader_id = config.raft_node_id;
            let metrics = raft
                .wait(Some(LEARNER_CATCH_UP_TIMEOUT))
                .metrics(
                    |metrics| {
                        let lost_leadership = metrics.current_leader != Some(leader_id);
                        let learner_matched = metrics
                            .replication
                            .as_ref()
                            .and_then(|replication| replication.get(&node_id))
                            .and_then(Option::as_ref)
                            .is_some_and(|matched| matched >= &membership_log_id);
                        lost_leadership || learner_matched
                    },
                    "learner must match its committed membership log",
                )
                .await
                .map_err(|error| error.to_string())?;
            if metrics.current_leader != Some(leader_id) {
                return Err("leadership changed before learner catch-up was proven".into());
            }
            let matched = metrics
                .replication
                .as_ref()
                .and_then(|replication| replication.get(&node_id))
                .and_then(Option::as_ref);
            if matched.is_none_or(|matched| matched < &membership_log_id) {
                return Err("learner catch-up was not proven through its membership log".into());
            }
            Ok(VyrmNodeResult::Ack)
        }
        VyrmNodeCommand::ChangeMembership { voters } => raft
            .change_membership(voters, false)
            .await
            .map(|_| VyrmNodeResult::Ack)
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::PlacementTransition {
            request_id,
            placement,
            expected_commit_index,
        } => {
            write_command(
                raft,
                VyrmRaftCommand::placement_transition(request_id, placement, expected_commit_index)
                    .map_err(|error| error.to_string())?,
            )
            .await
        }
        VyrmNodeCommand::Probe {
            request_id,
            placement_epoch,
            expected_commit_index,
            payload,
        } => {
            write_command(
                raft,
                VyrmRaftCommand::new(
                    request_id,
                    config.shard,
                    placement_epoch,
                    expected_commit_index,
                    payload,
                )
                .map_err(|error| error.to_string())?,
            )
            .await
        }
        VyrmNodeCommand::RuntimeCommit {
            request_id,
            placement_epoch,
            expected_commit_index,
            commit,
        } => {
            if commit.scope != config.project_scope {
                return Err("runtime commit scope differs from the configured project".into());
            }
            write_command(
                raft,
                VyrmRaftCommand::runtime_commit(
                    request_id,
                    config.shard,
                    placement_epoch,
                    expected_commit_index,
                    commit,
                )
                .map_err(|error| error.to_string())?,
            )
            .await
        }
        VyrmNodeCommand::TriggerSnapshot => raft
            .trigger()
            .snapshot()
            .await
            .map(|_| VyrmNodeResult::Ack)
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::PurgeLog { index } => raft
            .trigger()
            .purge_log(index)
            .await
            .map(|_| VyrmNodeResult::Ack)
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::WaitApplied {
            index,
            timeout_millis,
        } => {
            if timeout_millis == 0 || timeout_millis > 60_000 {
                return Err("wait timeout must be within 1..=60000 milliseconds".into());
            }
            raft.wait(Some(Duration::from_millis(timeout_millis)))
                .ge(
                    Metric::AppliedIndex(Some(index)),
                    "process control wait-at-least",
                )
                .await
                .map(|_| VyrmNodeResult::Status {
                    status: Box::new(
                        node_status(raft, state_machine, credentials, config, telemetry)
                            .expect("validated TLS credential state remains readable"),
                    ),
                })
                .map_err(|error| error.to_string())
        }
        VyrmNodeCommand::SetTransportEnabled { enabled } => {
            transport_gate.set_enabled(enabled);
            Ok(VyrmNodeResult::Ack)
        }
        VyrmNodeCommand::RotateCredentials {
            expected_generation,
            files,
        } => credentials
            .rotate(
                expected_generation,
                load_tls_material(&files).map_err(|error| error.to_string())?,
            )
            .map(|credentials| VyrmNodeResult::Credentials { credentials })
            .map_err(|error| error.to_string()),
        VyrmNodeCommand::Shutdown => unreachable!("shutdown is handled by the control loop"),
    }
}

async fn write_command(
    raft: &VyrmRaft,
    command: VyrmRaftCommand,
) -> Result<VyrmNodeResult, String> {
    let response = raft
        .client_write(command)
        .await
        .map_err(|error| error.to_string())?;
    Ok(VyrmNodeResult::Write {
        log_index: response.log_id.index,
        response: response.data,
    })
}

fn node_status(
    raft: &VyrmRaft,
    state_machine: &crate::VyrmRaftStateMachine,
    credentials: &VyrmTlsReloader,
    config: &VyrmNodeConfig,
    telemetry: &VyrmNodeTelemetrySources,
) -> Result<VyrmNodeStatus, String> {
    let metrics = raft.metrics().borrow().clone();
    let snapshot_index = state_machine
        .persisted_snapshot_meta()
        .map_err(|error| error.to_string())?
        .and_then(|meta| meta.last_log_id.map(|log| log.index));
    let observed_at = node_now_millis();
    Ok(VyrmNodeStatus {
        project_scope: config.project_scope.clone(),
        cluster: config.cluster.clone(),
        shard: config.shard,
        raft_node_id: metrics.id,
        canonical_node_id: NodeId::new(config.nodes[&config.raft_node_id].canonical_id.clone())
            .map_err(|error| error.to_string())?,
        current_term: metrics.current_term,
        current_leader: metrics.current_leader,
        last_log_index: metrics.last_log_index,
        last_applied_index: metrics.last_applied.map(|log| log.index),
        snapshot_index,
        purged_index: metrics.purged.map(|log| log.index),
        state: format!("{:?}", metrics.state).to_ascii_lowercase(),
        credentials: credentials.identity().map_err(|error| error.to_string())?,
        telemetry: VyrmNodeTelemetrySnapshot {
            observed_at,
            transport_ingress: telemetry
                .transport
                .telemetry_snapshot(observed_at)
                .map_err(|error| error.to_string())?,
            artifacts: telemetry
                .artifacts
                .telemetry_snapshot(observed_at)
                .map_err(|error| error.to_string())?,
            consensus_traces: telemetry
                .traces
                .snapshot(observed_at)
                .map_err(|error| error.to_string())?,
        },
    })
}

fn node_now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn telemetry_increment(counter: &AtomicU64, overflowed: &AtomicBool) {
    if counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .is_err()
    {
        overflowed.store(true, Ordering::Relaxed);
    }
}

fn load_tls_material(files: &VyrmTlsFiles) -> ClusterResult<VyrmTlsMaterial> {
    if files.trust_root_ders.is_empty()
        || files.trust_root_ders.len() > 32
        || files.revocation_list_ders.len() > 32
    {
        return Err(ClusterError::Invalid(
            "TLS file set requires 1..=32 roots and at most 32 CRLs".into(),
        ));
    }
    let certificate = read_bounded_der(&files.certificate_der, "certificate")?;
    let private_key = read_bounded_der(&files.private_key_der, "private key")?;
    let mut roots = RootCertStore::empty();
    for path in &files.trust_root_ders {
        roots
            .add(CertificateDer::from(read_bounded_der(path, "trust root")?))
            .map_err(|error| ClusterError::Invalid(format!("trust root DER: {error}")))?;
    }
    let revocation_lists = files
        .revocation_list_ders
        .iter()
        .map(|path| {
            read_bounded_der(path, "revocation list").map(CertificateRevocationListDer::from)
        })
        .collect::<ClusterResult<Vec<_>>>()?;
    Ok(VyrmTlsMaterial {
        certificate_chain: vec![CertificateDer::from(certificate)],
        private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key)),
        trust_roots: roots,
        revocation_lists,
    })
}

fn read_bounded_der(path: &Path, label: &str) -> ClusterResult<Vec<u8>> {
    let metadata = fs::metadata(path)
        .map_err(|error| ClusterError::Unavailable(format!("{label} metadata: {error}")))?;
    if metadata.len() == 0 || metadata.len() > VYRM_NODE_MAX_CONFIG_BYTES {
        return Err(ClusterError::Invalid(format!(
            "{label} DER must contain 1..=1048576 bytes"
        )));
    }
    fs::read(path).map_err(|error| ClusterError::Unavailable(format!("read {label}: {error}")))
}

async fn write_reply(reply: &VyrmNodeReply) -> ClusterResult<()> {
    let mut bytes = serde_json::to_vec(reply)
        .map_err(|error| ClusterError::Unavailable(format!("encode node reply: {error}")))?;
    bytes.push(b'\n');
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(&bytes)
        .await
        .map_err(|error| ClusterError::Unavailable(format!("write node reply: {error}")))?;
    stdout
        .flush()
        .await
        .map_err(|error| ClusterError::Unavailable(format!("flush node reply: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_reader_bounds_and_recovers_at_the_next_frame() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut bytes = vec![b'x'; VYRM_NODE_MAX_CONTROL_LINE_BYTES + 1];
                bytes.extend_from_slice(
                    b"\n{\"version\":1,\"request_id\":\"next\",\"command\":{\"command\":\"status\"}}\n",
                );
                let mut reader = BufReader::new(bytes.as_slice());
                assert_eq!(read_control_line(&mut reader).await.unwrap(), Some(Err(())));
                let next = read_control_line(&mut reader)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    serde_json::from_slice::<VyrmNodeRequest>(&next)
                        .unwrap()
                        .command,
                    VyrmNodeCommand::Status,
                );
                assert_eq!(read_control_line(&mut reader).await.unwrap(), None);
            });
    }

    #[test]
    fn control_contract_denies_unknown_fields() {
        let error = serde_json::from_str::<VyrmNodeRequest>(
            r#"{"version":1,"request_id":"test","command":{"command":"status"},"unversioned_surprise":true}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn control_contract_denies_unknown_versions_and_request_identities() {
        for request in [
            VyrmNodeRequest {
                version: VYRM_NODE_CONTROL_VERSION + 1,
                request_id: "future".into(),
                command: VyrmNodeCommand::Status,
            },
            VyrmNodeRequest {
                version: VYRM_NODE_CONTROL_VERSION,
                request_id: String::new(),
                command: VyrmNodeCommand::Status,
            },
        ] {
            assert!(request.validate().is_err());
        }
    }
}
