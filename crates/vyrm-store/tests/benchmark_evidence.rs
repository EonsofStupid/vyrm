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
            "2026-08-22-vyrmkv-ai-metadata-fanout-embedding-f32.json",
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
fn corrected_standard_evidence_records_bounded_scan_win_and_remaining_write_memory_gap() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/results/2026-08-22-vyrmkv-corrected-standard.json"
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(evidence["format_version"], 2);
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
    assert_eq!(evidence["promotion"]["passes"], false);
    let failures = evidence["promotion"]["failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|failure| failure.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(failures.contains("write throughput"));
    assert!(failures.contains("write p95"));
    assert!(failures.contains("peak RSS"));
    assert!(failures.contains("clean-reopen allocated footprint"));
    assert!(
        evidence["ratios"]["native_to_fjall_read_throughput"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    assert!(
        evidence["ratios"]["native_to_fjall_read_p95"]
            .as_f64()
            .unwrap()
            < 1.0
    );
    assert!(
        evidence["ratios"]["native_to_fjall_maintained_read_throughput"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    assert!(
        evidence["ratios"]["native_to_fjall_maintained_read_p95"]
            .as_f64()
            .unwrap()
            < 1.0
    );
}
