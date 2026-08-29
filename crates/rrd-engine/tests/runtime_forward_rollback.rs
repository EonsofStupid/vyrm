use rrd_contract::ForwardRollbackRequest;
use rrd_engine::{InstanceBinding, InstanceManifest, RrdEngine, RuntimeToolLifecyclePolicy};
use serde_json::{json, Value};

fn call(engine: &RrdEngine, root: &std::path::Path, name: &str, args: Value, at: u64) -> Value {
    let result = engine.call_runtime_tool(root, name, &args, at).unwrap();
    serde_json::from_str(&result.text).unwrap_or_else(|_| json!({"text": result.text}))
}

fn record(engine: &RrdEngine, root: &std::path::Path, payload: Value, at: u64) {
    call(
        engine,
        root,
        "rrflow_reasoning_record",
        json!({
            "run_id":"rollback-run",
            "actor":"agent:test",
            "at":at,
            "payload":payload
        }),
        at,
    );
}

fn authorize(engine: &RrdEngine, root: &std::path::Path, name: &str, args: &Value, at: u64) {
    let result = call(
        engine,
        root,
        "rrflow_hook",
        json!({
            "event":"pre-tool-use",
            "at":at,
            "input":{"tool_name":name,"tool_input":args}
        }),
        at,
    );
    assert_eq!(result, json!({"text":""}));
}

fn query(engine: &RrdEngine, root: &std::path::Path, scope: &str, ql: &str, at: u64) -> Value {
    call(
        engine,
        root,
        "rrflow_query",
        json!({"scope":scope,"ql":ql,"at":at}),
        at,
    )
}

fn seed_arguments() -> Value {
    json!({
        "idempotency_key":"rollback-seed-v1",
        "at":100,
        "mutations":[
            {
                "mutation":"put_schema",
                "registry":{
                    "revision":1,
                    "migration":"forward rollback fixture",
                    "records":{
                        "document":{
                            "properties":{"title":{"value_type":"string","required":true}},
                            "allow_additional_properties":false,
                            "unique_properties":[]
                        }
                    },
                    "relations":{
                        "linked":{
                            "from":["document"],
                            "to":["document"],
                            "properties":{},
                            "allow_additional_properties":true,
                            "unique_pair":true
                        }
                    },
                    "events":{}
                }
            },
            {
                "mutation":"put_record",
                "reference":{"kind":"document","id":"alpha"},
                "valid_from":100,
                "properties":{"title":{"type":"string","value":"Alpha"}}
            },
            {
                "mutation":"put_record",
                "reference":{"kind":"document","id":"beta"},
                "valid_from":100,
                "properties":{"title":{"type":"string","value":"Beta"}}
            },
            {
                "mutation":"put_relation",
                "reference":{"kind":"linked","id":"alpha-beta"},
                "from":{"kind":"document","id":"alpha"},
                "to":{"kind":"document","id":"beta"},
                "valid_from":100,
                "properties":{}
            }
        ]
    })
}

fn drift_arguments() -> Value {
    json!({
        "idempotency_key":"rollback-drift-v1",
        "at":200,
        "mutations":[
            {
                "mutation":"put_record",
                "reference":{"kind":"document","id":"alpha"},
                "valid_from":200,
                "properties":{"title":{"type":"string","value":"Alpha changed"}}
            },
            {
                "mutation":"put_record",
                "reference":{"kind":"document","id":"gamma"},
                "valid_from":200,
                "properties":{"title":{"type":"string","value":"Gamma"}}
            },
            {
                "mutation":"put_relation",
                "reference":{"kind":"linked","id":"alpha-gamma"},
                "from":{"kind":"document","id":"alpha"},
                "to":{"kind":"document","id":"gamma"},
                "valid_from":200,
                "properties":{}
            }
        ]
    })
}

#[test]
fn forward_rollback_appends_compensation_audit_and_replays_after_reopen() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    InstanceManifest::ensure_dedicated(root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn fixture() {}\n").unwrap();
    let binding = InstanceBinding::discover(root).unwrap();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let scope = format!("instance:{}", binding.manifest.id);

    call(
        &engine,
        root,
        "rrflow_preflight",
        json!({"at":90,"budget":1000}),
        90,
    );
    record(
        &engine,
        root,
        json!({"kind":"goal","statement":"prove forward rollback","acceptance":["append-only and audited"]}),
        91,
    );
    record(
        &engine,
        root,
        json!({"kind":"plan","hypothesis":"compensating versions restore the target","steps":["seed","drift","rollback","reopen"]}),
        92,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"seed the historical target","actions":["rrflow_data_commit"]}),
        93,
    );
    let seed = seed_arguments();
    let seeded = call(&engine, root, "rrflow_data_commit", seed, 100);
    let target_cursor = seeded["last_runtime_cursor"].as_u64().unwrap();

    record(
        &engine,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"create a later state to compensate"}),
        190,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"append changed and new structural state","actions":["rrflow_data_commit"]}),
        191,
    );
    let drift = drift_arguments();
    authorize(&engine, root, "rrflow_data_commit", &drift, 199);
    let drifted = call(&engine, root, "rrflow_data_commit", drift, 200);
    let pre_rollback_cursor = drifted["last_runtime_cursor"].as_u64().unwrap();
    let before = query(
        &engine,
        root,
        &scope,
        &format!("FROM record:document AT VALID 300 KNOWN {pre_rollback_cursor} PROJECT id, title"),
        201,
    );
    assert_eq!(before["execution"]["returned_rows"], 3);

    record(
        &engine,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"restore the selected retained state with forward compensation"}),
        290,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"append forward compensation","actions":["rrflow_data_rollback"]}),
        291,
    );
    let rollback = json!({
        "idempotency_key":"rollback-apply-v1",
        "target_valid_at":100,
        "target_known_at_cursor":target_cursor,
        "effective_at":300,
        "reason":"restore the accepted seed fixture",
        "timeout_ms":60000
    });
    authorize(&engine, root, "rrflow_data_rollback", &rollback, 299);
    let receipt = call(&engine, root, "rrflow_data_rollback", rollback.clone(), 300);
    assert!(receipt["read_cursor"].as_u64().unwrap() > pre_rollback_cursor);
    assert_eq!(receipt["counts"]["restored_records"], 1);
    assert_eq!(receipt["counts"]["retired_records"], 1);
    assert_eq!(receipt["counts"]["restored_relations"], 0);
    assert_eq!(receipt["counts"]["retired_relations"], 1);
    assert_eq!(receipt["commit"]["mutation_count"], 4);
    assert_eq!(receipt["commit"]["idempotent_replay"], false);

    let after = query(
        &engine,
        root,
        &scope,
        "FROM record:document AT VALID 300 KNOWN HEAD PROJECT id, title",
        301,
    );
    assert_eq!(after["execution"]["returned_rows"], 2);
    assert_eq!(
        after["execution"]["batches"][0]["rows"][0]["values"]["title"]["value"],
        "Alpha"
    );
    let relation = query(
        &engine,
        root,
        &scope,
        "FROM relation:linked AT VALID 300 KNOWN HEAD PROJECT id",
        302,
    );
    assert_eq!(relation["execution"]["returned_rows"], 1);
    assert_eq!(
        relation["execution"]["batches"][0]["rows"][0]["values"]["id"]["value"],
        "alpha-beta"
    );
    let old_prefix = query(
        &engine,
        root,
        &scope,
        &format!("FROM record:document AT VALID 300 KNOWN {pre_rollback_cursor} PROJECT id, title"),
        303,
    );
    assert_eq!(old_prefix["execution"]["returned_rows"], 3);
    let evidence = query(
        &engine,
        root,
        &scope,
        "FROM claim:historical-rollback AT VALID 300 KNOWN HEAD PROJECT object",
        304,
    );
    assert_eq!(evidence["execution"]["returned_rows"], 1);

    let invalid = ForwardRollbackRequest {
        target_valid_at: 100,
        target_known_at_cursor: receipt["commit"]["last_runtime_cursor"].as_u64().unwrap(),
        effective_at: 400,
        reason: "not historical".into(),
    };
    assert!(engine
        .plan_forward_rollback(
            &invalid,
            invalid.target_known_at_cursor,
            &rrd_contract::CorrelationId::new("invalid-rollback").unwrap(),
        )
        .is_err());

    record(
        &engine,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"prove exact retry after reopen"}),
        305,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"replay the rollback request","actions":["rrflow_data_rollback"]}),
        306,
    );
    authorize(&engine, root, "rrflow_data_rollback", &rollback, 307);
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let replay = call(&reopened, root, "rrflow_data_rollback", rollback, 308);
    assert_eq!(replay["commit"]["idempotent_replay"], true);
    assert_eq!(
        replay["target_state_sha256"],
        receipt["target_state_sha256"]
    );
    let reopened_state = query(
        &reopened,
        root,
        &scope,
        "FROM record:document AT VALID 300 KNOWN HEAD PROJECT id, title",
        309,
    );
    assert_eq!(reopened_state["execution"]["returned_rows"], 2);

    let lifecycle = rrd_engine::runtime_tool_catalogue()
        .into_iter()
        .find(|tool| tool.name == "rrflow_data_rollback")
        .unwrap()
        .lifecycle;
    assert_eq!(lifecycle, RuntimeToolLifecyclePolicy::PlannedMutation);
}
