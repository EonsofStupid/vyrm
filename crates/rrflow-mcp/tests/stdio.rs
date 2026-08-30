use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn stdio_server_negotiates_lists_tools_and_uses_the_shared_contract_gate() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn item() {}\n").unwrap();
    let db = root.path().join(".rrflow/rrd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rrflow-mcp"))
        .arg("embedded")
        .arg("--db")
        .arg(&db)
        .arg("--root")
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"rrflow_preflight","arguments":{"at":1000}}}),
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"rrflow_hook","arguments":{"event":"pre-tool-use","at":1001,"input":{"tool_name":"Edit","tool_input":{"file_path":"lib.rs"}}}}}),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"rrflow_query","arguments":{"at":1002,"ql":"FROM event:runtime_trace AT VALID 18446744073709551615 KNOWN HEAD PROJECT name, phase EXPLAIN CONTRACT"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"rrflow_remember","arguments":{"subject":"project:alpha","predicate":"status","object":"foundation-active","actor":"agent:test","at":2000}}}),
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"rrflow_inspect","arguments":{"subject":"project:alpha","predicate":"status"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"rrflow_context","arguments":{"subjects":["project:alpha"],"at":2000,"budget":1000}}}),
        serde_json::json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"rrflow_forget","arguments":{"subject":"project:alpha","predicate":"status","at":2001}}}),
        serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"rrflow_recall","arguments":{"subjects":["project:alpha"],"at":2001,"budget":1000}}}),
        serde_json::json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"rrflow_service_status","arguments":{"at":2002}}}),
    ];
    let stdin = child.stdin.as_mut().unwrap();
    for request in requests {
        serde_json::to_writer(&mut *stdin, &request).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        responses.len(),
        11,
        "notification must not receive a response"
    );
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    let embedded_profile = &responses[0]["result"]["_meta"]["io.rrflow/runtimeProfile"];
    assert_eq!(embedded_profile["mode"], "embedded");
    assert_eq!(
        embedded_profile["execution_authority"],
        "embedded_rrd_engine"
    );
    assert_eq!(embedded_profile["storage_access"], "exclusive_bound_engine");
    assert_eq!(embedded_profile["caller_authentication"], "none");
    assert_eq!(embedded_profile["enforcement_level"], "cooperative");
    assert_eq!(embedded_profile["planning_enforced"], false);
    assert_eq!(embedded_profile["mutation_enforced"], false);
    assert_eq!(embedded_profile["host_tool_interception"], false);
    let listed = responses[1]["result"]["tools"].as_array().unwrap();
    let executable = rrd_engine::runtime_tool_catalogue();
    assert_eq!(listed.len(), executable.len());
    assert_eq!(
        listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        executable
            .iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>()
    );
    let task_catalogue = &responses[1]["result"]["_meta"]["io.rrflow/taskCatalogue"];
    assert_eq!(
        responses[1]["result"]["_meta"]["io.rrflow/runtimeProfile"],
        *embedded_profile
    );
    assert_eq!(
        task_catalogue["dispositions"].as_array().unwrap().len(),
        rrd_engine::McpTaskDomain::ALL.len()
    );
    assert!(listed
        .iter()
        .all(|tool| tool["_meta"]["io.rrflow/taskDomains"]
            .as_array()
            .is_some_and(|domains| !domains.is_empty())));
    for operation in rrd_engine::WorkPlanOperation::ALL {
        assert!(
            listed
                .iter()
                .any(|tool| tool["name"] == operation.runtime_tool_name()),
            "MCP omitted generated work-plan operation {:?}",
            operation
        );
    }
    let hook = listed
        .iter()
        .find(|tool| tool["name"] == "rrflow_hook")
        .expect("provider hook adapter must be advertised separately");
    assert_eq!(
        hook["inputSchema"]["required"],
        serde_json::json!(["event", "input"])
    );
    let lifecycle = listed
        .iter()
        .find(|tool| tool["name"] == "rrflow_lifecycle")
        .expect("canonical lifecycle tool must be advertised");
    assert_eq!(
        lifecycle["inputSchema"]["required"],
        serde_json::json!(["command"])
    );
    assert!(responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("[rrflow] routing:"));
    let gate = responses[3]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(gate.contains("permissionDecision"));
    assert!(gate.contains("no active run"));
    let query: serde_json::Value = serde_json::from_str(
        responses[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(responses[4]["result"]["isError"], false);
    assert_eq!(query["execution"]["returned_rows"], 2);
    assert_eq!(query["execution"]["known_at_cursor"], 3);

    let remembered: serde_json::Value = serde_json::from_str(
        responses[5]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(remembered["claim"]["object"], "foundation-active");
    let inspected: serde_json::Value = serde_json::from_str(
        responses[6]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(inspected["claims"][0]["producer"]["actor"], "agent:test");
    let context: serde_json::Value = serde_json::from_str(
        responses[7]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        context["memory"]["claims"][0]["object"],
        "foundation-active"
    );
    assert_eq!(responses[8]["result"]["isError"], false);
    let recalled: serde_json::Value = serde_json::from_str(
        responses[9]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(recalled["claims"].as_array().unwrap().len(), 0);
    let service: serde_json::Value = serde_json::from_str(
        responses[10]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        service["endpoint_count"],
        rrd_contract::endpoint_catalogue().endpoints.len()
    );
    assert_eq!(
        service["executable_mcp_tool_count"],
        rrd_engine::runtime_tool_catalogue().len()
    );
    assert_eq!(service["security_enforced"], false);
    assert_eq!(service["mcp_task_catalogue"], *task_catalogue);

    let binding = rrd_engine::InstanceBinding::discover(root.path()).unwrap();
    let instance = rrd_contract::CanonicalId::new(binding.manifest.id).unwrap();
    let storage = rrd_store::PersistentEngine::open(&db).unwrap();
    let audit = rrd_security::SecurityRepository::new(&storage, instance)
        .audit_since(0, 1_024)
        .unwrap();
    for action in [
        rrd_contract::SecurityAction::ProjectAttune,
        rrd_contract::SecurityAction::LifecycleApply,
        rrd_contract::SecurityAction::QueryExecute,
        rrd_contract::SecurityAction::MemoryWrite,
        rrd_contract::SecurityAction::MemoryInspect,
        rrd_contract::SecurityAction::MemoryContextRead,
        rrd_contract::SecurityAction::MemoryRetire,
        rrd_contract::SecurityAction::MemoryRecall,
        rrd_contract::SecurityAction::ServiceInspect,
    ] {
        assert!(
            audit.records.iter().any(|(_, record)| {
                record.action == action && record.phase == rrd_contract::AuditPhase::Completed
            }),
            "embedded MCP call omitted structured audit for {action:?}"
        );
    }
}

#[test]
fn stdio_server_supports_the_stateless_2026_discovery_era() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    let db = root.path().join(".rrflow/rrd");
    let input = [
        serde_json::json!({"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"test","version":"1"}}}}),
        serde_json::json!({"jsonrpc":"2.0","id":"l","method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"test","version":"1"}}}}),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_rrflow-mcp"))
        .arg("embedded")
        .arg("--db")
        .arg(&db)
        .arg("--root")
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    for message in input {
        serde_json::to_writer(child.stdin.as_mut().unwrap(), &message).unwrap();
        child.stdin.as_mut().unwrap().write_all(b"\n").unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses[0]["result"]["supportedVersions"][0], "2026-07-28");
    assert_eq!(
        responses[1]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "rrflow-mcp"
    );
}

#[test]
fn stdio_server_refuses_a_foreign_instance_store_before_protocol_start() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(first.path()).unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(second.path()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rrflow-mcp"))
        .arg("embedded")
        .arg("--db")
        .arg(first.path().join(".rrflow/rrd"))
        .arg("--root")
        .arg(second.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not belong"));
}
