use tempfile::tempdir;
use vyrm_core::RuntimeProperties;
use vyrm_edge::{OfflineDocument, OfflineEdgeConfig, OfflineEdgeIndex};
use vyrm_vector::DenseMemoryPlacement;

fn documents() -> Vec<OfflineDocument> {
    vec![
        OfflineDocument {
            id: "alpha".into(),
            text: "alpha beta deterministic runtime".into(),
            properties: RuntimeProperties::new(),
        },
        OfflineDocument {
            id: "gamma".into(),
            text: "gamma delta unrelated corpus".into(),
            properties: RuntimeProperties::new(),
        },
    ]
}

#[test]
fn local_embedding_and_mmap_search_complete_in_one_offline_call() {
    let config = OfflineEdgeConfig::standard(64, 19).unwrap();
    let built = OfflineEdgeIndex::build(config.clone(), 1, documents()).unwrap();
    assert_eq!(
        built.artifact().memory_placement(),
        DenseMemoryPlacement::Owned
    );
    let root = tempdir().unwrap();
    let path = root.path().join("edge.vyrdense");
    built.write_atomic(&path).unwrap();

    let mut mapped = OfflineEdgeIndex::open_mmap(config, &path).unwrap();
    assert_eq!(
        mapped.artifact().memory_placement(),
        DenseMemoryPlacement::Mapped
    );
    let result = mapped
        .search_text("alpha beta deterministic runtime", 2, 1)
        .unwrap();
    assert_eq!(result.hits[0].reference.id.as_str(), "alpha");
    assert!((result.hits[0].score - 1.0).abs() <= 1e-6);
    assert_eq!(result.source_cursor, 2);
}

#[test]
fn a_different_model_seed_cannot_query_an_existing_artifact() {
    let root = tempdir().unwrap();
    let path = root.path().join("edge.vyrdense");
    let built =
        OfflineEdgeIndex::build(OfflineEdgeConfig::standard(64, 19).unwrap(), 1, documents())
            .unwrap();
    built.write_atomic(&path).unwrap();
    assert!(
        OfflineEdgeIndex::open_mmap(OfflineEdgeConfig::standard(64, 20).unwrap(), path).is_err()
    );
}

#[test]
fn build_is_deterministic_and_empty_inputs_fail_closed() {
    let config = OfflineEdgeConfig::standard(64, 19).unwrap();
    let first = OfflineEdgeIndex::build(config.clone(), 3, documents()).unwrap();
    let second = OfflineEdgeIndex::build(config.clone(), 3, documents()).unwrap();
    assert_eq!(first.artifact().as_bytes(), second.artifact().as_bytes());
    assert!(OfflineEdgeIndex::build(config, 1, Vec::new()).is_err());
}
