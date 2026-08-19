use vyrm_kv::{
    Database, Durability, Error, FailureMode, Mutation, SnapshotBundle, SnapshotInstallBoundary,
    WriteBatch, SNAPSHOT_BUNDLE_FORMAT_VERSION,
};

#[test]
fn physical_snapshot_bundle_round_trips_installs_atomically_and_continues_writes() {
    let source_directory = tempfile::tempdir().unwrap();
    let mut source = Database::create(source_directory.path()).unwrap();
    source
        .write_owned(
            WriteBatch::new(vec![put("alpha", "one"), put("beta", "temporary")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    source
        .write_owned(
            WriteBatch::new(vec![
                Mutation::Delete {
                    key: b"beta".to_vec(),
                },
                put("gamma", "three"),
            ])
            .unwrap(),
            Durability::Authoritative,
        )
        .unwrap();

    let bundle = source.export_snapshot_bundle(10).unwrap();
    assert_eq!(bundle.format_version, SNAPSHOT_BUNDLE_FORMAT_VERSION);
    assert_eq!(bundle.source_manifest.durable_sequence, 4);
    assert_eq!(bundle.source_manifest.wal_start_sequence, 5);
    assert!(!bundle.segments.is_empty());
    assert_eq!(
        source.export_snapshot_bundle(10).unwrap(),
        bundle,
        "an unchanged physical snapshot must be byte-identical"
    );
    let encoded = bundle.encode().unwrap();
    let actual_hex = encoded
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let expected_hex = include_str!("../fixtures/snapshot-bundle-v1.hex")
        .split_whitespace()
        .collect::<String>();
    assert_eq!(actual_hex, expected_hex, "snapshot v1 wire bytes changed");
    let decoded = SnapshotBundle::decode(&encoded).unwrap();
    assert_eq!(decoded, bundle);
    assert_eq!(decoded.encode().unwrap(), encoded);

    let target_directory = tempfile::tempdir().unwrap();
    let mut target = Database::create(target_directory.path()).unwrap();
    target
        .write_owned(
            WriteBatch::new(vec![put("target-only", "discard")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let prior_manifest = target.manifest().digest.clone();
    let installed = target.install_snapshot_bundle(&decoded, 20).unwrap();
    assert_eq!(installed.parent.as_deref(), Some(prior_manifest.as_str()));
    assert_eq!(installed.durable_sequence, 4);
    assert_eq!(installed.wal_start_sequence, 5);
    let snapshot = target.snapshot();
    assert_eq!(snapshot.sequence, 4);
    assert_eq!(target.get(b"alpha", snapshot), Some(b"one".as_slice()));
    assert_eq!(target.get(b"beta", snapshot), None);
    assert_eq!(target.get(b"gamma", snapshot), Some(b"three".as_slice()));
    assert_eq!(target.get(b"target-only", snapshot), None);
    assert_eq!(
        target.install_snapshot_bundle(&decoded, 21).unwrap(),
        installed,
        "reinstalling the current bundle is idempotent"
    );

    let receipt = target
        .write_owned(
            WriteBatch::new(vec![put("delta", "four")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    assert_eq!(receipt.first_sequence, 5);
    assert!(matches!(
        target.install_snapshot_bundle(&decoded, 22),
        Err(Error::InvalidManifest(reason)) if reason.contains("does not advance")
    ));
    drop(target);

    let reopened = Database::open(target_directory.path()).unwrap();
    let snapshot = reopened.snapshot();
    assert_eq!(snapshot.sequence, 5);
    assert_eq!(reopened.get(b"alpha", snapshot), Some(b"one".as_slice()));
    assert_eq!(reopened.get(b"delta", snapshot), Some(b"four".as_slice()));
    assert_eq!(reopened.get(b"target-only", snapshot), None);
}

#[test]
fn corruption_and_truncation_are_denied_before_manifest_publication() {
    let source_directory = tempfile::tempdir().unwrap();
    let mut source = Database::create(source_directory.path()).unwrap();
    source
        .write_owned(
            WriteBatch::new(vec![put("truth", "authenticated")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let encoded = source.export_snapshot_bundle(1).unwrap().encode().unwrap();

    let target_directory = tempfile::tempdir().unwrap();
    let mut target = Database::create(target_directory.path()).unwrap();
    let original_manifest = target.manifest().clone();

    let mut corrupt = encoded.clone();
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 0x40;
    assert!(SnapshotBundle::decode(&corrupt).is_err());
    assert_eq!(target.manifest(), &original_manifest);

    assert!(SnapshotBundle::decode(&encoded[..encoded.len() - 1]).is_err());
    assert_eq!(target.manifest(), &original_manifest);

    let manifest_len =
        u32::from_be_bytes(encoded[12..16].try_into().unwrap()) as usize;
    let manifest_end = 20 + manifest_len;
    let mut noncanonical = Vec::with_capacity(encoded.len() + 1);
    noncanonical.extend_from_slice(&encoded[..12]);
    noncanonical.extend_from_slice(&u32::try_from(manifest_len + 1).unwrap().to_be_bytes());
    noncanonical.extend_from_slice(&encoded[16..manifest_end]);
    noncanonical.push(b' ');
    noncanonical.extend_from_slice(&encoded[manifest_end..]);
    assert!(
        SnapshotBundle::decode(&noncanonical).is_err(),
        "the footer must authenticate the exact received envelope, not only its canonical object"
    );
    assert_eq!(target.manifest(), &original_manifest);

    let mut decoded = SnapshotBundle::decode(&encoded).unwrap();
    decoded.segments[0].bytes[0] ^= 0x01;
    assert!(target.install_snapshot_bundle(&decoded, 2).is_err());
    assert_eq!(target.manifest(), &original_manifest);
    assert_eq!(target.snapshot().sequence, 0);
}

#[test]
fn every_install_boundary_recovers_after_crash_and_storage_full() {
    let source_directory = tempfile::tempdir().unwrap();
    let mut source = Database::create(source_directory.path()).unwrap();
    source
        .write_owned(
            WriteBatch::new(vec![put("truth", "one"), put("more", "two")]).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    let bundle = source.export_snapshot_bundle(1).unwrap();

    for boundary in [
        SnapshotInstallBoundary::SegmentsSynced,
        SnapshotInstallBoundary::SuccessorWalSynced,
        SnapshotInstallBoundary::ManifestPublished,
    ] {
        for mode in [FailureMode::Crash, FailureMode::StorageFull] {
            let target_directory = tempfile::tempdir().unwrap();
            let mut target = Database::create(target_directory.path()).unwrap();
            let original = target.manifest().digest.clone();
            let error = target
                .install_snapshot_bundle_with_failure(&bundle, 2, boundary, mode)
                .unwrap_err();
            assert!(matches!(error, Error::InjectedFailure { .. }));
            if boundary == SnapshotInstallBoundary::ManifestPublished {
                assert_ne!(target.manifest().digest, original);
            } else {
                assert_eq!(target.manifest().digest, original);
            }
            drop(target);

            let mut recovered = Database::open(target_directory.path()).unwrap();
            if boundary != SnapshotInstallBoundary::ManifestPublished {
                assert_eq!(recovered.snapshot().sequence, 0);
                recovered.install_snapshot_bundle(&bundle, 3).unwrap();
            }
            let snapshot = recovered.snapshot();
            assert_eq!(snapshot.sequence, 2);
            assert_eq!(recovered.get(b"truth", snapshot), Some(b"one".as_slice()));
            assert_eq!(recovered.get(b"more", snapshot), Some(b"two".as_slice()));
        }
    }
}

fn put(key: &str, value: &str) -> Mutation {
    Mutation::Put {
        key: key.as_bytes().to_vec(),
        value: value.as_bytes().to_vec(),
    }
}
