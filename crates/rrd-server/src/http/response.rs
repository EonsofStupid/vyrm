use super::*;

pub(super) type HttpResponse = Response<Body>;

#[derive(Debug, Clone)]
pub(super) struct ResponseDigest(pub(super) String);

#[derive(Debug)]
pub(super) struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

impl ApiError {
    pub(super) fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
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

pub(in crate::http) fn generated_context(now: u64, seed: &str) -> RequestContext {
    let counter = HTTP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = sha256_hex(format!("{now}\0{seed}\0{counter}").as_bytes());
    RequestContext {
        request_id: CorrelationId::new(format!("request-{digest}")).expect("generated request id"),
        operation_id: CorrelationId::new(format!("operation-{digest}"))
            .expect("generated operation id"),
        idempotency_key: None,
        deadline_unix_ms: None,
    }
}

pub(in crate::http) fn instance_resource(instance: &CanonicalId) -> rrd_contract::ResourcePath {
    rrd_contract::ResourcePath {
        segments: vec![
            ResourceId::new(ResourceKind::Instance, instance.as_str().to_owned())
                .expect("server instance identity is already canonical"),
        ],
    }
}

pub(in crate::http) fn success<T: Serialize>(
    status: StatusCode,
    context: &RequestContext,
    payload: T,
) -> HttpResponse {
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

pub(in crate::http) fn failure(context: &RequestContext, error: ApiError) -> HttpResponse {
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

pub(in crate::http) fn json_response<T: Serialize>(status: StatusCode, body: &T) -> HttpResponse {
    let bytes = serde_json::to_vec(body)
        .unwrap_or_else(|error| format!("{{\"serialization_error\":{error:?}}}").into_bytes());
    let response_sha256 = sha256_hex(&bytes);
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

pub(in crate::http) fn api_error(error: ServiceError) -> ApiError {
    let message = error.to_string();
    let retryable = error.retryable();
    let code = match error.kind() {
        ServiceErrorKind::InvalidArgument => ErrorCode::InvalidArgument,
        ServiceErrorKind::NotFound => ErrorCode::NotFound,
        ServiceErrorKind::Unauthenticated => ErrorCode::Unauthenticated,
        ServiceErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        ServiceErrorKind::Conflict => ErrorCode::Conflict,
        ServiceErrorKind::FailedPrecondition => ErrorCode::FailedPrecondition,
        ServiceErrorKind::ResourceExhausted => ErrorCode::ResourceExhausted,
        ServiceErrorKind::Internal => ErrorCode::Internal,
    };
    ApiError::new(code, message, retryable)
}

pub(in crate::http) fn sha256_hex(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub(in crate::http) fn status_for(code: ErrorCode) -> StatusCode {
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
