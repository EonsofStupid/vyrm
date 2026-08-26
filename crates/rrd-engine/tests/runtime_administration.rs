use rrd_contract::{
    AuditDecision, AuditPhase, CanonicalId, ResourceId, ResourceKind, ResourcePath, SecurityAction,
};
use rrd_core::{digest, ClaimReader, Predicate, Subject};
use rrd_engine::{InstanceBinding, InstanceManifest, RrdEngine, RuntimeToolAttunement};
use rrd_estate::{EstateRepository, MutationContext};
use rrd_security::{AuditRecord, SecurityRepository};
use rrd_store::PersistentEngine;
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
        json!({"run_id":"mcp-admin-run","actor":"agent:test","at":at,"payload":payload}),
        at,
    );
}

fn authorize(engine: &RrdEngine, root: &std::path::Path, tool_name: &str, args: &Value, at: u64) {
    let result = call(
        engine,
        root,
        "rrflow_lifecycle",
        json!({
            "event":"pre-tool-use",
            "at":at,
            "input":{"tool_name":tool_name,"tool_input":args}
        }),
        at,
    );
    assert_eq!(result, json!({"text":""}));
}

fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).unwrap()
}

#[test]
fn administration_tools_backup_restore_estate_and_audit_through_one_engine() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    InstanceManifest::ensure_dedicated(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn fixture() {}\n").unwrap();
    let binding = InstanceBinding::discover(&root).unwrap();
    let database = binding.expected_store();

    let storage = PersistentEngine::open(&database).unwrap();
    EstateRepository::new(&storage, id("estate-a"))
        .create(&MutationContext {
            at: 10,
            actor: "test:estate".into(),
            request_id: "request-estate-create".into(),
            operation_id: id("operation-estate-create"),
        })
        .unwrap();
    SecurityRepository::new(&storage, id(binding.manifest.id.as_str()))
        .append_audit(&AuditRecord {
            audit_id: id("audit-fixture"),
            at_unix_ms: 11,
            principal_id: None,
            action: SecurityAction::EstateRead,
            resource: ResourcePath {
                segments: vec![ResourceId::new(
                    ResourceKind::Instance,
                    binding.manifest.id.as_str(),
                )
                .unwrap()],
            },
            request_id: "request-audit-fixture".into(),
            operation_id: "operation-audit-fixture".into(),
            phase: AuditPhase::Completed,
            decision: AuditDecision::Allowed,
            status_code: 200,
            request_sha256: digest::sha256_hex(b"audit request"),
            response_sha256: digest::sha256_hex(b"audit response"),
        })
        .unwrap();
    drop(storage);

    let engine = RrdEngine::open_bound(&binding).unwrap();
    call(
        &engine,
        &root,
        "rrflow_remember",
        json!({
            "subject":"project:alpha",
            "predicate":"status",
            "object":"ready-for-backup",
            "at":100,
            "valid_from":100
        }),
        100,
    );
    call(
        &engine,
        &root,
        "rrflow_preflight",
        json!({"at":200,"budget":1000}),
        200,
    );
    record(
        &engine,
        &root,
        json!({"kind":"goal","statement":"prove the MCP administration boundary","acceptance":["backup, restore, estate, and audit are durable"]}),
        201,
    );
    record(
        &engine,
        &root,
        json!({"kind":"plan","hypothesis":"the existing RRD authorities compose without an MCP control plane","steps":["backup","reopen","restore","inspect"]}),
        202,
    );
    record(
        &engine,
        &root,
        json!({"kind":"attempt","summary":"create the authenticated logical backup","actions":["rrflow_backup_create"]}),
        203,
    );
    let backup_arguments = json!({
        "idempotency_key":"mcp-backup-1",
        "label":"mcp-foundation",
        "created_at_unix_ms":204,
        "at":204
    });
    authorize(
        &engine,
        &root,
        "rrflow_backup_create",
        &backup_arguments,
        204,
    );
    let backup = call(
        &engine,
        &root,
        "rrflow_backup_create",
        backup_arguments.clone(),
        205,
    );
    assert_eq!(backup["idempotent_replay"], false);
    assert_eq!(backup["backup"]["archive"]["standalone_claims"], 1);
    let backup_sha256 = backup["backup"]["backup_sha256"]
        .as_str()
        .unwrap()
        .to_owned();

    record(
        &engine,
        &root,
        json!({"kind":"decision","decision":"continue","rationale":"prove exact backup replay after engine reopen"}),
        206,
    );
    record(
        &engine,
        &root,
        json!({"kind":"attempt","summary":"replay the exact backup request","actions":["reopen","rrflow_backup_create"]}),
        207,
    );
    authorize(
        &engine,
        &root,
        "rrflow_backup_create",
        &backup_arguments,
        208,
    );
    drop(engine);
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let replay = call(
        &engine,
        &root,
        "rrflow_backup_create",
        backup_arguments,
        209,
    );
    assert_eq!(replay["idempotent_replay"], true);
    assert_eq!(replay["backup"]["backup_sha256"], backup_sha256);

    let catalogue = call(
        &engine,
        &root,
        "rrflow_backup_list",
        json!({"verify_archives":true}),
        210,
    );
    assert_eq!(catalogue["archives_verified"], true);
    assert_eq!(catalogue["backups"].as_array().unwrap().len(), 1);

    let estate = call(
        &engine,
        &root,
        "rrflow_estate_read",
        json!({"estate_id":"estate-a"}),
        211,
    );
    assert_eq!(estate["id"], "estate-a");
    assert_eq!(estate["revision"], 1);

    let audit = call(
        &engine,
        &root,
        "rrflow_audit_read",
        json!({"after_sequence":0,"limit":100}),
        212,
    );
    assert_eq!(audit["records"].as_array().unwrap().len(), 1);
    assert_eq!(audit["records"][0]["audit_id"], "audit-fixture");

    record(
        &engine,
        &root,
        json!({"kind":"decision","decision":"continue","rationale":"restore the verified archive only to a new isolated root"}),
        213,
    );
    record(
        &engine,
        &root,
        json!({"kind":"attempt","summary":"restore and reopen the backup","actions":["rrflow_restore"]}),
        214,
    );
    let restore_arguments = json!({
        "idempotency_key":"mcp-restore-1",
        "confirmation":"restore_to_new_root",
        "backup_sha256":backup_sha256,
        "restore_id":"mcp-restore-a",
        "restored_at_unix_ms":215,
        "at":215
    });
    authorize(&engine, &root, "rrflow_restore", &restore_arguments, 215);
    let restored = call(
        &engine,
        &root,
        "rrflow_restore",
        restore_arguments.clone(),
        216,
    );
    assert_eq!(restored["restore_id"], "mcp-restore-a");
    assert_eq!(restored["reopened"], true);
    assert_eq!(restored["idempotent_replay"], false);

    let restored_root = database
        .parent()
        .unwrap()
        .join("rrd-service")
        .join(binding.manifest.id.as_str())
        .join("restores")
        .join("mcp-restore-a");
    assert!(restored_root.join("CURRENT").is_file());
    let restored_engine = PersistentEngine::open(&restored_root).unwrap();
    let claim = restored_engine
        .current(
            &Subject::new("project:alpha").unwrap(),
            &Predicate::new("status").unwrap(),
            1_000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(claim.object, "ready-for-backup");
    drop(restored_engine);

    record(
        &engine,
        &root,
        json!({"kind":"decision","decision":"continue","rationale":"prove exact restore replay after engine reopen"}),
        217,
    );
    record(
        &engine,
        &root,
        json!({"kind":"attempt","summary":"replay the exact restore request","actions":["reopen","rrflow_restore"]}),
        218,
    );
    authorize(&engine, &root, "rrflow_restore", &restore_arguments, 219);
    drop(engine);
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let restore_replay = call(&engine, &root, "rrflow_restore", restore_arguments, 220);
    assert_eq!(restore_replay["idempotent_replay"], true);
}

#[test]
fn reviewed_a_series_is_fully_generated_and_capability_mapped() {
    let catalogue = rrd_engine::runtime_tool_catalogue();
    assert_eq!(catalogue.len(), 28);
    for (name, capability, attunement, mutation, required_fields) in [
        (
            "rrflow_backup_create",
            "backup-create",
            RuntimeToolAttunement::ExactTool,
            true,
            &["idempotency_key", "label", "created_at_unix_ms"][..],
        ),
        (
            "rrflow_backup_list",
            "backup-list",
            RuntimeToolAttunement::None,
            false,
            &[][..],
        ),
        (
            "rrflow_restore",
            "restore-create",
            RuntimeToolAttunement::ExactTool,
            true,
            &[
                "idempotency_key",
                "confirmation",
                "backup_sha256",
                "restore_id",
                "restored_at_unix_ms",
            ][..],
        ),
        (
            "rrflow_estate_read",
            "estate-read",
            RuntimeToolAttunement::None,
            false,
            &["estate_id"][..],
        ),
        (
            "rrflow_audit_read",
            "audit-read",
            RuntimeToolAttunement::None,
            false,
            &["after_sequence", "limit"][..],
        ),
    ] {
        let definition = catalogue
            .iter()
            .find(|definition| definition.name == name)
            .unwrap();
        assert_eq!(definition.capability_id, Some(capability));
        assert_eq!(definition.attunement, attunement);
        assert_eq!(definition.mutation, mutation);
        let required = definition.input_schema["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for field in required_fields {
            assert!(required.iter().any(|required| required == field));
        }
    }
}
