//! Mutually authenticated, bounded OpenRaft RPC transport.
//!
//! Transport v2 uses one request per TLS connection, disables application
//! bearer credentials, and binds the TLS URI SAN to the canonical cluster/node
//! identity carried by the replicated membership. OpenRaft retains ownership of
//! retry, ordering, duplicate, and chunked-snapshot semantics.

use crate::{
    ArtifactTransferManifest, ArtifactTransferObservation, ArtifactTransferObserver,
    ArtifactTransferReceipt, ArtifactTransferReceiver, ArtifactTransferRpc,
    ArtifactTransferRpcResult, ClusterError, ClusterId, NodeId, Result as ClusterResult, ShardId,
    VyrmRaftCommand, VyrmRaftNode, VyrmRaftStateMachine, VyrmRaftTypeConfig,
    VyrmTransportAdmissionPolicy, VyrmTransportOperation, VyrmTransportOutcome,
    VyrmTransportTelemetry, VyrmTransportTelemetrySnapshot, ARTIFACT_TRANSFER_CHUNK_MAX_BYTES,
};
use openraft::error::{
    ClientWriteError, Fatal, InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError,
    ReplicationClosed, StreamingError, Timeout, Unreachable,
};
use openraft::network::snapshot_transport::{Chunked, SnapshotTransport};
use openraft::network::{RPCOption, RPCTypes, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, ClientWriteResponse, InstallSnapshotRequest,
    InstallSnapshotResponse, SnapshotResponse, VoteRequest, VoteResponse,
};
use openraft::{OptionalSend, Raft, Snapshot, Vote};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use vyrm_core::digest::sha256_hex;
use vyrm_core::ScopeId;
use vyrm_store::LocalObjectStore;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

pub const VYRM_RAFT_TRANSPORT_VERSION: u16 = 2;
pub const VYRM_RAFT_MAX_RPC_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const VYRM_RAFT_MAX_IN_FLIGHT_RPCS: usize = 256;
pub const VYRM_RAFT_SERVER_RPC_TIMEOUT: Duration = Duration::from_secs(30);
pub const VYRM_ARTIFACT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
pub const VYRM_CONSENSUS_COMMIT_RPC_TIMEOUT: Duration = Duration::from_secs(5);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

type VyrmRaft = Raft<VyrmRaftTypeConfig>;
type AppendResult = std::result::Result<AppendEntriesResponse<u64>, RaftError<u64>>;
type SnapshotResult =
    std::result::Result<InstallSnapshotResponse<u64>, RaftError<u64, InstallSnapshotError>>;
type VoteResult = std::result::Result<VoteResponse<u64>, RaftError<u64>>;
type ConsensusRaftError = RaftError<u64, ClientWriteError<u64, VyrmRaftNode>>;
type ConsensusCommitResult =
    std::result::Result<ClientWriteResponse<VyrmRaftTypeConfig>, ConsensusCommitWireError>;

#[derive(Debug, Serialize, Deserialize)]
enum ConsensusCommitWireError {
    Denied(String),
    Raft(ConsensusRaftError),
}

#[derive(Debug)]
pub enum VyrmConsensusCommitError {
    ForwardToLeader,
    Rejected(String),
    Unavailable(String),
}

impl fmt::Display for VyrmConsensusCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForwardToLeader => formatter.write_str("consensus leader changed while routing"),
            Self::Rejected(error) => write!(formatter, "consensus commit rejected: {error}"),
            Self::Unavailable(error) => write!(formatter, "consensus commit unavailable: {error}"),
        }
    }
}

impl std::error::Error for VyrmConsensusCommitError {}

#[derive(Debug, Clone)]
pub struct VyrmTransportGate {
    enabled: Arc<AtomicBool>,
}

impl VyrmTransportGate {
    pub fn enabled() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VyrmTransportBinding {
    pub trust_domain: String,
    pub cluster: ClusterId,
    pub shard: ShardId,
    pub raft_node_id: u64,
    pub canonical_node_id: NodeId,
}

impl VyrmTransportBinding {
    pub fn validate(&self) -> ClusterResult<()> {
        let labels = self.trust_domain.split('.').collect::<Vec<_>>();
        if self.trust_domain.is_empty()
            || self.trust_domain.len() > 253
            || labels.iter().any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || label.bytes().any(|byte| {
                        !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-'
                    })
            })
        {
            return Err(ClusterError::Invalid(
                "transport trust domain must be a bounded lowercase DNS name".into(),
            ));
        }
        Ok(())
    }

    pub fn spiffe_id(&self) -> ClusterResult<String> {
        self.validate()?;
        Ok(format!(
            "spiffe://{}/vyrm/{}/{}",
            self.trust_domain,
            sha256_hex(self.cluster.as_str().as_bytes()),
            sha256_hex(self.canonical_node_id.as_str().as_bytes())
        ))
    }
}

pub struct VyrmTlsMaterial {
    pub certificate_chain: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
    pub trust_roots: RootCertStore,
    pub revocation_lists: Vec<CertificateRevocationListDer<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VyrmTransportTrust {
    peers: BTreeMap<u64, NodeId>,
}

impl VyrmTransportTrust {
    pub fn new(peers: impl IntoIterator<Item = (u64, NodeId)>) -> ClusterResult<Self> {
        let mut trusted = BTreeMap::new();
        let mut canonical_ids = BTreeSet::new();
        for (raft_id, canonical_id) in peers {
            if !canonical_ids.insert(canonical_id.clone()) {
                return Err(ClusterError::Invalid(
                    "transport trust contains a duplicate canonical node id".into(),
                ));
            }
            if trusted.insert(raft_id, canonical_id).is_some() {
                return Err(ClusterError::Invalid(
                    "transport trust contains a duplicate Raft node id".into(),
                ));
            }
        }
        if trusted.is_empty() {
            return Err(ClusterError::Invalid(
                "transport trust must authorize at least one peer".into(),
            ));
        }
        Ok(Self { peers: trusted })
    }

    fn allows(&self, raft_id: u64, canonical_id: &NodeId) -> bool {
        self.peers.get(&raft_id) == Some(canonical_id)
    }
}

pub fn build_vyrm_tls_configs(
    material: VyrmTlsMaterial,
) -> ClusterResult<(Arc<ClientConfig>, Arc<ServerConfig>)> {
    if material.certificate_chain.is_empty() || material.trust_roots.is_empty() {
        return Err(ClusterError::Invalid(
            "TLS identity requires a certificate chain and at least one trust root".into(),
        ));
    }
    let client_verifier = if material.revocation_lists.is_empty() {
        WebPkiClientVerifier::builder(Arc::new(material.trust_roots.clone())).build()
    } else {
        WebPkiClientVerifier::builder(Arc::new(material.trust_roots.clone()))
            .with_crls(material.revocation_lists.clone())
            .only_check_end_entity_revocation()
            .enforce_revocation_expiration()
            .build()
    }
    .map_err(|error| ClusterError::Invalid(format!("client verifier: {error}")))?;
    let server = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(
            material.certificate_chain.clone(),
            material.private_key.clone_key(),
        )
        .map_err(|error| ClusterError::Invalid(format!("server TLS identity: {error}")))?;
    let server_verifier = if material.revocation_lists.is_empty() {
        WebPkiServerVerifier::builder(Arc::new(material.trust_roots)).build()
    } else {
        WebPkiServerVerifier::builder(Arc::new(material.trust_roots))
            .with_crls(material.revocation_lists)
            .only_check_end_entity_revocation()
            .enforce_revocation_expiration()
            .build()
    }
    .map_err(|error| ClusterError::Invalid(format!("server verifier: {error}")))?;
    let client = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(server_verifier)
        .with_client_auth_cert(material.certificate_chain, material.private_key)
        .map_err(|error| ClusterError::Invalid(format!("client TLS identity: {error}")))?;
    Ok((Arc::new(client), Arc::new(server)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VyrmTlsGeneration {
    pub generation: u64,
    pub leaf_digest: String,
}

#[derive(Debug)]
struct VyrmTlsState {
    identity: VyrmTlsGeneration,
    client: Arc<ClientConfig>,
    server: Arc<ServerConfig>,
}

#[derive(Debug, Clone)]
pub struct VyrmTlsReloader {
    binding: VyrmTransportBinding,
    state: Arc<RwLock<VyrmTlsState>>,
}

impl VyrmTlsReloader {
    pub fn new(
        binding: VyrmTransportBinding,
        generation: u64,
        material: VyrmTlsMaterial,
    ) -> ClusterResult<Self> {
        if generation == 0 {
            return Err(ClusterError::Invalid(
                "TLS credential generation must begin above zero".into(),
            ));
        }
        let state = build_tls_state(&binding, generation, material)?;
        Ok(Self {
            binding,
            state: Arc::new(RwLock::new(state)),
        })
    }

    pub fn rotate(
        &self,
        expected_generation: u64,
        material: VyrmTlsMaterial,
    ) -> ClusterResult<VyrmTlsGeneration> {
        let next_generation = expected_generation
            .checked_add(1)
            .ok_or_else(|| ClusterError::Invalid("TLS credential generation overflow".into()))?;
        let next = build_tls_state(&self.binding, next_generation, material)?;
        let identity = next.identity.clone();
        let mut state = self
            .state
            .write()
            .map_err(|_| ClusterError::Unavailable("TLS credential state is poisoned".into()))?;
        if state.identity.generation != expected_generation {
            return Err(ClusterError::Denied(format!(
                "TLS credential generation expected {expected_generation} but was {}",
                state.identity.generation
            )));
        }
        *state = next;
        Ok(identity)
    }

    pub fn identity(&self) -> ClusterResult<VyrmTlsGeneration> {
        self.state
            .read()
            .map(|state| state.identity.clone())
            .map_err(|_| ClusterError::Unavailable("TLS credential state is poisoned".into()))
    }

    fn client_config(&self) -> ClusterResult<Arc<ClientConfig>> {
        self.state
            .read()
            .map(|state| Arc::clone(&state.client))
            .map_err(|_| ClusterError::Unavailable("TLS credential state is poisoned".into()))
    }

    fn server_config(&self) -> ClusterResult<Arc<ServerConfig>> {
        self.state
            .read()
            .map(|state| Arc::clone(&state.server))
            .map_err(|_| ClusterError::Unavailable("TLS credential state is poisoned".into()))
    }
}

fn build_tls_state(
    binding: &VyrmTransportBinding,
    generation: u64,
    material: VyrmTlsMaterial,
) -> ClusterResult<VyrmTlsState> {
    verify_vyrm_certificate_identity(Some(&material.certificate_chain), binding)?;
    let leaf = material
        .certificate_chain
        .first()
        .ok_or_else(|| ClusterError::Invalid("TLS identity has no leaf certificate".into()))?;
    let identity = VyrmTlsGeneration {
        generation,
        leaf_digest: sha256_hex(leaf.as_ref()),
    };
    let (client, server) = build_vyrm_tls_configs(material)?;
    Ok(VyrmTlsState {
        identity,
        client,
        server,
    })
}

#[derive(Clone)]
pub struct VyrmRaftNetworkFactory {
    binding: VyrmTransportBinding,
    connector: TlsConnector,
    gate: VyrmTransportGate,
    reloader: Option<VyrmTlsReloader>,
    artifact_source: Option<VyrmArtifactSource>,
}

#[derive(Clone)]
struct VyrmArtifactSource {
    state_machine: VyrmRaftStateMachine,
    objects: LocalObjectStore,
    scope: ScopeId,
    observer: Option<Arc<dyn ArtifactTransferObserver>>,
    attempt_counter: Arc<AtomicU64>,
}

async fn observe_artifact(
    source: &VyrmArtifactSource,
    observation: ArtifactTransferObservation,
) -> ClusterResult<()> {
    observation.validate()?;
    match &source.observer {
        Some(observer) => observer.observe(observation).await,
        None => Ok(()),
    }
}

impl VyrmRaftNetworkFactory {
    pub fn new(binding: VyrmTransportBinding, client: Arc<ClientConfig>) -> ClusterResult<Self> {
        Self::new_with_gate(binding, client, VyrmTransportGate::enabled())
    }

    pub fn new_with_gate(
        binding: VyrmTransportBinding,
        client: Arc<ClientConfig>,
        gate: VyrmTransportGate,
    ) -> ClusterResult<Self> {
        binding.validate()?;
        Ok(Self {
            binding,
            connector: TlsConnector::from(client),
            gate,
            reloader: None,
            artifact_source: None,
        })
    }

    pub fn new_reloadable(
        binding: VyrmTransportBinding,
        credentials: VyrmTlsReloader,
        gate: VyrmTransportGate,
    ) -> ClusterResult<Self> {
        if binding != credentials.binding {
            return Err(ClusterError::Denied(
                "TLS credential binding differs from the Raft network binding".into(),
            ));
        }
        let client = credentials.client_config()?;
        binding.validate()?;
        Ok(Self {
            binding,
            connector: TlsConnector::from(client),
            gate,
            reloader: Some(credentials),
            artifact_source: None,
        })
    }

    pub fn new_reloadable_with_artifacts(
        binding: VyrmTransportBinding,
        credentials: VyrmTlsReloader,
        gate: VyrmTransportGate,
        state_machine: VyrmRaftStateMachine,
        objects: LocalObjectStore,
        scope: ScopeId,
    ) -> ClusterResult<Self> {
        let mut factory = Self::new_reloadable(binding, credentials, gate)?;
        factory.artifact_source = Some(VyrmArtifactSource {
            state_machine,
            objects,
            scope,
            observer: None,
            attempt_counter: Arc::new(AtomicU64::new(0)),
        });
        Ok(factory)
    }

    pub fn with_artifact_observer(mut self, observer: Arc<dyn ArtifactTransferObserver>) -> Self {
        if let Some(source) = &mut self.artifact_source {
            source.observer = Some(observer);
        }
        self
    }

    pub async fn submit_runtime_commit(
        &self,
        target: u64,
        node: &VyrmRaftNode,
        command: VyrmRaftCommand,
    ) -> std::result::Result<ClientWriteResponse<VyrmRaftTypeConfig>, VyrmConsensusCommitError>
    {
        let mut factory = self.clone();
        let client = factory.new_client(target, node).await;
        client.submit_runtime_commit(command).await
    }
}

pub struct VyrmRaftNetworkClient {
    source: VyrmTransportBinding,
    target_id: u64,
    target: VyrmRaftNode,
    connector: TlsConnector,
    gate: VyrmTransportGate,
    reloader: Option<VyrmTlsReloader>,
    artifact_source: Option<VyrmArtifactSource>,
    hydrated_snapshot: Option<String>,
}

impl RaftNetworkFactory<VyrmRaftTypeConfig> for VyrmRaftNetworkFactory {
    type Network = VyrmRaftNetworkClient;

    async fn new_client(&mut self, target: u64, node: &VyrmRaftNode) -> Self::Network {
        VyrmRaftNetworkClient {
            source: self.binding.clone(),
            target_id: target,
            target: node.clone(),
            connector: self.connector.clone(),
            gate: self.gate.clone(),
            reloader: self.reloader.clone(),
            artifact_source: self.artifact_source.clone(),
            hydrated_snapshot: None,
        }
    }
}

impl RaftNetwork<VyrmRaftTypeConfig> for VyrmRaftNetworkClient {
    async fn append_entries(
        &mut self,
        request: AppendEntriesRequest<VyrmRaftTypeConfig>,
        option: RPCOption,
    ) -> std::result::Result<AppendEntriesResponse<u64>, RPCError<u64, VyrmRaftNode, RaftError<u64>>>
    {
        match self
            .call_with_timeout(
                WireRequest::Append(request),
                &option,
                RPCTypes::AppendEntries,
            )
            .await?
        {
            WireResponse::Append(result) => {
                remote_result(self.target_id, self.target.clone(), result)
            }
            _ => Err(unreachable(
                "transport response kind did not match append request",
            )),
        }
    }

    async fn install_snapshot(
        &mut self,
        request: InstallSnapshotRequest<VyrmRaftTypeConfig>,
        option: RPCOption,
    ) -> std::result::Result<
        InstallSnapshotResponse<u64>,
        RPCError<u64, VyrmRaftNode, RaftError<u64, InstallSnapshotError>>,
    > {
        // `full_snapshot` hydrates the closure once per transfer attempt before
        // OpenRaft starts its per-chunk deadline. Keep this fallback for direct
        // `install_snapshot` callers, but never repeat hydration merely because
        // the first chunk has offset zero: doing so moves even an idempotent
        // target round-trip back inside OpenRaft's short chunk timeout.
        if artifact_hydration_required(
            self.artifact_source.is_some(),
            self.hydrated_snapshot.as_deref(),
            &request.meta.snapshot_id,
        ) {
            let attempt = self
                .next_artifact_attempt()
                .map_err(|error| unreachable(error.to_string()))?;
            self.hydrate_snapshot_artifacts(&request.meta, attempt)
                .await
                .map_err(|error| unreachable(error.to_string()))?;
            self.hydrated_snapshot = Some(request.meta.snapshot_id.clone());
        }
        match self
            .call_with_timeout(
                WireRequest::Snapshot(request),
                &option,
                RPCTypes::InstallSnapshot,
            )
            .await?
        {
            WireResponse::Snapshot(result) => {
                remote_result(self.target_id, self.target.clone(), result)
            }
            _ => Err(unreachable(
                "transport response kind did not match snapshot request",
            )),
        }
    }

    async fn vote(
        &mut self,
        request: VoteRequest<u64>,
        option: RPCOption,
    ) -> std::result::Result<VoteResponse<u64>, RPCError<u64, VyrmRaftNode, RaftError<u64>>> {
        match self
            .call_with_timeout(WireRequest::Vote(request), &option, RPCTypes::Vote)
            .await?
        {
            WireResponse::Vote(result) => {
                remote_result(self.target_id, self.target.clone(), result)
            }
            _ => Err(unreachable(
                "transport response kind did not match vote request",
            )),
        }
    }

    async fn full_snapshot(
        &mut self,
        vote: Vote<u64>,
        snapshot: Snapshot<VyrmRaftTypeConfig>,
        cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        option: RPCOption,
    ) -> std::result::Result<SnapshotResponse<u64>, StreamingError<VyrmRaftTypeConfig, Fatal<u64>>>
    {
        let mut cancel = Box::pin(cancel);
        if self.artifact_source.is_some() {
            let attempt = self
                .next_artifact_attempt()
                .map_err(|error| NetworkError::new(&io::Error::other(error.to_string())))?;
            tokio::select! {
                closed = cancel.as_mut() => return Err(closed.into()),
                hydrated = self.hydrate_snapshot_artifacts(&snapshot.meta, attempt) => {
                    if let Err(error) = hydrated {
                        let error = io::Error::other(error.to_string());
                        return Err(NetworkError::new(&error).into());
                    }
                }
            }
            self.hydrated_snapshot = Some(snapshot.meta.snapshot_id.clone());
        }
        Chunked::send_snapshot(self, vote, snapshot, cancel, option).await
    }
}

fn artifact_hydration_required(
    artifact_source_enabled: bool,
    hydrated_snapshot: Option<&str>,
    requested_snapshot: &str,
) -> bool {
    artifact_source_enabled && hydrated_snapshot != Some(requested_snapshot)
}

impl VyrmRaftNetworkClient {
    async fn submit_runtime_commit(
        &self,
        command: VyrmRaftCommand,
    ) -> std::result::Result<ClientWriteResponse<VyrmRaftTypeConfig>, VyrmConsensusCommitError>
    {
        command
            .validate()
            .map_err(|error| VyrmConsensusCommitError::Rejected(error.to_string()))?;
        if !matches!(
            command.operation,
            crate::VyrmRaftOperation::RuntimeCommit { .. }
        ) {
            return Err(VyrmConsensusCommitError::Rejected(
                "internal consensus route accepts only runtime commits".into(),
            ));
        }
        let response = tokio::time::timeout(
            VYRM_CONSENSUS_COMMIT_RPC_TIMEOUT,
            self.call::<std::io::Error>(WireRequest::RuntimeCommit(Box::new(command))),
        )
        .await
        .map_err(|_| {
            VyrmConsensusCommitError::Unavailable("consensus commit RPC timed out".into())
        })?
        .map_err(|error| VyrmConsensusCommitError::Unavailable(error.to_string()))?;
        match response {
            WireResponse::RuntimeCommit(Ok(response)) => Ok(response),
            WireResponse::RuntimeCommit(Err(ConsensusCommitWireError::Raft(error)))
                if matches!(
                    error.api_error(),
                    Some(ClientWriteError::ForwardToLeader(_))
                ) =>
            {
                Err(VyrmConsensusCommitError::ForwardToLeader)
            }
            WireResponse::RuntimeCommit(Err(ConsensusCommitWireError::Denied(error))) => {
                Err(VyrmConsensusCommitError::Rejected(error))
            }
            WireResponse::RuntimeCommit(Err(ConsensusCommitWireError::Raft(error))) => {
                Err(VyrmConsensusCommitError::Rejected(error.to_string()))
            }
            _ => Err(VyrmConsensusCommitError::Unavailable(
                "consensus commit response kind did not match request".into(),
            )),
        }
    }

    fn next_artifact_attempt(&self) -> ClusterResult<u64> {
        let source = self.artifact_source.as_ref().ok_or_else(|| {
            ClusterError::Unavailable("artifact transfer source is not configured".into())
        })?;
        source
            .attempt_counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |attempt| {
                attempt.checked_add(1)
            })
            .map(|attempt| attempt + 1)
            .map_err(|_| {
                ClusterError::Unavailable("artifact transfer attempt counter overflowed".into())
            })
    }

    async fn hydrate_snapshot_artifacts(
        &self,
        meta: &openraft::SnapshotMeta<u64, VyrmRaftNode>,
        attempt: u64,
    ) -> ClusterResult<()> {
        let Some(source) = self.artifact_source.clone() else {
            return Ok(());
        };
        let state_machine = source.state_machine.clone();
        let meta = meta.clone();
        let scope = source.scope.clone();
        let source_id = self.source.canonical_node_id.clone();
        let target_id = NodeId::new(self.target.canonical_id.clone())?;
        let manifest = tokio::task::spawn_blocking(move || {
            state_machine
                .artifact_manifest_for_cached_snapshot(&meta, &scope, source_id, target_id)
                .map_err(|error| ClusterError::Unavailable(error.to_string()))
        })
        .await
        .map_err(|error| ClusterError::Unavailable(format!("artifact manifest task: {error}")))??;
        let Some(manifest) = manifest else {
            return Ok(());
        };
        observe_artifact(
            &source,
            ArtifactTransferObservation::prepared(&manifest, attempt, now_millis())?,
        )
        .await?;
        let started = Instant::now();
        match self
            .transfer_artifact_manifest(&source, &manifest, attempt)
            .await
        {
            Ok(receipt) => {
                observe_artifact(
                    &source,
                    ArtifactTransferObservation::completed(
                        &manifest,
                        attempt,
                        now_millis(),
                        started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
                        &receipt,
                    )?,
                )
                .await
            }
            Err(error) => {
                let rendered = error.to_string();
                let observation = ArtifactTransferObservation::failed(
                    &manifest,
                    attempt,
                    now_millis(),
                    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
                    &rendered,
                )?;
                if let Err(observer_error) = observe_artifact(&source, observation).await {
                    return Err(ClusterError::Unavailable(format!(
                        "{rendered}; artifact failure observation: {observer_error}"
                    )));
                }
                Err(error)
            }
        }
    }

    async fn transfer_artifact_manifest(
        &self,
        source: &VyrmArtifactSource,
        manifest: &ArtifactTransferManifest,
        attempt: u64,
    ) -> ClusterResult<ArtifactTransferReceipt> {
        manifest.validate()?;
        let progress = self
            .call_artifact(ArtifactTransferRpc::begin(manifest.clone())?)
            .await?;
        let ArtifactTransferRpcResult::Progress {
            manifest_digest,
            objects,
        } = progress
        else {
            return Err(ClusterError::Unavailable(
                "artifact begin response had the wrong result kind".into(),
            ));
        };
        if manifest_digest != manifest.manifest_digest {
            return Err(ClusterError::Denied(
                "artifact begin response changed the manifest digest".into(),
            ));
        }
        let mut progress = objects
            .into_iter()
            .map(|object| (object.sha256.clone(), object))
            .collect::<BTreeMap<_, _>>();
        let distinct = manifest
            .objects
            .iter()
            .map(|object| (object.sha256.clone(), object.clone()))
            .collect::<BTreeMap<_, _>>();
        if progress.len() != distinct.len()
            || progress.keys().ne(distinct.keys())
            || progress.iter().any(|(digest, state)| {
                state.expected_length != distinct[digest].length
                    || state.next_offset > state.expected_length
                    || state.complete != (state.next_offset == state.expected_length)
            })
        {
            return Err(ClusterError::Denied(
                "artifact begin response differs from the manifest closure".into(),
            ));
        }
        for (sha256, object) in distinct {
            let mut state = progress
                .remove(&sha256)
                .expect("validated progress contains every digest");
            if state.complete {
                continue;
            }
            let reference = object.clone();
            let source_store = source.objects.clone();
            let path = tokio::task::spawn_blocking(move || source_store.verified_path(&reference))
                .await
                .map_err(|error| {
                    ClusterError::Unavailable(format!("artifact source verify task: {error}"))
                })?
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
            let mut file = tokio::fs::File::open(path)
                .await
                .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
            while !state.complete {
                file.seek(std::io::SeekFrom::Start(state.next_offset))
                    .await
                    .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
                let remaining = state.expected_length - state.next_offset;
                let chunk_length = remaining.min(ARTIFACT_TRANSFER_CHUNK_MAX_BYTES as u64) as usize;
                let mut bytes = vec![0u8; chunk_length];
                file.read_exact(&mut bytes)
                    .await
                    .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
                let sent_offset = state.next_offset;
                let response = self
                    .call_artifact(ArtifactTransferRpc::chunk(
                        manifest.manifest_digest.clone(),
                        sha256.clone(),
                        sent_offset,
                        bytes,
                    )?)
                    .await?;
                let ArtifactTransferRpcResult::ChunkAccepted {
                    manifest_digest,
                    object: next,
                } = response
                else {
                    return Err(ClusterError::Unavailable(
                        "artifact chunk response had the wrong result kind".into(),
                    ));
                };
                if manifest_digest != manifest.manifest_digest
                    || next.sha256 != sha256
                    || next.expected_length != object.length
                    || next.next_offset > object.length
                    || next.complete != (next.next_offset == object.length)
                    || next.next_offset == sent_offset
                {
                    return Err(ClusterError::Denied(
                        "artifact chunk response did not make valid progress".into(),
                    ));
                }
                observe_artifact(
                    source,
                    ArtifactTransferObservation::progress(manifest, attempt, now_millis(), &next)?,
                )
                .await?;
                state = next;
            }
        }
        let completed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let response = self
            .call_artifact(ArtifactTransferRpc::complete(
                manifest.manifest_digest.clone(),
                completed_at,
            )?)
            .await?;
        let ArtifactTransferRpcResult::Completed { receipt } = response else {
            return Err(ClusterError::Unavailable(
                "artifact completion response had the wrong result kind".into(),
            ));
        };
        receipt.validate(manifest)?;
        Ok(receipt)
    }

    async fn call_artifact(
        &self,
        request: ArtifactTransferRpc,
    ) -> ClusterResult<ArtifactTransferRpcResult> {
        let response = tokio::time::timeout(
            VYRM_ARTIFACT_RPC_TIMEOUT,
            self.call::<std::io::Error>(WireRequest::Artifact(request)),
        )
        .await
        .map_err(|_| ClusterError::Unavailable("artifact RPC timed out".into()))?
        .map_err(|error| ClusterError::Unavailable(error.to_string()))?;
        match response {
            WireResponse::Artifact(Ok(response)) => Ok(response),
            WireResponse::Artifact(Err(error)) => Err(ClusterError::Unavailable(error)),
            _ => Err(ClusterError::Unavailable(
                "artifact response kind did not match request".into(),
            )),
        }
    }

    async fn call_with_timeout<E>(
        &self,
        request: WireRequest,
        option: &RPCOption,
        action: RPCTypes,
    ) -> std::result::Result<WireResponse, RPCError<u64, VyrmRaftNode, E>>
    where
        E: std::error::Error,
    {
        match tokio::time::timeout(option.hard_ttl(), self.call(request)).await {
            Ok(result) => result,
            Err(_) => Err(RPCError::Timeout(Timeout {
                action,
                id: self.source.raft_node_id,
                target: self.target_id,
                timeout: option.hard_ttl(),
            })),
        }
    }

    async fn call<E>(
        &self,
        request: WireRequest,
    ) -> std::result::Result<WireResponse, RPCError<u64, VyrmRaftNode, E>>
    where
        E: std::error::Error,
    {
        if !self.gate.is_enabled() {
            return Err(unreachable("local Raft transport is disabled"));
        }
        self.target
            .validate()
            .map_err(|error| unreachable(error.to_string()))?;
        let endpoint = VyrmTlsEndpoint::parse(&self.target.endpoint)
            .map_err(|error| unreachable(error.to_string()))?;
        let stream = TcpStream::connect(&endpoint.address)
            .await
            .map_err(|error| RPCError::Unreachable(Unreachable::new(&error)))?;
        let server_name = ServerName::try_from(endpoint.server_name.clone())
            .map_err(|error| unreachable(format!("invalid TLS server name: {error}")))?;
        let connector = self
            .reloader
            .as_ref()
            .map(VyrmTlsReloader::client_config)
            .transpose()
            .map_err(|error| unreachable(error.to_string()))?
            .map(TlsConnector::from)
            .unwrap_or_else(|| self.connector.clone());
        let mut stream = connector
            .connect(server_name, stream)
            .await
            .map_err(|error| RPCError::Unreachable(Unreachable::new(&error)))?;
        let expected_peer = peer_binding_for_node(&self.source, self.target_id, &self.target)
            .map_err(|error| unreachable(error.to_string()))?;
        verify_peer_identity(stream.get_ref().1.peer_certificates(), &expected_peer)
            .map_err(|error| unreachable(error.to_string()))?;
        let envelope = WireEnvelope::new(&self.source, self.target_id, &self.target, request)
            .map_err(|error| unreachable(error.to_string()))?;
        write_frame(&mut stream, &envelope)
            .await
            .map_err(|error| RPCError::Unreachable(Unreachable::new(&error)))?;
        read_frame(&mut stream)
            .await
            .map_err(|error| RPCError::Unreachable(Unreachable::new(&error)))
    }
}

#[derive(Clone)]
pub struct VyrmRaftTlsServer {
    binding: VyrmTransportBinding,
    trust: VyrmTransportTrust,
    raft: VyrmRaft,
    acceptor: TlsAcceptor,
    admission: Arc<Semaphore>,
    gate: VyrmTransportGate,
    reloader: Option<VyrmTlsReloader>,
    artifacts: Option<ArtifactTransferReceiver>,
    project_scope: Option<ScopeId>,
    telemetry: VyrmTransportTelemetry,
}

impl VyrmRaftTlsServer {
    pub fn new(
        binding: VyrmTransportBinding,
        trust: VyrmTransportTrust,
        raft: VyrmRaft,
        server: Arc<ServerConfig>,
    ) -> ClusterResult<Self> {
        Self::new_with_gate(binding, trust, raft, server, VyrmTransportGate::enabled())
    }

    pub fn new_with_gate(
        binding: VyrmTransportBinding,
        trust: VyrmTransportTrust,
        raft: VyrmRaft,
        server: Arc<ServerConfig>,
        gate: VyrmTransportGate,
    ) -> ClusterResult<Self> {
        binding.validate()?;
        let telemetry =
            VyrmTransportTelemetry::new(VyrmTransportAdmissionPolicy::default(), now_millis())?;
        Ok(Self {
            binding,
            trust,
            raft,
            acceptor: TlsAcceptor::from(server),
            admission: Arc::new(Semaphore::new(VYRM_RAFT_MAX_IN_FLIGHT_RPCS)),
            gate,
            reloader: None,
            artifacts: None,
            project_scope: None,
            telemetry,
        })
    }

    pub fn new_reloadable(
        binding: VyrmTransportBinding,
        trust: VyrmTransportTrust,
        raft: VyrmRaft,
        credentials: VyrmTlsReloader,
        gate: VyrmTransportGate,
    ) -> ClusterResult<Self> {
        if binding != credentials.binding {
            return Err(ClusterError::Denied(
                "TLS credential binding differs from the Raft server binding".into(),
            ));
        }
        let server = credentials.server_config()?;
        binding.validate()?;
        let telemetry =
            VyrmTransportTelemetry::new(VyrmTransportAdmissionPolicy::default(), now_millis())?;
        Ok(Self {
            binding,
            trust,
            raft,
            acceptor: TlsAcceptor::from(server),
            admission: Arc::new(Semaphore::new(VYRM_RAFT_MAX_IN_FLIGHT_RPCS)),
            gate,
            reloader: Some(credentials),
            artifacts: None,
            project_scope: None,
            telemetry,
        })
    }

    pub fn new_reloadable_with_artifacts(
        binding: VyrmTransportBinding,
        trust: VyrmTransportTrust,
        raft: VyrmRaft,
        credentials: VyrmTlsReloader,
        gate: VyrmTransportGate,
        artifacts: ArtifactTransferReceiver,
        project_scope: ScopeId,
    ) -> ClusterResult<Self> {
        let mut server = Self::new_reloadable(binding, trust, raft, credentials, gate)?;
        server.artifacts = Some(artifacts);
        server.project_scope = Some(project_scope);
        Ok(server)
    }

    pub fn with_admission_policy(
        mut self,
        policy: VyrmTransportAdmissionPolicy,
    ) -> ClusterResult<Self> {
        policy.validate()?;
        self.admission = Arc::new(Semaphore::new(policy.max_global_in_flight));
        self.telemetry = VyrmTransportTelemetry::new(policy, now_millis())?;
        Ok(self)
    }

    pub fn telemetry_snapshot(
        &self,
        observed_at: u64,
    ) -> ClusterResult<VyrmTransportTelemetrySnapshot> {
        self.telemetry.snapshot(observed_at)
    }

    pub async fn serve(self, listener: TcpListener) -> io::Result<()> {
        loop {
            let permit = Arc::clone(&self.admission)
                .acquire_owned()
                .await
                .map_err(|_| io::Error::other("Raft transport admission closed"))?;
            let (stream, _) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = tokio::time::timeout(
                    VYRM_RAFT_SERVER_RPC_TIMEOUT,
                    server.serve_connection(stream),
                )
                .await;
            });
        }
    }

    pub async fn serve_connection(&self, stream: TcpStream) -> io::Result<()> {
        if !self.gate.is_enabled() {
            let _ = self.telemetry.reject_connection(0);
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "local Raft transport is disabled",
            ));
        }
        let acceptor = self
            .reloader
            .as_ref()
            .map(VyrmTlsReloader::server_config)
            .transpose()
            .map_err(|error| io::Error::other(error.to_string()))?
            .map(TlsAcceptor::from)
            .unwrap_or_else(|| self.acceptor.clone());
        let mut stream = match acceptor.accept(stream).await {
            Ok(stream) => stream,
            Err(error) => {
                let _ = self.telemetry.reject_connection(0);
                return Err(error);
            }
        };
        let peer = match certificate_spiffe_id(stream.get_ref().1.peer_certificates()) {
            Ok(peer) => peer,
            Err(error) => {
                let _ = self.telemetry.reject_connection(0);
                return Err(error);
            }
        };
        let (envelope, request_bytes): (WireEnvelope, u64) =
            match read_frame_with_len(&mut stream).await {
                Ok(envelope) => envelope,
                Err(error) => {
                    let _ = self.telemetry.reject_connection(0);
                    return Err(error);
                }
            };
        if let Err(error) = envelope.validate(&self.binding, &self.trust, &peer) {
            let _ = self.telemetry.reject_connection(request_bytes);
            return Err(error);
        }
        self.telemetry
            .accept_connection(request_bytes)
            .map_err(invalid_data)?;
        let operation = envelope.request.operation();
        let admission = self
            .telemetry
            .admit(
                &envelope.source_canonical_id,
                operation,
                request_bytes,
                now_millis(),
            )
            .map_err(invalid_data)?;
        let source = envelope.source_canonical_id.clone();
        let (response, outcome) = match envelope.request {
            WireRequest::Append(request) => {
                let response = self.raft.append_entries(request).await;
                let outcome = result_outcome(&response);
                (WireResponse::Append(response), outcome)
            }
            WireRequest::Snapshot(request) => {
                let response = self.raft.install_snapshot(request).await;
                let outcome = result_outcome(&response);
                (WireResponse::Snapshot(response), outcome)
            }
            WireRequest::Vote(request) => {
                let response = self.raft.vote(request).await;
                let outcome = result_outcome(&response);
                (WireResponse::Vote(response), outcome)
            }
            WireRequest::Artifact(request) => {
                let (response, outcome) = match &self.artifacts {
                    Some(receiver) => {
                        match receiver.handle(&source, &self.binding.canonical_node_id, request) {
                            Ok(response) => (Ok(response), VyrmTransportOutcome::Allowed),
                            Err(error @ (ClusterError::Invalid(_) | ClusterError::Denied(_))) => {
                                (Err(error.to_string()), VyrmTransportOutcome::Denied)
                            }
                            Err(error) => (Err(error.to_string()), VyrmTransportOutcome::Failed),
                        }
                    }
                    None => (
                        Err("artifact transport is not configured on this node".into()),
                        VyrmTransportOutcome::Failed,
                    ),
                };
                (WireResponse::Artifact(response), outcome)
            }
            WireRequest::RuntimeCommit(command) => {
                let result = (|| {
                    command.validate().map_err(|error| error.to_string())?;
                    let Some(scope) = &self.project_scope else {
                        return Err("consensus runtime commit transport is not configured".into());
                    };
                    let crate::VyrmRaftOperation::RuntimeCommit { commit } = &command.operation
                    else {
                        return Err("consensus route accepts only runtime commits".into());
                    };
                    if &commit.scope != scope {
                        return Err(
                            "consensus runtime commit scope differs from the configured project"
                                .into(),
                        );
                    }
                    Ok(())
                })();
                let response = match result {
                    Ok(()) => self
                        .raft
                        .client_write(*command)
                        .await
                        .map_err(ConsensusCommitWireError::Raft),
                    Err(error) => Err(ConsensusCommitWireError::Denied(error)),
                };
                let outcome = match &response {
                    Ok(response) if response.data.accepted => VyrmTransportOutcome::Allowed,
                    Ok(_) => VyrmTransportOutcome::Denied,
                    Err(ConsensusCommitWireError::Denied(_)) => VyrmTransportOutcome::Denied,
                    Err(ConsensusCommitWireError::Raft(_)) => VyrmTransportOutcome::Failed,
                };
                (WireResponse::RuntimeCommit(response), outcome)
            }
        };
        let response_bytes = write_frame(&mut stream, &response).await?;
        admission
            .finish(outcome, response_bytes)
            .map_err(invalid_data)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEnvelope {
    version: u16,
    cluster: ClusterId,
    shard: ShardId,
    source_raft_id: u64,
    source_canonical_id: NodeId,
    target_raft_id: u64,
    target_canonical_id: NodeId,
    request_digest: String,
    request: WireRequest,
}

impl WireEnvelope {
    fn new(
        source: &VyrmTransportBinding,
        target_raft_id: u64,
        target: &VyrmRaftNode,
        request: WireRequest,
    ) -> ClusterResult<Self> {
        source.validate()?;
        let target_canonical_id = NodeId::new(target.canonical_id.clone())?;
        let request_digest = wire_digest(&request)?;
        Ok(Self {
            version: VYRM_RAFT_TRANSPORT_VERSION,
            cluster: source.cluster.clone(),
            shard: source.shard,
            source_raft_id: source.raft_node_id,
            source_canonical_id: source.canonical_node_id.clone(),
            target_raft_id,
            target_canonical_id,
            request_digest,
            request,
        })
    }

    fn validate(
        &self,
        local: &VyrmTransportBinding,
        trust: &VyrmTransportTrust,
        peer_spiffe_id: &str,
    ) -> io::Result<()> {
        if self.version != VYRM_RAFT_TRANSPORT_VERSION
            || self.cluster != local.cluster
            || self.shard != local.shard
            || self.target_raft_id != local.raft_node_id
            || self.target_canonical_id != local.canonical_node_id
            || self.request_digest != wire_digest(&self.request).map_err(invalid_data)?
        {
            return Err(invalid_data(
                "transport envelope binding or digest mismatch",
            ));
        }
        if self
            .request
            .source_raft_id()
            .is_some_and(|source| source != self.source_raft_id)
        {
            return Err(invalid_data(
                "authenticated envelope source does not match the Raft vote source",
            ));
        }
        let expected_peer = VyrmTransportBinding {
            trust_domain: local.trust_domain.clone(),
            cluster: local.cluster.clone(),
            shard: local.shard,
            raft_node_id: self.source_raft_id,
            canonical_node_id: self.source_canonical_id.clone(),
        };
        if !trust.allows(self.source_raft_id, &self.source_canonical_id) {
            return Err(invalid_data(
                "transport source Raft id and canonical id are not authorized",
            ));
        }
        if expected_peer.spiffe_id().map_err(invalid_data)? != peer_spiffe_id {
            return Err(invalid_data(
                "authenticated peer identity does not match envelope source",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "rpc", content = "body", rename_all = "snake_case")]
enum WireRequest {
    Append(AppendEntriesRequest<VyrmRaftTypeConfig>),
    Snapshot(InstallSnapshotRequest<VyrmRaftTypeConfig>),
    Vote(VoteRequest<u64>),
    Artifact(ArtifactTransferRpc),
    RuntimeCommit(Box<VyrmRaftCommand>),
}

impl WireRequest {
    fn operation(&self) -> VyrmTransportOperation {
        match self {
            Self::Append(_) => VyrmTransportOperation::Append,
            Self::Snapshot(_) => VyrmTransportOperation::Snapshot,
            Self::Vote(_) => VyrmTransportOperation::Vote,
            Self::Artifact(_) => VyrmTransportOperation::Artifact,
            Self::RuntimeCommit(_) => VyrmTransportOperation::RuntimeCommit,
        }
    }

    fn source_raft_id(&self) -> Option<u64> {
        match self {
            Self::Append(request) => request.vote.leader_id().voted_for(),
            Self::Snapshot(request) => request.vote.leader_id().voted_for(),
            Self::Vote(request) => request.vote.leader_id().voted_for(),
            Self::Artifact(_) => None,
            Self::RuntimeCommit(_) => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "rpc", content = "body", rename_all = "snake_case")]
enum WireResponse {
    Append(AppendResult),
    Snapshot(SnapshotResult),
    Vote(VoteResult),
    Artifact(std::result::Result<ArtifactTransferRpcResult, String>),
    RuntimeCommit(ConsensusCommitResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VyrmTlsEndpoint {
    address: String,
    server_name: String,
}

impl VyrmTlsEndpoint {
    fn parse(value: &str) -> ClusterResult<Self> {
        let rest = value
            .strip_prefix("vyrm+tls://")
            .ok_or_else(|| ClusterError::Invalid("Raft endpoint must use vyrm+tls://".into()))?;
        let (address, server_name) = rest.split_once("?server_name=").ok_or_else(|| {
            ClusterError::Invalid("Raft TLS endpoint requires server_name".into())
        })?;
        if address.is_empty()
            || address.len() > 512
            || server_name.is_empty()
            || server_name.len() > 253
            || server_name.contains('&')
            || ServerName::try_from(server_name.to_owned()).is_err()
        {
            return Err(ClusterError::Invalid(
                "Raft TLS endpoint address or server_name is invalid".into(),
            ));
        }
        Ok(Self {
            address: address.to_owned(),
            server_name: server_name.to_owned(),
        })
    }
}

fn peer_binding_for_node(
    local: &VyrmTransportBinding,
    raft_node_id: u64,
    node: &VyrmRaftNode,
) -> ClusterResult<VyrmTransportBinding> {
    Ok(VyrmTransportBinding {
        trust_domain: local.trust_domain.clone(),
        cluster: local.cluster.clone(),
        shard: local.shard,
        raft_node_id,
        canonical_node_id: NodeId::new(node.canonical_id.clone())?,
    })
}

pub fn verify_vyrm_certificate_identity(
    certificates: Option<&[CertificateDer<'_>]>,
    expected: &VyrmTransportBinding,
) -> ClusterResult<()> {
    let actual = certificate_spiffe_id(certificates)
        .map_err(|error| ClusterError::Denied(error.to_string()))?;
    if actual != expected.spiffe_id()? {
        return Err(ClusterError::Denied(
            "TLS peer SPIFFE identity does not match canonical target".into(),
        ));
    }
    Ok(())
}

fn verify_peer_identity(
    certificates: Option<&[CertificateDer<'_>]>,
    expected: &VyrmTransportBinding,
) -> ClusterResult<()> {
    verify_vyrm_certificate_identity(certificates, expected)
}

fn certificate_spiffe_id(certificates: Option<&[CertificateDer<'_>]>) -> io::Result<String> {
    let certificate = certificates
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| invalid_data("mutual TLS peer supplied no leaf certificate"))?;
    let (remainder, certificate) = X509Certificate::from_der(certificate.as_ref())
        .map_err(|error| invalid_data(format!("peer certificate parse failed: {error}")))?;
    if !remainder.is_empty() {
        return Err(invalid_data(
            "peer certificate contains trailing non-certificate bytes",
        ));
    }
    let names = certificate
        .subject_alternative_name()
        .map_err(|error| invalid_data(format!("peer certificate SAN parse failed: {error}")))?
        .ok_or_else(|| invalid_data("peer certificate has no SAN extension"))?;
    let uri_names = names
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::URI(uri) => Some((*uri).to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match uri_names.as_slice() {
        [identity] if identity.starts_with("spiffe://") => Ok(identity.clone()),
        [] => Err(invalid_data("peer certificate has no URI SAN")),
        [_] => Err(invalid_data("peer certificate URI SAN is not a SPIFFE id")),
        _ => Err(invalid_data(
            "peer certificate has multiple ambiguous URI SANs",
        )),
    }
}

fn wire_digest<T: Serialize>(value: &T) -> ClusterResult<String> {
    serde_json::to_vec(value)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| ClusterError::Invalid(format!("transport encoding failed: {error}")))
}

fn result_outcome<T, E>(result: &std::result::Result<T, E>) -> VyrmTransportOutcome {
    if result.is_ok() {
        VyrmTransportOutcome::Allowed
    } else {
        VyrmTransportOutcome::Failed
    }
}

async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<u64> {
    let bytes = serde_json::to_vec(value).map_err(invalid_data)?;
    if bytes.is_empty() || bytes.len() > VYRM_RAFT_MAX_RPC_FRAME_BYTES {
        return Err(invalid_data("transport frame exceeds its bounded size"));
    }
    writer
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(bytes.len() as u64)
}

async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> io::Result<T> {
    read_frame_with_len(reader).await.map(|(value, _)| value)
}

async fn read_frame_with_len<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> io::Result<(T, u64)> {
    let length = reader.read_u64().await?;
    if length == 0 || length > VYRM_RAFT_MAX_RPC_FRAME_BYTES as u64 {
        return Err(invalid_data("transport frame length is outside its bound"));
    }
    let mut bytes = vec![0; length as usize];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map(|value| (value, length))
        .map_err(invalid_data)
}

#[allow(clippy::result_large_err)]
fn remote_result<T, E>(
    target: u64,
    node: VyrmRaftNode,
    result: std::result::Result<T, E>,
) -> std::result::Result<T, RPCError<u64, VyrmRaftNode, E>>
where
    E: std::error::Error,
{
    result.map_err(|error| RPCError::RemoteError(RemoteError::new_with_node(target, node, error)))
}

fn unreachable<E>(message: impl Into<String>) -> RPCError<u64, VyrmRaftNode, E>
where
    E: std::error::Error,
{
    let error = io::Error::other(message.into());
    RPCError::Unreachable(Unreachable::new(&error))
}

fn invalid_data(error: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::Vote;

    #[test]
    fn envelope_binds_tls_identity_route_digest_and_raft_vote_source() {
        let cluster = ClusterId::new("cluster:transport-unit").unwrap();
        let source = binding(&cluster, 1);
        let target = binding(&cluster, 2);
        let target_node = VyrmRaftNode {
            canonical_id: "node-2".into(),
            zone: "az-2".into(),
            endpoint: "vyrm+tls://127.0.0.1:9443?server_name=node-2.vyrm.test".into(),
        };
        let trust = VyrmTransportTrust::new([(1, NodeId::new("node-1").unwrap())]).unwrap();
        let mut envelope = WireEnvelope::new(
            &source,
            2,
            &target_node,
            WireRequest::Vote(VoteRequest::new(Vote::new(3, 1), None)),
        )
        .unwrap();
        let peer = source.spiffe_id().unwrap();
        envelope.validate(&target, &trust, &peer).unwrap();

        envelope.request_digest = "0".repeat(64);
        assert!(envelope.validate(&target, &trust, &peer).is_err());
        envelope.request_digest = wire_digest(&envelope.request).unwrap();
        envelope.request = WireRequest::Vote(VoteRequest::new(Vote::new(4, 3), None));
        envelope.request_digest = wire_digest(&envelope.request).unwrap();
        assert!(envelope.validate(&target, &trust, &peer).is_err());
        assert!(envelope
            .validate(&target, &trust, &binding(&cluster, 3).spiffe_id().unwrap())
            .is_err());
    }

    #[test]
    fn only_explicit_tls_endpoints_and_canonical_trust_domains_parse() {
        assert!(
            VyrmTlsEndpoint::parse("vyrm+tls://127.0.0.1:9443?server_name=node-1.vyrm.test")
                .is_ok()
        );
        assert!(VyrmTlsEndpoint::parse("http://127.0.0.1:9443").is_err());
        assert!(VyrmTlsEndpoint::parse("vyrm+tls://127.0.0.1:9443").is_err());
        let mut invalid = binding(&ClusterId::new("cluster:unit").unwrap(), 1);
        invalid.trust_domain = "Uppercase.invalid".into();
        assert!(invalid.validate().is_err());
        invalid.trust_domain = "empty..label".into();
        assert!(invalid.validate().is_err());
        assert!(VyrmTransportTrust::new([
            (1, NodeId::new("same-node").unwrap()),
            (2, NodeId::new("same-node").unwrap()),
        ])
        .is_err());
    }

    #[test]
    fn snapshot_chunks_do_not_repeat_an_already_completed_artifact_hydration() {
        assert!(artifact_hydration_required(true, None, "snapshot-1"));
        assert!(artifact_hydration_required(
            true,
            Some("snapshot-0"),
            "snapshot-1"
        ));
        assert!(!artifact_hydration_required(
            true,
            Some("snapshot-1"),
            "snapshot-1"
        ));
        assert!(!artifact_hydration_required(false, None, "snapshot-1"));
    }

    fn binding(cluster: &ClusterId, id: u64) -> VyrmTransportBinding {
        VyrmTransportBinding {
            trust_domain: "vyrm.test".into(),
            cluster: cluster.clone(),
            shard: ShardId(7),
            raft_node_id: id,
            canonical_node_id: NodeId::new(format!("node-{id}")).unwrap(),
        }
    }
}
