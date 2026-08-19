//! Mutually authenticated, bounded OpenRaft RPC transport.
//!
//! Transport v1 uses one request per TLS connection, disables application
//! bearer credentials, and binds the TLS URI SAN to the canonical cluster/node
//! identity carried by the replicated membership. OpenRaft retains ownership of
//! retry, ordering, duplicate, and chunked-snapshot semantics.

use crate::{
    ClusterError, ClusterId, NodeId, Result as ClusterResult, ShardId, VyrmRaftNode,
    VyrmRaftTypeConfig,
};
use openraft::error::{
    InstallSnapshotError, RPCError, RaftError, RemoteError, Timeout, Unreachable,
};
use openraft::network::{RPCOption, RPCTypes, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::Raft;
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use vyrm_core::digest::sha256_hex;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

pub const VYRM_RAFT_TRANSPORT_VERSION: u16 = 1;
pub const VYRM_RAFT_MAX_RPC_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const VYRM_RAFT_MAX_IN_FLIGHT_RPCS: usize = 256;
pub const VYRM_RAFT_SERVER_RPC_TIMEOUT: Duration = Duration::from_secs(30);

type VyrmRaft = Raft<VyrmRaftTypeConfig>;
type AppendResult = std::result::Result<AppendEntriesResponse<u64>, RaftError<u64>>;
type SnapshotResult =
    std::result::Result<InstallSnapshotResponse<u64>, RaftError<u64, InstallSnapshotError>>;
type VoteResult = std::result::Result<VoteResponse<u64>, RaftError<u64>>;

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
        })
    }
}

pub struct VyrmRaftNetworkClient {
    source: VyrmTransportBinding,
    target_id: u64,
    target: VyrmRaftNode,
    connector: TlsConnector,
    gate: VyrmTransportGate,
    reloader: Option<VyrmTlsReloader>,
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
}

impl VyrmRaftNetworkClient {
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
        Ok(Self {
            binding,
            trust,
            raft,
            acceptor: TlsAcceptor::from(server),
            admission: Arc::new(Semaphore::new(VYRM_RAFT_MAX_IN_FLIGHT_RPCS)),
            gate,
            reloader: None,
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
        Ok(Self {
            binding,
            trust,
            raft,
            acceptor: TlsAcceptor::from(server),
            admission: Arc::new(Semaphore::new(VYRM_RAFT_MAX_IN_FLIGHT_RPCS)),
            gate,
            reloader: Some(credentials),
        })
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
        let mut stream = acceptor.accept(stream).await?;
        let peer = certificate_spiffe_id(stream.get_ref().1.peer_certificates())?;
        let envelope: WireEnvelope = read_frame(&mut stream).await?;
        envelope.validate(&self.binding, &self.trust, &peer)?;
        let response = match envelope.request {
            WireRequest::Append(request) => {
                WireResponse::Append(self.raft.append_entries(request).await)
            }
            WireRequest::Snapshot(request) => {
                WireResponse::Snapshot(self.raft.install_snapshot(request).await)
            }
            WireRequest::Vote(request) => WireResponse::Vote(self.raft.vote(request).await),
        };
        write_frame(&mut stream, &response).await
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
        if self.request.source_raft_id() != Some(self.source_raft_id) {
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
}

impl WireRequest {
    fn source_raft_id(&self) -> Option<u64> {
        match self {
            Self::Append(request) => request.vote.leader_id().voted_for(),
            Self::Snapshot(request) => request.vote.leader_id().voted_for(),
            Self::Vote(request) => request.vote.leader_id().voted_for(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "rpc", content = "body", rename_all = "snake_case")]
enum WireResponse {
    Append(AppendResult),
    Snapshot(SnapshotResult),
    Vote(VoteResult),
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

async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(invalid_data)?;
    if bytes.is_empty() || bytes.len() > VYRM_RAFT_MAX_RPC_FRAME_BYTES {
        return Err(invalid_data("transport frame exceeds its bounded size"));
    }
    writer
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> io::Result<T> {
    let length = reader.read_u64().await?;
    if length == 0 || length > VYRM_RAFT_MAX_RPC_FRAME_BYTES as u64 {
        return Err(invalid_data("transport frame length is outside its bound"));
    }
    let mut bytes = vec![0; length as usize];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(invalid_data)
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
