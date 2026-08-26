//! Persistent Connectome connection profiles and live RRD negotiation.

use rrd_client::{ClientConfig, RrdClient};
use rrd_contract::{CanonicalId, ServiceCapabilities};
use rrd_core::digest;
use rrd_store::{ControlTransition, Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

pub const CONNECTION_CATALOGUE_FORMAT: u16 = 1;
pub const MAX_CONNECTION_PROFILES: usize = 64;
pub const MAX_CONNECTION_RECEIPTS: usize = 256;
const CONNECTION_CATALOGUE_KEY: &str = "server/state/connectome/connections";

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Store(rrd_store::Error),
    Transport(String),
    Conflict(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid connection profile: {message}"),
            Self::Store(error) => write!(formatter, "connection catalogue storage failed: {error}"),
            Self::Transport(message) => write!(formatter, "RRD connection failed: {message}"),
            Self::Conflict(key) => {
                write!(formatter, "connection request identity was rebound: {key}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<rrd_store::Error> for Error {
    fn from(error: rrd_store::Error) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMode {
    LocalRrd,
    RemoteRrd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionProfile {
    pub id: CanonicalId,
    pub label: String,
    pub mode: ConnectionMode,
    /// `host:port` for the currently qualified local transport. Remote profiles
    /// retain an HTTPS endpoint but remain unprobeable until Connectome can
    /// resolve credential references into mTLS client material.
    pub endpoint: String,
    pub instance_id: CanonicalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<CanonicalId>,
}

impl ConnectionProfile {
    pub fn validate(&self) -> Result<()> {
        if self.label.trim().is_empty() || self.label.len() > 128 {
            return Err(Error::Invalid("label must contain 1..=128 bytes".into()));
        }
        if self.endpoint.is_empty() || self.endpoint.len() > 512 || !self.endpoint.is_ascii() {
            return Err(Error::Invalid(
                "endpoint must contain 1..=512 ASCII bytes".into(),
            ));
        }
        match self.mode {
            ConnectionMode::LocalRrd => {
                let address: SocketAddr = self.endpoint.parse().map_err(|_| {
                    Error::Invalid("local RRD endpoint must be a host:port socket address".into())
                })?;
                if !address.ip().is_loopback() {
                    return Err(Error::Invalid(
                        "local RRD profiles must use a loopback address".into(),
                    ));
                }
            }
            ConnectionMode::RemoteRrd => {
                if !self.endpoint.starts_with("https://") {
                    return Err(Error::Invalid(
                        "remote RRD profiles require an https:// endpoint".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionObservation {
    pub observed_at: u64,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ServiceCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRecord {
    pub profile: ConnectionProfile,
    pub observation: ConnectionObservation,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionCatalogue {
    pub format_version: u16,
    pub revision: u64,
    pub records: BTreeMap<CanonicalId, ConnectionRecord>,
    pub idempotency_bindings: BTreeMap<String, String>,
}

impl Default for ConnectionCatalogue {
    fn default() -> Self {
        Self {
            format_version: CONNECTION_CATALOGUE_FORMAT,
            revision: 0,
            records: BTreeMap::new(),
            idempotency_bindings: BTreeMap::new(),
        }
    }
}

impl ConnectionCatalogue {
    fn validate(&self) -> Result<()> {
        if self.format_version != CONNECTION_CATALOGUE_FORMAT {
            return Err(Error::Invalid("unsupported catalogue format".into()));
        }
        if self.records.len() > MAX_CONNECTION_PROFILES {
            return Err(Error::Invalid("connection profile limit exceeded".into()));
        }
        if self.idempotency_bindings.len() > MAX_CONNECTION_RECEIPTS {
            return Err(Error::Invalid("connection receipt limit exceeded".into()));
        }
        for (id, record) in &self.records {
            if id != &record.profile.id || record.generation == 0 {
                return Err(Error::Invalid(
                    "connection record identity is inconsistent".into(),
                ));
            }
            record.profile.validate()?;
            if record.observation.state != "connected" && record.observation.state != "unavailable"
            {
                return Err(Error::Invalid(
                    "unknown connection observation state".into(),
                ));
            }
            if record.observation.state == "connected" && record.observation.capabilities.is_none()
            {
                return Err(Error::Invalid(
                    "connected record lacks negotiated capabilities".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureConnectionProfile {
    pub profile: ConnectionProfile,
    pub idempotency_key: String,
}

impl EnsureConnectionProfile {
    fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 128
            || !self.idempotency_key.is_ascii()
        {
            return Err(Error::Invalid(
                "idempotency key must contain 1..=128 ASCII bytes".into(),
            ));
        }
        Ok(())
    }

    fn sha256(&self) -> String {
        digest::sha256_hex(&serde_json::to_vec(self).expect("connection request serializes"))
    }
}

pub struct ConnectionRepository<'a, E: Engine + ?Sized> {
    engine: &'a E,
}

impl<'a, E: Engine + ?Sized> ConnectionRepository<'a, E> {
    pub fn new(engine: &'a E) -> Self {
        Self { engine }
    }

    pub fn load(&self) -> Result<ConnectionCatalogue> {
        let Some(bytes) = self.engine.control_record(CONNECTION_CATALOGUE_KEY)? else {
            return Ok(ConnectionCatalogue::default());
        };
        let catalogue: ConnectionCatalogue = serde_json::from_slice(&bytes)
            .map_err(|error| Error::Invalid(format!("catalogue decode failed: {error}")))?;
        catalogue.validate()?;
        Ok(catalogue)
    }

    pub fn ensure(
        &self,
        request: &EnsureConnectionProfile,
        observation: ConnectionObservation,
        actor: &str,
        at: u64,
    ) -> Result<ConnectionCatalogue> {
        request.validate()?;
        if at == 0 || observation.observed_at != at {
            return Err(Error::Invalid(
                "observation time must match mutation time".into(),
            ));
        }
        let operation_sha256 = request.sha256();
        let current_bytes = self.engine.control_record(CONNECTION_CATALOGUE_KEY)?;
        let mut catalogue = match current_bytes.as_deref() {
            Some(bytes) => serde_json::from_slice(bytes)
                .map_err(|error| Error::Invalid(format!("catalogue decode failed: {error}")))?,
            None => ConnectionCatalogue::default(),
        };
        catalogue.validate()?;
        if let Some(bound) = catalogue.idempotency_bindings.get(&request.idempotency_key) {
            if bound != &operation_sha256 {
                return Err(Error::Conflict(request.idempotency_key.clone()));
            }
            return Ok(catalogue);
        }
        if catalogue.records.len() >= MAX_CONNECTION_PROFILES
            && !catalogue.records.contains_key(&request.profile.id)
        {
            return Err(Error::Invalid("connection profile limit exceeded".into()));
        }
        if catalogue.idempotency_bindings.len() >= MAX_CONNECTION_RECEIPTS {
            let oldest = catalogue
                .idempotency_bindings
                .keys()
                .next()
                .cloned()
                .ok_or_else(|| {
                    Error::Invalid("connection receipt catalogue is inconsistent".into())
                })?;
            catalogue.idempotency_bindings.remove(&oldest);
        }
        let generation = catalogue
            .records
            .get(&request.profile.id)
            .map_or(1, |record| record.generation.saturating_add(1));
        catalogue.records.insert(
            request.profile.id.clone(),
            ConnectionRecord {
                profile: request.profile.clone(),
                observation,
                generation,
            },
        );
        catalogue
            .idempotency_bindings
            .insert(request.idempotency_key.clone(), operation_sha256);
        catalogue.revision = catalogue.revision.saturating_add(1);
        catalogue.validate()?;
        let replacement = serde_json::to_vec(&catalogue)
            .map_err(|error| Error::Invalid(format!("catalogue encode failed: {error}")))?;
        self.engine.commit_control_transition(&ControlTransition {
            key: CONNECTION_CATALOGUE_KEY.into(),
            expected: current_bytes,
            replacement: Some(replacement),
            at,
            actor: actor.into(),
            action: "connectome.connection.ensure".into(),
            request_id: request.idempotency_key.clone(),
            operation_id: request.profile.id.to_string(),
        })?;
        Ok(catalogue)
    }
}

pub fn probe_connection(
    profile: &ConnectionProfile,
    observed_at: u64,
) -> Result<ConnectionObservation> {
    profile.validate()?;
    match profile.mode {
        ConnectionMode::RemoteRrd => Err(Error::Invalid(
            "remote RRD probing requires a Connectome mTLS credential provider".into(),
        )),
        ConnectionMode::LocalRrd => {
            let address = profile
                .endpoint
                .parse::<SocketAddr>()
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| Error::Transport(error.to_string()))?;
            let client = RrdClient::connect_local(
                address,
                profile.instance_id.clone(),
                ClientConfig {
                    request_timeout: Duration::from_secs(2),
                    max_attempts: 1,
                },
            )
            .map_err(|error| Error::Transport(error.to_string()))?;
            let capabilities = runtime
                .block_on(client.capabilities())
                .map_err(|error| Error::Transport(error.to_string()))?;
            Ok(ConnectionObservation {
                observed_at,
                state: "connected".into(),
                capabilities: Some(capabilities),
                error: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rrd_store::MemoryEngine;

    fn request(key: &str) -> EnsureConnectionProfile {
        EnsureConnectionProfile {
            profile: ConnectionProfile {
                id: CanonicalId::new("profile-local").unwrap(),
                label: "Local RRD".into(),
                mode: ConnectionMode::LocalRrd,
                endpoint: "127.0.0.1:4317".into(),
                instance_id: CanonicalId::new("instance-test").unwrap(),
                credential_ref: Some(CanonicalId::new("secret-rrd-local").unwrap()),
            },
            idempotency_key: key.into(),
        }
    }

    #[test]
    fn catalogue_is_persistent_idempotent_and_rejects_rebinding() {
        let engine = MemoryEngine::new();
        let repository = ConnectionRepository::new(&engine);
        let observation = ConnectionObservation {
            observed_at: 10,
            state: "connected".into(),
            capabilities: Some(test_capabilities()),
            error: None,
        };
        let first = repository
            .ensure(
                &request("connect-1"),
                observation.clone(),
                "operator:test",
                10,
            )
            .unwrap();
        assert_eq!(first.revision, 1);
        let replay = repository
            .ensure(&request("connect-1"), observation, "operator:test", 10)
            .unwrap();
        assert_eq!(replay.revision, 1);
        assert_eq!(repository.load().unwrap(), replay);

        let mut rebound = request("connect-1");
        rebound.profile.endpoint = "127.0.0.1:4318".into();
        assert!(matches!(
            repository.ensure(
                &rebound,
                ConnectionObservation {
                    observed_at: 11,
                    state: "connected".into(),
                    capabilities: Some(test_capabilities()),
                    error: None,
                },
                "operator:test",
                11,
            ),
            Err(Error::Conflict(_))
        ));
    }

    #[test]
    fn profiles_retain_secret_references_and_deny_unsafe_transport() {
        let mut local = request("connect-2").profile;
        local.endpoint = "10.0.0.4:4317".into();
        assert!(local.validate().is_err());

        let remote = ConnectionProfile {
            id: CanonicalId::new("profile-remote").unwrap(),
            label: "Enterprise".into(),
            mode: ConnectionMode::RemoteRrd,
            endpoint: "http://rrd.example.test".into(),
            instance_id: CanonicalId::new("instance-remote").unwrap(),
            credential_ref: Some(CanonicalId::new("secret-enterprise-rrd").unwrap()),
        };
        assert!(remote.validate().is_err());
    }

    #[test]
    fn live_probe_negotiates_protocol_and_exact_instance() {
        use rrd_contract::{
            CorrelationId, ResponseEnvelope, ResponseOutcome, PROTOCOL, PROTOCOL_VERSION,
        };
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request_bytes = [0_u8; 2_048];
            let read = stream.read(&mut request_bytes).unwrap();
            assert!(
                String::from_utf8_lossy(&request_bytes[..read]).starts_with("GET /v1/capabilities")
            );
            let body = serde_json::to_vec(&ResponseEnvelope {
                protocol: PROTOCOL.into(),
                protocol_version: PROTOCOL_VERSION,
                request_id: CorrelationId::new("connectome-capabilities").unwrap(),
                operation_id: CorrelationId::new("connectome-capabilities").unwrap(),
                outcome: ResponseOutcome::Ok {
                    payload: test_capabilities(),
                },
            })
            .unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut profile = request("probe-live").profile;
        profile.endpoint = address.to_string();
        let observed = probe_connection(&profile, 50).unwrap();
        assert_eq!(observed.state, "connected");
        assert_eq!(
            observed.capabilities.unwrap().instance.id.as_str(),
            "instance-test"
        );
        server.join().unwrap();
    }

    fn test_capabilities() -> ServiceCapabilities {
        use rrd_contract::{DeploymentMode, ResourceId, ResourceKind, PROTOCOL, PROTOCOL_VERSION};
        ServiceCapabilities {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            implementation: CanonicalId::new("rrflow").unwrap(),
            implementation_version: "0.1.0".into(),
            deployment_mode: DeploymentMode::LocalServer,
            instance: ResourceId::new(ResourceKind::Instance, "instance-test").unwrap(),
            capabilities: Vec::new(),
        }
    }
}
