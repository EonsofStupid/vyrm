use connectome_ui::{ConnectomeBackend, ConnectomeConfig, InvokeToolRequest, QueryRequest};
use rrd_contract::{
    CanonicalId, QueryBudget, ResourceId, ResourceKind, ResourcePath, WorkGateDefinition,
    WorkItemDefinition, WorkPlanDefinition,
};
use rrd_core::{
    digest, RuntimeCommit, RuntimeMutation, RuntimeProperties, RuntimePropertySchema,
    RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeSchemaRegistry, RuntimeType,
    RuntimeValue, RuntimeValueType, ScopeId,
};
use rrd_engine::{
    install_work_plan, load_or_create_token_key, InstanceBinding, InstanceManifest, RrdEngine,
    WorkPlanOperation,
};
use rrd_security::{
    Action, Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState,
    SECURITY_FORMAT,
};
use rrd_server::RrdHttpServer;
use rrd_store::{Engine, PersistentEngine};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

struct RunningRrd {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for RunningRrd {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn start_rrd() -> (tempfile::TempDir, RunningRrd) {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    InstanceManifest::ensure_dedicated_as(&project, "connectome-test").unwrap();
    let binding = InstanceBinding::discover(&project).unwrap();
    let database = binding.expected_store();
    let storage = PersistentEngine::open(&database).unwrap();

    let mut registry = RuntimeSchemaRegistry::empty(1, "Connectome client fixture");
    registry.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "title".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRecordSchema::default()
        },
    );
    storage
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:connectome-test").unwrap(),
            at: 100,
            actor: "connectome-fixture".into(),
            expected_cursor: 0,
            mutations: vec![
                RuntimeMutation::Schema { registry },
                RuntimeMutation::Record {
                    record: RuntimeRecord {
                        reference: RuntimeRef::new("document", "alpha").unwrap(),
                        valid_from: 100,
                        valid_to: None,
                        properties: RuntimeProperties::from([(
                            "title".into(),
                            RuntimeValue::String("Alpha".into()),
                        )]),
                    },
                },
            ],
        })
        .unwrap();
    install_work_plan(
        &storage,
        WorkPlanDefinition {
            schema_version: 1,
            plan_id: "connectome-plan".into(),
            title: "Connectome generated work-plan contract".into(),
            authority: "RRD".into(),
            status_policy: "Evidence only".into(),
            gate: vec![WorkGateDefinition {
                id: "G00".into(),
                title: "Control".into(),
                depends_on: vec![],
            }],
            item: vec![WorkItemDefinition {
                id: "G00-W05".into(),
                gate: "G00".into(),
                title: "Generated surfaces".into(),
                depends_on: vec![],
                acceptance: vec!["Connectome consumes RRD discovery".into()],
            }],
        },
        101,
        "connectome-fixture",
        "connectome-plan-install",
    )
    .unwrap();

    let instance = CanonicalId::new("connectome-test").unwrap();
    let resource = ResourcePath {
        segments: vec![ResourceId::new(ResourceKind::Instance, instance.as_str()).unwrap()],
    };
    let principal = Principal {
        id: CanonicalId::new("connectome-client").unwrap(),
        kind: PrincipalKind::Service,
        credential_sha256: digest::sha256_hex(b"connectome-api-key"),
        credential_revision: 1,
        not_before_unix_ms: 1,
        expires_at_unix_ms: u64::MAX,
        disabled: false,
        role_ids: Default::default(),
        grants: [
            Action::SessionCreate,
            Action::DiagnosticsRead,
            Action::RuntimeToolCatalogueRead,
            Action::ServiceInspect,
            Action::QueryExecute,
            Action::WorkPlanRead,
        ]
        .into_iter()
        .map(|action| ResourceGrant {
            action,
            resource_prefix: resource.clone(),
            data_policy: None,
        })
        .collect(),
    };
    SecurityRepository::new(&storage, instance.clone())
        .initialize(
            SecurityState {
                format_version: SECURITY_FORMAT,
                revision: 1,
                principals: BTreeMap::from([(principal.id.clone(), principal)]),
                roles: BTreeMap::new(),
                identity_bindings: BTreeMap::new(),
                jwt_issuers: BTreeMap::new(),
            },
            1,
            "bootstrap",
            "request-bootstrap",
            "operation-bootstrap",
        )
        .unwrap();
    drop(storage);

    let token_key = load_or_create_token_key(&database.join("RRD.SERVER.SECRET")).unwrap();
    let engine = RrdEngine::open_bound_with_token_key(&binding, instance, token_key, 2).unwrap();
    let authority = binding.authority_binding().unwrap();
    let server =
        RrdHttpServer::bind_project(engine, authority, "127.0.0.1:0".parse().unwrap()).unwrap();
    let address = server.local_addr();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime
            .block_on(server.serve_until(async move {
                while !stop_for_thread.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }))
            .unwrap();
    });
    (
        temporary,
        RunningRrd {
            address,
            stop,
            thread: Some(thread),
        },
    )
}

#[test]
fn connectome_uses_authenticated_public_rrd_contracts_end_to_end() {
    let (_temporary, rrd) = start_rrd();
    let backend = ConnectomeBackend::connect(&ConnectomeConfig {
        rrd_address: rrd.address,
        instance: CanonicalId::new("connectome-test").unwrap(),
        principal: CanonicalId::new("connectome-client").unwrap(),
        api_key: "connectome-api-key".into(),
        scope: "instance:connectome-test".into(),
        bind: "127.0.0.1:4387".parse().unwrap(),
        shutdown: None,
    })
    .unwrap();

    let service = backend.service_capabilities().unwrap();
    assert_eq!(service.instance.id.as_str(), "connectome-test");
    assert_eq!(
        service.product_capabilities,
        rrd_engine::product_capability_catalogue()
    );
    let tools = backend.runtime_tool_catalogue().unwrap();
    for operation in WorkPlanOperation::ALL {
        assert!(
            tools
                .tools
                .iter()
                .any(|tool| tool.name.as_str() == operation.runtime_tool_name()),
            "Connectome omitted generated work-plan operation {:?}",
            operation
        );
    }
    let work_plan_status = backend
        .invoke_runtime_tool(InvokeToolRequest {
            tool: CanonicalId::new(WorkPlanOperation::Status.runtime_tool_name()).unwrap(),
            arguments: json!({"plan_id":"connectome-plan"}),
        })
        .unwrap();
    let work_plan: serde_json::Value = serde_json::from_str(&work_plan_status.content).unwrap();
    assert_eq!(work_plan["plan_id"], "connectome-plan");
    assert_eq!(work_plan["items"][0]["status"], "pending");
    let snapshot = backend.diagnostic_snapshot().unwrap();
    assert_eq!(snapshot.scope, "instance:connectome-test");
    assert_eq!(snapshot.read.runtime_cursor, 2);
    assert_eq!(snapshot.graph.records.len(), 1);
    assert_eq!(snapshot.graph.records[0].reference.id, "alpha");
    assert!(snapshot
        .models
        .models
        .iter()
        .any(|model| model.id.as_str() == "document"));
    snapshot.validate().unwrap();

    let query = backend
        .execute_query(QueryRequest {
            query:
                "FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title EXPLAIN CONTRACT"
                    .into(),
            parameters: BTreeMap::new(),
            budget: QueryBudget {
                max_scanned_changes: 100,
                max_rows: 10,
                max_output_bytes: 4_096,
                max_batch_rows: 10,
                ..QueryBudget::default()
            },
        })
        .unwrap();
    assert_eq!(query.rows.len(), 1);
    assert_eq!(query.rows[0].identity, "record:document:alpha");

    let status = backend
        .invoke_runtime_tool(InvokeToolRequest {
            tool: CanonicalId::new("rrflow_service_status").unwrap(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(status.tool.as_str(), "rrflow_service_status");
    assert!(status.content.contains("connectome-test"));
}
