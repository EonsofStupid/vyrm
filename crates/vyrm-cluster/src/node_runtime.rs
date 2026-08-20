//! Process boundary for one durable Vyrm Raft node.
//!
//! Raft traffic uses the authenticated transport. Administrative lifecycle
//! commands use a bounded, versioned JSON-lines protocol over the process's
//! inherited stdin/stdout. Keeping this surface off the network makes the
//! executable safe to supervise while Clyffy grows a separately authenticated
//! management plane.

use crate::{
    ArtifactTransferReceiver, ClusterError, ClusterId, NodeId, Result as ClusterResult, ShardId,
    ShardPlacement, VyrmRaftCommand, VyrmRaftNetworkFactory, VyrmRaftNode, VyrmRaftResponse,
    VyrmRaftStore, VyrmRaftTlsServer, VyrmTlsGeneration, VyrmTlsMaterial, VyrmTlsReloader,
    VyrmTransportBinding, VyrmTransportGate, VyrmTransportTrust,
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
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use vyrm_core::{RuntimeCommit, ScopeId};

pub const VYRM_NODE_CONFIG_VERSION: u16 = 2;
pub const VYRM_NODE_CONTROL_VERSION: u16 = 2;
pub const VYRM_NODE_MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const VYRM_NODE_MAX_CONTROL_LINE_BYTES: usize = 1024 * 1024;

type VyrmRaft = Raft<crate::VyrmRaftTypeConfig>;

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
        .validate()
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
    pub raft_node_id: u64,
    pub current_term: u64,
    pub current_leader: Option<u64>,
    pub last_log_index: Option<u64>,
    pub last_applied_index: Option<u64>,
    pub snapshot_index: Option<u64>,
    pub purged_index: Option<u64>,
    pub state: String,
    pub credentials: VyrmTlsGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum VyrmNodeResult {
    Ready {
        raft_node_id: u64,
    },
    Ack,
    Status {
        status: VyrmNodeStatus,
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
    let network = VyrmRaftNetworkFactory::new_reloadable_with_artifacts(
        binding.clone(),
        credentials.clone(),
        transport_gate.clone(),
        state_machine.clone(),
        application_objects,
        config.project_scope.clone(),
    )?;
    let raft_config = Arc::new(
        Config {
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
    )?;
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
    command: VyrmNodeCommand,
) -> Result<VyrmNodeResult, String> {
    match command {
        VyrmNodeCommand::Status => Ok(VyrmNodeResult::Status {
            status: node_status(raft, state_machine, credentials)?,
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
            raft.add_learner(node_id, node, true)
                .await
                .map(|_| VyrmNodeResult::Ack)
                .map_err(|error| error.to_string())
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
                    status: node_status(raft, state_machine, credentials)
                        .expect("validated TLS credential state remains readable"),
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
) -> Result<VyrmNodeStatus, String> {
    let metrics = raft.metrics().borrow().clone();
    let snapshot_index = state_machine
        .persisted_snapshot_meta()
        .map_err(|error| error.to_string())?
        .and_then(|meta| meta.last_log_id.map(|log| log.index));
    Ok(VyrmNodeStatus {
        raft_node_id: metrics.id,
        current_term: metrics.current_term,
        current_leader: metrics.current_leader,
        last_log_index: metrics.last_log_index,
        last_applied_index: metrics.last_applied.map(|log| log.index),
        snapshot_index,
        purged_index: metrics.purged.map(|log| log.index),
        state: format!("{:?}", metrics.state).to_ascii_lowercase(),
        credentials: credentials.identity().map_err(|error| error.to_string())?,
    })
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
