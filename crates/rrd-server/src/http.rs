use crate::{RrdService, ServiceError};
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use rrd_contract::{
    AbortTransaction, BeginTransaction, CanonicalId, CapabilityDescriptor, CapabilityStatus,
    CloseSession, CommitTransaction, CorrelationId, CreateSession, DeploymentMode, ErrorBody,
    ErrorCode, Liveness, PreviewTransaction, Readiness, RenewSession, RequestContext,
    RequestEnvelope, ResourceId, ResourceKind, ResponseEnvelope, ResponseOutcome,
    ServiceCapabilities, PROTOCOL, PROTOCOL_VERSION,
};
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
        let listener =
            TcpListener::bind(bind).map_err(|error| HttpError::Bind(error.to_string()))?;
        listener.set_nonblocking(true)?;
        let capabilities = capabilities(&instance, backend);
        capabilities
            .validate()
            .map_err(|error| HttpError::Contract(error.to_string()))?;
        let state = Arc::new(AppState {
            service: RrdService::new(engine, instance, token_key),
            capabilities,
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
        let response = match (method, path.as_str()) {
            (Method::GET, "/v1/health/live") => {
                let context = generated_context(now, "health-live");
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
                match self.readiness(now) {
                    Ok(ready) => success(StatusCode::OK, &context, ready),
                    Err(error) => failure(&context, api_error(error)),
                }
            }
            (Method::GET, "/v1/capabilities") => {
                let context = generated_context(now, "capabilities");
                success(StatusCode::OK, &context, self.capabilities.clone())
            }
            (Method::POST, "/v1/sessions") => self.create_session(&headers, &body, now),
            (Method::POST, path) if session_action(path, "renew").is_some() => {
                self.renew_session(&headers, &body, path, now)
            }
            (Method::DELETE, path) if session_id(path).is_some() => {
                self.close_session(&headers, &body, path, now)
            }
            (Method::POST, "/v1/transactions") => self.begin_transaction(&headers, &body, now),
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
                failure(
                    &context,
                    ApiError::new(ErrorCode::NotFound, "endpoint not found", false),
                )
            }
        };
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
        self.with_envelope::<CreateSession, _, _>(headers, body, now, true, |envelope| {
            let idempotency_key = required_idempotency(&envelope.context)?;
            self.service
                .create_session(
                    &envelope.payload,
                    idempotency_key,
                    now,
                    envelope.context.request_id.as_str(),
                    envelope.context.operation_id.as_str(),
                )
                .map_err(api_error)
        })
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
        operation: F,
    ) -> HttpResponse
    where
        T: DeserializeOwned,
        O: Serialize,
        F: FnOnce(&RequestEnvelope<T>) -> std::result::Result<O, ApiError>,
    {
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, &self.service.instance) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    return failure(&context, error);
                }
            };
        match operation(&envelope) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        }
    }

    fn with_authenticated_envelope<T, O, F>(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
        mutation: bool,
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
        let session = match authenticated_session(headers, expected_session) {
            Ok(value) => value,
            Err(error) => return failure(&fallback, error),
        };
        let envelope =
            match parse_envelope::<T>(headers, body, now, mutation, &self.service.instance) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let (context, error) = *error;
                    return failure(&context, error);
                }
            };
        match operation(&envelope, &session.0, &session.1) {
            Ok(payload) => success(StatusCode::OK, &envelope.context, payload),
            Err(error) => failure(&envelope.context, error),
        }
    }
}

type HttpResponse = Response<Body>;

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
}

fn api_error(error: ServiceError) -> ApiError {
    let message = error.to_string();
    match error {
        ServiceError::Contract(_)
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

fn capabilities(instance: &CanonicalId, backend: CanonicalId) -> ServiceCapabilities {
    let mut capabilities = vec![
        CapabilityDescriptor {
            name: CanonicalId::new("claim-transactions").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Experimental,
            limits: BTreeMap::from([(
                CanonicalId::new("max-mutations").unwrap(),
                rrd_contract::MAX_TRANSACTION_CLAIMS as u64,
            )]),
            limitation: Some(format!(
                "claim mutation surface on {backend}; F6 multi-model transactions remain open"
            )),
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
            limitation: Some("availability leases are not F4 user authentication".into()),
        },
        CapabilityDescriptor {
            name: CanonicalId::new("remote-listen").unwrap(),
            contract_version: 1,
            status: CapabilityStatus::Unavailable,
            limits: BTreeMap::new(),
            limitation: Some("non-loopback bind denied until F4 security".into()),
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
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            read_token_key(path).map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
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
