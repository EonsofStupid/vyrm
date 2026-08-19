#[test]
fn checked_in_native_baseline_is_structurally_valid_and_correctness_green() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/results/2026-08-19-vyrmkv-baseline.json"
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(evidence["format_version"], 1);
    assert!(evidence["config"]["trials"].as_u64().unwrap() >= 5);
    let operations = evidence["config"]["operations"].as_u64().unwrap();
    for backend in ["fjall", "native"] {
        assert_eq!(evidence[backend]["correctness_verified"], true);
        assert_eq!(evidence[backend]["semantic_sequence"], operations);
    }
    assert_eq!(
        evidence["fjall_trials"].as_array().unwrap().len(),
        evidence["config"]["trials"].as_u64().unwrap() as usize
    );
    assert_eq!(
        evidence["native_trials"].as_array().unwrap().len(),
        evidence["config"]["trials"].as_u64().unwrap() as usize
    );
    for ratio in evidence["ratios"].as_object().unwrap().values() {
        if !ratio.is_null() {
            assert!(ratio.as_f64().unwrap().is_finite());
            assert!(ratio.as_f64().unwrap() > 0.0);
        }
    }
    assert!(evidence["promotion"]["policy"]
        .as_str()
        .unwrap()
        .contains("equal-or-better"));
}
