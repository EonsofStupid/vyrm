//! Frozen portable JSON/canonical identities for the M4 unified data contract.

use vyrm_core::{
    projection_family, GeoPoint, GeoValue, ObjectReceipt, ObjectReference, ProjectionWork,
    RuntimeCommit, RuntimeGeo, RuntimeMutation, RuntimeProperties, RuntimeRef, RuntimeSeriesSample,
    RuntimeVector, ScopeId, SeriesValue, VectorValue,
};

fn contract() -> serde_json::Value {
    let subject = RuntimeRef::new("entity", "doc-1").unwrap();
    let sha256 = "6b94be64314982ce8c55d1580e305112d36ae3eee6f85446f8b95c4b9b1df880";
    let object = ObjectReference::for_bytes(
        "source",
        Some(subject.clone()),
        "text/plain",
        b"golden object bytes",
        ObjectReceipt {
            backend: "local".into(),
            key: ObjectReference::canonical_key(sha256).unwrap(),
            version: None,
            etag: Some(sha256.into()),
        },
    )
    .unwrap();
    let mutations = vec![
        RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", "doc-1-title").unwrap(),
                subject: subject.clone(),
                field: "title".into(),
                valid_from: 1_000,
                valid_to: None,
                value: VectorValue::Dense {
                    values: vec![0.25, -0.5, 0.75],
                },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: RuntimeRef::new("sample", "doc-1-score-1000").unwrap(),
                series: subject.clone(),
                observed_at: 1_000,
                value: SeriesValue::Decimal("42.125".into()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: RuntimeRef::new("location", "doc-1-region").unwrap(),
                subject,
                field: "region".into(),
                valid_from: 1_000,
                valid_to: Some(2_000),
                value: GeoValue::BoundingBox {
                    southwest: GeoPoint {
                        longitude: -123.0,
                        latitude: 37.0,
                    },
                    northeast: GeoPoint {
                        longitude: -122.0,
                        latitude: 38.0,
                    },
                },
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Object { object },
    ];
    let commit = RuntimeCommit {
        scope: ScopeId::new("instance:golden").unwrap(),
        at: 1_100,
        actor: "agent:golden".into(),
        expected_cursor: 7,
        mutations,
    };
    commit.validate().unwrap();
    let commit_id = commit.digest();
    let work = ProjectionWork::for_change(
        commit.scope.clone(),
        8,
        commit_id.clone(),
        0,
        projection_family(&commit.mutations[0]).unwrap(),
    )
    .unwrap();
    serde_json::json!({
        "comment": "M4 portable values; changes require an explicit wire-version decision",
        "commit": commit,
        "commit_digest": commit_id,
        "first_projection_work": work,
        "sparse_vector": VectorValue::Sparse {
            dimensions: 8,
            indices: vec![1, 4, 7],
            values: vec![0.5, -1.0, 2.0],
        },
        "multi_vector": VectorValue::MultiDense {
            dimensions: 2,
            vectors: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
        },
    })
}

#[test]
fn unified_data_wire_contract_matches_checked_in_vector() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/unified-data-v1.json")).unwrap();
    assert_eq!(contract(), expected);
}
