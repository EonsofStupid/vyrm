use serde_json::{json, Value};
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

fn exchange(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>, request: Value) -> Value {
    serde_json::to_writer(&mut *stdin, &request).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let mut response = String::new();
    stdout.read_line(&mut response).unwrap();
    assert!(!response.is_empty(), "MCP process closed before responding");
    serde_json::from_str(&response).unwrap()
}

fn tool_call(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    next_id: &mut u64,
    name: &str,
    arguments: Value,
) -> Value {
    let id = *next_id;
    *next_id += 1;
    let response = exchange(
        stdin,
        stdout,
        json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(response["result"]["isError"], false, "{name}: {text}");
    serde_json::from_str(text).unwrap_or_else(|_| json!({"text":text}))
}

fn record(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    next_id: &mut u64,
    payload: Value,
) {
    tool_call(
        stdin,
        stdout,
        next_id,
        "rrflow_reasoning_record",
        json!({"run_id":"stdio-foundation-run","actor":"agent:stdio","payload":payload}),
    );
}

fn authorize(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    next_id: &mut u64,
    tool_name: &str,
    arguments: &Value,
) {
    let result = tool_call(
        stdin,
        stdout,
        next_id,
        "rrflow_hook",
        json!({
            "event":"pre-tool-use",
            "input":{"tool_name":tool_name,"tool_input":arguments}
        }),
    );
    assert_eq!(result, json!({"text":""}));
}

fn prepare_next_mutation(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    next_id: &mut u64,
    tool_name: &str,
    arguments: &Value,
) {
    record(
        stdin,
        stdout,
        next_id,
        json!({"kind":"decision","decision":"continue","rationale":format!("continue to {tool_name}")}),
    );
    record(
        stdin,
        stdout,
        next_id,
        json!({"kind":"attempt","summary":format!("execute {tool_name}"),"actions":[tool_name]}),
    );
    authorize(stdin, stdout, next_id, tool_name, arguments);
}

#[test]
fn stdio_process_executes_every_existing_engine_domain_through_one_registry() {
    let root = tempfile::tempdir().unwrap();
    rrd_engine::InstanceManifest::ensure_dedicated(root.path()).unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn item() {}\n").unwrap();
    let binding = rrd_engine::InstanceBinding::discover(root.path()).unwrap();
    let scope = format!("instance:{}", binding.manifest.id);
    let db = binding.expected_store();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rrflow-mcp"))
        .arg("embedded")
        .arg("--db")
        .arg(&db)
        .arg("--root")
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let initialized = exchange(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"foundation-test","version":"1"}}}),
    );
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    let mut next_id = 2;

    tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_preflight",
        json!({"budget":1000}),
    );
    record(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        json!({"kind":"goal","statement":"exercise every existing RRD domain through stdio MCP","acceptance":["dependent calls return real data"]}),
    );
    record(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        json!({"kind":"plan","hypothesis":"one generated registry reaches every existing engine authority","steps":["data","index","realtime","vector","administration"]}),
    );
    record(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        json!({"kind":"attempt","summary":"commit the document fixture","actions":["rrflow_data_commit"]}),
    );
    let data_arguments = json!({
        "idempotency_key":"stdio-data-1",
        "mutations":[
            {
                "mutation":"put_schema",
                "registry":{
                    "revision":1,
                    "migration":"stdio foundation",
                    "records":{
                        "document":{
                            "properties":{"title":{"value_type":"string","required":true}},
                            "allow_additional_properties":false,
                            "unique_properties":[]
                        }
                    }
                }
            },
            {
                "mutation":"put_record",
                "reference":{"kind":"document","id":"alpha"},
                "valid_from":100,
                "properties":{"title":{"type":"string","value":"Alpha"}}
            }
        ]
    });
    authorize(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_data_commit",
        &data_arguments,
    );
    let committed = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_data_commit",
        data_arguments,
    );
    assert_eq!(committed["mutation_count"], 2);

    let index_arguments = json!({
        "idempotency_key":"stdio-index-1",
        "scope":scope,
        "index_id":"document-title",
        "definition_query":"FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
        "unique":false
    });
    prepare_next_mutation(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_query_index_ensure",
        &index_arguments,
    );
    let index = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_query_index_ensure",
        index_arguments,
    );
    assert_eq!(index["index"]["artifact_rows"], 1);
    let indexes = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_query_index_list",
        json!({"scope":scope}),
    );
    assert_eq!(indexes["indexes"].as_array().unwrap().len(), 1);
    let live = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_live_query_poll",
        json!({
            "scope":scope,
            "query":"FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title",
            "after_cursor":0,
            "max_delta_rows":10,
            "wait_timeout_ms":0
        }),
    );
    assert_eq!(live["added"].as_array().unwrap().len(), 1);
    let changes = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_changefeed_read",
        json!({"scope":scope,"after_cursor":0,"limit":100}),
    );
    assert!(!changes["changes"].as_array().unwrap().is_empty());

    let collection_arguments = json!({
        "idempotency_key":"stdio-vector-collection-1",
        "scope":scope,
        "collection_id":"documents",
        "vectors":[{
            "name":"title",
            "field":"title-embedding",
            "kind":"dense",
            "dimensions":2,
            "metric":"cosine",
            "memory_tier":"cached"
        }]
    });
    prepare_next_mutation(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_collection_ensure",
        &collection_arguments,
    );
    tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_collection_ensure",
        collection_arguments,
    );
    let point_arguments = json!({
        "idempotency_key":"stdio-vector-point-1",
        "mutations":[{
            "mutation":"put_vector",
            "reference":{"kind":"embedding","id":"alpha-title"},
            "subject":{"kind":"document","id":"alpha"},
            "collection_id":"documents",
            "vector_name":"title",
            "field":"title-embedding",
            "valid_from":100,
            "value":{"kind":"dense","values":[1.0,0.0]},
            "properties":{}
        }]
    });
    prepare_next_mutation(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_data_commit",
        &point_arguments,
    );
    tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_data_commit",
        point_arguments,
    );
    let collections = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_collection_list",
        json!({"scope":scope}),
    );
    assert_eq!(collections["collections"].as_array().unwrap().len(), 1);
    let retrieved = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_points_retrieve",
        json!({
            "scope":scope,
            "collection_id":"documents",
            "vector_name":"title",
            "valid_at":100,
            "references":[{"kind":"embedding","id":"alpha-title"}],
            "max_scanned_changes":10000
        }),
    );
    assert_eq!(retrieved["points"].as_array().unwrap().len(), 1);
    let scrolled = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_points_scroll",
        json!({
            "scope":scope,
            "collection_id":"documents",
            "vector_name":"title",
            "valid_at":100,
            "limit":10,
            "max_scanned_changes":10000
        }),
    );
    assert_eq!(scrolled["points"].as_array().unwrap().len(), 1);
    let searched = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_vector_search",
        json!({
            "scope":scope,
            "valid_at":100,
            "collection_id":"documents",
            "vector_name":"title",
            "query":{"kind":"dense","values":[1.0,0.0]},
            "top_k":10,
            "max_scanned_changes":10000
        }),
    );
    assert_eq!(searched["hits"].as_array().unwrap().len(), 1);

    let backup_arguments = json!({
        "idempotency_key":"stdio-backup-1",
        "label":"stdio-foundation",
        "created_at_unix_ms":100
    });
    prepare_next_mutation(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_backup_create",
        &backup_arguments,
    );
    let backup = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_backup_create",
        backup_arguments,
    );
    let backup_sha256 = backup["backup"]["backup_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let listed = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_backup_list",
        json!({"verify_archives":true}),
    );
    assert_eq!(listed["archives_verified"], true);
    let restore_arguments = json!({
        "idempotency_key":"stdio-restore-1",
        "confirmation":"restore_to_new_root",
        "backup_sha256":backup_sha256,
        "restore_id":"stdio-restore-a",
        "restored_at_unix_ms":200
    });
    prepare_next_mutation(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_restore",
        &restore_arguments,
    );
    let restored = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_restore",
        restore_arguments,
    );
    assert_eq!(restored["reopened"], true);
    let estate = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_estate_read",
        json!({"estate_id":"not-configured"}),
    );
    assert!(estate.is_null());
    let audit = tool_call(
        &mut stdin,
        &mut stdout,
        &mut next_id,
        "rrflow_audit_read",
        json!({"after_sequence":0,"limit":100}),
    );
    assert!(audit["records"].is_array());

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
