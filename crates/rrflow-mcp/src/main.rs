//! Stdio MCP face over one explicitly selected embedded or daemon RRD authority.

mod authority;
mod config;

use authority::RuntimeAuthority;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    if let Err(error) = run() {
        eprintln!("rrflow-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::parse_args(std::env::args().skip(1))?;
    let mut authority = RuntimeAuthority::open(config)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                write_message(
                    &mut stdout,
                    &rpc_error(Value::Null, -32700, &error.to_string()),
                )?;
                continue;
            }
        };
        let Some(id) = request.get("id").cloned() else {
            continue; // notifications are acknowledged by silence
        };
        let mut response = dispatch(&mut authority, id, &request);
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
    authority.close()?;
    Ok(())
}

fn dispatch(authority: &mut RuntimeAuthority, id: Value, request: &Value) -> Value {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "server/discover" => json!({
            "jsonrpc":"2.0",
            "id":id,
            "result":{
                "resultType":"complete",
                "supportedVersions":["2026-07-28","2025-11-25","2025-06-18"],
                "capabilities":{"tools":{}},
                "instructions":"Call rrflow_preflight before reasoning. Project mutation must cross rrflow_lifecycle(pre-tool-use), then report its result with rrflow_lifecycle(post-tool-use)."
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
                    "serverInfo": {"name": "rrflow-mcp", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Call rrflow_preflight before reasoning. Project mutation must cross rrflow_lifecycle(pre-tool-use), then report the result through rrflow_lifecycle(post-tool-use)."
                }
            })
        }
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => {
            json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools(authority)}})
        }
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if !authority
                .catalogue()
                .tools
                .iter()
                .any(|descriptor| descriptor.name.as_str() == name)
            {
                return rpc_error(id, -32602, &format!("unknown tool {name:?}"));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = authority.call(name, &args);
            match result {
                Ok(content) => json!({
                    "jsonrpc":"2.0","id":id,
                    "result":{"content":[{"type":"text","text":content}],"isError":false}
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

fn tools(authority: &RuntimeAuthority) -> Value {
    Value::Array(
        authority
            .catalogue()
            .tools
            .iter()
            .map(|descriptor| {
                json!({
                    "name": descriptor.name,
                    "description": descriptor.description,
                    "inputSchema": descriptor.input_schema,
                    "annotations": {
                        "readOnlyHint": !descriptor.mutation,
                        "destructiveHint": descriptor.name.as_str() == "rrflow_forget"
                    }
                })
            })
            .collect(),
    )
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
            json!({"name":"rrflow-mcp","version":env!("CARGO_PKG_VERSION")}),
        );
    }
}

fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *out, message).map_err(std::io::Error::other)?;
    out.write_all(b"\n")?;
    out.flush()
}
