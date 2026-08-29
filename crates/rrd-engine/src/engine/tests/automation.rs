use super::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use rrd_contract::{
    AutomationCatalogue, DataEventSchema, DataLogicalModel, DataRecordSchema, DataReference,
    DataSchemaMode, DataSchemaRegistry, DataTableSchema, ExecuteFunction, FunctionCapability,
    FunctionDefinition, FunctionLimits, FunctionRuntime, FunctionTrigger, FunctionTriggerEffect,
    FunctionTriggerMutation, QueryValue, ReadDataSnapshot, ReplaceAutomationCatalogue,
    WebAssemblyAbi, FUNCTION_CONTRACT_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

fn limits() -> FunctionLimits {
    FunctionLimits {
        max_input_bytes: 32 * 1024,
        max_output_bytes: 32 * 1024,
        memory_bytes: 8 * 1024 * 1024,
        stack_bytes: 128 * 1024,
        max_interrupts: 10_000,
        max_fuel: 100_000,
    }
}

fn javascript(id: &str, source: &str) -> FunctionDefinition {
    FunctionDefinition {
        function_id: CanonicalId::new(id).unwrap(),
        runtime: FunctionRuntime::JavaScriptEs2020 {
            source: source.into(),
            source_sha256: digest::sha256_hex(source.as_bytes()),
        },
        limits: limits(),
        capabilities: BTreeSet::new(),
    }
}

fn webassembly(id: &str, wat: &str) -> FunctionDefinition {
    let module = wat::parse_str(wat).unwrap();
    FunctionDefinition {
        function_id: CanonicalId::new(id).unwrap(),
        runtime: FunctionRuntime::WebAssemblyV1 {
            abi: WebAssemblyAbi::JsonV1,
            module_base64: STANDARD.encode(&module),
            module_sha256: digest::sha256_hex(&module),
        },
        limits: limits(),
        capabilities: BTreeSet::new(),
    }
}

fn catalogue(
    revision: u64,
    functions: Vec<FunctionDefinition>,
    triggers: Vec<FunctionTrigger>,
) -> AutomationCatalogue {
    AutomationCatalogue {
        contract_version: FUNCTION_CONTRACT_VERSION,
        revision,
        functions: functions
            .into_iter()
            .map(|function| (function.function_id.clone(), function))
            .collect(),
        triggers: triggers
            .into_iter()
            .map(|trigger| (trigger.trigger_id.clone(), trigger))
            .collect(),
    }
}

fn install(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    expected_revision: u64,
    catalogue: AutomationCatalogue,
    suffix: &str,
) {
    engine
        .replace_automation_catalogue(
            &lease.session_id,
            &lease.token,
            &ReplaceAutomationCatalogue {
                expected_revision,
                catalogue,
            },
            1_100 + expected_revision,
            &format!("request-catalogue-{suffix}"),
            &format!("operation-catalogue-{suffix}"),
        )
        .unwrap();
}

#[test]
fn javascript_and_webassembly_are_bounded_deterministic_and_restart_safe() {
    let (root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(5_000, 4),
            &id("function-session"),
            1_000,
            "request-function-session",
            "operation-function-session",
        )
        .unwrap();
    let javascript_definition = javascript("add-one", "(input) => ({ answer: input.value + 1 })");
    let wasm = webassembly(
        "wasm-answer",
        r#"(module
            (memory (export "memory") 1 1)
            (data (i32.const 0) "{\22answer\22:42}")
            (func (export "rrd_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "rrd_run") (param i32 i32) (result i64) i64.const 13)
        )"#,
    );
    install(
        &engine,
        &lease,
        0,
        catalogue(1, vec![javascript_definition, wasm], vec![]),
        "one",
    );
    let installed = engine
        .automation_catalogue(
            &lease.session_id,
            &lease.token,
            1_150,
            "request-catalogue-before-conflict",
            "operation-catalogue-before-conflict",
        )
        .unwrap();
    let conflict = engine.replace_automation_catalogue(
        &lease.session_id,
        &lease.token,
        &ReplaceAutomationCatalogue {
            expected_revision: 0,
            catalogue: catalogue(
                1,
                vec![javascript(
                    "substituted-function",
                    "() => ({ substituted: true })",
                )],
                vec![],
            ),
        },
        1_151,
        "request-catalogue-conflict",
        "operation-catalogue-conflict",
    );
    assert!(matches!(conflict, Err(ServiceError::StorageConflict(_))));
    assert_eq!(
        engine
            .automation_catalogue(
                &lease.session_id,
                &lease.token,
                1_152,
                "request-catalogue-after-conflict",
                "operation-catalogue-after-conflict",
            )
            .unwrap(),
        installed,
        "a stale replacement must not alter the immutable revision or head",
    );

    let input = QueryValue::Map(BTreeMap::from([("value".into(), QueryValue::Integer(41))]));
    let javascript_result = engine
        .execute_function(
            &lease.session_id,
            &lease.token,
            &ExecuteFunction {
                function_id: CanonicalId::new("add-one").unwrap(),
                input: input.clone(),
            },
            1_200,
            "request-js",
            "operation-js",
        )
        .unwrap();
    assert_eq!(
        javascript_result.output,
        QueryValue::Map(BTreeMap::from([("answer".into(), QueryValue::Integer(42))]))
    );
    assert_eq!(javascript_result.fuel_consumed, 0);
    let unsafe_integer_input = engine.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("add-one").unwrap(),
            input: QueryValue::Unsigned(rrd_contract::MAX_FUNCTION_JSON_SAFE_INTEGER + 1),
        },
        1_200,
        "request-unsafe-integer-input",
        "operation-unsafe-integer-input",
    );
    assert!(matches!(
        unsafe_integer_input,
        Err(ServiceError::Contract(_))
    ));

    let wasm_result = engine
        .execute_function(
            &lease.session_id,
            &lease.token,
            &ExecuteFunction {
                function_id: CanonicalId::new("wasm-answer").unwrap(),
                input,
            },
            1_201,
            "request-wasm",
            "operation-wasm",
        )
        .unwrap();
    assert_eq!(wasm_result.output, javascript_result.output);
    assert!(wasm_result.fuel_consumed > 0);

    drop(engine);
    let reopened = RrdEngine::open(root.path(), instance(), TOKEN_KEY).unwrap();
    let restored = reopened
        .automation_catalogue(
            &lease.session_id,
            &lease.token,
            1_300,
            "request-list-functions",
            "operation-list-functions",
        )
        .unwrap();
    assert_eq!(restored.revision, 1);
    assert_eq!(restored.functions.len(), 2);

    let mut endless = javascript("endless", "() => { while (true) {} }");
    endless.limits.max_interrupts = 1;
    let heap_hog = javascript("heap-hog", "() => new ArrayBuffer(16777216)");
    let stack_hog = javascript(
        "stack-hog",
        "() => { function recurse() { return recurse(); } return recurse(); }",
    );
    let random = javascript("random", "() => Math.random()");
    let unsafe_integer_output = javascript("unsafe-integer-output", "() => 9007199254740992");
    let imported = webassembly(
        "imported",
        r#"(module
            (import "env" "clock" (func))
            (memory (export "memory") 1 1)
            (func (export "rrd_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "rrd_run") (param i32 i32) (result i64) i64.const 4)
        )"#,
    );
    let wasm_loop = webassembly(
        "wasm-loop",
        r#"(module
            (memory (export "memory") 1 1)
            (func (export "rrd_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "rrd_run") (param i32 i32) (result i64)
                (loop $forever br $forever)
                i64.const 0)
        )"#,
    );
    let wasm_stack = webassembly(
        "wasm-stack",
        r#"(module
            (memory (export "memory") 1 1)
            (func (export "rrd_alloc") (param i32) (result i32) i32.const 1024)
            (func $recurse (result i64) call $recurse)
            (func (export "rrd_run") (param i32 i32) (result i64) call $recurse)
        )"#,
    );
    install(
        &reopened,
        &lease,
        1,
        catalogue(
            2,
            restored
                .functions
                .into_values()
                .chain([
                    endless,
                    heap_hog,
                    stack_hog,
                    random,
                    unsafe_integer_output,
                    imported,
                    wasm_loop,
                    wasm_stack,
                ])
                .collect(),
            vec![],
        ),
        "two",
    );
    let exhausted = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("endless").unwrap(),
            input: QueryValue::Null,
        },
        1_400,
        "request-endless",
        "operation-endless",
    );
    assert!(matches!(exhausted, Err(ServiceError::FunctionLimit(_))));
    for function_id in ["heap-hog", "stack-hog"] {
        let exhausted = reopened.execute_function(
            &lease.session_id,
            &lease.token,
            &ExecuteFunction {
                function_id: CanonicalId::new(function_id).unwrap(),
                input: QueryValue::Null,
            },
            1_400,
            &format!("request-{function_id}"),
            &format!("operation-{function_id}"),
        );
        assert!(
            matches!(exhausted, Err(ServiceError::FunctionLimit(_))),
            "{function_id} must exhaust its configured resource: {exhausted:?}"
        );
    }
    let denied_random = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("random").unwrap(),
            input: QueryValue::Null,
        },
        1_400,
        "request-random",
        "operation-random",
    );
    assert!(matches!(denied_random, Err(ServiceError::Function(_))));
    let denied_unsafe_integer = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("unsafe-integer-output").unwrap(),
            input: QueryValue::Null,
        },
        1_400,
        "request-unsafe-integer-output",
        "operation-unsafe-integer-output",
    );
    assert!(matches!(
        denied_unsafe_integer,
        Err(ServiceError::Contract(_))
    ));
    let denied_import = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("imported").unwrap(),
            input: QueryValue::Null,
        },
        1_401,
        "request-imported",
        "operation-imported",
    );
    assert!(matches!(denied_import, Err(ServiceError::Function(_))));
    let exhausted_fuel = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("wasm-loop").unwrap(),
            input: QueryValue::Null,
        },
        1_402,
        "request-wasm-loop",
        "operation-wasm-loop",
    );
    assert!(matches!(
        exhausted_fuel,
        Err(ServiceError::FunctionLimit(_))
    ));
    let exhausted_stack = reopened.execute_function(
        &lease.session_id,
        &lease.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("wasm-stack").unwrap(),
            input: QueryValue::Null,
        },
        1_403,
        "request-wasm-stack",
        "operation-wasm-stack",
    );
    assert!(matches!(
        exhausted_stack,
        Err(ServiceError::FunctionLimit(_))
    ));
}

#[test]
fn trigger_effects_commit_atomically_and_failures_leave_data_unchanged() {
    let (_root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(10_000, 4),
            &id("trigger-session"),
            1_000,
            "request-trigger-session",
            "operation-trigger-session",
        )
        .unwrap();
    let mut event_function = javascript(
        "emit-person-event",
        "(input) => ({ mutation_index: input.mutation_index })",
    );
    event_function
        .capabilities
        .insert(FunctionCapability::EmitEvent);
    let trigger = FunctionTrigger {
        trigger_id: CanonicalId::new("person-event").unwrap(),
        function_id: event_function.function_id.clone(),
        mutation: FunctionTriggerMutation::PutRecord,
        kind: Some(CanonicalId::new("person").unwrap()),
        effect: FunctionTriggerEffect::AppendEvent {
            kind: CanonicalId::new("person-changed").unwrap(),
        },
        max_attempts: 1,
    };
    install(
        &engine,
        &lease,
        0,
        catalogue(1, vec![event_function], vec![trigger]),
        "trigger-one",
    );

    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 5_000,
            },
            &mutation_context(
                &id("begin-trigger-one"),
                "request-begin-trigger-one",
                "operation-begin-trigger-one",
            ),
            1_100,
        )
        .unwrap();
    let person = CanonicalId::new("person").unwrap();
    let changed = CanonicalId::new("person-changed").unwrap();
    let registry = DataSchemaRegistry {
        revision: 1,
        migration: "install trigger fixture".into(),
        catalogue: DataCatalogueIdentity::default(),
        tables: BTreeMap::from([
            (
                person.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Document,
                    mode: DataSchemaMode::Strict,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
            (
                changed.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Event,
                    mode: DataSchemaMode::Strict,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
        ]),
        records: BTreeMap::from([(
            person.clone(),
            DataRecordSchema {
                properties: BTreeMap::new(),
                allow_additional_properties: true,
                unique_properties: BTreeSet::new(),
            },
        )]),
        relations: BTreeMap::new(),
        events: BTreeMap::from([(
            changed.clone(),
            DataEventSchema {
                subject_required: false,
                subject_types: BTreeSet::new(),
                properties: BTreeMap::new(),
                allow_additional_properties: true,
            },
        )]),
    };
    let mutations = vec![
        TransactionMutation::PutSchema { registry },
        TransactionMutation::PutRecord {
            reference: DataReference {
                kind: person.clone(),
                id: CanonicalId::new("alice").unwrap(),
            },
            valid_from: 1_200,
            valid_to: None,
            properties: BTreeMap::new(),
        },
    ];
    let commit = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
        mutations,
    };
    engine
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("commit-trigger-one"),
            &commit,
            1_200,
            "request-commit-trigger-one",
            "operation-commit-trigger-one",
        )
        .unwrap();
    let snapshot = engine
        .read_data_snapshot(
            &lease.session_id,
            &lease.token,
            &ReadDataSnapshot {
                valid_at: 1_200,
                max_scanned_changes: 100,
            },
            1_201,
            "request-trigger-snapshot",
            "operation-trigger-snapshot",
        )
        .unwrap();
    assert!(snapshot.entries.iter().any(|entry| {
        matches!(
            &entry.value,
            TransactionMutation::AppendEvent { kind, .. } if kind == &changed
        )
    }));
    let trigger_audit = SecurityRepository::new(&engine.storage, instance())
        .audit_since(0, 64)
        .unwrap();
    assert!(trigger_audit.records.iter().any(|(_, record)| {
        record.action == SecurityAction::FunctionExecute
            && record.phase == rrd_contract::AuditPhase::Completed
            && record.decision == AuditDecision::Allowed
    }));

    let reject = javascript("reject-person", "() => false");
    let reject_trigger = FunctionTrigger {
        trigger_id: CanonicalId::new("reject-person").unwrap(),
        function_id: reject.function_id.clone(),
        mutation: FunctionTriggerMutation::PutRecord,
        kind: Some(person.clone()),
        effect: FunctionTriggerEffect::RequireTrue,
        max_attempts: 1,
    };
    install(
        &engine,
        &lease,
        1,
        catalogue(2, vec![reject], vec![reject_trigger]),
        "trigger-two",
    );
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &BeginTransaction {
                scope: CanonicalId::new("data").unwrap(),
                timeout_ms: 5_000,
            },
            &mutation_context(
                &id("begin-trigger-two"),
                "request-begin-trigger-two",
                "operation-begin-trigger-two",
            ),
            1_300,
        )
        .unwrap();
    let before = engine.storage.runtime_cursor().unwrap();
    let mutations = vec![TransactionMutation::PutRecord {
        reference: DataReference {
            kind: person,
            id: CanonicalId::new("bob").unwrap(),
        },
        valid_from: 1_300,
        valid_to: None,
        properties: BTreeMap::new(),
    }];
    let rejected = engine.commit_transaction(
        &lease.session_id,
        &lease.token,
        &transaction.transaction_id,
        &id("commit-trigger-two"),
        &CommitTransaction {
            operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
            mutations,
        },
        1_301,
        "request-commit-trigger-two",
        "operation-commit-trigger-two",
    );
    assert!(matches!(rejected, Err(ServiceError::Function(_))));
    assert_eq!(engine.storage.runtime_cursor().unwrap(), before);
}

#[test]
fn transaction_retry_executes_the_catalogue_revision_pinned_before_the_crash_window() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("automation-retry");
    let engine = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let lease = engine
        .create_session(
            &session_request(5_000, 2),
            &id("automation-retry-session"),
            1_000,
            "request-automation-retry-session",
            "operation-automation-retry-session",
        )
        .unwrap();
    let trigger_id = CanonicalId::new("claim-validator").unwrap();
    let accepted = javascript("claim-policy", "() => true");
    let accepted_trigger = FunctionTrigger {
        trigger_id: trigger_id.clone(),
        function_id: accepted.function_id.clone(),
        mutation: FunctionTriggerMutation::AssertClaim,
        kind: None,
        effect: FunctionTriggerEffect::RequireTrue,
        max_attempts: 1,
    };
    install(
        &engine,
        &lease,
        0,
        catalogue(1, vec![accepted], vec![accepted_trigger]),
        "retry-one",
    );
    let transaction = engine
        .begin_transaction(
            &lease.session_id,
            &lease.token,
            &begin_request(),
            &mutation_context(
                &id("automation-retry-begin"),
                "request-automation-retry-begin",
                "operation-automation-retry-begin",
            ),
            1_100,
        )
        .unwrap();
    let request = commit_request("revision-one-must-win");
    let runtime_at = 1_150;
    let base = public_runtime_commit(
        &request,
        &lease.session_id,
        &instance(),
        transaction.read_cursor,
        runtime_at,
    )
    .unwrap();
    let revision_one = engine.load_current_automation_catalogue().unwrap();
    let derived = engine
        .apply_transaction_triggers(
            &revision_one,
            &request,
            base,
            None,
            runtime_at,
            "request-automation-retry-prepare",
            "operation-automation-retry-prepare",
        )
        .unwrap();
    let session_key = format!(
        "server/state/test-instance/session/{}",
        lease.session_id.as_str()
    );
    let before = engine
        .storage
        .control_record(&session_key)
        .unwrap()
        .unwrap();
    let mut prepared: Value = serde_json::from_slice(&before).unwrap();
    prepared["transactions"][transaction.transaction_id.as_str()]["commit_intent"] = serde_json::json!({
        "idempotency_key": "automation-retry-commit",
        "operation_sha256": request.operation_sha256.clone(),
        "runtime_at_unix_ms": runtime_at,
        "runtime_commit_sha256": derived.digest(),
        "automation_revision": 1,
    });
    engine
        .storage
        .commit_control_transition(&ControlTransition {
            key: session_key,
            expected: Some(before),
            replacement: Some(serde_json::to_vec(&prepared).unwrap()),
            at: runtime_at,
            actor: "rrd-engine-test".into(),
            action: "transaction.commit_prepared".into(),
            request_id: "request-automation-retry-prepare".into(),
            operation_id: "operation-automation-retry-prepare".into(),
        })
        .unwrap();

    let rejected = javascript("claim-policy", "() => false");
    let rejected_trigger = FunctionTrigger {
        trigger_id,
        function_id: rejected.function_id.clone(),
        mutation: FunctionTriggerMutation::AssertClaim,
        kind: None,
        effect: FunctionTriggerEffect::RequireTrue,
        max_attempts: 1,
    };
    install(
        &engine,
        &lease,
        1,
        catalogue(2, vec![rejected], vec![rejected_trigger]),
        "retry-two",
    );
    drop(engine);

    let reopened = RrdEngine::open(&path, instance(), TOKEN_KEY).unwrap();
    let receipt = reopened
        .commit_transaction(
            &lease.session_id,
            &lease.token,
            &transaction.transaction_id,
            &id("automation-retry-commit"),
            &request,
            1_300,
            "request-automation-retry-commit",
            "operation-automation-retry-commit",
        )
        .unwrap();
    assert_eq!(receipt.last_runtime_cursor, Some(1));
    assert_eq!(
        reopened
            .load_current_automation_catalogue()
            .unwrap()
            .revision,
        2,
        "recovery must retain the newer head while executing pinned revision one",
    );
}

#[test]
fn exact_security_actions_gate_catalogue_execution_and_transaction_triggers() {
    let (_root, engine) = isolated_engine();
    let admin_id = CanonicalId::new("function-admin").unwrap();
    let limited_id = CanonicalId::new("function-reader").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance().as_str()).unwrap()],
    };
    let principal =
        |principal_id: CanonicalId, credential: &[u8], actions: &[SecurityAction]| Principal {
            id: principal_id,
            kind: PrincipalKind::Service,
            credential_sha256: digest::sha256_hex(credential),
            credential_revision: 1,
            not_before_unix_ms: 1,
            expires_at_unix_ms: u64::MAX,
            disabled: false,
            role_ids: BTreeSet::new(),
            grants: actions
                .iter()
                .copied()
                .map(|action| ResourceGrant {
                    action,
                    resource_prefix: resource.clone(),
                    data_policy: None,
                })
                .collect(),
        };
    SecurityRepository::new(&engine.storage, instance())
        .initialize(
            SecurityState {
                format_version: rrd_security::SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([
                    (
                        admin_id.clone(),
                        principal(
                            admin_id.clone(),
                            b"function-admin-key",
                            &[
                                SecurityAction::SessionCreate,
                                SecurityAction::FunctionCatalogueRead,
                                SecurityAction::FunctionCatalogueWrite,
                                SecurityAction::FunctionExecute,
                            ],
                        ),
                    ),
                    (
                        limited_id.clone(),
                        principal(
                            limited_id.clone(),
                            b"function-reader-key",
                            &[
                                SecurityAction::SessionCreate,
                                SecurityAction::FunctionCatalogueRead,
                                SecurityAction::TransactionBegin,
                                SecurityAction::TransactionCommit,
                            ],
                        ),
                    ),
                ]),
                roles: BTreeMap::new(),
                identity_bindings: BTreeMap::new(),
                jwt_issuers: BTreeMap::new(),
            },
            1,
            "function-security-test",
            "request-function-security",
            "operation-function-security",
        )
        .unwrap();
    let admin = engine
        .create_authenticated_session(
            &admin_id,
            b"function-admin-key",
            &session_request(5_000, 2),
            &id("function-admin-session"),
            1_000,
            "request-function-admin-session",
            "operation-function-admin-session",
        )
        .unwrap();
    let limited = engine
        .create_authenticated_session(
            &limited_id,
            b"function-reader-key",
            &session_request(5_000, 2),
            &id("function-reader-session"),
            1_000,
            "request-function-reader-session",
            "operation-function-reader-session",
        )
        .unwrap();
    let definition = javascript("secured-policy", "() => true");
    let trigger = FunctionTrigger {
        trigger_id: CanonicalId::new("secured-trigger").unwrap(),
        function_id: definition.function_id.clone(),
        mutation: FunctionTriggerMutation::AssertClaim,
        kind: None,
        effect: FunctionTriggerEffect::RequireTrue,
        max_attempts: 1,
    };
    install(
        &engine,
        &admin,
        0,
        catalogue(1, vec![definition], vec![trigger]),
        "secured",
    );

    assert_eq!(
        engine
            .automation_catalogue(
                &limited.session_id,
                &limited.token,
                1_100,
                "request-function-list-allowed",
                "operation-function-list-allowed",
            )
            .unwrap()
            .revision,
        1,
    );
    let denied_execute = engine.execute_function(
        &limited.session_id,
        &limited.token,
        &ExecuteFunction {
            function_id: CanonicalId::new("secured-policy").unwrap(),
            input: QueryValue::Null,
        },
        1_101,
        "request-function-execute-denied",
        "operation-function-execute-denied",
    );
    assert!(matches!(
        denied_execute,
        Err(ServiceError::PermissionDenied)
    ));
    let denied_write = engine.replace_automation_catalogue(
        &limited.session_id,
        &limited.token,
        &ReplaceAutomationCatalogue {
            expected_revision: 1,
            catalogue: catalogue(
                2,
                vec![javascript("replacement-policy", "() => true")],
                vec![],
            ),
        },
        1_102,
        "request-function-write-denied",
        "operation-function-write-denied",
    );
    assert!(matches!(denied_write, Err(ServiceError::PermissionDenied)));

    let transaction = engine
        .begin_transaction(
            &limited.session_id,
            &limited.token,
            &begin_request(),
            &mutation_context(
                &id("function-trigger-begin"),
                "request-function-trigger-begin",
                "operation-function-trigger-begin",
            ),
            1_110,
        )
        .unwrap();
    let before = engine.storage.runtime_cursor().unwrap();
    let request = commit_request("function-trigger-must-be-denied");
    let denied_trigger = engine.commit_transaction(
        &limited.session_id,
        &limited.token,
        &transaction.transaction_id,
        &id("function-trigger-commit"),
        &request,
        1_120,
        "request-function-trigger-denied",
        "operation-function-trigger-denied",
    );
    assert!(matches!(
        denied_trigger,
        Err(ServiceError::PermissionDenied)
    ));
    assert_eq!(engine.storage.runtime_cursor().unwrap(), before);

    let records = SecurityRepository::new(&engine.storage, instance())
        .audit_since(0, 64)
        .unwrap()
        .records;
    for (request_id, action) in [
        (
            "request-function-execute-denied",
            SecurityAction::FunctionExecute,
        ),
        (
            "request-function-write-denied",
            SecurityAction::FunctionCatalogueWrite,
        ),
        (
            "request-function-trigger-denied",
            SecurityAction::FunctionExecute,
        ),
    ] {
        assert!(records.iter().any(|(_, record)| {
            record.request_id == request_id
                && record.action == action
                && record.phase == AuditPhase::Completed
                && record.decision == AuditDecision::Denied
        }));
    }
}
