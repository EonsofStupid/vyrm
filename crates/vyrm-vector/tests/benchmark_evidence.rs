use std::collections::BTreeSet;

#[test]
fn checked_in_m5_evidence_is_structurally_complete_and_meets_local_gate() {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/evidence/m5-vector-local-10000x128.json"
    ))
    .unwrap();
    assert_eq!(evidence["schema"], "vyrm.vector-evidence.v1");
    assert_eq!(evidence["profile"]["vectors"], 10_000);
    assert_eq!(evidence["profile"]["dimensions"], 128);
    assert_eq!(evidence["profile"]["top_k"], 10);
    assert!(evidence["build"]["milliseconds"].as_f64().unwrap() > 0.0);
    assert!(
        evidence["build"]["artifact_to_raw_payload_ratio"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    assert!(
        evidence["quantization"]["raw_payload_ratio"]
            .as_f64()
            .unwrap()
            < 0.30
    );
    assert_eq!(
        evidence["quantization"]["recall_at_k_after_exact_rerank"],
        1.0
    );

    let rows = evidence["searches"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    let matrix = rows
        .iter()
        .map(|row| {
            (
                row["filter_percent"].as_u64().unwrap(),
                row["ef_search"].as_u64().unwrap(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        matrix,
        [100, 50, 10, 1]
            .into_iter()
            .flat_map(|filter| [32, 64, 128, 256].map(move |ef| (filter, ef)))
            .collect()
    );
    assert!(rows.iter().all(|row| row["complete_result_rate"] == 1.0));
    let unfiltered_high_quality = rows
        .iter()
        .find(|row| row["filter_percent"] == 100 && row["ef_search"] == 256)
        .unwrap();
    assert!(
        unfiltered_high_quality["mean_recall_at_k"]
            .as_f64()
            .unwrap()
            >= 0.95
    );
    assert!(rows
        .iter()
        .filter(|row| row["filter_percent"] == 1)
        .all(|row| row["planner_preference"] == "exact_scan"));
}
