use vyrm_kv::{Error, Manifest, ManifestStore, SegmentDescriptor};

fn segment(id: &str, first: &[u8], last: &[u8], minimum: u64, maximum: u64) -> SegmentDescriptor {
    SegmentDescriptor {
        id: id.into(),
        level: 0,
        first_key: first.into(),
        last_key: last.into(),
        minimum_sequence: minimum,
        maximum_sequence: maximum,
        entries: 2,
        bytes: 128,
        checksum: "22".repeat(32),
    }
}

#[test]
fn manifest_identity_is_stable_and_segment_order_is_canonical() {
    let left = Manifest::new(
        1,
        None,
        100,
        4,
        5,
        vec![
            segment(&"bb".repeat(32), b"m", b"z", 3, 4),
            segment(&"aa".repeat(32), b"a", b"l", 1, 2),
        ],
    )
    .unwrap();
    let right = Manifest::new(
        1,
        None,
        100,
        4,
        5,
        vec![
            segment(&"aa".repeat(32), b"a", b"l", 1, 2),
            segment(&"bb".repeat(32), b"m", b"z", 3, 4),
        ],
    )
    .unwrap();
    assert_eq!(left, right);
    left.validate().unwrap();

    let actual = format!("{}\n", serde_json::to_string_pretty(&left).unwrap());
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/manifest-v1.json");
    if std::env::var_os("VYRM_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(fixture, &actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(fixture).unwrap());
}

#[test]
fn tampering_and_invalid_reachability_fail_closed() {
    let mut manifest = Manifest::new(
        2,
        Some("11".repeat(32)),
        100,
        4,
        3,
        vec![segment(&"aa".repeat(32), b"a", b"z", 1, 4)],
    )
    .unwrap();
    manifest.durable_sequence = 3;
    assert!(matches!(
        manifest.validate(),
        Err(Error::InvalidManifest(_))
    ));

    assert!(matches!(
        Manifest::new(
            1,
            None,
            100,
            2,
            3,
            vec![
                segment(&"aa".repeat(32), b"a", b"b", 1, 1),
                segment(&"aa".repeat(32), b"c", b"d", 2, 2),
            ],
        ),
        Err(Error::InvalidManifest(_))
    ));
}

#[test]
fn current_publication_is_ordered_content_addressed_and_compare_and_swap() {
    let directory = tempfile::tempdir().unwrap();
    let store = ManifestStore::open(directory.path()).unwrap();
    assert!(store.current().unwrap().is_none());
    let first = Manifest::new(1, None, 100, 0, 1, Vec::new()).unwrap();
    let pointer = store.publish(&first, None).unwrap();
    assert_eq!(pointer.manifest, first.digest);
    assert_eq!(store.current().unwrap().unwrap().1, first);

    let second = Manifest::new(2, Some(first.digest.clone()), 101, 0, 1, Vec::new()).unwrap();
    assert!(matches!(
        store.publish(&second, None),
        Err(Error::ManifestConflict { .. })
    ));
    store.publish(&second, Some(&first.digest)).unwrap();
    assert_eq!(store.current().unwrap().unwrap().1, second);
    assert_eq!(store.load(&first.digest).unwrap(), first);
    drop(store);

    let reopened = ManifestStore::open(directory.path()).unwrap();
    assert_eq!(reopened.current().unwrap().unwrap().1, second);
}

#[test]
fn a_tampered_current_pointer_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let store = ManifestStore::open(directory.path()).unwrap();
    let manifest = Manifest::new(1, None, 100, 0, 1, Vec::new()).unwrap();
    store.publish(&manifest, None).unwrap();
    drop(store);
    let path = directory.path().join("CURRENT");
    let mut bytes = std::fs::read(&path).unwrap();
    let index = bytes.iter().position(|byte| *byte == b'1').unwrap();
    bytes[index] = b'2';
    std::fs::write(path, bytes).unwrap();
    let reopened = ManifestStore::open(directory.path()).unwrap();
    assert!(reopened.current().is_err());
}

#[test]
fn named_checkpoints_pin_historical_manifests_until_explicit_release() {
    let directory = tempfile::tempdir().unwrap();
    let store = ManifestStore::open(directory.path()).unwrap();
    let first = Manifest::new(1, None, 100, 0, 1, Vec::new()).unwrap();
    store.publish(&first, None).unwrap();
    let checkpoint = store
        .checkpoint("before-migration", &first.digest, 101)
        .unwrap();
    assert_eq!(checkpoint.manifest, first.digest);
    assert_eq!(
        store
            .checkpoint("before-migration", &first.digest, 101)
            .unwrap(),
        checkpoint,
        "identical checkpoint creation is idempotent"
    );
    let second = Manifest::new(2, Some(first.digest.clone()), 102, 0, 1, Vec::new()).unwrap();
    store.publish(&second, Some(&first.digest)).unwrap();
    assert_eq!(store.checkpoints().unwrap(), vec![checkpoint]);
    assert_eq!(store.load(&first.digest).unwrap(), first);
    assert!(matches!(
        store.checkpoint("before-migration", &second.digest, 103),
        Err(Error::InvalidManifest(_))
    ));
    assert!(store.release_checkpoint("before-migration").unwrap());
    assert!(!store.release_checkpoint("before-migration").unwrap());
    assert!(store.checkpoints().unwrap().is_empty());
    assert!(store.checkpoint("../escape", &second.digest, 103).is_err());
}
