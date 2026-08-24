//! Supported asynchronous Rust client for the public RRD v1 protocol.
//!
//! This crate depends on `rrd-contract`, never on Vyrm storage/query internals.
//! The current transport is deliberately loopback HTTP while the F4 TLS gate
//! remains closed.

use bytes::{BufMut, Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::header::CONTENT_TYPE;
use hyper::{Method, Request, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rrd_contract::{
    AbortTransaction, AuditPage, BeginTransaction, CanonicalId, ChangefeedFollowResult,
    ChangefeedPage, CloseSession, CommitReceipt, CommitTransaction, CorrelationId,
    CreateInstanceBackup, CreateInstanceBackupResult, CreateSession, EndpointCatalogue, ErrorBody,
    ErrorCode, EstateSnapshot, EnsureVectorCollection, EnsureVectorCollectionResult, ExecuteQuery,
    FollowChangefeed, InstanceBackupCatalogueSnapshot, ListInstanceBackups, ListVectorCollections,
    PreviewTransaction, QueryResult, ReadAudit, ReadChangefeed, ReadEstate, RenewSession,
    RequestContext, RequestEnvelope, ResourceId, ResourceKind, ResourcePath, ResponseEnvelope,
    ResponseOutcome, RestoreInstanceBackup, RestoreInstanceBackupResult, ScrollVectorPoints,
    SearchVectors, ServiceCapabilities, SessionLease, SessionTermination, TransactionLease,
    TransactionPreview, VectorCollectionCatalogueSnapshot, VectorPointPage, VectorSearchResult,
    PROTOCOL, PROTOCOL_VERSION,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Contract(String),
    Transport(String),
    Timeout,
    ResponseTooLarge,
    Decode(String),
    Api {
        status: StatusCode,
        error: ErrorBody,
    },
    ResponseIdentityMismatch,
    UnsupportedProtocol {
        protocol: String,
        version: u16,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(message) => write!(formatter, "RRD client contract error: {message}"),
            Self::Transport(message) => write!(formatter, "RRD transport error: {message}"),
            Self::Timeout => formatter.write_str("RRD request timed out"),
            Self::ResponseTooLarge => formatter.write_str("RRD response exceeded four MiB"),
            Self::Decode(message) => write!(formatter, "RRD response decode failed: {message}"),
            Self::Api { status, error } => {
                write!(
                    formatter,
                    "RRD API {}: {:?}: {}",
                    status, error.code, error.message
                )
            }
            Self::ResponseIdentityMismatch => {
                formatter.write_str("RRD response request/operation identity differs")
            }
            Self::UnsupportedProtocol { protocol, version } => {
                write!(
                    formatter,
                    "unsupported RRD protocol {protocol:?} version {version}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    pub request_id: CorrelationId,
    pub operation_id: CorrelationId,
    pub idempotency_key: Option<CorrelationId>,
    pub deadline_unix_ms: Option<u64>,
}

impl RequestOptions {
    pub fn read(request_id: &str, operation_id: &str) -> Result<Self> {
        Self::new(request_id, operation_id, None, None)
    }

    pub fn mutation(request_id: &str, operation_id: &str, idempotency_key: &str) -> Result<Self> {
        Self::new(request_id, operation_id, Some(idempotency_key), None)
    }

    pub fn new(
        request_id: &str,
        operation_id: &str,
        idempotency_key: Option<&str>,
        deadline_unix_ms: Option<u64>,
    ) -> Result<Self> {
        let options = Self {
            request_id: CorrelationId::new(request_id).map_err(contract)?,
            operation_id: CorrelationId::new(operation_id).map_err(contract)?,
            idempotency_key: idempotency_key
                .map(CorrelationId::new)
                .transpose()
                .map_err(contract)?,
            deadline_unix_ms,
        };
        options.context().validate(false).map_err(contract)?;
        Ok(options)
    }

    fn context(&self) -> RequestContext {
        RequestContext {
            request_id: self.request_id.clone(),
            operation_id: self.operation_id.clone(),
            idempotency_key: self.idempotency_key.clone(),
            deadline_unix_ms: self.deadline_unix_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub principal_id: CanonicalId,
    pub lease: SessionLease,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub request_timeout: Duration,
    pub max_attempts: u8,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            max_attempts: 2,
        }
    }
}

#[derive(Clone)]
pub struct RrdClient {
    transport: Client<HttpConnector, Full<Bytes>>,
    address: SocketAddr,
    instance: CanonicalId,
    config: ClientConfig,
}

impl RrdClient {
    pub fn connect_local(
        address: SocketAddr,
        instance: CanonicalId,
        config: ClientConfig,
    ) -> Result<Self> {
        if !address.ip().is_loopback() {
            return Err(Error::Contract(
                "RRD Rust client permits only loopback HTTP before TLS qualification".into(),
            ));
        }
        if config.request_timeout.is_zero() || config.max_attempts == 0 || config.max_attempts > 8 {
            return Err(Error::Contract(
                "request timeout must be nonzero and max_attempts must be in 1..=8".into(),
            ));
        }
        let connector = HttpConnector::new();
        let transport = Client::builder(TokioExecutor::new()).build(connector);
        Ok(Self {
            transport,
            address,
            instance,
            config,
        })
    }

    pub async fn capabilities(&self) -> Result<ServiceCapabilities> {
        let response: ResponseEnvelope<ServiceCapabilities> = self
            .send_raw(Method::GET, "/v1/capabilities", Vec::new(), &[], true, None)
            .await?;
        let capabilities = outcome(StatusCode::OK, response, None)?;
        capabilities.validate().map_err(contract)?;
        if capabilities.protocol != PROTOCOL || capabilities.protocol_version != PROTOCOL_VERSION {
            return Err(Error::UnsupportedProtocol {
                protocol: capabilities.protocol,
                version: capabilities.protocol_version,
            });
        }
        if capabilities.instance.id != self.instance {
            return Err(Error::ResponseIdentityMismatch);
        }
        Ok(capabilities)
    }

    pub async fn endpoint_catalogue(&self) -> Result<EndpointCatalogue> {
        let response: ResponseEnvelope<EndpointCatalogue> = self
            .send_raw(
                Method::GET,
                "/v1/schema/endpoints",
                Vec::new(),
                &[],
                true,
                None,
            )
            .await?;
        let catalogue = outcome(StatusCode::OK, response, None)?;
        catalogue.validate().map_err(contract)?;
        Ok(catalogue)
    }

    pub async fn openapi_document(&self) -> Result<serde_json::Value> {
        let response: ResponseEnvelope<serde_json::Value> = self
            .send_raw(
                Method::GET,
                "/v1/schema/openapi",
                Vec::new(),
                &[],
                true,
                None,
            )
            .await?;
        let document = outcome(StatusCode::OK, response, None)?;
        if document["openapi"] != "3.1.0"
            || document["x-rrd-protocol"] != PROTOCOL
            || document["x-rrd-protocol-version"] != PROTOCOL_VERSION
        {
            return Err(Error::UnsupportedProtocol {
                protocol: document["x-rrd-protocol"]
                    .as_str()
                    .unwrap_or("missing")
                    .into(),
                version: document["x-rrd-protocol-version"]
                    .as_u64()
                    .and_then(|version| u16::try_from(version).ok())
                    .unwrap_or_default(),
            });
        }
        Ok(document)
    }

    pub async fn create_session(
        &self,
        principal_id: CanonicalId,
        api_key: &str,
        request: CreateSession,
        options: RequestOptions,
    ) -> Result<Session> {
        let expected = options.context();
        let envelope = self.envelope(request, &options, true)?;
        let bytes = serde_json::to_vec(&envelope).map_err(decode)?;
        let authorization = format!("ApiKey {api_key}");
        let headers = [
            ("X-RRD-Principal", principal_id.as_str()),
            ("Authorization", authorization.as_str()),
        ];
        let response = self
            .send_raw(
                Method::POST,
                "/v1/sessions",
                bytes,
                &headers,
                true,
                expected.deadline_unix_ms,
            )
            .await?;
        let lease = outcome(StatusCode::OK, response, Some(&expected))?;
        Ok(Session {
            principal_id,
            lease,
        })
    }

    pub async fn execute_query(
        &self,
        session: &Session,
        request: ExecuteQuery,
        options: RequestOptions,
    ) -> Result<QueryResult> {
        self.session_call(Method::POST, "/v1/query", session, request, options, false)
            .await
    }

    pub async fn renew_session(
        &self,
        session: &Session,
        request: RenewSession,
        options: RequestOptions,
    ) -> Result<Session> {
        let lease = self
            .session_call(
                Method::POST,
                &format!("/v1/sessions/{}/renew", session.lease.session_id.as_str()),
                session,
                request,
                options,
                true,
            )
            .await?;
        Ok(Session {
            principal_id: session.principal_id.clone(),
            lease,
        })
    }

    pub async fn close_session(
        &self,
        session: &Session,
        request: CloseSession,
        options: RequestOptions,
    ) -> Result<SessionTermination> {
        self.session_call(
            Method::DELETE,
            &format!("/v1/sessions/{}", session.lease.session_id.as_str()),
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn begin_transaction(
        &self,
        session: &Session,
        request: BeginTransaction,
        options: RequestOptions,
    ) -> Result<TransactionLease> {
        self.session_call(
            Method::POST,
            "/v1/transactions",
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn preview_transaction(
        &self,
        session: &Session,
        transaction: &CorrelationId,
        request: PreviewTransaction,
        options: RequestOptions,
    ) -> Result<TransactionPreview> {
        self.session_call(
            Method::POST,
            &format!("/v1/transactions/{}/preview", transaction.as_str()),
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn commit_transaction(
        &self,
        session: &Session,
        transaction: &CorrelationId,
        request: CommitTransaction,
        options: RequestOptions,
    ) -> Result<CommitReceipt> {
        self.session_call(
            Method::POST,
            &format!("/v1/transactions/{}/commit", transaction.as_str()),
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn abort_transaction(
        &self,
        session: &Session,
        transaction: &CorrelationId,
        request: AbortTransaction,
        options: RequestOptions,
    ) -> Result<TransactionLease> {
        self.session_call(
            Method::DELETE,
            &format!("/v1/transactions/{}", transaction.as_str()),
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn search_vectors(
        &self,
        session: &Session,
        request: SearchVectors,
        options: RequestOptions,
    ) -> Result<VectorSearchResult> {
        self.session_call(
            Method::POST,
            "/v1/vector/search",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn ensure_vector_collection(
        &self,
        session: &Session,
        request: EnsureVectorCollection,
        options: RequestOptions,
    ) -> Result<EnsureVectorCollectionResult> {
        self.session_call(
            Method::POST,
            "/v1/vector/collections/ensure",
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn list_vector_collections(
        &self,
        session: &Session,
        request: ListVectorCollections,
        options: RequestOptions,
    ) -> Result<VectorCollectionCatalogueSnapshot> {
        self.session_call(
            Method::POST,
            "/v1/vector/collections/list",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn scroll_vector_points(
        &self,
        session: &Session,
        request: ScrollVectorPoints,
        options: RequestOptions,
    ) -> Result<VectorPointPage> {
        self.session_call(
            Method::POST,
            "/v1/vector/points/scroll",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn read_changefeed(
        &self,
        session: &Session,
        request: ReadChangefeed,
        options: RequestOptions,
    ) -> Result<ChangefeedPage> {
        self.session_call(
            Method::POST,
            "/v1/changes/read",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn follow_changefeed(
        &self,
        session: &Session,
        request: FollowChangefeed,
        options: RequestOptions,
    ) -> Result<ChangefeedFollowResult> {
        self.session_call(
            Method::POST,
            "/v1/changes/follow",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn create_backup(
        &self,
        session: &Session,
        request: CreateInstanceBackup,
        options: RequestOptions,
    ) -> Result<CreateInstanceBackupResult> {
        self.session_call(Method::POST, "/v1/backups", session, request, options, true)
            .await
    }

    pub async fn list_backups(
        &self,
        session: &Session,
        request: ListInstanceBackups,
        options: RequestOptions,
    ) -> Result<InstanceBackupCatalogueSnapshot> {
        self.session_call(
            Method::POST,
            "/v1/backups/list",
            session,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn restore_backup(
        &self,
        session: &Session,
        request: RestoreInstanceBackup,
        options: RequestOptions,
    ) -> Result<RestoreInstanceBackupResult> {
        self.session_call(
            Method::POST,
            "/v1/restores",
            session,
            request,
            options,
            true,
        )
        .await
    }

    pub async fn read_estate(
        &self,
        session: &Session,
        estate: CanonicalId,
        request: ReadEstate,
        options: RequestOptions,
    ) -> Result<EstateSnapshot> {
        let resource = ResourcePath {
            segments: vec![
                ResourceId::new(ResourceKind::Estate, estate.as_str().to_owned())
                    .map_err(contract)?,
                ResourceId::new(ResourceKind::Instance, self.instance.as_str().to_owned())
                    .map_err(contract)?,
            ],
        };
        self.session_call_resource(
            Method::POST,
            &format!("/v1/estates/{estate}/read"),
            session,
            resource,
            request,
            options,
            false,
        )
        .await
    }

    pub async fn read_audit(
        &self,
        session: &Session,
        request: ReadAudit,
        options: RequestOptions,
    ) -> Result<AuditPage> {
        self.session_call(
            Method::POST,
            "/v1/audit/read",
            session,
            request,
            options,
            false,
        )
        .await
    }

    async fn session_call<T, O>(
        &self,
        method: Method,
        path: &str,
        session: &Session,
        request: T,
        options: RequestOptions,
        mutation: bool,
    ) -> Result<O>
    where
        T: Serialize,
        O: DeserializeOwned,
    {
        self.session_call_resource(
            method,
            path,
            session,
            self.instance_resource()?,
            request,
            options,
            mutation,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn session_call_resource<T, O>(
        &self,
        method: Method,
        path: &str,
        session: &Session,
        resource: ResourcePath,
        request: T,
        options: RequestOptions,
        mutation: bool,
    ) -> Result<O>
    where
        T: Serialize,
        O: DeserializeOwned,
    {
        let expected = options.context();
        let envelope = self.envelope_for(resource, request, &options, mutation)?;
        let bytes = serde_json::to_vec(&envelope).map_err(decode)?;
        let authorization = format!("Bearer {}", session.lease.token.as_str());
        let headers = [
            ("X-RRD-Session", session.lease.session_id.as_str()),
            ("Authorization", authorization.as_str()),
        ];
        let response = self
            .send_raw(
                method,
                path,
                bytes,
                &headers,
                true,
                expected.deadline_unix_ms,
            )
            .await?;
        outcome(StatusCode::OK, response, Some(&expected))
    }

    fn envelope<T: Serialize>(
        &self,
        payload: T,
        options: &RequestOptions,
        mutation: bool,
    ) -> Result<RequestEnvelope<T>> {
        self.envelope_for(self.instance_resource()?, payload, options, mutation)
    }

    fn envelope_for<T: Serialize>(
        &self,
        resource: ResourcePath,
        payload: T,
        options: &RequestOptions,
        mutation: bool,
    ) -> Result<RequestEnvelope<T>> {
        let envelope = RequestEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            context: options.context(),
            resource,
            payload,
        };
        envelope.validate(mutation).map_err(contract)?;
        Ok(envelope)
    }

    fn instance_resource(&self) -> Result<ResourcePath> {
        Ok(ResourcePath {
            segments: vec![
                ResourceId::new(ResourceKind::Instance, self.instance.as_str().to_owned())
                    .map_err(contract)?,
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_raw<O: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        headers: &[(&str, &str)],
        retry_safe: bool,
        deadline_unix_ms: Option<u64>,
    ) -> Result<ResponseEnvelope<O>> {
        let attempts = if retry_safe {
            self.config.max_attempts
        } else {
            1
        };
        let mut last_error = None;
        for _ in 0..attempts {
            let remaining = request_timeout(self.config.request_timeout, deadline_unix_ms)?;
            match tokio::time::timeout(
                remaining,
                self.send_once(method.clone(), path, body.clone(), headers),
            )
            .await
            {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(error @ Error::Transport(_))) => last_error = Some(error),
                Ok(Err(error)) => return Err(error),
                Err(_) => last_error = Some(Error::Timeout),
            }
        }
        Err(last_error.unwrap_or(Error::Timeout))
    }

    async fn send_once<O: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        headers: &[(&str, &str)],
    ) -> Result<ResponseEnvelope<O>> {
        let uri: Uri = format!("http://{}{}", self.address, path)
            .parse()
            .map_err(|error| Error::Contract(format!("invalid request URI: {error}")))?;
        let mut builder = Request::builder().method(method.clone()).uri(uri);
        if method != Method::GET {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| Error::Contract(error.to_string()))?;
        let response = self
            .transport
            .request(request)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;
        let status = response.status();
        let mut incoming = response.into_body();
        let mut bytes = BytesMut::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|error| Error::Transport(error.to_string()))?;
            if let Some(data) = frame.data_ref() {
                if bytes.len().saturating_add(data.len()) > MAX_RESPONSE_BYTES {
                    return Err(Error::ResponseTooLarge);
                }
                bytes.put_slice(data);
            }
        }
        let decoded: ResponseEnvelope<O> = serde_json::from_slice(&bytes).map_err(decode)?;
        if status.is_success() != matches!(decoded.outcome, ResponseOutcome::Ok { .. }) {
            return Err(Error::Decode(
                "HTTP status and response outcome disagree".into(),
            ));
        }
        if let ResponseOutcome::Error { ref error } = decoded.outcome {
            return Err(Error::Api {
                status,
                error: error.clone(),
            });
        }
        Ok(decoded)
    }
}

fn outcome<T>(
    status: StatusCode,
    response: ResponseEnvelope<T>,
    expected: Option<&RequestContext>,
) -> Result<T> {
    if response.protocol != PROTOCOL || response.protocol_version != PROTOCOL_VERSION {
        return Err(Error::UnsupportedProtocol {
            protocol: response.protocol,
            version: response.protocol_version,
        });
    }
    if expected.is_some_and(|context| {
        response.request_id != context.request_id || response.operation_id != context.operation_id
    }) {
        return Err(Error::ResponseIdentityMismatch);
    }
    match response.outcome {
        ResponseOutcome::Ok { payload } if status.is_success() => Ok(payload),
        ResponseOutcome::Ok { .. } => Err(Error::Decode(
            "successful outcome used error HTTP status".into(),
        )),
        ResponseOutcome::Error { error } => Err(Error::Api { status, error }),
    }
}

fn request_timeout(configured: Duration, deadline_unix_ms: Option<u64>) -> Result<Duration> {
    let Some(deadline) = deadline_unix_ms else {
        return Ok(configured);
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Contract(error.to_string()))?
        .as_millis();
    let now = u64::try_from(now).map_err(|_| Error::Contract("clock exceeds u64".into()))?;
    if deadline <= now {
        return Err(Error::Timeout);
    }
    Ok(configured.min(Duration::from_millis(deadline - now)))
}

fn contract(error: impl fmt::Display) -> Error {
    Error::Contract(error.to_string())
}

fn decode(error: impl fmt::Display) -> Error {
    Error::Decode(error.to_string())
}

pub fn is_unauthenticated(error: &Error) -> bool {
    matches!(
        error,
        Error::Api {
            error: ErrorBody {
                code: ErrorCode::Unauthenticated,
                ..
            },
            ..
        }
    )
}
