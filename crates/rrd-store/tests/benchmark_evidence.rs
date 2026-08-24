fn assert_legacy_evidence(
    file: &str,
    expected_trials: u64,
    operations: u64,
    batch_size: u64,
    reads: u64,
    read_width: u64,
) {
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(evidence["format_version"], 1, "{file}");
    assert_eq!(evidence["config"]["trials"], expected_trials, "{file}");
    assert_eq!(evidence["config"]["operations"], operations, "{file}");
    assert_eq!(evidence["config"]["batch_size"], batch_size, "{file}");
    assert_eq!(evidence["config"]["reads"], reads, "{file}");
    assert_eq!(evidence["config"]["read_width"], read_width, "{file}");
    for backend in ["fjall", "native"] {
        assert_eq!(evidence[backend]["correctness_verified"], true, "{file}");
        assert_eq!(evidence[backend]["semantic_sequence"], operations, "{file}");
    }
    for trials in ["fjall_trials", "native_trials"] {
        assert_eq!(
            evidence[trials].as_array().unwrap().len(),
            expected_trials as usize,
            "{file}"
        );
    }
    for ratio in evidence["ratios"].as_object().unwrap().values() {
        if !ratio.is_null() {
            assert!(ratio.as_f64().unwrap().is_finite(), "{file}");
            assert!(ratio.as_f64().unwrap() > 0.0, "{file}");
        }
    }
}

#[test]
fn legacy_native_matrix_remains_structurally_valid_but_is_not_current_promotion_evidence() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../eval/results/");
    for (name, trials, operations, batch, reads, width) in [
        ("2026-08-19-vyrmkv-baseline.json", 5, 2_048, 64, 512, 32),
        (
            "2026-08-19-vyrmkv-small-batch.json",
            9,
            2_048,
            16,
            1_024,
            16,
        ),
        ("2026-08-19-vyrmkv-standard.json", 9, 2_048, 64, 1_024, 32),
        ("2026-08-19-vyrmkv-read-heavy.json", 9, 4_096, 64, 4_096, 64),
        (
            "2026-08-19-vyrmkv-sustained.json",
            9,
            16_384,
            128,
            2_048,
            64,
        ),
        ("2026-08-20-vyrmkv-extended.json", 3, 70_000, 128, 2_048, 64),
    ] {
        assert_legacy_evidence(
            &format!("{root}{name}"),
            trials,
            operations,
            batch,
            reads,
            width,
        );
    }
}

#[test]
fn checked_in_ai_read_matrix_is_structurally_valid_and_green() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../eval/results/");
    for (name, workload, payload, items_per_sample) in [
        (
            "2026-08-22-vyrmkv-ai-current-hot-hit.json",
            "current_hot_hit",
            "repeated_byte",
            1,
        ),
        (
            "2026-08-22-vyrmkv-ai-cold-hit.json",
            "cold_hit",
            "repeated_byte",
            1,
        ),
        (
            "2026-08-22-vyrmkv-ai-point-miss.json",
            "point_miss",
            "repeated_byte",
            1,
        ),
        (
            "2026-08-22-vyrmkv-ai-historical-hot-hit.json",
            "historical_hot_hit",
            "repeated_byte",
            1,
        ),
        (
            "2026-08-22-vyrmkv-ai-metadata-fanout.json",
            "metadata_fanout",
            "repeated_byte",
            32,
        ),
        (
            "2026-08-22-vyrmkv-ai-metadata-fanout-structured-json.json",
            "metadata_fanout",
            "structured_json",
            32,
        ),
        (
            "2026-08-22-vyrmkv-ai-metadata-fanout-deterministic-entropy.json",
            "metadata_fanout",
            "deterministic_entropy",
            32,
        ),
        (
            "2026-08-23-vyrmkv-ai-embedding-batch-v2.json",
            "metadata_fanout",
            "embedding_f32",
            32,
        ),
    ] {
        let file = format!("{root}{name}");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(evidence["format_version"], 3, "{file}");
        assert_eq!(evidence["workload"], workload, "{file}");
        assert_eq!(evidence["config"]["payload_profile"], payload, "{file}");
        assert_eq!(evidence["config"]["trials"], 5, "{file}");
        assert_eq!(evidence["config"]["cold_keys"], 8_192, "{file}");
        assert_eq!(evidence["config"]["hot_keys"], 128, "{file}");
        assert_eq!(evidence["config"]["reads"], 8_192, "{file}");
        assert_eq!(evidence["config"]["fanout_width"], 32, "{file}");
        for backend in ["fjall", "native"] {
            assert_eq!(evidence[backend]["correctness_verified"], true, "{file}");
            assert_eq!(
                evidence[backend]["items_per_sample"], items_per_sample,
                "{file}"
            );
            for state in ["active", "reopened", "maintained"] {
                assert!(
                    evidence[backend]["footprint"][state]["allocated_bytes"]
                        .as_u64()
                        .unwrap()
                        > 0,
                    "{file}: {backend}/{state}"
                );
            }
        }
        for trials in ["fjall_trials", "native_trials"] {
            assert_eq!(evidence[trials].as_array().unwrap().len(), 5, "{file}");
        }
        assert_eq!(evidence["promotion"]["passes"], true, "{file}");
        assert_eq!(
            evidence["footprint_comparison"]["promotion_state"],
            "clean_reopen_without_explicit_maintenance",
            "{file}"
        );
        assert_eq!(
            evidence["footprint_comparison"]["maintained_cross_backend_comparable"], false,
            "{file}"
        );
        assert!(
            evidence["native_to_fjall_read_throughput"]
                .as_f64()
                .unwrap()
                >= 1.0,
            "{file}"
        );
        assert!(
            evidence["native_to_fjall_p95_latency"].as_f64().unwrap() <= 1.0,
            "{file}"
        );
        assert!(
            evidence["footprint_comparison"]["native_to_fjall_reopened_allocated"]
                .as_f64()
                .unwrap()
                <= 1.0,
            "{file}"
        );
        assert!(
            evidence["fjall"]["footprint"]["active"]["apparent_bytes"]
                .as_u64()
                .unwrap()
                > evidence["fjall"]["footprint"]["active"]["allocated_bytes"]
                    .as_u64()
                    .unwrap(),
            "{file}"
        );
    }
}

#[test]
fn corrected_standard_evidence_records_a_bounded_strict_promotion() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/results/2026-08-23-vyrmkv-standard-streaming-scan-v4.json"
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(evidence["format_version"], 4);
    assert!(evidence["verification_contract"]
        .as_str()
        .unwrap()
        .contains("complete corpus is verified in read-width pages"));
    for backend in ["fjall", "native"] {
        assert_eq!(evidence[backend]["correctness_verified"], true);
        for state in ["active", "reopened", "maintained"] {
            assert!(
                evidence[backend]["footprint"][state]["allocated_bytes"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
    }
    assert_eq!(evidence["promotion"]["passes"], true);
    assert_eq!(
        evidence["promotion"]["failures"].as_array().unwrap().len(),
        0
    );
    for ratio in [
        "native_to_fjall_write_throughput",
        "native_to_fjall_read_throughput",
    ] {
        assert!(evidence["ratios"][ratio].as_f64().unwrap() > 1.0);
    }
    for ratio in [
        "native_to_fjall_write_p95",
        "native_to_fjall_read_p95",
        "native_to_fjall_recovery",
        "native_to_fjall_peak_rss",
        "native_to_fjall_reopened_allocated",
    ] {
        assert!(evidence["ratios"][ratio].as_f64().unwrap() < 1.0);
    }
    let profile = &evidence["native"]["native_maintenance"];
    assert_eq!(profile["memtable_version_record_bytes"], 24);
    assert_eq!(profile["memtable_spilled_chains"], 1);
    assert!(
        profile["memtable_owned_bytes_lower_bound"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn streaming_scan_scale_evidence_retains_the_one_extended_rss_failure() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../eval/results/");
    for (name, operations, maximum_rss_ratio, should_pass) in [
        (
            "2026-08-23-vyrmkv-read-heavy-streaming-scan-v4.json",
            4_096,
            1.0,
            true,
        ),
        (
            "2026-08-23-vyrmkv-sustained-streaming-scan-v4.json",
            16_384,
            1.0,
            true,
        ),
        (
            "2026-08-23-vyrmkv-extended-streaming-scan-v4.json",
            70_000,
            1.03,
            false,
        ),
    ] {
        let file = format!("{root}{name}");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(evidence["format_version"], 4, "{file}");
        assert_eq!(evidence["config"]["trials"], 9, "{file}");
        assert_eq!(evidence["config"]["operations"], operations, "{file}");
        assert_eq!(evidence["promotion"]["passes"], should_pass, "{file}");
        for backend in ["fjall", "native"] {
            assert_eq!(evidence[backend]["correctness_verified"], true, "{file}");
        }
        let profile = &evidence["native"]["native_maintenance"];
        assert_eq!(profile["memtable_spilled_chains"], 1, "{file}");
        assert_eq!(profile["memtable_version_record_bytes"], 24, "{file}");
        assert!(
            profile["memtable_keys"].as_u64().unwrap()
                < profile["memtable_versions"].as_u64().unwrap(),
            "{file}"
        );
        assert!(
            profile["memtable_owned_bytes_lower_bound"]
                .as_u64()
                .unwrap()
                > profile["memtable_value_payload_bytes"].as_u64().unwrap(),
            "{file}"
        );
        let rss = evidence["ratios"]["native_to_fjall_peak_rss"]
            .as_f64()
            .unwrap();
        assert_eq!(rss <= 1.0, should_pass, "{file}");
        assert!(rss <= maximum_rss_ratio, "{file}");
        assert!(
            evidence["ratios"]["native_to_fjall_write_p95"]
                .as_f64()
                .unwrap()
                <= 1.0,
            "{file}"
        );
        let allocated = evidence["ratios"]["native_to_fjall_reopened_allocated"]
            .as_f64()
            .unwrap();
        assert!(allocated <= 1.0, "{file}");
    }
}

#[test]
fn compact_keyspace_format_closes_extended_rss_and_retains_read_heavy_tail_failure() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../eval/results/");
    for (name, operations, should_pass) in [
        (
            "2026-08-23-vyrmkv-standard-keyspace-tag-v2-v4.json",
            2_048,
            true,
        ),
        (
            "2026-08-23-vyrmkv-sustained-keyspace-tag-v2-v4.json",
            16_384,
            true,
        ),
        (
            "2026-08-23-vyrmkv-extended-keyspace-tag-v2-v4.json",
            70_000,
            true,
        ),
        (
            "2026-08-23-vyrmkv-read-heavy-direct-tag-v2-v4.json",
            4_096,
            false,
        ),
    ] {
        let file = format!("{root}{name}");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(evidence["format_version"], 4, "{file}");
        assert_eq!(evidence["config"]["trials"], 9, "{file}");
        assert_eq!(evidence["config"]["operations"], operations, "{file}");
        assert_eq!(evidence["promotion"]["passes"], should_pass, "{file}");
        assert_eq!(evidence["fjall"]["correctness_verified"], true, "{file}");
        assert_eq!(evidence["native"]["correctness_verified"], true, "{file}");
        assert!(
            evidence["ratios"]["native_to_fjall_peak_rss"]
                .as_f64()
                .unwrap()
                < 1.0,
            "{file}"
        );
        assert!(
            evidence["ratios"]["native_to_fjall_reopened_allocated"]
                .as_f64()
                .unwrap()
                < 1.0,
            "{file}"
        );
    }

    let compact: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!(
            "{root}2026-08-23-vyrmkv-extended-keyspace-tag-v2-v4.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let textual: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!(
            "{root}2026-08-23-vyrmkv-extended-streaming-scan-v4.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        textual["native"]["native_maintenance"]["memtable_key_payload_bytes"]
            .as_u64()
            .unwrap()
            - compact["native"]["native_maintenance"]["memtable_key_payload_bytes"]
                .as_u64()
                .unwrap(),
        1_400_004
    );
    let read_heavy: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!(
            "{root}2026-08-23-vyrmkv-read-heavy-direct-tag-v2-v4.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        read_heavy["promotion"]["failures"],
        serde_json::json!(["native write p95 exceeds Fjall"])
    );
}

#[test]
fn surrealdb_claim_diagnostic_records_bounded_wins_and_the_disk_loss() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/results/2026-08-23-vyrm-surrealdb-3.0.5-claim-diagnostic-v1.json"
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(evidence["schema"], "vyrm.surrealdb-claim-differential.v1");
    assert_eq!(evidence["config"]["trials"], 3);
    assert_eq!(evidence["config"]["operations"], 2_048);
    assert_eq!(
        evidence["surrealdb"]["version"],
        "3.0.5 for linux on x86_64"
    );
    assert_eq!(
        evidence["surrealdb"]["binary_sha256"],
        "a9a5e9e36e4f6fe922e1991a4fb0ea1ee4fe90819c5e3a8dce238a56666e8cec"
    );
    assert_eq!(evidence["native"]["correctness_verified"], true);
    assert_eq!(evidence["surreal"]["correctness_verified"], true);
    for ratio in [
        "vyrm_to_surreal_write_throughput",
        "vyrm_to_surreal_read_throughput",
        "vyrm_to_surreal_server_write_throughput",
        "vyrm_to_surreal_server_read_throughput",
    ] {
        assert!(evidence["ratios"][ratio].as_f64().unwrap() > 1.0, "{ratio}");
    }
    for ratio in [
        "vyrm_to_surreal_write_p95",
        "vyrm_to_surreal_read_p95",
        "vyrm_to_surreal_server_write_p95",
        "vyrm_to_surreal_server_read_p95",
        "vyrm_to_surreal_recovery",
        "vyrm_to_surreal_peak_rss",
    ] {
        assert!(evidence["ratios"][ratio].as_f64().unwrap() < 1.0, "{ratio}");
    }
    assert!(
        evidence["ratios"]["vyrm_to_surreal_reopened_allocated"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    assert_eq!(
        evidence["bounded_verdict"]["all_measured_cells_favor_vyrm"],
        false
    );
    assert_eq!(
        evidence["bounded_verdict"]["general_database_superiority"],
        false
    );
}
