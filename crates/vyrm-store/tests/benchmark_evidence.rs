fn assert_passing_evidence(
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
    assert!(evidence["promotion"]["policy"]
        .as_str()
        .unwrap()
        .contains("equal-or-better"));
    assert_eq!(evidence["promotion"]["passes"], true, "{file}");
    assert!(
        evidence["promotion"]["failures"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{file}"
    );
    for name in [
        "native_to_fjall_write_throughput",
        "native_to_fjall_read_throughput",
    ] {
        assert!(
            evidence["ratios"][name].as_f64().unwrap() >= 1.0,
            "{file}: {name}"
        );
    }
    for name in [
        "native_to_fjall_write_p95",
        "native_to_fjall_read_p95",
        "native_to_fjall_recovery",
        "native_to_fjall_peak_rss",
        "native_to_fjall_disk",
    ] {
        assert!(
            evidence["ratios"][name].as_f64().unwrap() <= 1.0,
            "{file}: {name}"
        );
    }
}

#[test]
fn checked_in_native_promotion_matrix_is_structurally_valid_and_green() {
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
        assert_passing_evidence(
            &format!("{root}{name}"),
            trials,
            operations,
            batch,
            reads,
            width,
        );
    }

    let extended: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!("{root}2026-08-20-vyrmkv-extended.json")).unwrap(),
    )
    .unwrap();
    let maintenance = &extended["native"]["native_maintenance"];
    assert_eq!(maintenance["automatic_flushes"], 0);
    assert_eq!(maintenance["write_stalls"], 0);
    assert_eq!(maintenance["failed_flushes"], 0);
    assert_eq!(maintenance["oversized_batches"], 0);
    assert!(
        maintenance["wal_payload_bytes"].as_u64().unwrap()
            < maintenance["wal_payload_max_bytes"].as_u64().unwrap()
    );
    assert!(
        maintenance["memtable_versions"].as_u64().unwrap()
            < maintenance["memtable_max_versions"].as_u64().unwrap()
    );
}

#[test]
fn checked_in_ai_read_matrix_is_structurally_valid_and_green() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../eval/results/");
    for (name, workload, items_per_sample) in [
        (
            "2026-08-22-vyrmkv-ai-current-hot-hit.json",
            "current_hot_hit",
            1,
        ),
        ("2026-08-22-vyrmkv-ai-cold-hit.json", "cold_hit", 1),
        ("2026-08-22-vyrmkv-ai-point-miss.json", "point_miss", 1),
        (
            "2026-08-22-vyrmkv-ai-historical-hot-hit.json",
            "historical_hot_hit",
            1,
        ),
        (
            "2026-08-22-vyrmkv-ai-metadata-fanout.json",
            "metadata_fanout",
            32,
        ),
    ] {
        let file = format!("{root}{name}");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(evidence["format_version"], 2, "{file}");
        assert_eq!(evidence["workload"], workload, "{file}");
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
        }
        for trials in ["fjall_trials", "native_trials"] {
            assert_eq!(evidence[trials].as_array().unwrap().len(), 5, "{file}");
        }
        assert_eq!(evidence["promotion"]["passes"], true, "{file}");
        assert!(
            evidence["promotion"]["failures"]
                .as_array()
                .unwrap()
                .is_empty(),
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
    }
}
