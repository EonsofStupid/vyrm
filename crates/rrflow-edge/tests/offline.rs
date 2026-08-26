use rrd_engine::{OfflineDocument, OfflineEdgeConfig, OfflineEdgeIndex};
use tempfile::tempdir;

fn documents() -> Vec<OfflineDocument> {
    vec![
        OfflineDocument::new("alpha", "alpha beta deterministic runtime"),
        OfflineDocument::new("gamma", "gamma delta unrelated corpus"),
    ]
}

#[test]
fn local_embedding_and_mmap_search_complete_in_one_offline_call() {
    let config = OfflineEdgeConfig::standard(64, 19).unwrap();
    let built = OfflineEdgeIndex::build(config.clone(), 1, documents()).unwrap();
    assert!(!built.is_memory_mapped());
    let root = tempdir().unwrap();
    let path = root.path().join("edge.rrdense");
    built.write_atomic(&path).unwrap();

    let mut mapped = OfflineEdgeIndex::open_mmap(config, &path).unwrap();
    assert!(mapped.is_memory_mapped());
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
    let path = root.path().join("edge.rrdense");
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
