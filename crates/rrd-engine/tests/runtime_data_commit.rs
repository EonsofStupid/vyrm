use rrd_engine::{
    InstanceBinding, InstanceManifest, RrdEngine, RuntimeToolAttunement, ServiceError,
};
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
        json!({"run_id":"mcp-data-run","actor":"agent:test","at":at,"payload":payload}),
        at,
    );
}

fn authorize(engine: &RrdEngine, root: &std::path::Path, tool_name: &str, args: &Value, at: u64) {
    let result = call(
        engine,
        root,
        "rrflow_hook",
        json!({
            "event":"pre-tool-use",
            "at":at,
            "input":{"tool_name":tool_name,"tool_input":args}
        }),
        at,
    );
    assert_eq!(result, json!({"text":""}));
}

fn multi_model_commit() -> Value {
    json!({
        "idempotency_key":"mcp-data-commit-1",
        "at":1000,
        "timeout_ms":60000,
        "mutations":[
            {
                "mutation":"put_schema",
                "registry":{
                    "revision":1,
                    "migration":"bootstrap MCP multi-model fixture",
                    "records":{
                        "document":{
                            "properties":{"title":{"value_type":"string","required":true}},
                            "allow_additional_properties":false,
                            "unique_properties":[]
                        },
                        "series":{
                            "properties":{},
                            "allow_additional_properties":true,
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
                    "events":{
                        "observed":{
                            "subject_required":true,
                            "subject_types":["document"],
                            "properties":{},
                            "allow_additional_properties":true
                        }
                    }
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
                "properties":{"reason":{"type":"string","value":"fixture"}}
            },
            {
                "mutation":"append_event",
                "kind":"observed",
                "subject":{"kind":"document","id":"alpha"},
                "properties":{"source":{"type":"string","value":"mcp"}}
            },
            {
                "mutation":"put_vector",
                "reference":{"kind":"embedding","id":"alpha-title"},
                "subject":{"kind":"document","id":"alpha"},
                "field":"title-embedding",
                "valid_from":100,
                "value":{"kind":"dense","values":[0.6,0.8]},
                "properties":{}
            },
            {
                "mutation":"put_record",
                "reference":{"kind":"series","id":"latency"},
                "valid_from":100,
                "properties":{}
            },
            {
                "mutation":"append_series_sample",
                "reference":{"kind":"sample","id":"latency-100"},
                "series":{"kind":"series","id":"latency"},
                "observed_at":100,
                "value":{"type":"unsigned","value":17},
                "properties":{}
            },
            {
                "mutation":"put_geo",
                "reference":{"kind":"location","id":"alpha-office"},
                "subject":{"kind":"document","id":"alpha"},
                "field":"office",
                "valid_from":100,
                "value":{"kind":"point","point":{"longitude":-73.0,"latitude":40.0}},
                "properties":{}
            }
        ]
    })
}

#[test]
fn data_commit_is_exactly_authorized_multi_model_idempotent_and_reopen_safe() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    InstanceManifest::ensure_dedicated(root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn fixture() {}\n").unwrap();
    let binding = InstanceBinding::discover(root).unwrap();
    let engine = RrdEngine::open_bound(&binding).unwrap();
    let arguments = multi_model_commit();

    call(
        &engine,
        root,
        "rrflow_preflight",
        json!({"at":900,"budget":1000}),
        900,
    );
    record(
        &engine,
        root,
        json!({"kind":"goal","statement":"commit one multi-model transaction","acceptance":["durable and queryable"]}),
        901,
    );
    record(
        &engine,
        root,
        json!({"kind":"plan","hypothesis":"the shared RRD transaction preserves every model","steps":["commit","query","reopen"]}),
        902,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"commit exact mutation set","actions":["rrflow_data_commit"]}),
        903,
    );

    let denied = engine.call_runtime_tool(root, "rrflow_data_commit", &arguments, 904);
    assert!(matches!(denied, Err(ServiceError::Runtime(_))));

    authorize(&engine, root, "rrflow_data_commit", &arguments, 905);
    let first = call(&engine, root, "rrflow_data_commit", arguments.clone(), 906);
    assert_eq!(first["mutation_count"], 9);
    assert_eq!(first["idempotent_replay"], false);
    assert!(first["runtime_commit_sha256"].as_str().is_some());

    let scope = format!("instance:{}", binding.manifest.id);
    for (query, expected) in [
        (
            "FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title",
            2,
        ),
        (
            "FROM relation:linked AT VALID 100 KNOWN HEAD PROJECT id, from_id, to_id",
            1,
        ),
        (
            "FROM series:series AT VALID 100 KNOWN HEAD PROJECT observed_at, value",
            1,
        ),
        (
            "FROM geo:location AT VALID 100 KNOWN HEAD PROJECT subject_id, longitude, latitude",
            1,
        ),
    ] {
        let result = call(
            &engine,
            root,
            "rrflow_query",
            json!({"scope":scope,"ql":query,"at":907}),
            907,
        );
        assert_eq!(result["execution"]["returned_rows"], expected);
    }

    record(
        &engine,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"prove durable replay after reopen"}),
        908,
    );
    record(
        &engine,
        root,
        json!({"kind":"attempt","summary":"replay exact committed request","actions":["reopen","rrflow_data_commit"]}),
        909,
    );
    authorize(&engine, root, "rrflow_data_commit", &arguments, 910);
    drop(engine);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let replay = call(&reopened, root, "rrflow_data_commit", arguments, 911);
    assert_eq!(replay["mutation_count"], 9);
    assert_eq!(replay["idempotent_replay"], true);

    record(
        &reopened,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"build an index through the same authority"}),
        912,
    );
    record(
        &reopened,
        root,
        json!({"kind":"attempt","summary":"ensure the document title BM25 index","actions":["rrflow_query_index_ensure"]}),
        913,
    );
    let index_arguments = json!({
        "idempotency_key":"mcp-index-ensure-1",
        "scope":scope,
        "index_id":"document-title-bm25",
        "definition_query":"FROM record:document AT VALID 100 KNOWN HEAD PROJECT title",
        "unique":false,
        "kind":"bm25"
    });
    authorize(
        &reopened,
        root,
        "rrflow_query_index_ensure",
        &index_arguments,
        914,
    );
    let ensured = call(
        &reopened,
        root,
        "rrflow_query_index_ensure",
        index_arguments.clone(),
        915,
    );
    assert_eq!(ensured["index"]["index_id"], "document-title-bm25");
    assert_eq!(ensured["index"]["kind"], "bm25");
    assert_eq!(ensured["index"]["state"], "ready");
    assert_eq!(ensured["index"]["artifact_rows"], 2);
    assert_eq!(ensured["idempotent_replay"], false);

    let bm25 = call(
        &reopened,
        root,
        "rrflow_query",
        json!({
            "scope":scope,
            "ql":"FROM record:document AT VALID 100 KNOWN HEAD WHERE title MATCH \"Alpha\" PROJECT id, title",
            "at":915
        }),
        915,
    );
    assert_eq!(bm25["execution"]["returned_rows"], 1);
    assert_eq!(
        bm25["execution"]["batches"][0]["rows"][0]["identity"],
        "record:document:alpha"
    );
    assert_eq!(
        bm25["plan"]["operators"][0]["operator"], "materialized_index",
        "{bm25:#}"
    );

    record(
        &reopened,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"prove index ensure replay after lease expiry and reopen"}),
        916,
    );
    record(
        &reopened,
        root,
        json!({"kind":"attempt","summary":"replay the exact index ensure","actions":["reopen","rrflow_query_index_ensure"]}),
        917,
    );
    authorize(
        &reopened,
        root,
        "rrflow_query_index_ensure",
        &index_arguments,
        3_700_000,
    );
    drop(reopened);
    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let replayed_index = call(
        &reopened,
        root,
        "rrflow_query_index_ensure",
        index_arguments,
        3_700_001,
    );
    assert_eq!(replayed_index["idempotent_replay"], true);
    assert_eq!(replayed_index["index"]["artifact_rows"], 2);
    assert_eq!(replayed_index["index"]["kind"], "bm25");

    let reopened_bm25 = call(
        &reopened,
        root,
        "rrflow_query",
        json!({
            "scope":scope,
            "ql":"FROM record:document AT VALID 100 KNOWN HEAD WHERE title MATCH \"Beta\" PROJECT id, title",
            "at":3_700_002
        }),
        3_700_002,
    );
    assert_eq!(reopened_bm25["execution"]["returned_rows"], 1);
    assert_eq!(
        reopened_bm25["execution"]["batches"][0]["rows"][0]["identity"],
        "record:document:beta"
    );

    let listed = call(
        &reopened,
        root,
        "rrflow_query_index_list",
        json!({"scope":scope}),
        3_700_002,
    );
    assert_eq!(listed["indexes"].as_array().unwrap().len(), 1);
    assert_eq!(listed["indexes"][0]["index_id"], "document-title-bm25");
    assert_eq!(listed["indexes"][0]["state"], "ready");

    let live = call(
        &reopened,
        root,
        "rrflow_live_query_poll",
        json!({
            "scope":scope,
            "query":"FROM record:document AT VALID 100 KNOWN HEAD PROJECT id, title",
            "after_cursor":0,
            "max_delta_rows":10,
            "wait_timeout_ms":0
        }),
        3_700_003,
    );
    assert_eq!(live["from_cursor"], 0);
    assert_eq!(live["timed_out"], false);
    assert_eq!(live["added"].as_array().unwrap().len(), 2);
    assert!(live["through_cursor"].as_u64().unwrap() >= 9);

    let changes = call(
        &reopened,
        root,
        "rrflow_changefeed_read",
        json!({"scope":scope,"after_cursor":0,"limit":100}),
        3_700_004,
    );
    assert_eq!(changes["requested_after_cursor"], 0);
    let data_commit_sha256 = first["runtime_commit_sha256"].as_str().unwrap();
    let data_changes = changes["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|change| change["commit_sha256"] == data_commit_sha256)
        .count();
    assert_eq!(data_changes, 9);
    assert!(changes["changes"].as_array().unwrap().len() > data_changes);
    assert_eq!(changes["validation"]["method"], "bounded_hash_chain_page");
    let head = changes["head_cursor"].as_u64().unwrap();
    let followed = call(
        &reopened,
        root,
        "rrflow_changefeed_follow",
        json!({
            "read":{"scope":scope,"after_cursor":head,"limit":10},
            "wait_timeout_ms":1
        }),
        3_700_005,
    );
    assert_eq!(followed["timed_out"], true);
    assert_eq!(followed["page"]["changes"].as_array().unwrap().len(), 0);
    assert_eq!(followed["page"]["head_cursor"], head);

    record(
        &reopened,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"establish the governed vector collection"}),
        3_700_006,
    );
    record(
        &reopened,
        root,
        json!({"kind":"attempt","summary":"ensure the documents vector collection","actions":["rrflow_vector_collection_ensure"]}),
        3_700_007,
    );
    let collection_arguments = json!({
        "idempotency_key":"mcp-vector-collection-1",
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
    authorize(
        &reopened,
        root,
        "rrflow_vector_collection_ensure",
        &collection_arguments,
        3_700_008,
    );
    let collection = call(
        &reopened,
        root,
        "rrflow_vector_collection_ensure",
        collection_arguments.clone(),
        3_700_009,
    );
    assert_eq!(collection["collection"]["collection_id"], "documents");
    assert_eq!(collection["collection"]["vectors"][0]["name"], "title");
    assert_eq!(collection["idempotent_replay"], false);

    record(
        &reopened,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"prove vector catalogue replay after expiry"}),
        3_700_010,
    );
    record(
        &reopened,
        root,
        json!({"kind":"attempt","summary":"replay vector collection ensure","actions":["reopen","rrflow_vector_collection_ensure"]}),
        3_700_011,
    );
    authorize(
        &reopened,
        root,
        "rrflow_vector_collection_ensure",
        &collection_arguments,
        7_400_000,
    );
    drop(reopened);
    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let replayed_collection = call(
        &reopened,
        root,
        "rrflow_vector_collection_ensure",
        collection_arguments,
        7_400_001,
    );
    assert_eq!(replayed_collection["idempotent_replay"], true);
    let collections = call(
        &reopened,
        root,
        "rrflow_vector_collection_list",
        json!({"scope":scope}),
        7_400_002,
    );
    assert_eq!(collections["collections"].as_array().unwrap().len(), 1);
    assert_eq!(collections["collections"][0]["collection_id"], "documents");

    record(
        &reopened,
        root,
        json!({"kind":"decision","decision":"continue","rationale":"persist and isolate one collection-addressed point"}),
        7_400_003,
    );
    record(
        &reopened,
        root,
        json!({"kind":"attempt","summary":"commit a documents/title vector point","actions":["rrflow_data_commit","reopen","retrieve","scroll","search"]}),
        7_400_004,
    );
    let point_arguments = json!({
        "idempotency_key":"mcp-vector-point-1",
        "at":7_400_005,
        "mutations":[{
            "mutation":"put_vector",
            "reference":{"kind":"embedding","id":"alpha-title-bound"},
            "subject":{"kind":"document","id":"alpha"},
            "collection_id":"documents",
            "vector_name":"title",
            "field":"title-embedding",
            "valid_from":100,
            "value":{"kind":"dense","values":[1.0,0.0]},
            "properties":{"tenant":{"type":"string","value":"alpha"}}
        }]
    });
    authorize(
        &reopened,
        root,
        "rrflow_data_commit",
        &point_arguments,
        7_400_005,
    );
    let point_commit = call(
        &reopened,
        root,
        "rrflow_data_commit",
        point_arguments,
        7_400_006,
    );
    assert_eq!(point_commit["mutation_count"], 1);
    drop(reopened);

    let reopened = RrdEngine::open_bound(&binding).unwrap();
    let retrieved = call(
        &reopened,
        root,
        "rrflow_vector_points_retrieve",
        json!({
            "scope":scope,
            "collection_id":"documents",
            "vector_name":"title",
            "valid_at":100,
            "references":[
                {"kind":"embedding","id":"alpha-title-bound"},
                {"kind":"embedding","id":"alpha-title"}
            ],
            "max_scanned_changes":10000
        }),
        7_400_007,
    );
    assert_eq!(retrieved["points"].as_array().unwrap().len(), 1);
    assert_eq!(
        retrieved["points"][0]["reference"]["id"],
        "alpha-title-bound"
    );
    assert_eq!(retrieved["missing"].as_array().unwrap().len(), 1);
    assert_eq!(retrieved["missing"][0]["id"], "alpha-title");

    let scrolled = call(
        &reopened,
        root,
        "rrflow_vector_points_scroll",
        json!({
            "scope":scope,
            "collection_id":"documents",
            "vector_name":"title",
            "valid_at":100,
            "limit":10,
            "max_scanned_changes":10000
        }),
        7_400_008,
    );
    assert_eq!(scrolled["points"].as_array().unwrap().len(), 1);
    assert_eq!(
        scrolled["points"][0]["reference"]["id"],
        "alpha-title-bound"
    );
    assert_eq!(scrolled["truncated"], false);

    let searched = call(
        &reopened,
        root,
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
        7_400_009,
    );
    assert_eq!(searched["hits"].as_array().unwrap().len(), 1);
    assert_eq!(searched["hits"][0]["reference"]["id"], "alpha-title-bound");
    assert_eq!(searched["access_path"], "exact_scan");
    assert_eq!(searched["exact"], true);
}

#[test]
fn data_commit_contract_is_typed_and_maps_to_the_existing_transaction_capability() {
    let definition = rrd_engine::runtime_tool_catalogue()
        .into_iter()
        .find(|definition| definition.name == "rrflow_data_commit")
        .unwrap();
    assert_eq!(definition.capability_id, Some("transaction-commit"));
    assert_eq!(definition.attunement, RuntimeToolAttunement::ExactTool);
    let required = definition.input_schema["required"].as_array().unwrap();
    assert!(required.iter().any(|field| field == "idempotency_key"));
    assert!(required.iter().any(|field| field == "mutations"));

    let index = rrd_engine::runtime_tool_catalogue()
        .into_iter()
        .find(|definition| definition.name == "rrflow_query_index_ensure")
        .unwrap();
    assert_eq!(index.capability_id, Some("query-index-ensure"));
    assert_eq!(index.attunement, RuntimeToolAttunement::ExactTool);
    let required = index.input_schema["required"].as_array().unwrap();
    for field in ["idempotency_key", "scope", "index_id", "definition_query"] {
        assert!(required.iter().any(|required| required == field));
    }

    let list = rrd_engine::runtime_tool_catalogue()
        .into_iter()
        .find(|definition| definition.name == "rrflow_query_index_list")
        .unwrap();
    assert_eq!(list.capability_id, Some("query-index-list"));
    assert_eq!(list.attunement, RuntimeToolAttunement::None);
    assert!(!list.mutation);
    let required = list.input_schema["required"].as_array().unwrap();
    assert!(required.iter().any(|field| field == "scope"));

    let live = rrd_engine::runtime_tool_catalogue()
        .into_iter()
        .find(|definition| definition.name == "rrflow_live_query_poll")
        .unwrap();
    assert_eq!(live.capability_id, Some("query-live-poll"));
    assert_eq!(live.attunement, RuntimeToolAttunement::None);
    assert!(!live.mutation);
    let required = live.input_schema["required"].as_array().unwrap();
    for field in ["scope", "query", "after_cursor", "max_delta_rows"] {
        assert!(required.iter().any(|required| required == field));
    }

    for (name, capability, required_fields) in [
        (
            "rrflow_changefeed_read",
            "changefeed-read",
            &["scope", "after_cursor", "limit"][..],
        ),
        (
            "rrflow_changefeed_follow",
            "changefeed-follow",
            &["read", "wait_timeout_ms"][..],
        ),
    ] {
        let definition = rrd_engine::runtime_tool_catalogue()
            .into_iter()
            .find(|definition| definition.name == name)
            .unwrap();
        assert_eq!(definition.capability_id, Some(capability));
        assert_eq!(definition.attunement, RuntimeToolAttunement::None);
        assert!(!definition.mutation);
        let required = definition.input_schema["required"].as_array().unwrap();
        for field in required_fields {
            assert!(required.iter().any(|required| required == field));
        }
    }

    for (name, capability, attunement, mutation, required_fields) in [
        (
            "rrflow_vector_collection_ensure",
            "vector-collection-ensure",
            RuntimeToolAttunement::ExactTool,
            true,
            &["idempotency_key", "scope", "collection_id", "vectors"][..],
        ),
        (
            "rrflow_vector_collection_list",
            "vector-collection-list",
            RuntimeToolAttunement::None,
            false,
            &["scope"][..],
        ),
        (
            "rrflow_vector_points_retrieve",
            "vector-point-retrieve",
            RuntimeToolAttunement::None,
            false,
            &[
                "scope",
                "collection_id",
                "vector_name",
                "valid_at",
                "references",
                "max_scanned_changes",
            ][..],
        ),
        (
            "rrflow_vector_points_scroll",
            "vector-point-scroll",
            RuntimeToolAttunement::None,
            false,
            &[
                "scope",
                "collection_id",
                "vector_name",
                "valid_at",
                "limit",
                "max_scanned_changes",
            ][..],
        ),
        (
            "rrflow_vector_search",
            "vector-search",
            RuntimeToolAttunement::None,
            false,
            &["scope", "valid_at", "query", "top_k", "max_scanned_changes"][..],
        ),
    ] {
        let definition = rrd_engine::runtime_tool_catalogue()
            .into_iter()
            .find(|definition| definition.name == name)
            .unwrap();
        assert_eq!(definition.capability_id, Some(capability));
        assert_eq!(definition.attunement, attunement);
        assert_eq!(definition.mutation, mutation);
        let required = definition.input_schema["required"].as_array().unwrap();
        for field in required_fields {
            assert!(required.iter().any(|required| required == field));
        }
    }
}
