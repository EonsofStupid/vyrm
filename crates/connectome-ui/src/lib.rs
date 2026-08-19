//! Local API and embedded frontend for the connectome workbench.

mod flight;

pub use flight::{
    ContextMode, Flight, FlightEvent, FlightMetrics, FlightStatus, LaunchFlight, ReasoningProfile,
};

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use vyrm_core::{
    resolve_as_of, Claim, ClaimSource, ReasoningEvent, ReasoningPayload, RuntimeGraphSnapshot,
    RuntimeSchemaRegistry, ScopeId,
};
use vyrm_node::{InstanceBinding, InstanceMode};
use vyrm_store::{Engine, Invocation, ProjectionStatus, Store};

const INDEX: &str = include_str!("../static/index.html");
const CSS: &str = include_str!("../static/app.css");
const JS: &str = include_str!("../static/app.js");

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub generated_at: u64,
    pub instance: InstanceView,
    pub health: HealthView,
    pub claims: Vec<ClaimView>,
    pub runs: Vec<RunView>,
    pub files: Vec<FileView>,
    pub invocations: Vec<Invocation>,
    pub flights: Vec<Flight>,
    pub schema: Option<RuntimeSchemaRegistry>,
    pub capabilities: CapabilitiesView,
    pub graph: GraphView,
}

#[derive(Debug, Default, Serialize)]
pub struct CapabilitiesView {
    pub runners_enabled: bool,
    pub providers: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct InstanceView {
    pub id: String,
    pub mode: &'static str,
    pub root: PathBuf,
    pub member: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct HealthView {
    pub state: &'static str,
    pub claim_sequence: u64,
    pub runtime_cursor: u64,
    pub current_claims: usize,
    pub subjects: usize,
    pub projection_watermark: u64,
    pub projection_state: String,
    pub last_grounded_at: Option<u64>,
    pub routing_generation: Option<u64>,
    pub indexed_files: usize,
    pub indexed_symbols: usize,
    pub active_run: Option<String>,
    pub schema_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaimView {
    pub id: String,
    #[serde(flatten)]
    pub claim: Claim,
}

#[derive(Debug, Serialize)]
pub struct RunView {
    pub id: String,
    pub state: String,
    pub complete: bool,
    pub events: Vec<ReasoningEvent>,
}

#[derive(Debug, Serialize)]
pub struct FileView {
    pub path: PathBuf,
    pub language: String,
    pub lines: usize,
    pub symbols: usize,
    pub terms: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct GraphView {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: &'static str,
    pub detail: String,
    pub state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
    pub label: &'static str,
}

pub fn snapshot(
    store: &Store,
    binding: &InstanceBinding,
    at: u64,
) -> Result<Snapshot, Box<dyn std::error::Error>> {
    binding.require_runtime_ready()?;
    binding.verify_store_path(store.path())?;

    let subjects = store.subjects()?;
    let mut claims = Vec::new();
    for subject in &subjects {
        let mut by_predicate: BTreeMap<String, Vec<Claim>> = BTreeMap::new();
        for claim in store.subject_versions(subject)? {
            by_predicate
                .entry(claim.predicate.as_str().to_owned())
                .or_default()
                .push(claim);
        }
        for versions in by_predicate.into_values() {
            if let Some(claim) = resolve_as_of(&versions, at).cloned() {
                claims.push(ClaimView {
                    id: claim.digest(),
                    claim,
                });
            }
        }
    }
    claims.sort_by(|a, b| {
        a.claim
            .subject
            .as_str()
            .cmp(b.claim.subject.as_str())
            .then_with(|| a.claim.predicate.as_str().cmp(b.claim.predicate.as_str()))
    });

    let runs = vyrm_node::reasoning_runs(store)?
        .into_iter()
        .map(|run| RunView {
            id: run.id().to_owned(),
            state: format!("{:?}", run.state()),
            complete: run.is_complete(),
            events: run.events().to_vec(),
        })
        .collect::<Vec<_>>();
    let active_run = runs
        .iter()
        .find(|run| !run.complete)
        .map(|run| run.id.clone());

    let routing = vyrm_node::load_routing(store, &binding.project_root)?;
    let files = routing
        .as_ref()
        .map(|index| {
            index
                .files()
                .map(|file| FileView {
                    path: file.path.clone(),
                    language: format!("{:?}", file.language),
                    lines: file.lines,
                    symbols: file.occurrences.len(),
                    terms: file.terms.len(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let projection = store.current_projection()?;
    let (projection_state, quarantined) = match &projection.status {
        ProjectionStatus::Active => ("active".to_owned(), false),
        ProjectionStatus::Quarantined { at, differences } => (
            format!("quarantined at {at}: {} difference(s)", differences.len()),
            true,
        ),
    };
    let invocations = store.invocations_since(0)?;
    let flights = flight::stored_flights(store)?;
    let graph = build_graph(
        &binding.manifest.id,
        &claims,
        &runs,
        &files,
        &invocations,
        &flights,
    );
    let schema = store.runtime_schema(&ScopeId::new(vyrm_node::REASONING_SCOPE)?)?;
    let health = HealthView {
        state: if quarantined {
            "blocked"
        } else if routing.is_none() {
            "attention"
        } else {
            "ready"
        },
        claim_sequence: store.sequence()?,
        runtime_cursor: store.runtime_cursor()?,
        current_claims: claims.len(),
        subjects: subjects.len(),
        projection_watermark: projection.watermark,
        projection_state,
        last_grounded_at: projection.last_grounded.map(|stamp| stamp.at),
        routing_generation: routing.as_ref().map(|index| index.generation()),
        indexed_files: routing.as_ref().map_or(0, |index| index.file_count()),
        indexed_symbols: routing.as_ref().map_or(0, |index| index.symbol_count()),
        active_run,
        schema_revision: schema.as_ref().map(|schema| schema.revision),
    };

    Ok(Snapshot {
        generated_at: at,
        instance: InstanceView {
            id: binding.manifest.id.clone(),
            mode: match binding.manifest.mode {
                InstanceMode::Dedicated => "dedicated",
                InstanceMode::Umbrella => "umbrella",
            },
            root: binding.instance_root.clone(),
            member: binding.member.clone(),
        },
        health,
        claims,
        runs,
        files,
        invocations,
        flights,
        schema,
        capabilities: CapabilitiesView {
            runners_enabled: false,
            providers: vec!["observe"],
        },
        graph,
    })
}

/// Reconstructs the authoritative typed graph at a valid-time instant and a
/// transaction cursor. The bounded feed is the authority; this helper is a
/// read-only lens used by the workbench freeze/scrub API.
pub fn runtime_graph(
    store: &Store,
    scope: ScopeId,
    valid_at: u64,
    known_at_cursor: Option<u64>,
) -> Result<RuntimeGraphSnapshot, Box<dyn std::error::Error>> {
    let head = store.runtime_cursor()?;
    let target = known_at_cursor.unwrap_or(head).min(head);
    let mut cursor = 0;
    let mut changes = Vec::new();
    while cursor < target {
        let page = store.runtime_changes_since(cursor, 1_024, Some(&scope))?;
        changes.extend(
            page.changes
                .into_iter()
                .filter(|change| change.cursor <= target),
        );
        if page.through_cursor <= cursor {
            break;
        }
        cursor = page.through_cursor.min(target);
    }
    Ok(RuntimeGraphSnapshot::from_changes(
        &changes, scope, valid_at, target,
    ))
}

fn build_graph(
    instance: &str,
    claims: &[ClaimView],
    runs: &[RunView],
    files: &[FileView],
    invocations: &[Invocation],
    flights: &[Flight],
) -> GraphView {
    let root = format!("instance:{instance}");
    let mut graph = GraphView::default();
    graph.nodes.push(GraphNode {
        id: root.clone(),
        label: instance.to_owned(),
        kind: "instance",
        detail: "runtime instance".into(),
        state: "ready",
    });

    let mut subjects = BTreeMap::new();
    for claim in claims {
        let subject_id = format!("subject:{}", claim.claim.subject.as_str());
        if subjects.insert(subject_id.clone(), ()).is_none() {
            graph.nodes.push(GraphNode {
                id: subject_id.clone(),
                label: claim.claim.subject.as_str().to_owned(),
                kind: "subject",
                detail: "claim subject".into(),
                state: "current",
            });
            graph.edges.push(GraphEdge {
                from: root.clone(),
                to: subject_id.clone(),
                kind: "contains",
                label: "contains",
            });
        }
        let claim_id = format!("claim:{}", claim.id);
        graph.nodes.push(GraphNode {
            id: claim_id.clone(),
            label: claim.claim.predicate.as_str().to_owned(),
            kind: "claim",
            detail: claim.claim.object.clone(),
            state: "current",
        });
        graph.edges.push(GraphEdge {
            from: subject_id,
            to: claim_id,
            kind: "asserts",
            label: "asserts",
        });
    }

    for run in runs {
        let run_id = format!("run:{}", run.id);
        graph.nodes.push(GraphNode {
            id: run_id.clone(),
            label: run.id.clone(),
            kind: "run",
            detail: run.state.clone(),
            state: if run.complete { "complete" } else { "active" },
        });
        graph.edges.push(GraphEdge {
            from: root.clone(),
            to: run_id.clone(),
            kind: "runs",
            label: "runs",
        });
        let mut previous = run_id;
        for event in &run.events {
            let event_id = format!("event:{}:{}", run.id, event.ordinal);
            graph.nodes.push(GraphNode {
                id: event_id.clone(),
                label: event.payload.name().to_owned(),
                kind: "event",
                detail: format!("{} #{}", event.actor, event.ordinal),
                state: "recorded",
            });
            graph.edges.push(GraphEdge {
                from: previous,
                to: event_id.clone(),
                kind: "transition",
                label: "then",
            });
            let evidence = match &event.payload {
                ReasoningPayload::Observation { evidence, .. } => evidence
                    .iter()
                    .map(|item| (item, "observed"))
                    .collect::<Vec<_>>(),
                ReasoningPayload::Verification { checks } => checks
                    .iter()
                    .flat_map(|check| {
                        let state = match check.status {
                            vyrm_core::CheckStatus::Passed => "passed",
                            vyrm_core::CheckStatus::Failed => "failed",
                        };
                        check.evidence.iter().map(move |item| (item, state))
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            for (evidence, state) in evidence {
                let evidence_id = format!("evidence:{}", evidence.digest);
                if !graph.nodes.iter().any(|node| node.id == evidence_id) {
                    graph.nodes.push(GraphNode {
                        id: evidence_id.clone(),
                        label: evidence.source.clone(),
                        kind: "evidence",
                        detail: evidence.summary.clone(),
                        state,
                    });
                }
                graph.edges.push(GraphEdge {
                    from: event_id.clone(),
                    to: evidence_id,
                    kind: "evidence",
                    label: "evidence",
                });
            }
            previous = event_id;
        }
    }

    for file in files.iter().take(80) {
        let id = format!("file:{}", file.path.display());
        graph.nodes.push(GraphNode {
            id: id.clone(),
            label: file
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("file")
                .to_owned(),
            kind: "file",
            detail: file.path.display().to_string(),
            state: "indexed",
        });
        graph.edges.push(GraphEdge {
            from: root.clone(),
            to: id,
            kind: "indexes",
            label: "indexes",
        });
    }

    for invocation in invocations.iter().rev().take(20) {
        let id = format!("invocation:{}", invocation.ordinal);
        graph.nodes.push(GraphNode {
            id: id.clone(),
            label: invocation.command.clone(),
            kind: "invocation",
            detail: invocation
                .detail
                .clone()
                .unwrap_or_else(|| format!("{} ms", invocation.duration_ms)),
            state: match invocation.outcome {
                vyrm_store::Outcome::Ok => "ok",
                vyrm_store::Outcome::Error => "error",
            },
        });
        graph.edges.push(GraphEdge {
            from: root.clone(),
            to: id,
            kind: "invokes",
            label: "invokes",
        });
    }
    for flight in flights.iter().rev().take(24) {
        let flight_id = format!("flight:{}", flight.id);
        graph.nodes.push(GraphNode {
            id: flight_id.clone(),
            label: format!("{:?} · {}", flight.context_mode, flight.provider),
            kind: "flight",
            detail: flight.prompt.clone(),
            state: match flight.status {
                FlightStatus::Preparing => "preparing",
                FlightStatus::Prepared => "prepared",
                FlightStatus::Running => "running",
                FlightStatus::Succeeded => "succeeded",
                FlightStatus::Failed => "failed",
            },
        });
        graph.edges.push(GraphEdge {
            from: root.clone(),
            to: flight_id.clone(),
            kind: "captures",
            label: "captures",
        });
        let mut previous = flight_id;
        for event in &flight.events {
            let event_id = format!("flight-event:{}:{}", flight.id, event.ordinal);
            graph.nodes.push(GraphNode {
                id: event_id.clone(),
                label: event.kind.clone(),
                kind: "flight_event",
                detail: event.label.clone(),
                state: "observed",
            });
            graph.edges.push(GraphEdge {
                from: previous,
                to: event_id.clone(),
                kind: "transition",
                label: "then",
            });
            previous = event_id;
        }
    }
    graph
}

pub fn serve(
    store: Store,
    binding: InstanceBinding,
    bind: &str,
    runners_enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(bind)?;
    let store = Arc::new(store);
    let recorder = Arc::new(flight::FlightRecorder::new(
        Arc::clone(&store),
        binding.clone(),
        runners_enabled,
    ));
    eprintln!(
        "connectome: http://{bind} [{}] runners={}",
        binding.manifest.id,
        if runners_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    for request in server.incoming_requests() {
        respond(request, store.as_ref(), &binding, &recorder);
    }
    Ok(())
}

fn respond(
    mut request: Request,
    store: &Store,
    binding: &InstanceBinding,
    recorder: &Arc<flight::FlightRecorder>,
) {
    let url = request.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    if request.method() == &Method::Post && path == "/api/flights" {
        let parsed =
            serde_json::from_reader::<_, LaunchFlight>(request.as_reader().take(128 * 1024));
        let response = match parsed {
            Ok(launch) => match recorder.launch(launch, now()) {
                Ok(flight) => json_response(StatusCode(202), &flight),
                Err(error) => json_response(
                    StatusCode(400),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            },
            Err(error) => json_response(
                StatusCode(400),
                &serde_json::json!({"error":format!("invalid flight request: {error}")}),
            ),
        };
        let _ = request.respond(response);
        return;
    }
    if request.method() == &Method::Post && path == "/api/demos/prompt-strength" {
        let response = match recorder.seed_prompt_demos(now()) {
            Ok(flights) => json_response(StatusCode(201), &flights),
            Err(error) => json_response(
                StatusCode(400),
                &serde_json::json!({"error":error.to_string()}),
            ),
        };
        let _ = request.respond(response);
        return;
    }
    if request.method() != &Method::Get && request.method() != &Method::Head {
        let _ = request.respond(json_response(
            StatusCode(405),
            &serde_json::json!({"error":"connectome workbench is read-only except for explicit prompt flights"}),
        ));
        return;
    }

    let response = match path {
        "/" | "/index.html" => text_response(StatusCode(200), "text/html; charset=utf-8", INDEX),
        "/app.css" => text_response(StatusCode(200), "text/css; charset=utf-8", CSS),
        "/app.js" => text_response(StatusCode(200), "text/javascript; charset=utf-8", JS),
        "/api/snapshot" => match snapshot(store, binding, now()) {
            Ok(mut value) => match recorder.flights() {
                Ok(flights) => {
                    value.flights = flights;
                    value.capabilities = CapabilitiesView {
                        runners_enabled: recorder.runners_enabled(),
                        providers: if recorder.runners_enabled() {
                            vec!["observe", "codex", "claude"]
                        } else {
                            vec!["observe"]
                        },
                    };
                    json_response(StatusCode(200), &value)
                }
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            },
            Err(error) => json_response(
                StatusCode(500),
                &serde_json::json!({"error":error.to_string()}),
            ),
        },
        "/api/flights" => match recorder.flights() {
            Ok(flights) => json_response(StatusCode(200), &flights),
            Err(error) => json_response(
                StatusCode(500),
                &serde_json::json!({"error":error.to_string()}),
            ),
        },
        "/api/changes" => {
            let params = query_params(query);
            let after = params
                .get("after")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let limit = params
                .get("limit")
                .and_then(|value| value.parse().ok())
                .unwrap_or(256usize)
                .clamp(1, 4_096);
            match ScopeId::new("instance:default").and_then(|scope| {
                store
                    .runtime_changes_since(after, limit, Some(&scope))
                    .map_err(|error| vyrm_core::Error::InvalidRuntime {
                        reason: error.to_string(),
                    })
            }) {
                Ok(page) => json_response(StatusCode(200), &page),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        "/api/runtime/schema" => match ScopeId::new(vyrm_node::REASONING_SCOPE)
            .map_err(|error| error.into())
            .and_then(|scope| {
                store
                    .runtime_schema(&scope)
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })
            }) {
            Ok(Some(schema)) => json_response(StatusCode(200), &schema),
            Ok(None) => json_response(
                StatusCode(404),
                &serde_json::json!({"error":"runtime schema is not installed for this scope"}),
            ),
            Err(error) => json_response(
                StatusCode(500),
                &serde_json::json!({"error":error.to_string()}),
            ),
        },
        "/api/runtime/graph" => {
            let params = query_params(query);
            let valid_at = params
                .get("valid_at")
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(now);
            let cursor = params.get("cursor").and_then(|value| value.parse().ok());
            match ScopeId::new("instance:default")
                .map_err(|error| error.into())
                .and_then(|scope| runtime_graph(store, scope, valid_at, cursor))
            {
                Ok(graph) => json_response(StatusCode(200), &graph),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        "/api/runtime/diff" => {
            let params = query_params(query);
            let valid_at = params
                .get("valid_at")
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(now);
            let from = params
                .get("from")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let to = params.get("to").and_then(|value| value.parse().ok());
            let result = ScopeId::new("instance:default")
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })
                .and_then(|scope| {
                    let before = runtime_graph(store, scope.clone(), valid_at, Some(from))?;
                    let after = runtime_graph(store, scope, valid_at, to)?;
                    Ok(before.diff(&after))
                });
            match result {
                Ok(diff) => json_response(StatusCode(200), &diff),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        "/api/route" => {
            let params = query_params(query);
            let term = params.get("query").map(String::as_str).unwrap_or("");
            let limit = params
                .get("limit")
                .and_then(|value| value.parse().ok())
                .unwrap_or(8);
            match vyrm_node::load_routing(store, &binding.project_root) {
                Ok(Some(index)) => json_response(StatusCode(200), &index.route(term, limit)),
                Ok(None) => json_response(
                    StatusCode(409),
                    &serde_json::json!({"error":"routing projection is absent; run preflight first"}),
                ),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        _ => json_response(StatusCode(404), &serde_json::json!({"error":"not found"})),
    };
    let _ = request.respond(response);
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
    text_response(
        status,
        "application/json; charset=utf-8",
        &serde_json::to_string(body).unwrap_or_else(|error| format!("{{\"error\":{error:?}}}")),
    )
}

fn query_params(query: &str) -> BTreeMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (percent_decode(key), percent_decode(value)))
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                    if let Ok(byte) = u8::from_str_radix(hex, 16) {
                        output.push(byte);
                        index += 2;
                    }
                }
            }
            byte => output.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn resolve_paths(
    root: &Path,
    db: Option<&Path>,
) -> Result<(InstanceBinding, PathBuf), Box<dyn std::error::Error>> {
    let binding = InstanceBinding::discover(root)?;
    binding.require_runtime_ready()?;
    let candidate = db
        .map(Path::to_path_buf)
        .unwrap_or_else(|| binding.expected_store());
    let db = binding.verify_store_path(&candidate)?;
    Ok((binding, db))
}
