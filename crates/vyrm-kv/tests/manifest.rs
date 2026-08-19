use vyrm_kv::{Error, Manifest, SegmentDescriptor};

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
