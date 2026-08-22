//! Stdio MCP face over the same runtime kernel used by hooks and the CLI.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use vyrm_core::{digest, Millis, Reader, RecallQuery, ReasoningPayload, ScopeId, Subject};
use vyrm_store::{Effectiveness, Engine, InvocationInput, Outcome, PersistentEngine, Trigger};

struct Config {
    db: PathBuf,
    root: PathBuf,
}

struct ToolResult {
    text: String,
    effectiveness: Option<Effectiveness>,
    detail: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("vyrmd: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = parse_args()?;
    let binding = vyrm_node::InstanceBinding::discover(&config.root)?;
    binding.require_runtime_ready()?;
    config.db = binding.verify_store_path(&config.db)?;
    config.root = binding.project_root;
    let store = PersistentEngine::open(&config.db)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                write_message(&mut stdout, &rpc_error(Value::Null, -32700, &error.to_string()))?;
                continue;
            }
        };
        let Some(id) = request.get("id").cloned() else {
            continue; // notifications are acknowledged by silence
        };
        let mut response = dispatch(&store, &config.root, id, &request);
        if request.get("method").and_then(Value::as_str) == Some("server/discover")
            || request
                .pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
                .and_then(Value::as_str)
                == Some("2026-07-28")
        {
            stamp_server_info(&mut response);
        }
        write_message(&mut stdout, &response)?;
    }
    Ok(())
}

fn dispatch(store: &PersistentEngine, root: &Path, id: Value, request: &Value) -> Value {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "server/discover" => json!({
            "jsonrpc":"2.0",
            "id":id,
            "result":{
                "resultType":"complete",
                "supportedVersions":["2026-07-28","2025-11-25","2025-06-18"],
                "capabilities":{"tools":{}},
                "instructions":"Call vyrm_preflight before reasoning. Project mutation must cross vyrm_lifecycle(pre-tool-use), then report its result with vyrm_lifecycle(post-tool-use)."
            }
        }),
        "initialize" => {
            let requested = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-11-25");
            let negotiated = if matches!(requested, "2025-11-25" | "2025-06-18") {
                requested
            } else {
                "2025-11-25"
            };
            json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": negotiated,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "vyrmd", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Call vyrm_preflight before reasoning. Project mutation must cross vyrm_lifecycle(pre-tool-use), then report the result through vyrm_lifecycle(post-tool-use)."
            }
        })
        }
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}}),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("unknown");
            if !known_tool(name) {
                return rpc_error(id, -32602, &format!("unknown tool {name:?}"));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let started = Instant::now();
            let result = call_tool(store, root, name, &args);
            let duration_ms = started.elapsed().as_millis() as u64;
            let (outcome, detail, effectiveness) = match &result {
                Ok(result) => (Outcome::Ok, result.detail.clone(), result.effectiveness.clone()),
                Err(error) => (Outcome::Error, Some(error.to_string()), None),
            };
            let arguments = invocation_arguments(name, &args);
            let _ = store.record_invocation(InvocationInput {
                at: now(),
                trigger: Trigger::Manual,
                command: &format!("mcp:{name}"),
                arguments: &arguments,
                outcome,
                duration_ms,
                detail,
                effectiveness,
            });
            match result {
                Ok(result) => json!({
                    "jsonrpc":"2.0","id":id,
                    "result":{"content":[{"type":"text","text":result.text}],"isError":false}
                }),
                Err(error) => json!({
                    "jsonrpc":"2.0","id":id,
                    "result":{"content":[{"type":"text","text":error.to_string()}],"isError":true}
                }),
            }
        }
        _ => rpc_error(id, -32601, &format!("method {method:?} not found")),
    }
}

fn call_tool(
    store: &PersistentEngine,
    root: &Path,
    name: &str,
    args: &Value,
) -> Result<ToolResult, Box<dyn std::error::Error>> {
    match name {
        "vyrm_preflight" => {
            let reader = reader(args)?;
            let at = arg_u64(args, "at").unwrap_or_else(now);
            let budget = arg_u64(args, "budget").unwrap_or(1_500) as usize;
            let harness = args.get("harness").and_then(Value::as_str);
            let flight = vyrm_node::preflight(store, root, harness, &reader, at, budget)?;
            Ok(ToolResult {
                text: flight.context,
                effectiveness: Some(flight.effectiveness),
                detail: (!flight.warnings.is_empty()).then(|| format!("{} warning(s)", flight.warnings.len())),
            })
        }
        "vyrm_recall" => {
            let reader = reader(args)?;
            let subjects = args
                .get("subjects")
                .and_then(Value::as_array)
                .ok_or("subjects must be an array")?
                .iter()
                .map(|value| Subject::new(value.as_str().unwrap_or_default()))
                .collect::<vyrm_core::Result<Vec<_>>>()?;
            let query = RecallQuery {
                subjects,
                predicates: None,
                as_of: arg_u64(args, "at").unwrap_or_else(now),
            };
            let set = vyrm_core::recall(store, &query, arg_u64(args, "budget").unwrap_or(1_500) as usize)?;
            for claim in &set.claims { store.observe(&reader, &claim.subject, &claim.predicate, query.as_of)?; }
            Ok(ToolResult {
                text: serde_json::to_string_pretty(&set)?,
                effectiveness: Some(Effectiveness {
                    query: query.subjects.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
                    claims_returned: set.claims.len(),
                    tokens_emitted: set.token_estimate as u64,
                    baseline_tokens: None,
                    baseline_mode: None,
                    provider: "mcp".into(),
                    outcome: Default::default(),
                }),
                detail: None,
            })
        }
        "vyrm_route" => {
            let ready = vyrm_node::ensure_routing_fresh(store, root)?;
            let index = vyrm_node::load_routing(store, root)?.ok_or("routing projection absent after refresh")?;
            let query = arg_str(args, "query")?;
            let routed = index.route(query, arg_u64(args, "limit").unwrap_or(5) as usize);
            Ok(ToolResult { text: serde_json::to_string_pretty(&json!({"freshness": ready.render(), "files": routed}))?, effectiveness: None, detail: Some(ready.render()) })
        }
        "vyrm_query" => {
            let reader = reader(args)?;
            let scope = ScopeId::new(
                args.get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or(vyrm_node::REASONING_SCOPE),
            )?;
            let empty_parameters = json!({});
            let parameters = vyrm_node::query_parameters_from_json(
                args.get("parameters").unwrap_or(&empty_parameters),
            )?;
            let budget = vyrm_node::ExecutionBudget {
                max_scanned_changes: arg_u64(args, "max_scanned_changes")
                    .unwrap_or(100_000)
                    .clamp(1, 1_000_000) as usize,
                max_rows: arg_u64(args, "max_rows")
                    .unwrap_or(10_000)
                    .clamp(1, 100_000) as usize,
                max_output_bytes: arg_u64(args, "max_output_bytes")
                    .unwrap_or(8 * 1024 * 1024)
                    .clamp(1, 64 * 1024 * 1024) as usize,
                max_batch_rows: arg_u64(args, "max_batch_rows")
                    .unwrap_or(256)
                    .clamp(1, 4_096) as usize,
            };
            let result = vyrm_node::execute_traced_query(
                store,
                scope,
                arg_str(args, "ql")?,
                &parameters,
                &budget,
                reader.as_str(),
                arg_u64(args, "at").unwrap_or_else(now),
            )?;
            Ok(ToolResult {
                text: serde_json::to_string_pretty(&result)?,
                effectiveness: None,
                detail: Some(format!("plan={}", result.plan.digest)),
            })
        }
        "vyrm_reasoning_record" => {
            let run = arg_str(args, "run_id")?;
            let actor = args.get("actor").and_then(Value::as_str).unwrap_or("agent:mcp");
            let payload: ReasoningPayload = serde_json::from_value(args.get("payload").cloned().ok_or("payload is required")?)?;
            let event = vyrm_node::record_reasoning(store, run, arg_u64(args, "at").unwrap_or_else(now), actor, payload)?;
            Ok(ToolResult { text: serde_json::to_string_pretty(&event)?, effectiveness: None, detail: Some(format!("recorded {} #{}", event.payload.name(), event.ordinal)) })
        }
        "vyrm_reasoning_show" => {
            let run = match args.get("run_id").and_then(Value::as_str) {
                Some(id) => vyrm_node::reasoning_run(store, id)?,
                None => vyrm_node::active_reasoning_run(store)?,
            };
            let value = run.map(|run| json!({"run_id":run.id(),"state":run.state(),"events":run.events()}));
            Ok(ToolResult { text: serde_json::to_string_pretty(&value)?, effectiveness: None, detail: None })
        }
        "vyrm_lifecycle" => {
            let event_name = arg_str(args, "event")?;
            let event = vyrm_node::HookEvent::parse(event_name).ok_or("unknown lifecycle event")?;
            let input = args.get("input").cloned().unwrap_or_else(|| json!({}));
            let reader = reader(args)?;
            let ctx = vyrm_node::HookContext {
                store,
                root,
                harness: Some("mcp"),
                reader: &reader,
                now: arg_u64(args, "at").unwrap_or_else(now),
                budget: arg_u64(args, "budget").unwrap_or(1_500) as usize,
            };
            let response = vyrm_node::handle(&ctx, event, &input)?;
            Ok(ToolResult { text: response.stdout, effectiveness: response.effectiveness, detail: response.detail })
        }
        _ => Err(format!("unknown tool {name:?}").into()),
    }
}

fn tools() -> Value {
    json!([
        tool("vyrm_preflight", "Attune, refresh routing, and inject current memory before reasoning", json!({"type":"object","properties":{"at":{"type":"integer"},"budget":{"type":"integer"},"harness":{"type":"string"}}})),
        tool("vyrm_recall", "Recall current claims for exact subjects", json!({"type":"object","required":["subjects"],"properties":{"subjects":{"type":"array","items":{"type":"string"}},"at":{"type":"integer"},"budget":{"type":"integer"}}})),
        tool("vyrm_route", "Refresh and route a symbol/query to complete files", json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"limit":{"type":"integer"}}})),
        tool("vyrm_query", "Execute a project-scoped vyrmQL read with durable parse, plan, and execution evidence", json!({"type":"object","required":["ql"],"properties":{"ql":{"type":"string"},"scope":{"type":"string"},"parameters":{"type":"object"},"at":{"type":"integer"},"max_scanned_changes":{"type":"integer"},"max_rows":{"type":"integer"},"max_output_bytes":{"type":"integer"},"max_batch_rows":{"type":"integer"}}})),
        tool("vyrm_reasoning_record", "Append one typed goal/plan/attempt/observation/decision/verification/outcome transition", json!({"type":"object","required":["run_id","payload"],"properties":{"run_id":{"type":"string"},"actor":{"type":"string"},"at":{"type":"integer"},"payload":{"type":"object"}}})),
        tool("vyrm_reasoning_show", "Show a run or the active reasoning run", json!({"type":"object","properties":{"run_id":{"type":"string"}}})),
        tool("vyrm_lifecycle", "Apply the same session/pre-tool/post-tool lifecycle semantics as hook runtimes", json!({"type":"object","required":["event","input"],"properties":{"event":{"type":"string","enum":["session-start","user-prompt-submit","pre-tool-use","post-tool-use","stop","pre-compact"]},"input":{"type":"object"},"at":{"type":"integer"},"budget":{"type":"integer"}}}))
    ])
}

fn known_tool(name: &str) -> bool {
    matches!(
        name,
        "vyrm_preflight"
            | "vyrm_recall"
            | "vyrm_route"
            | "vyrm_query"
            | "vyrm_reasoning_record"
            | "vyrm_reasoning_show"
            | "vyrm_lifecycle"
    )
}

fn invocation_arguments(name: &str, args: &Value) -> Vec<String> {
    if name != "vyrm_query" {
        return vec![args.to_string()];
    }
    let query = args.get("ql").and_then(Value::as_str).unwrap_or_default();
    let parameters = args.get("parameters").cloned().unwrap_or_else(|| json!({}));
    let parameter_bytes = serde_json::to_vec(&parameters).unwrap_or_default();
    vec![
        format!(
            "scope={}",
            args.get("scope")
                .and_then(Value::as_str)
                .unwrap_or(vyrm_node::REASONING_SCOPE)
        ),
        format!("query_digest={}", digest::sha256_hex(query.as_bytes())),
        format!(
            "parameter_digest={}",
            digest::sha256_hex(&parameter_bytes)
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":schema})
}

fn reader(args: &Value) -> Result<Reader, Box<dyn std::error::Error>> {
    Ok(Reader::new(args.get("reader").and_then(Value::as_str).unwrap_or("agent:mcp"))?)
}

fn arg_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    args.get(name).and_then(Value::as_str).ok_or_else(|| format!("{name} is required").into())
}

fn arg_u64(args: &Value, name: &str) -> Option<u64> {
    args.get(name).and_then(Value::as_u64)
}

fn now() -> Millis {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn parse_args() -> Result<Config, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut db = None;
    let mut root = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next().map(PathBuf::from),
            "--root" => root = args.next().map(PathBuf::from).ok_or("--root needs a path")?,
            _ => return Err(format!("unknown argument {arg:?}").into()),
        }
    }
    Ok(Config { db: db.ok_or("--db is required")?, root })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn stamp_server_info(response: &mut Value) {
    let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) else {
        return;
    };
    let meta = result.entry("_meta").or_insert_with(|| json!({}));
    if let Some(meta) = meta.as_object_mut() {
        meta.insert(
            "io.modelcontextprotocol/serverInfo".into(),
            json!({"name":"vyrmd","version":env!("CARGO_PKG_VERSION")}),
        );
    }
}

fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *out, message).map_err(std::io::Error::other)?;
    out.write_all(b"\n")?;
    out.flush()
}
