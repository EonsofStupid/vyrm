use serde_json::Value;

#[test]
fn checked_in_m6_edge_evidence_is_complete_and_inside_local_budgets() {
    let value: Value = serde_json::from_str(include_str!(
        "../../../docs/evidence/m6-edge-local-10000x128.json"
    ))
    .unwrap();
    assert_eq!(value["schema"], "vyrm-m6-edge-evidence-v1");
    assert_eq!(value["profile"]["network_policy"], "deny");
    assert_eq!(value["profile"]["memory_placement"], "mmap");
    let measurements = &value["measurements"];
    let budgets = &value["budgets"];
    assert!(
        measurements["release_binary_bytes"].as_u64().unwrap()
            <= budgets["release_binary_max_bytes"].as_u64().unwrap()
    );
    assert!(
        measurements["artifact_to_raw_ratio"].as_f64().unwrap()
            <= budgets["artifact_to_raw_max_ratio"].as_f64().unwrap()
    );
    assert!(
        measurements["fresh_query_process_peak_rss_kib"]
            .as_u64()
            .unwrap()
            <= budgets["fresh_query_peak_rss_max_kib"].as_u64().unwrap()
    );
    assert!(
        measurements["query_p95_ms"].as_f64().unwrap()
            <= budgets["query_p95_max_ms"].as_f64().unwrap()
    );
    assert_eq!(measurements["socket_messages_sent"], 0);
    assert_eq!(measurements["socket_messages_received"], 0);
    assert_eq!(
        value["dependency_audit"]["network_runtime_dependencies"]
            .as_array()
            .unwrap()
            .len(),
        budgets["network_runtime_dependencies_max"]
            .as_u64()
            .unwrap() as usize
    );
}
