//! Client-only HTTP gateway and embedded frontend for Connectome.
//!
//! RRD is the sole data authority. This crate owns presentation and a bounded
//! authenticated session, but it never opens an RRD store or composes physical
//! query, vector, estate, cluster, or engine crates.

use rrd_client::{ClientConfig, RequestOptions, RrdClient, Session};
use rrd_contract::{
    CanonicalId, CreateSession, DiagnosticSnapshot, ExecuteQuery, QueryBudget, QueryResult,
    QueryValue, ReadDiagnosticSnapshot, RuntimeToolCatalogue, RuntimeToolInvocationResult,
    ServiceCapabilities, SessionLimits,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX: &str = include_str!("../static/index.html");
const CSS: &str = include_str!("../static/app.css");
const JS: &str = include_str!("../static/app.js");
const REQUEST_BODY_LIMIT: u64 = 1024 * 1024;
const SESSION_RENEWAL_MARGIN_MS: u64 = 30_000;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Connection and presentation settings. The API key is deliberately never
/// included in a debug representation or HTTP response.
pub struct ConnectomeConfig {
    pub rrd_address: SocketAddr,
    pub instance: CanonicalId,
    pub principal: CanonicalId,
    pub api_key: String,
    pub scope: String,
    pub bind: SocketAddr,
}

impl ConnectomeConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.rrd_address.ip().is_loopback() {
            return Err("Connectome local HTTP mode requires a loopback RRD address".into());
        }
        if self.api_key.is_empty() || self.api_key.as_bytes().contains(&0) {
            return Err("RRD API key is empty or contains NUL".into());
        }
        let request = diagnostic_request(&self.scope, now_unix_ms()?);
        request.validate()?;
        Ok(())
    }
}

/// A synchronous presentation adapter around the supported asynchronous Rust
/// client. Each outward operation is a typed RRD request under one renewable
/// authenticated session.
pub struct ConnectomeBackend {
    runtime: tokio::runtime::Runtime,
    client: RrdClient,
    session: Mutex<Session>,
    principal: CanonicalId,
    api_key: String,
    scope: String,
    sequence: AtomicU64,
}

impl ConnectomeBackend {
    pub fn connect(config: &ConnectomeConfig) -> Result<Self> {
        config.validate()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        let client = RrdClient::connect_local(
            config.rrd_address,
            config.instance.clone(),
            ClientConfig {
                request_timeout: Duration::from_secs(10),
                max_attempts: 2,
            },
        )?;
        runtime.block_on(client.capabilities())?;
        let session = runtime.block_on(client.create_session(
            config.principal.clone(),
            &config.api_key,
            session_request(),
            RequestOptions::mutation(
                "connectome-session-request-1",
                "connectome-session-operation-1",
                "connectome-session-key-1",
            )?,
        ))?;
        Ok(Self {
            runtime,
            client,
            session: Mutex::new(session),
            principal: config.principal.clone(),
            api_key: config.api_key.clone(),
            scope: config.scope.clone(),
            sequence: AtomicU64::new(1),
        })
    }

    pub fn service_capabilities(&self) -> Result<ServiceCapabilities> {
        Ok(self.runtime.block_on(self.client.capabilities())?)
    }

    pub fn diagnostic_snapshot(&self) -> Result<DiagnosticSnapshot> {
        let session = self.current_session()?;
        let now = now_unix_ms()?;
        let (request, operation) = self.read_options("diagnostic-snapshot")?;
        Ok(self.runtime.block_on(self.client.read_diagnostic_snapshot(
            &session,
            diagnostic_request(&self.scope, now),
            RequestOptions::read(&request, &operation)?,
        ))?)
    }

    pub fn runtime_tool_catalogue(&self) -> Result<RuntimeToolCatalogue> {
        let session = self.current_session()?;
        let (request, operation) = self.read_options("runtime-tools")?;
        Ok(self.runtime.block_on(
            self.client
                .runtime_tool_catalogue(&session, RequestOptions::read(&request, &operation)?),
        )?)
    }

    pub fn execute_query(&self, request: QueryRequest) -> Result<QueryResult> {
        request.validate()?;
        let session = self.current_session()?;
        let (request_id, operation_id) = self.read_options("query")?;
        Ok(self.runtime.block_on(self.client.execute_query(
            &session,
            ExecuteQuery {
                scope: self.scope.clone(),
                query: request.query,
                parameters: request.parameters,
                budget: request.budget,
            },
            RequestOptions::read(&request_id, &operation_id)?,
        ))?)
    }

    pub fn invoke_runtime_tool(
        &self,
        request: InvokeToolRequest,
    ) -> Result<RuntimeToolInvocationResult> {
        request.validate()?;
        let session = self.current_session()?;
        let catalogue = self.runtime_tool_catalogue()?;
        let descriptor = catalogue
            .tools
            .iter()
            .find(|descriptor| descriptor.name == request.tool)
            .ok_or("requested runtime tool is absent from the RRD catalogue")?;
        let ordinal = self.next_sequence()?;
        let request_id = format!("connectome-tool-request-{ordinal}");
        let operation_id = format!("connectome-tool-operation-{ordinal}");
        let options = if descriptor.mutation {
            let key = format!("connectome-tool-key-{ordinal}");
            RequestOptions::mutation(&request_id, &operation_id, &key)?
        } else {
            RequestOptions::read(&request_id, &operation_id)?
        };
        Ok(self.runtime.block_on(self.client.invoke_runtime_tool(
            &session,
            &catalogue,
            &request.tool,
            request.arguments,
            options,
        ))?)
    }

    fn current_session(&self) -> Result<Session> {
        let now = now_unix_ms()?;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| "Connectome session lock is poisoned")?;
        if now.saturating_add(SESSION_RENEWAL_MARGIN_MS) >= guard.lease.idle_expires_at_unix_ms
            || now.saturating_add(SESSION_RENEWAL_MARGIN_MS)
                >= guard.lease.absolute_expires_at_unix_ms
        {
            let ordinal = self.next_sequence()?;
            *guard = self.runtime.block_on(self.client.create_session(
                self.principal.clone(),
                &self.api_key,
                session_request(),
                RequestOptions::mutation(
                    &format!("connectome-session-request-{ordinal}"),
                    &format!("connectome-session-operation-{ordinal}"),
                    &format!("connectome-session-key-{ordinal}"),
                )?,
            ))?;
        }
        Ok(guard.clone())
    }

    fn read_options(&self, name: &str) -> Result<(String, String)> {
        let ordinal = self.next_sequence()?;
        Ok((
            format!("connectome-{name}-request-{ordinal}"),
            format!("connectome-{name}-operation-{ordinal}"),
        ))
    }

    fn next_sequence(&self) -> Result<u64> {
        self.sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "Connectome request sequence exhausted".into())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, QueryValue>,
    #[serde(default)]
    pub budget: QueryBudget,
}

impl QueryRequest {
    fn validate(&self) -> Result<()> {
        if self.query.trim().is_empty() {
            return Err("query is required".into());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeToolRequest {
    pub tool: CanonicalId,
    pub arguments: serde_json::Value,
}

impl InvokeToolRequest {
    fn validate(&self) -> Result<()> {
        if !self.tool.as_str().starts_with("rrflow_") || !self.arguments.is_object() {
            return Err(
                "tool must name an rrflow_* operation and arguments must be an object".into(),
            );
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct CapabilityView {
    service: ServiceCapabilities,
    runtime_tools: RuntimeToolCatalogue,
}

pub fn serve(config: ConnectomeConfig) -> Result<()> {
    config.validate()?;
    let server = Server::http(config.bind)?;
    let backend = ConnectomeBackend::connect(&config)?;
    eprintln!(
        "connectome: http://{} -> rrd://{} instance={} scope={} principal={}",
        config.bind, config.rrd_address, config.instance, config.scope, config.principal
    );
    for request in server.incoming_requests() {
        respond(request, &backend);
    }
    Ok(())
}

fn respond(mut request: Request, backend: &ConnectomeBackend) {
    let path = request.url().split('?').next().unwrap_or(request.url());
    let response = match (request.method(), path) {
        (&Method::Get | &Method::Head, "/" | "/index.html") => {
            text_response(StatusCode(200), "text/html; charset=utf-8", INDEX)
        }
        (&Method::Get | &Method::Head, "/app.css") => {
            text_response(StatusCode(200), "text/css; charset=utf-8", CSS)
        }
        (&Method::Get | &Method::Head, "/app.js") => {
            text_response(StatusCode(200), "text/javascript; charset=utf-8", JS)
        }
        (&Method::Get | &Method::Head, "/api/snapshot") => match backend.diagnostic_snapshot() {
            Ok(snapshot) => json_response(StatusCode(200), &snapshot),
            Err(error) => gateway_error(error),
        },
        (&Method::Get | &Method::Head, "/api/runtime/capabilities") => {
            match (
                backend.service_capabilities(),
                backend.runtime_tool_catalogue(),
            ) {
                (Ok(service), Ok(runtime_tools)) => json_response(
                    StatusCode(200),
                    &CapabilityView {
                        service,
                        runtime_tools,
                    },
                ),
                (Err(error), _) | (_, Err(error)) => gateway_error(error),
            }
        }
        (&Method::Post, "/api/runtime/query") => {
            match read_json::<QueryRequest>(&mut request)
                .and_then(|value| backend.execute_query(value))
            {
                Ok(result) => json_response(StatusCode(200), &result),
                Err(error) => request_error(error),
            }
        }
        (&Method::Post, "/api/runtime/tools/invoke") => {
            match read_json::<InvokeToolRequest>(&mut request)
                .and_then(|value| backend.invoke_runtime_tool(value))
            {
                Ok(result) => json_response(StatusCode(200), &result),
                Err(error) => request_error(error),
            }
        }
        (&Method::Post, "/api/flights" | "/api/demos/prompt-strength" | "/api/cluster/samples") => {
            json_response(
                StatusCode(501),
                &serde_json::json!({
                    "error": "the retired embedded diagnostics mutation has no governed RRD operation",
                    "required_action": "add an exact rrd-contract/rrd-engine operation before exposing this mutation"
                }),
            )
        }
        (
            &Method::Get | &Method::Head,
            "/api/flights" | "/api/cluster/history" | "/api/runtime/traces" | "/api/route",
        ) => json_response(
            StatusCode(501),
            &serde_json::json!({
                "error": "this legacy projection is not part of the authoritative diagnostic snapshot",
                "available": ["/api/snapshot", "/api/runtime/capabilities", "/api/runtime/query", "/api/runtime/tools/invoke"]
            }),
        ),
        (&Method::Get | &Method::Head, _) => {
            json_response(StatusCode(404), &serde_json::json!({"error": "not found"}))
        }
        _ => json_response(
            StatusCode(405),
            &serde_json::json!({"error": "method not allowed"}),
        ),
    };
    let _ = request.respond(response);
}

fn diagnostic_request(scope: &str, now: u64) -> ReadDiagnosticSnapshot {
    ReadDiagnosticSnapshot {
        scope: scope.to_owned(),
        graph_valid_at_unix_ms: now,
        graph_known_at_cursor: None,
        graph_compare_cursor: 0,
        runtime_max_scanned_changes: 1_000_000,
        changes_after_cursor: 0,
        change_limit: 4_096,
        audit_after_sequence: 0,
        audit_limit: 1_024,
    }
}

fn session_request() -> CreateSession {
    CreateSession {
        limits: SessionLimits {
            idle_timeout_ms: 900_000,
            absolute_timeout_ms: 3_600_000,
            max_open_transactions: 4,
        },
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T> {
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(REQUEST_BODY_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > REQUEST_BODY_LIMIT {
        return Err("request body exceeds one MiB".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn request_error(
    error: Box<dyn std::error::Error + Send + Sync>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        StatusCode(400),
        &serde_json::json!({"error": error.to_string()}),
    )
}

fn gateway_error(
    error: Box<dyn std::error::Error + Send + Sync>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(
        StatusCode(502),
        &serde_json::json!({"error": error.to_string()}),
    )
}

fn text_response(
    status: StatusCode,
    content_type: &str,
    body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", content_type).expect("valid header"))
        .with_header(Header::from_bytes("Cache-Control", "no-store").expect("valid header"))
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").expect("valid header"))
        .with_header(Header::from_bytes("Referrer-Policy", "no-referrer").expect("valid header"))
}

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    match serde_json::to_vec(body) {
        Ok(body) => Response::from_data(body)
            .with_status_code(status)
            .with_header(
                Header::from_bytes("Content-Type", "application/json; charset=utf-8")
                    .expect("valid header"),
            )
            .with_header(Header::from_bytes("Cache-Control", "no-store").expect("valid header"))
            .with_header(
                Header::from_bytes("X-Content-Type-Options", "nosniff").expect("valid header"),
            )
            .with_header(
                Header::from_bytes("Referrer-Policy", "no-referrer").expect("valid header"),
            ),
        Err(error) => text_response(
            StatusCode(500),
            "application/json; charset=utf-8",
            &format!("{error}"),
        ),
    }
}

fn now_unix_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_rrd_and_empty_credentials() {
        let mut config = ConnectomeConfig {
            rrd_address: "10.0.0.1:9477".parse().unwrap(),
            instance: CanonicalId::new("test-instance").unwrap(),
            principal: CanonicalId::new("connectome").unwrap(),
            api_key: "secret".into(),
            scope: "instance:test-instance".into(),
            bind: "127.0.0.1:4387".parse().unwrap(),
        };
        assert!(config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("loopback"));
        config.rrd_address = "127.0.0.1:9477".parse().unwrap();
        config.api_key.clear();
        assert!(config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("API key"));
    }
}
