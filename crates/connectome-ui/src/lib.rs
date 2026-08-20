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
    resolve_as_of, AuditEnvelope, Claim, ClaimSource, ReasoningEvent, ReasoningPayload,
    RetentionPin, RuntimeGraphSnapshot, RuntimeMutation, RuntimeSchemaRegistry, RuntimeValue,
    ScopeId, SnapshotHandle,
};
use vyrm_mx::{BoundQuery, Catalog, ExecutionBudget, Parameters, PhysicalPlan, QueryExecution};
use vyrm_node::{InstanceBinding, InstanceMode};
use vyrm_ql::Query;
use vyrm_store::{Engine, Invocation, PersistentEngine, ProjectionStatus};

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
    pub temporal_events: Vec<TemporalEventView>,
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
    pub storage_backend: &'static str,
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
    pub snapshot_leases: usize,
    pub retention_pins: usize,
    pub oldest_retained_cursor: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeRetentionView {
    pub observed_at: u64,
    pub snapshots: Vec<SnapshotHandle>,
    pub pins: Vec<RetentionPin>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeQueryView {
    pub canonical: String,
    pub query: Query,
    pub bound: BoundQuery,
    pub plan: PhysicalPlan,
    pub execution: QueryExecution,
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

/// A bounded read-only lens over one authoritative runtime change. The full
/// mutation and its hash-chained audit envelope stay attached so the UI never
/// substitutes decorative telemetry for persisted evidence.
#[derive(Debug, Clone, Serialize)]
pub struct TemporalEventView {
    pub cursor: u64,
    pub commit_id: String,
    pub commit_ordinal: u64,
    pub scope: String,
    pub at: u64,
    pub actor: String,
    pub family: &'static str,
    pub action: String,
    pub label: String,
    pub detail: String,
    pub digest: String,
    pub mutation: RuntimeMutation,
    pub audit: Option<AuditEnvelope>,
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
    store: &PersistentEngine,
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
    let temporal_events = temporal_events(store, 512)?;
    let graph = build_graph(
        &binding.manifest.id,
        &claims,
        &runs,
        &files,
        &invocations,
        &flights,
    );
    let schema = store.runtime_schema(&ScopeId::new(vyrm_node::REASONING_SCOPE)?)?;
    let retention = runtime_retention(store, at)?;
    let health = HealthView {
        state: if quarantined {
            "blocked"
        } else if routing.is_none() {
            "attention"
        } else {
            "ready"
        },
        storage_backend: store.backend().as_str(),
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
        snapshot_leases: retention.snapshots.len(),
        retention_pins: retention.pins.len(),
        oldest_retained_cursor: retention.pins.iter().map(|pin| pin.minimum_cursor).min(),
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
        temporal_events,
        schema,
        capabilities: CapabilitiesView {
            runners_enabled: false,
            providers: vec!["observe"],
        },
        graph,
    })
}

/// Returns the newest persisted runtime mutations in global cursor order.
/// Scope is deliberately not filtered: reasoning/prompt flights still use the
/// portable legacy scope while project-owned workflows use the instance ID.
pub fn temporal_events(
    store: &PersistentEngine,
    limit: usize,
) -> Result<Vec<TemporalEventView>, Box<dyn std::error::Error>> {
    let limit = limit.clamp(1, 4_096);
    let head = store.runtime_cursor()?;
    let after = head.saturating_sub(limit as u64);
    let page = store.runtime_changes_since(after, limit, None)?;
    let mut audits = BTreeMap::<String, Option<AuditEnvelope>>::new();
    let mut events = Vec::with_capacity(page.changes.len());
    for change in page.changes {
        let audit = match audits.get(&change.commit_id) {
            Some(audit) => audit.clone(),
            None => {
                let audit = store.runtime_audit(&change.commit_id)?;
                audits.insert(change.commit_id.clone(), audit.clone());
                audit
            }
        };
        let (family, action, label, detail) = describe_mutation(&change.mutation);
        events.push(TemporalEventView {
            cursor: change.cursor,
            commit_id: change.commit_id,
            commit_ordinal: change.commit_ordinal,
            scope: change.scope.as_str().to_owned(),
            at: change.at,
            actor: change.actor,
            family,
            action,
            label,
            detail,
            digest: change.digest,
            mutation: change.mutation,
            audit,
        });
    }
    Ok(events)
}

fn describe_mutation(mutation: &RuntimeMutation) -> (&'static str, String, String, String) {
    match mutation {
        RuntimeMutation::Claim { claim } if claim.subject.as_str().starts_with("package:") => {
            let observation = serde_json::from_str::<vyrm_node::WorkflowObservation>(&claim.object);
            let detail = observation.map_or_else(
                |_| format!("{} = {}", claim.predicate, claim.object),
                |observation| {
                    format!(
                        "status={:?}; exit={}; manifest={}",
                        observation.status,
                        observation
                            .exit_code
                            .map_or_else(|| "unreported".into(), |code| code.to_string()),
                        observation.manifest_digest
                    )
                },
            );
            (
                "workflow",
                "claim".into(),
                claim.subject.as_str().to_owned(),
                detail,
            )
        }
        RuntimeMutation::Claim { claim } => (
            "memory",
            "claim".into(),
            format!("{} · {}", claim.subject, claim.predicate),
            claim.object.clone(),
        ),
        RuntimeMutation::Schema { registry } => (
            "storage",
            "schema".into(),
            format!("schema revision {}", registry.revision),
            registry.migration.clone(),
        ),
        RuntimeMutation::Record { record } => {
            let kind = record.reference.kind.as_str();
            let family = if kind.starts_with("reasoning_") {
                "reasoning"
            } else if kind.starts_with("prompt_flight") {
                "model"
            } else {
                "storage"
            };
            (
                family,
                "record".into(),
                format!("{}:{}", kind, record.reference.id),
                format!("{} typed properties", record.properties.len()),
            )
        }
        RuntimeMutation::Relation { relation } => {
            let family = if relation.reference.kind.as_str().starts_with("reasoning_") {
                "reasoning"
            } else {
                "storage"
            };
            (
                family,
                "relation".into(),
                relation.reference.kind.as_str().to_owned(),
                format!("{} → {}", relation.from.id, relation.to.id),
            )
        }
        RuntimeMutation::Event { event } if event.kind.as_str() == "runtime_trace" => {
            let domain =
                runtime_string(&event.properties, "domain").unwrap_or_else(|| "unknown".into());
            let family = match domain.as_str() {
                "reasoning" => "reasoning",
                "lifecycle" | "tool" => "workflow",
                "query" | "planning" => "routing",
                "projection" | "search" | "embedding" => "search",
                "model" => "model",
                "storage" | "adapter" | "cluster" => "storage",
                _ => "storage",
            };
            let phase =
                runtime_string(&event.properties, "phase").unwrap_or_else(|| "unknown".into());
            let outcome =
                runtime_string(&event.properties, "outcome").unwrap_or_else(|| "unknown".into());
            let duration = runtime_unsigned(&event.properties, "duration_micros")
                .map_or_else(|| "open".into(), |value| format!("{value} µs"));
            (
                family,
                format!("trace_{phase}"),
                runtime_string(&event.properties, "name").unwrap_or_else(|| "runtime trace".into()),
                format!("{domain}; {outcome}; {duration}"),
            )
        }
        RuntimeMutation::Event { event } => {
            let stage = runtime_string(&event.properties, "stage");
            let family = match stage.as_deref() {
                Some("context" | "recall" | "routing") => "routing",
                Some("tools") => "workflow",
                Some("outcome") => "reasoning",
                _ if event.kind.as_str().starts_with("reasoning_") => "reasoning",
                _ if event.kind.as_str().starts_with("prompt_flight") => "model",
                _ => "storage",
            };
            let label = runtime_string(&event.properties, "kind")
                .unwrap_or_else(|| event.kind.as_str().to_owned());
            (
                family,
                "event".into(),
                label,
                stage.map_or_else(
                    || format!("{} typed properties", event.properties.len()),
                    |stage| format!("{stage} stage; {} typed properties", event.properties.len()),
                ),
            )
        }
        RuntimeMutation::Vector { vector } => (
            "search",
            "vector".into(),
            format!("{}:{}", vector.reference.kind, vector.reference.id),
            format!(
                "{} dimensions; model={}",
                vector.value.dimensions(),
                vector
                    .provenance
                    .as_ref()
                    .map_or("supplied", |provenance| provenance.model.as_str())
            ),
        ),
        RuntimeMutation::SeriesSample { sample } => (
            "data",
            "series_sample".into(),
            format!("{}:{}", sample.reference.kind, sample.reference.id),
            format!("observed_at={}", sample.observed_at),
        ),
        RuntimeMutation::Geo { geo } => (
            "data",
            "geo".into(),
            format!("{}:{}", geo.reference.kind, geo.reference.id),
            "geospatial value".into(),
        ),
        RuntimeMutation::Object { object } => (
            "storage",
            "object".into(),
            format!("{}:{}", object.reference.kind, object.reference.id),
            format!("{} bytes · {}", object.length, object.sha256),
        ),
    }
}

fn runtime_string(properties: &vyrm_core::RuntimeProperties, name: &str) -> Option<String> {
    match properties.get(name) {
        Some(RuntimeValue::String(value) | RuntimeValue::Digest(value)) => Some(value.clone()),
        _ => None,
    }
}

fn runtime_unsigned(properties: &vyrm_core::RuntimeProperties, name: &str) -> Option<u64> {
    match properties.get(name) {
        Some(RuntimeValue::Unsigned(value)) => Some(*value),
        _ => None,
    }
}

pub fn runtime_retention(
    store: &PersistentEngine,
    at: u64,
) -> Result<RuntimeRetentionView, Box<dyn std::error::Error>> {
    let snapshots = store.runtime_snapshots(at)?;
    let pins = snapshots
        .iter()
        .map(RetentionPin::from_snapshot)
        .collect::<vyrm_core::Result<Vec<_>>>()?;
    Ok(RuntimeRetentionView {
        observed_at: at,
        snapshots,
        pins,
    })
}

/// Parses, binds, plans, and executes one read-only query against a single
/// captured runtime stamp. The returned plan is the inspectable proof of the
/// selected path, not server-side commentary added after execution.
pub fn runtime_query(
    store: &PersistentEngine,
    scope: ScopeId,
    source: &str,
    budget: &ExecutionBudget,
) -> Result<RuntimeQueryView, Box<dyn std::error::Error>> {
    let query = vyrm_ql::parse(source)?;
    let canonical = query.canonical();
    let catalog = Catalog::capture(store, &scope)?;
    let bound = vyrm_mx::bind(&query, &Parameters::new(), &catalog)?;
    let plan = vyrm_mx::plan(&bound)?;
    let execution = vyrm_mx::execute(store, &plan, budget)?;
    Ok(RuntimeQueryView {
        canonical,
        query,
        bound,
        plan,
        execution,
    })
}

/// Reconstructs the authoritative typed graph at a valid-time instant and a
/// transaction cursor. The bounded feed is the authority; this helper is a
/// read-only lens used by the workbench freeze/scrub API.
pub fn runtime_graph(
    store: &PersistentEngine,
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
    store: PersistentEngine,
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
    store: &PersistentEngine,
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
            let result = requested_scope(&params, None).and_then(|scope| {
                store
                    .runtime_changes_since(after, limit, scope.as_ref())
                    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })
            });
            match result {
                Ok(page) => json_response(StatusCode(200), &page),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        "/api/runtime/events" => {
            let params = query_params(query);
            let limit = params
                .get("limit")
                .and_then(|value| value.parse().ok())
                .unwrap_or(512usize)
                .clamp(1, 4_096);
            match temporal_events(store, limit) {
                Ok(events) => json_response(StatusCode(200), &events),
                Err(error) => json_response(
                    StatusCode(500),
                    &serde_json::json!({"error":error.to_string()}),
                ),
            }
        }
        "/api/runtime/schema" => {
            match requested_scope(&query_params(query), Some(vyrm_node::REASONING_SCOPE))
                .and_then(required_scope)
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
            }
        }
        "/api/runtime/retention" => match runtime_retention(store, now()) {
            Ok(retention) => json_response(StatusCode(200), &retention),
            Err(error) => json_response(
                StatusCode(500),
                &serde_json::json!({"error":error.to_string()}),
            ),
        },
        "/api/runtime/query" => {
            let params = query_params(query);
            let source = params.get("ql").map(String::as_str).unwrap_or("");
            let budget = ExecutionBudget {
                max_scanned_changes: params
                    .get("max_scanned_changes")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(100_000)
                    .clamp(1, 1_000_000),
                max_rows: params
                    .get("max_rows")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1_000)
                    .clamp(1, 10_000),
                max_output_bytes: 2 * 1024 * 1024,
                max_batch_rows: 256,
            };
            if source.trim().is_empty() {
                json_response(
                    StatusCode(400),
                    &serde_json::json!({"error":"ql query parameter is required"}),
                )
            } else {
                match requested_scope(&params, Some(vyrm_node::REASONING_SCOPE))
                    .and_then(required_scope)
                    .and_then(|scope| runtime_query(store, scope, source, &budget))
                {
                    Ok(result) => json_response(StatusCode(200), &result),
                    Err(error) => json_response(
                        StatusCode(400),
                        &serde_json::json!({"error":error.to_string()}),
                    ),
                }
            }
        }
        "/api/runtime/graph" => {
            let params = query_params(query);
            let valid_at = params
                .get("valid_at")
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(now);
            let cursor = params.get("cursor").and_then(|value| value.parse().ok());
            match requested_scope(&params, Some(vyrm_node::REASONING_SCOPE))
                .and_then(required_scope)
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
            let result = requested_scope(&params, Some(vyrm_node::REASONING_SCOPE))
                .and_then(required_scope)
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

fn requested_scope(
    params: &BTreeMap<String, String>,
    default: Option<&str>,
) -> Result<Option<ScopeId>, Box<dyn std::error::Error>> {
    match params.get("scope").map(String::as_str).or(default) {
        Some("all") => Ok(None),
        Some(scope) if !scope.trim().is_empty() => Ok(Some(ScopeId::new(scope.to_owned())?)),
        Some(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "runtime scope must not be empty",
        )
        .into()),
        None => Ok(None),
    }
}

fn required_scope(scope: Option<ScopeId>) -> Result<ScopeId, Box<dyn std::error::Error>> {
    scope.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "this runtime endpoint requires one concrete scope",
        )
        .into()
    })
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
