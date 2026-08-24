use crate::{RrdService, ServiceError};
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use rrd_contract::{
    AbortTransaction, AuditDecision, AuditPhase, BeginTransaction, CanonicalId,
    CapabilityDescriptor, CapabilityStatus, CloseSession, CommitTransaction, CorrelationId,
    CreateInstanceBackup, CreateSession, DeploymentMode, EnsureQueryIndex, ErrorBody, ErrorCode,
    ExecuteQuery, FollowChangefeed, ListInstanceBackups, ListQueryIndexes, Liveness, PollLiveQuery,
    PreviewTransaction, ReadAudit, ReadChangefeed, ReadEstate, Readiness, RenewSession,
    RequestContext, RequestEnvelope, ResourceId, ResourceKind, ResponseEnvelope, ResponseOutcome,
    RestoreInstanceBackup, SearchVectors, ServiceCapabilities, PROTOCOL, PROTOCOL_VERSION,
};
use rrd_security::{Action as SecurityAction, AuditRecord, SecurityRepository};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use vyrm_core::digest;
use vyrm_store::{Engine, Error as StoreError, PersistentEngine};

pub const RRD_MAX_BODY_BYTES: usize = 1024 * 1024;
const TOKEN_KEY_BYTES: usize = 32;
static HTTP_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub type Result<T> = std::result::Result<T, HttpError>;

#[derive(Debug)]
pub enum HttpError {
    RemoteBindDenied(SocketAddr),
    Bind(String),
    Io(io::Error),
    Contract(String),
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RemoteBindDenied(address) => {
                write!(formatter, "remote bind {address} denied before F4 security")
            }
            Self::Bind(error) | Self::Contract(error) => formatter.write_str(error),
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

pub struct RrdHttpServer {
    listener: TcpListener,
    app: Router,
}

struct AppState {
    service: RrdService<PersistentEngine>,
    capabilities: ServiceCapabilities,
    security_enforced: bool,
}

impl RrdHttpServer {
    pub fn bind(
        engine: PersistentEngine,
        instance: CanonicalId,
        token_key: [u8; TOKEN_KEY_BYTES],
        bind: SocketAddr,
    ) -> Result<Self> {
        if !bind.ip().is_loopback() {
            return Err(HttpError::RemoteBindDenied(bind));
        }
        let backend = CanonicalId::new(engine.backend().as_str())
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let security_enforced = SecurityRepository::new(&engine, instance.clone())
            .is_initialized()
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let listener =
            TcpListener::bind(bind).map_err(|error| HttpError::Bind(error.to_string()))?;
        listener.set_nonblocking(true)?;
        let capabilities = capabilities(&instance, backend, security_enforced);
        capabilities
            .validate()
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let state = Arc::new(AppState {
            service: RrdService::new(engine, instance, token_key),
            capabilities,
            security_enforced,
        });
        let app = Router::new().fallback(any(dispatch)).with_state(state);
        Ok(Self { listener, app })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("bound RRD listener has a local address")
    }

    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::from_std(self.listener)?;
        axum::serve(listener, self.app).await.map_err(HttpError::Io)
    }

    pub async fn serve_until<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::from_std(self.listener)?;
        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(HttpError::Io)
    }
}

async fn dispatch(State(state): State<Arc<AppState>>, request: Request) -> HttpResponse {
    let now = unix_time_ms();
    let (parts, body) = request.into_parts();
    let body = match to_bytes(body, RRD_MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            let context = generated_context(now, "body-limit");
            return failure(
                &context,
                ApiError::new(
                    ErrorCode::ResourceExhausted,
                    "request body exceeds one MiB",
                    false,
                ),
            );
        }
    };
    let path = parts.uri.path().to_owned();
    match tokio::task::spawn_blocking(move || {
        state.handle(parts.method, path, parts.headers, body, now)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let context = generated_context(now, "handler-join");
            failure(
                &context,
                ApiError::new(ErrorCode::Internal, error.to_string(), false),
            )
        }
    }
}

impl AppState {
    fn handle(
        &self,
        method: Method,
        path: String,
        headers: HeaderMap,
        body: Bytes,
        now: u64,
    ) -> HttpResponse {
        let span = tracing::info_span!(
            "rrd.http.request",
            method = %method,
            path = %path,
            status = tracing::field::Empty,
        );
        let _entered = span.enter();
        let mut public_audit = None;
        let response = match (method, path.as_str()) {
            (Method::GET, "/v1/health/live") => {
                let context = generated_context(now, "health-live");
                public_audit = Some((SecurityAction::ServiceInspect, context.clone()));
                success(
                    StatusCode::OK,
                    &context,
                    Liveness {
                        observed_at_unix_ms: now,
                    },
                )
            }
            (Method::GET, "/v1/health/ready") => {
                let context = generated_context(now, "health-ready");
                public_audit = Some((SecurityAction::ServiceInspect, context.clone()));
                match self.readiness(now) {
                    Ok(ready) => success(StatusCode::OK, &context, ready),
                    Err(error) => failure(&context, api_error(error)),
                }
            }
            (Method::GET, "/v1/capabilities") => {
                let context = generated_context(now, "capabilities");
                public_audit = Some((SecurityAction::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, self.capabilities.clone())
            }
            (Method::GET, "/v1/schema/endpoints") => {
                let context = generated_context(now, "endpoint-catalogue");
                public_audit = Some((SecurityAction::ServiceInspect, context.clone()));
                success(StatusCode::OK, &context, rrd_contract::endpoint_catalogue())
            }
            (Method::GET, "/v1/schema/openapi") => {
                let context = generated_context(now, "openapi-read");
                public_audit = Some((SecurityAction::ServiceInspect, context.clone()));
                match rrd_contract::openapi_document() {
                    Ok(document) => success(StatusCode::OK, &context, document),
                    Err(error) => failure(
                        &context,
                        ApiError::new(ErrorCode::Internal, error.to_string(), false),
                    ),
                }
            }
            (Method::POST, "/v1/sessions") => self.create_session(&headers, &body, now),
            (Method::POST, path) if session_action(path, "renew").is_some() => {
                self.renew_session(&headers, &body, path, now)
            }
            (Method::DELETE, path) if session_id(path).is_some() => {
                self.close_session(&headers, &body, path, now)
            }
            (Method::POST, "/v1/transactions") => self.begin_transaction(&headers, &body, now),
            (Method::POST, "/v1/query") => self.execute_query(&headers, &body, now),
            (Method::POST, "/v1/query/indexes/ensure") => {
                self.ensure_query_index(&headers, &body, now)
            }
            (Method::POST, "/v1/query/indexes/list") => {
                self.list_query_indexes(&headers, &body, now)
            }
            (Method::POST, "/v1/query/live/poll") => self.poll_live_query(&headers, &body, now),
            (Method::POST, "/v1/backups") => self.create_instance_backup(&headers, &body, now),
            (Method::POST, "/v1/backups/list") => self.list_instance_backups(&headers, &body, now),
            (Method::POST, "/v1/restores") => self.restore_instance_backup(&headers, &body, now),
            (Method::POST, "/v1/audit/read") => self.read_audit(&headers, &body, now),
            (Method::POST, "/v1/changes/read") => self.read_changefeed(&headers, &body, now),
            (Method::POST, "/v1/changes/follow") => self.follow_changefeed(&headers, &body, now),
            (Method::POST, "/v1/vector/search") => self.search_vectors(&headers, &body, now),
            (Method::POST, path) if estate_action(path, "read").is_some() => {
                self.read_estate(&headers, &body, path, now)
            }
            (Method::POST, path) if transaction_action(path, "preview").is_some() => {
                self.preview_transaction(&headers, &body, path, now)
            }
            (Method::POST, path) if transaction_action(path, "commit").is_some() => {
                self.commit_transaction(&headers, &body, path, now)
            }
            (Method::DELETE, path) if transaction_id(path).is_some() => {
                self.abort_transaction(&headers, &body, path, now)
            }
            _ => {
                let context = generated_context(now, "not-found");
                public_audit = Some((SecurityAction::UnknownRequest, context.clone()));
                failure(
                    &context,
                    ApiError::new(ErrorCode::NotFound, "endpoint not found", false),
                )
            }
        };
        if self.security_enforced {
            if let Some((action, context)) = public_audit {
                self.audit_response(
                    action,
                    &instance_resource(&self.service.instance),
                    &context,
                    None,
                    &body,
                    &response,
                    now,
                    HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
                )
                .unwrap_or_else(|error| {
                    tracing::error!(error = %error, "RRD security audit append failed");
                });
            }
        }
        span.record("status", response.status().as_u16());
        response
    }

    fn readiness(&self, now: u64) -> std::result::Result<Readiness, ServiceError> {
        Ok(Readiness {
            observed_at_unix_ms: now,
            claim_sequence: self.service.engine().sequence()?,
            runtime_cursor: self.service.engine().runtime_cursor()?,
            backend: CanonicalId::new(self.service.engine().backend().as_str())
                .map_err(|error| ServiceError::Contract(error.to_string()))?,
        })
    }

    fn create_session(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_envelope::<CreateSession, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::SessionCreate,
            |envelope, principal_id| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .create_session_as(
                        &envelope.payload,
                        idempotency_key,
                        principal_id,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn read_audit(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<ReadAudit, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::AuditRead,
            None,
            |envelope, session, token| {
                self.service
                    .read_audit(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn renew_session(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let path_id = session_action(path, "renew").expect("route checked");
        self.with_authenticated_envelope::<RenewSession, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::SessionRenew,
            Some(path_id),
            |envelope, session, token| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .renew_session(
                        session,
                        token,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn close_session(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let path_id = session_id(path).expect("route checked");
        self.with_authenticated_envelope::<CloseSession, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::SessionClose,
            Some(path_id),
            |envelope, session, token| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .close_session(
                        session,
                        token,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn begin_transaction(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<BeginTransaction, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::TransactionBegin,
            None,
            |envelope, session, token| {
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .begin_transaction(
                        session,
                        token,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn read_estate(&self, headers: &HeaderMap, body: &[u8], path: &str, now: u64) -> HttpResponse {
        let estate = estate_action(path, "read").expect("route checked");
        self.with_authenticated_envelope::<ReadEstate, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::EstateRead,
            None,
            |envelope, session, token| {
                let estate = CanonicalId::new(estate).map_err(|error| {
                    ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false)
                })?;
                let resource_estate = envelope
                    .resource
                    .segments
                    .iter()
                    .find(|segment| segment.kind == ResourceKind::Estate)
                    .map(|segment| &segment.id);
                if resource_estate != Some(&estate) {
                    return Err(ApiError::new(
                        ErrorCode::FailedPrecondition,
                        "request resource does not target the path estate",
                        false,
                    ));
                }
                self.service
                    .read_estate(
                        session,
                        token,
                        estate,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)?
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, "estate authority not found", false)
                    })
            },
        )
    }

    fn execute_query(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<ExecuteQuery, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::QueryExecute,
            None,
            |envelope, session, token| {
                self.service
                    .execute_query(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn poll_live_query(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<PollLiveQuery, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::QueryLivePoll,
            None,
            |envelope, session, token| {
                self.service
                    .poll_live_query(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn ensure_query_index(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<EnsureQueryIndex, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::QueryIndexEnsure,
            None,
            |envelope, session, token| {
                let idempotency_key = envelope
                    .context
                    .idempotency_key
                    .as_ref()
                    .expect("mutating envelopes require idempotency");
                self.service
                    .ensure_query_index(
                        session,
                        token,
                        idempotency_key,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn list_query_indexes(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<ListQueryIndexes, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::QueryIndexList,
            None,
            |envelope, session, token| {
                self.service
                    .list_query_indexes(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn create_instance_backup(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<CreateInstanceBackup, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::BackupCreate,
            None,
            |envelope, session, token| {
                self.service
                    .create_instance_backup(
                        session,
                        token,
                        required_idempotency(&envelope.context)?,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn list_instance_backups(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<ListInstanceBackups, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::BackupList,
            None,
            |envelope, session, token| {
                self.service
                    .list_instance_backups(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn restore_instance_backup(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<RestoreInstanceBackup, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::RestoreCreate,
            None,
            |envelope, session, token| {
                self.service
                    .restore_instance_backup(
                        session,
                        token,
                        required_idempotency(&envelope.context)?,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn search_vectors(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<SearchVectors, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::VectorSearch,
            None,
            |envelope, session, token| {
                self.service
                    .search_vectors(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn read_changefeed(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<ReadChangefeed, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::ChangefeedRead,
            None,
            |envelope, session, token| {
                self.service
                    .read_changefeed(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn follow_changefeed(&self, headers: &HeaderMap, body: &[u8], now: u64) -> HttpResponse {
        self.with_authenticated_envelope::<FollowChangefeed, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::ChangefeedFollow,
            None,
            |envelope, session, token| {
                if envelope.context.deadline_unix_ms.is_some_and(|deadline| {
                    now.checked_add(envelope.payload.wait_timeout_ms)
                        .is_none_or(|completion| completion > deadline)
                }) {
                    return Err(ApiError::new(
                        ErrorCode::DeadlineExceeded,
                        "changefeed follow wait exceeds the request deadline",
                        false,
                    ));
                }
                self.service
                    .follow_changefeed(
                        session,
                        token,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn preview_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_action(path, "preview").expect("route checked");
        self.with_authenticated_envelope::<PreviewTransaction, _, _>(
            headers,
            body,
            now,
            false,
            SecurityAction::TransactionPreview,
            None,
            |envelope, session, token| {
                let transaction = parse_correlation(transaction)?;
                self.service
                    .preview_transaction(
                        session,
                        token,
                        &transaction,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn commit_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_action(path, "commit").expect("route checked");
        self.with_authenticated_envelope::<CommitTransaction, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::TransactionCommit,
            None,
            |envelope, session, token| {
                if envelope.context.deadline_unix_ms.is_none() {
                    return Err(ApiError::new(
                        ErrorCode::InvalidArgument,
                        "commit requires deadline_unix_ms",
                        false,
                    ));
                }
                let transaction = parse_correlation(transaction)?;
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .commit_transaction(
                        session,
                        token,
                        &transaction,
                        idempotency_key,
                        &envelope.payload,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn abort_transaction(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        path: &str,
        now: u64,
    ) -> HttpResponse {
        let transaction = transaction_id(path).expect("route checked");
        self.with_authenticated_envelope::<AbortTransaction, _, _>(
            headers,
            body,
            now,
            true,
            SecurityAction::TransactionAbort,
            None,
            |envelope, session, token| {
                let transaction = parse_correlation(transaction)?;
                let idempotency_key = required_idempotency(&envelope.context)?;
                self.service
                    .abort_transaction(
                        session,
                        token,
                        &transaction,
                        &envelope.payload,
                        idempotency_key,
                        now,
                        envelope.context.request_id.as_str(),
                        envelope.context.operation_id.as_str(),
                    )
                    .map_err(api_error)
            },
        )
    }

    fn with_envelope<T, O, F>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
        mutation: bool,
        action: SecurityAction,
        operation: F,
    ) -> HttpResponse
    where
        T: DeserializeOwned,
        O: Serialize,
        F: FnOnce(&RequestEnvelope<T>, Option<CanonicalId>) -> std::result::Result<O, ApiError>,
    {
        let attempt = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, &self.service.instance) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    let response = failure(&context, error);
                    if self.security_enforced {
                        self.audit_response(
                            action,
                            &instance_resource(&self.service.instance),
                            &context,
                            None,
                            body,
                            &response,
                            now,
                            attempt,
                        )
                        .unwrap_or_else(|error| {
                            tracing::error!(error = %error, "RRD security audit append failed");
                        });
                    }
                    return response;
                }
            };
        let principal = if self.security_enforced {
            let (principal, credential) = match api_key_identity(headers) {
                Ok(identity) => identity,
                Err(error) => {
                    let response = failure(&envelope.context, error);
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
            };
            if let Err(error) =
                SecurityRepository::new(self.service.engine(), self.service.instance.clone())
                    .authenticate_and_authorize(
                        &principal,
                        credential.as_bytes(),
                        action,
                        &envelope.resource,
                        now,
                    )
            {
                let response = failure(&envelope.context, security_error(error));
                self.audit_response(
                    action,
                    &envelope.resource,
                    &envelope.context,
                    Some(principal),
                    body,
                    &response,
                    now,
                    attempt,
                )
                .unwrap_or_else(|error| {
                    tracing::error!(error = %error, "RRD security audit append failed");
                });
                return response;
            }
            if let Err(error) = self.audit_authorized(
                action,
                &envelope.resource,
                &envelope.context,
                Some(principal.clone()),
                body,
                now,
                attempt,
            ) {
                return failure(&envelope.context, security_error(error));
            }
            Some(principal)
        } else {
            None
        };
        let response = match operation(&envelope, principal.clone()) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        };
        if self.security_enforced {
            self.audit_response(
                action,
                &envelope.resource,
                &envelope.context,
                principal,
                body,
                &response,
                now,
                attempt,
            )
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "RRD security audit append failed");
            });
        }
        response
    }

    #[allow(clippy::too_many_arguments)]
    fn with_authenticated_envelope<T, O, F>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
        mutation: bool,
        action: SecurityAction,
        expected_session: Option<&str>,
        operation: F,
    ) -> HttpResponse
    where
        T: DeserializeOwned,
        O: Serialize,
        F: FnOnce(
            &RequestEnvelope<T>,
            &CorrelationId,
            &CorrelationId,
        ) -> std::result::Result<O, ApiError>,
    {
        let fallback = generated_context(now, "authentication");
        let attempt = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let session = match authenticated_session(headers, expected_session) {
            Ok(value) => value,
            Err(error) => {
                let response = failure(&fallback, error);
                if self.security_enforced {
                    self.audit_response(
                        action,
                        &instance_resource(&self.service.instance),
                        &fallback,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                }
                return response;
            }
        };
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, &self.service.instance) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    let response = failure(&context, error);
                    if self.security_enforced {
                        self.audit_response(
                            action,
                            &instance_resource(&self.service.instance),
                            &context,
                            None,
                            body,
                            &response,
                            now,
                            attempt,
                        )
                        .unwrap_or_else(|error| {
                            tracing::error!(error = %error, "RRD security audit append failed");
                        });
                    }
                    return response;
                }
            };
        let principal = if self.security_enforced {
            match self.service.session_principal(
                &session.0,
                &session.1,
                now,
                envelope.context.request_id.as_str(),
                envelope.context.operation_id.as_str(),
            ) {
                Ok(Some(principal)) => Some(principal),
                Ok(None) => {
                    let response = failure(
                        &envelope.context,
                        ApiError::new(
                            ErrorCode::Unauthenticated,
                            "session is not bound to a security principal",
                            false,
                        ),
                    );
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
                Err(error) => {
                    let response = failure(&envelope.context, api_error(error));
                    self.audit_response(
                        action,
                        &envelope.resource,
                        &envelope.context,
                        None,
                        body,
                        &response,
                        now,
                        attempt,
                    )
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %error, "RRD security audit append failed");
                    });
                    return response;
                }
            }
        } else {
            None
        };
        if let Some(principal) = &principal {
            if let Err(error) =
                SecurityRepository::new(self.service.engine(), self.service.instance.clone())
                    .authorize_principal(principal, action, &envelope.resource, now)
            {
                let response = failure(&envelope.context, security_error(error));
                self.audit_response(
                    action,
                    &envelope.resource,
                    &envelope.context,
                    Some(principal.clone()),
                    body,
                    &response,
                    now,
                    attempt,
                )
                .unwrap_or_else(|error| {
                    tracing::error!(error = %error, "RRD security audit append failed");
                });
                return response;
            }
            if let Err(error) = self.audit_authorized(
                action,
                &envelope.resource,
                &envelope.context,
                Some(principal.clone()),
                body,
                now,
                attempt,
            ) {
                return failure(&envelope.context, security_error(error));
            }
        }
        let response = match operation(&envelope, &session.0, &session.1) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        };
        if self.security_enforced {
            self.audit_response(
                action,
                &envelope.resource,
                &envelope.context,
                principal,
                body,
                &response,
                now,
                attempt,
            )
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "RRD security audit append failed");
            });
        }
        response
    }

    #[allow(clippy::too_many_arguments)]
    fn audit_response(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        response: &HttpResponse,
        now: u64,
        attempt: u64,
    ) -> std::result::Result<(), rrd_security::Error> {
        let status_code = response.status().as_u16();
        let decision = match status_code {
            200..=299 => AuditDecision::Allowed,
            401 | 403 => AuditDecision::Denied,
            _ => AuditDecision::Failed,
        };
        let response_sha256 = response
            .extensions()
            .get::<ResponseDigest>()
            .map(|digest| digest.0.clone())
            .unwrap_or_else(|| digest::sha256_hex(status_code.to_string().as_bytes()));
        self.append_audit_record(
            action,
            resource,
            context,
            principal_id,
            request_body,
            now,
            attempt,
            AuditPhase::Completed,
            decision,
            status_code,
            response_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn audit_authorized(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        now: u64,
        attempt: u64,
    ) -> std::result::Result<(), rrd_security::Error> {
        self.append_audit_record(
            action,
            resource,
            context,
            principal_id,
            request_body,
            now,
            attempt,
            AuditPhase::Authorized,
            AuditDecision::Allowed,
            100,
            digest::sha256_hex(b"rrd-audit-completion-pending"),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn append_audit_record(
        &self,
        action: SecurityAction,
        resource: &rrd_contract::ResourcePath,
        context: &RequestContext,
        principal_id: Option<CanonicalId>,
        request_body: &[u8],
        now: u64,
        attempt: u64,
        phase: AuditPhase,
        decision: AuditDecision,
        status_code: u16,
        response_sha256: String,
    ) -> std::result::Result<(), rrd_security::Error> {
        let request_sha256 = digest::sha256_hex(request_body);
        let identity = digest::sha256_hex(
            serde_json::to_vec(&(
                context.request_id.as_str(),
                context.operation_id.as_str(),
                action,
                phase,
                now,
                attempt,
                &request_sha256,
                &response_sha256,
            ))
            .expect("audit identity fields serialize")
            .as_slice(),
        );
        SecurityRepository::new(self.service.engine(), self.service.instance.clone()).append_audit(
            &AuditRecord {
                audit_id: CanonicalId::new(format!("audit-{identity}"))
                    .expect("digest audit identity is canonical"),
                at_unix_ms: now,
                principal_id,
                action,
                resource: resource.clone(),
                request_id: context.request_id.as_str().into(),
                operation_id: context.operation_id.as_str().into(),
                phase,
                decision,
                status_code,
                request_sha256,
                response_sha256,
            },
        )
    }
}

type HttpResponse = Response<Body>;

#[derive(Debug, Clone)]
struct ResponseDigest(String);

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

impl ApiError {
    fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        let status = status_for(code);
        Self {
            status,
            body: ErrorBody {
                code,
                message: message.into(),
                retryable,
                details: BTreeMap::new(),
            },
        }
    }
}

fn parse_envelope<T: DeserializeOwned>(
    headers: &HeaderMap,
    bytes: &[u8],
    now: u64,
    mutation: bool,
    instance: &CanonicalId,
) -> std::result::Result<RequestEnvelope<T>, Box<(RequestContext, ApiError)>> {
    let fallback = generated_context(now, "invalid-envelope");
    if !header_values(headers, "Content-Type")
        .is_ok_and(|values| values.len() == 1 && values[0].starts_with("application/json"))
    {
        return Err(Box::new((
            fallback,
            ApiError::new(
                ErrorCode::InvalidArgument,
                "Content-Type must be application/json",
                false,
            ),
        )));
    }
    if bytes.len() > RRD_MAX_BODY_BYTES {
        return Err(Box::new((
            fallback,
            ApiError::new(
                ErrorCode::ResourceExhausted,
                "request body exceeds one MiB",
                false,
            ),
        )));
    }
    let envelope: RequestEnvelope<T> = serde_json::from_slice(bytes).map_err(|error| {
        Box::new((
            fallback.clone(),
            ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
        ))
    })?;
    envelope.validate(mutation).map_err(|error| {
        Box::new((
            envelope.context.clone(),
            ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false),
        ))
    })?;
    let target = envelope
        .resource
        .segments
        .iter()
        .find(|segment| segment.kind == ResourceKind::Instance);
    if target.is_none_or(|target| target.id != *instance) {
        return Err(Box::new((
            envelope.context.clone(),
            ApiError::new(
                ErrorCode::FailedPrecondition,
                "request resource does not target this instance",
                false,
            ),
        )));
    }
    if envelope
        .context
        .deadline_unix_ms
        .is_some_and(|deadline| deadline <= now)
    {
        return Err(Box::new((
            envelope.context.clone(),
            ApiError::new(
                ErrorCode::DeadlineExceeded,
                "request deadline elapsed",
                false,
            ),
        )));
    }
    Ok(envelope)
}

fn authenticated_session(
    headers: &HeaderMap,
    expected_session: Option<&str>,
) -> std::result::Result<(CorrelationId, CorrelationId), ApiError> {
    let authentication_error = || {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one valid session and authorization header are required",
            false,
        )
    };
    let sessions = header_values(headers, "X-RRD-Session").map_err(|_| authentication_error())?;
    let authorizations =
        header_values(headers, "Authorization").map_err(|_| authentication_error())?;
    if sessions.len() != 1 || authorizations.len() != 1 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one session and authorization header are required",
            false,
        ));
    }
    if expected_session.is_some_and(|expected| sessions[0] != expected) {
        return Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "session path and header differ",
            false,
        ));
    }
    let token = authorizations[0].strip_prefix("Bearer ").ok_or_else(|| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "Authorization must use Bearer",
            false,
        )
    })?;
    Ok((parse_correlation(sessions[0])?, parse_correlation(token)?))
}

fn api_key_identity(headers: &HeaderMap) -> std::result::Result<(CanonicalId, String), ApiError> {
    let principals = header_values(headers, "X-RRD-Principal").map_err(|_| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        )
    })?;
    let authorizations = header_values(headers, "Authorization").map_err(|_| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        )
    })?;
    if principals.len() != 1 || authorizations.len() != 1 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "exactly one principal and API-key authorization header are required",
            false,
        ));
    }
    let credential = authorizations[0].strip_prefix("ApiKey ").ok_or_else(|| {
        ApiError::new(
            ErrorCode::Unauthenticated,
            "session creation Authorization must use ApiKey",
            false,
        )
    })?;
    if credential.is_empty() || credential.len() > 4_096 {
        return Err(ApiError::new(
            ErrorCode::Unauthenticated,
            "API key is empty or oversized",
            false,
        ));
    }
    Ok((
        CanonicalId::new(principals[0])
            .map_err(|error| ApiError::new(ErrorCode::Unauthenticated, error.to_string(), false))?,
        credential.into(),
    ))
}

fn header_values<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> std::result::Result<Vec<&'a str>, ApiError> {
    let values = headers
        .get_all(name)
        .iter()
        .map(|value| {
            value.to_str().map_err(|_| {
                ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("{name} header is not visible ASCII"),
                    false,
                )
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidArgument,
            format!("missing {name} header"),
            false,
        ));
    }
    Ok(values)
}

fn parse_correlation(value: &str) -> std::result::Result<CorrelationId, ApiError> {
    CorrelationId::new(value)
        .map_err(|error| ApiError::new(ErrorCode::InvalidArgument, error.to_string(), false))
}

fn required_idempotency(context: &RequestContext) -> std::result::Result<&CorrelationId, ApiError> {
    context.idempotency_key.as_ref().ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidArgument,
            "mutation requires idempotency key",
            false,
        )
    })
}

fn generated_context(now: u64, seed: &str) -> RequestContext {
    let counter = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = digest::sha256_hex(format!("{now}\0{seed}\0{counter}").as_bytes());
    RequestContext {
        request_id: CorrelationId::new(format!("request-{digest}")).expect("generated request id"),
        operation_id: CorrelationId::new(format!("operation-{digest}"))
            .expect("generated operation id"),
        idempotency_key: None,
        deadline_unix_ms: None,
    }
}

fn instance_resource(instance: &CanonicalId) -> rrd_contract::ResourcePath {
    rrd_contract::ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Instance, instance.as_str().to_owned())
                .expect("server instance identity is already canonical"),
        ],
    }
}

fn success<T: Serialize>(status: StatusCode, context: &RequestContext, payload: T) -> HttpResponse {
    json_response(
        status,
        &ResponseEnvelope {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.clone(),
            outcome: ResponseOutcome::Ok { payload },
        },
    )
}

fn failure(context: &RequestContext, error: ApiError) -> HttpResponse {
    json_response(
        error.status,
        &ResponseEnvelope::<serde_json::Value> {
            protocol: PROTOCOL.into(),
            protocol_version: PROTOCOL_VERSION,
            request_id: context.request_id.clone(),
            operation_id: context.operation_id.clone(),
            outcome: ResponseOutcome::Error { error: error.body },
        },
    )
}

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> HttpResponse {
    let bytes = serde_json::to_vec(body)
        .unwrap_or_else(|error| format!("{{\"serialization_error\":{error:?}}}").into_bytes());
    let response_sha256 = digest::sha256_hex(&bytes);
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .extensions_mut()
        .insert(ResponseDigest(response_sha256));
    response
}

fn api_error(error: ServiceError) -> ApiError {
    let message = error.to_string();
    match error {
        ServiceError::Contract(_)
        | ServiceError::Changefeed(_)
        | ServiceError::Query(_)
        | ServiceError::Vector(_)
        | ServiceError::OperationDigestMismatch
        | ServiceError::WrongScope => ApiError::new(ErrorCode::InvalidArgument, message, false),
        ServiceError::SessionNotFound | ServiceError::TransactionNotFound => {
            ApiError::new(ErrorCode::NotFound, message, false)
        }
        ServiceError::Unauthenticated => ApiError::new(ErrorCode::Unauthenticated, message, false),
        ServiceError::IdempotencyConflict => ApiError::new(ErrorCode::Conflict, message, false),
        ServiceError::SessionExpired
        | ServiceError::TransactionExpired
        | ServiceError::TransactionClosed
        | ServiceError::CommitInProgress => {
            ApiError::new(ErrorCode::FailedPrecondition, message, false)
        }
        ServiceError::TransactionQuota | ServiceError::RenewalQuota => {
            ApiError::new(ErrorCode::ResourceExhausted, message, false)
        }
        ServiceError::Store(StoreError::ControlConflict(_))
        | ServiceError::Store(StoreError::IdempotencyConflict(_)) => {
            ApiError::new(ErrorCode::Conflict, message, true)
        }
        ServiceError::Store(_) => ApiError::new(ErrorCode::Internal, message, false),
        ServiceError::Backup(_) => ApiError::new(ErrorCode::Internal, message, false),
        ServiceError::Estate(_) => ApiError::new(ErrorCode::Internal, message, false),
    }
}

fn security_error(error: rrd_security::Error) -> ApiError {
    let message = error.to_string();
    match error {
        rrd_security::Error::Unauthenticated | rrd_security::Error::PrincipalNotFound => {
            ApiError::new(ErrorCode::Unauthenticated, message, false)
        }
        rrd_security::Error::PermissionDenied | rrd_security::Error::NotInitialized => {
            ApiError::new(ErrorCode::PermissionDenied, message, false)
        }
        rrd_security::Error::Invalid(_) => {
            ApiError::new(ErrorCode::InvalidArgument, message, false)
        }
        rrd_security::Error::IdempotencyConflict | rrd_security::Error::AlreadyInitialized => {
            ApiError::new(ErrorCode::Conflict, message, false)
        }
        rrd_security::Error::Store(_) => ApiError::new(ErrorCode::Internal, message, false),
    }
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::InvalidArgument => StatusCode::BAD_REQUEST,
        ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::AlreadyExists | ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        ErrorCode::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::Cancelled => StatusCode::from_u16(499).expect("valid nonstandard status"),
        ErrorCode::Internal | ErrorCode::Corruption => StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        ErrorCode::UnsupportedVersion => StatusCode::HTTP_VERSION_NOT_SUPPORTED,
    }
}

fn session_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/sessions/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn session_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/v1/sessions/")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

fn transaction_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/transactions/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn transaction_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/v1/transactions/")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

fn estate_action<'a>(path: &'a str, action: &str) -> Option<&'a str> {
    path.strip_prefix("/v1/estates/")?
        .strip_suffix(&format!("/{action}"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn capabilities(
    instance: &CanonicalId,
    backend: CanonicalId,
    security_enforced: bool,
) -> ServiceCapabilities {
    let mut capabilities = vec![
        CapabilityDescriptor {
            name: CanonicalId::new("changefeed-follow").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-wait-ms").unwrap(),
                rrd_contract::MAX_CHANGEFEED_WAIT_MS,
            )]),
            limitation: Some(
                "bounded authenticated long-poll over retained replay; streaming transport, durable subscription leases, and server push remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("changefeed-replay").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-page-changes").unwrap(),
                rrd_contract::MAX_CHANGEFEED_PAGE,
            )]),
            limitation: Some(
                "authenticated retained cursor replay with lossless claim and typed data snapshots"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("claim-transactions").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-mutations").unwrap(),
                rrd_contract::MAX_TRANSACTION_CLAIMS as u64,
            )]),
            limitation: Some(format!("legacy claim-only scope on {backend}")),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("multi-model-transactions").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-mutations").unwrap(),
                rrd_contract::MAX_TRANSACTION_CLAIMS as u64,
            )]),
            limitation: Some(
                "atomic schema, claim, record, relation, event, vector, series, geo, and pre-staged object-reference commits; mutating VyrmQL and read-your-writes remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("exact-vyrmql-query").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([
                (
                    CanonicalId::new("max-query-bytes").unwrap(),
                    rrd_contract::MAX_QUERY_BYTES as u64,
                ),
                (
                    CanonicalId::new("max-output-bytes").unwrap(),
                    rrd_contract::MAX_QUERY_OUTPUT_BYTES,
                ),
            ]),
            limitation: Some(
                "exact session-scoped VyrmQL/VyrmMX reads; mutating VyrmQL and live queries remain open"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("endpoint-catalogue").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Available,
            limits: BTreeMap::from([(
                CanonicalId::new("endpoint-count").unwrap(),
                rrd_contract::endpoint_catalogue().endpoints.len() as u64,
            )]),
            limitation: Some(
                "machine-readable operation catalogue and OpenAPI 3.1 schemas; generated language packages and shared conformance remain F5 work"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("estate-authority-read").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::new(),
            limitation: Some(
                "read-only estate snapshots; estate mutations remain unavailable"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("lifecycle-journal").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Available,
            limits: BTreeMap::new(),
            limitation: None,
        },
        CapabilityDescriptor {
            name: CanonicalId::new("local-transport-leases").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-body-bytes").unwrap(),
                RRD_MAX_BODY_BYTES as u64,
            )]),
            limitation: Some(if security_enforced {
                "principal-authenticated policy-bound local sessions; TLS remote transport remains unavailable"
                    .into()
            } else {
                "development availability leases; no security authority is initialized".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("logical-backup-restore").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::new(),
            limitation: Some(
                "content-authenticated logical backup and restore-to-generated-new-root; object payloads are referenced-only and restore never switches the active instance"
                    .into(),
            ),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("remote-listen").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Unavailable,
            limits: BTreeMap::new(),
            limitation: Some("non-loopback bind denied until F4 security".into()),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("security-policy").unwrap(),
            contract_version: 1,
            status: if security_enforced {
                CapabilityStatus::Experimental
            } else {
                CapabilityStatus::Unavailable
            },
            limits: BTreeMap::new(),
            limitation: Some(if security_enforced {
                "persistent principals and exact deny-by-default endpoint policy; provisioning and TLS remain open"
                    .into()
            } else {
                "no persistent security authority is initialized for this instance".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("security-audit").unwrap(),
            contract_version: 1,
            status: if security_enforced {
                CapabilityStatus::Experimental
            } else {
                CapabilityStatus::Unavailable
            },
            limits: BTreeMap::from([(
                CanonicalId::new("max-page-records").unwrap(),
                rrd_security::MAX_AUDIT_PAGE as u64,
            )]),
            limitation: Some(if security_enforced {
                "redacted authenticated-journal audit for routed HTTP outcomes; atomic authorization reservation and external archival remain open"
                    .into()
            } else {
                "audit requires initialized persistent security authority".into()
            }),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("vector-search").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([
                (
                    CanonicalId::new("max-scanned-changes").unwrap(),
                    rrd_contract::MAX_VECTOR_SEARCH_CHANGES,
                ),
                (
                    CanonicalId::new("max-top-k").unwrap(),
                    rrd_contract::MAX_VECTOR_SEARCH_TOP_K,
                ),
            ]),
            limitation: Some(
                "exact canonical dense, sparse, and multi-vector search; public filters and persisted HNSW/TurboQuant artifact serving remain open"
                    .into(),
            ),
        },
    ];
    capabilities.sort_by(|left, right| left.name.cmp(&right.name));
    ServiceCapabilities {
        protocol: PROTOCOL.into(),
        protocol_version: PROTOCOL_VERSION,
        implementation: CanonicalId::new("vyrm").unwrap(),
        implementation_version: env!("CARGO_PKG_VERSION").into(),
        deployment_mode: DeploymentMode::LocalServer,
        instance: ResourceId {
            kind: ResourceKind::Instance,
            id: instance.clone(),
        },
        capabilities,
    }
}

pub fn load_or_create_token_key(path: &Path) -> Result<[u8; TOKEN_KEY_BYTES]> {
    match read_token_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
        Err(_) => {}
    }
    let mut key = [0_u8; TOKEN_KEY_BYTES];
    getrandom::fill(&mut key)
        .map_err(|error| io::Error::other(format!("token-key entropy failed: {error}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key)?;
            file.sync_all()?;
            sync_parent(path)?;
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            read_token_key(path).map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(path.parent().expect("token key has a parent"))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn read_token_key(path: &Path) -> io::Result<[u8; TOKEN_KEY_BYTES]> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "token key is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "token key must not be accessible to group or other users",
            ));
        }
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((TOKEN_KEY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "token key must contain exactly 32 bytes",
        )
    })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
