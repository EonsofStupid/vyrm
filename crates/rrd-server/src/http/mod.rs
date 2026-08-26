use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rrd_contract::{
    AbortTransaction, AuditDecision, BeginTransaction, CanonicalId, CapabilityDescriptor,
    CapabilityStatus, CloseSession, CommitTransaction, CorrelationId, CreateInstanceBackup,
    CreateSession, DeploymentMode, EnsureQueryIndex, EnsureVectorCollection, ErrorBody, ErrorCode,
    ExecuteQuery, FollowChangefeed, ListInstanceBackups, ListQueryIndexes, ListRuntimeTools,
    ListVectorCollections, Liveness, PollLiveQuery, PreviewTransaction, ReadAudit, ReadChangefeed,
    ReadDiagnosticSnapshot, ReadEstate, Readiness, RenewSession, RequestContext, RequestEnvelope,
    ResourceId, ResourceKind, ResponseEnvelope, ResponseOutcome, RestoreInstanceBackup,
    RetrieveVectorPoints, RuntimeToolInvocation, ScrollVectorPoints, SearchVectors,
    ServiceCapabilities, PROTOCOL, PROTOCOL_VERSION,
};
use rrd_engine::{
    runtime_tool_contract_catalogue, runtime_tool_operation, AuthorizedInvocation, Invocation,
    InvocationCompletion, InvocationCredential, ProjectAuthorityBinding, RrdEngine, RrdOperation,
    ServiceError, ServiceErrorKind, MAX_AUDIT_PAGE_RECORDS,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

mod auth;
mod capabilities;
mod envelope;
mod handlers;
mod response;
mod router;
mod server;

use auth::{api_key_identity, authenticated_session, header_values};
use capabilities::{
    capabilities, estate_action, parse_correlation, session_action, session_id, transaction_action,
    transaction_id, unix_time_ms,
};
use envelope::{invocation_completion, required_idempotency};
use response::{
    api_error, failure, generated_context, instance_resource, sha256_hex, success, ApiError,
    HttpResponse, ResponseDigest,
};
use router::dispatch;
use server::AppState;

pub use server::{RrdHttpServer, RrdMutualTlsServerConfig};

pub const RRD_MAX_BODY_BYTES: usize = 1024 * 1024;
static HTTP_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub type Result<T> = std::result::Result<T, HttpError>;

#[derive(Debug)]
pub enum HttpError {
    RemoteBindDenied(SocketAddr),
    RemoteSecurityRequired,
    Bind(String),
    Io(io::Error),
    Contract(String),
    Tls(String),
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RemoteBindDenied(address) => {
                write!(formatter, "remote bind {address} denied before F4 security")
            }
            Self::RemoteSecurityRequired => formatter.write_str(
                "RRD mTLS listeners require an initialized application security authority",
            ),
            Self::Bind(error) | Self::Contract(error) | Self::Tls(error) => {
                formatter.write_str(error)
            }
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for HttpError {}

impl From<io::Error> for HttpError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
