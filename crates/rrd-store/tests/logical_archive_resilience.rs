use rrd_core::{
    Claim, DataTransaction, EmbeddingProvenance, GeoPoint, GeoValue, Predicate, Producer,
    RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo, RuntimeMutation,
    RuntimeProperties, RuntimeRecord, RuntimeRecordSchema, RuntimeRef, RuntimeRelation,
    RuntimeRelationSchema, RuntimeSchemaRegistry, RuntimeSeriesSample, RuntimeType, RuntimeVector,
    ScopeId, SeriesValue, Subject, VectorNormalization, VectorValue,
};
use rrd_store::{
    export_logical_archive, export_logical_archive_with_progress, inspect_logical_archive,
    restore_logical_archive_to_new_root, restore_logical_archive_to_new_root_with_progress,
    DataRuntime, Engine, Error, LocalObjectStore, LogicalArchiveCheckpoint, NativeEngine,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

fn claim(name: impl std::fmt::Display, at: u64) -> Claim {
    Claim::new(
        Subject::new(format!("archive:{name}")).unwrap(),
        Predicate::new("status").unwrap(),
        "retained",
        at,
        at,
        Producer {
            actor: "agent:archive-resilience".into(),
            on_behalf_of: None,
            session: Some("archive-resilience".into()),
        },
    )
}

fn mixed_source(root: &Path) -> NativeEngine {
    let engine = NativeEngine::open(root).unwrap();
    engine.append_batch(&[claim("before", 10)]).unwrap();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:resilience").unwrap(),
            at: 20,
            actor: "agent:archive-resilience".into(),
            expected_cursor: 0,
            mutations: vec![RuntimeMutation::Claim {
                claim: claim("runtime-one", 20),
            }],
        })
        .unwrap();
    engine.append_batch(&[claim("middle", 30)]).unwrap();
    engine
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:resilience").unwrap(),
            at: 40,
            actor: "agent:archive-resilience".into(),
            expected_cursor: 1,
            mutations: vec![RuntimeMutation::Claim {
                claim: claim("runtime-two", 40),
            }],
        })
        .unwrap();
    engine.append_batch(&[claim("after", 50)]).unwrap();
    engine
}

fn assert_same_runtime(left: &NativeEngine, right: &NativeEngine) {
    assert_eq!(left.sequence().unwrap(), right.sequence().unwrap());
    assert_eq!(
        left.runtime_cursor().unwrap(),
        right.runtime_cursor().unwrap()
    );
    let claims = left.sequence().unwrap();
    assert_eq!(
        left.claims_in_range(0, claims).unwrap(),
        right.claims_in_range(0, claims).unwrap()
    );
    let cursor = left.runtime_cursor().unwrap();
    let left_changes = left
        .runtime_changes_since(0, cursor as usize + 1, None)
        .unwrap()
        .changes;
    let right_changes = right
        .runtime_changes_since(0, cursor as usize + 1, None)
        .unwrap()
        .changes;
    assert_eq!(left_changes, right_changes);
    for commit_id in left_changes
        .iter()
        .map(|change| &change.commit_id)
        .collect::<BTreeSet<_>>()
    {
        assert_eq!(
            left.runtime_audit(commit_id).unwrap(),
            right.runtime_audit(commit_id).unwrap()
        );
    }
}

#[test]
fn interrupted_export_after_database_boundary_resumes_to_identical_bytes() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let uninterrupted = root.path().join("uninterrupted.rrd-archive");
    let resumed = root.path().join("resumed.rrd-archive");
    let expected = export_logical_archive(&source, &uninterrupted).unwrap();

    let error = export_logical_archive_with_progress(&source, &resumed, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ActionDurable
            && progress.completed_actions == 2
        {
            return Err(Error::FaultInjected("export_action_durable"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(
        error,
        Error::FaultInjected("export_action_durable")
    ));
    assert!(!resumed.exists());

    let actual = export_logical_archive(&source, &resumed).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        std::fs::read(resumed).unwrap(),
        std::fs::read(uninterrupted).unwrap()
    );
}

#[test]
fn interrupted_export_after_receipt_boundary_resumes_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("receipt-resume.rrd-archive");
    let error = export_logical_archive_with_progress(&source, &archive, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ReceiptDurable
            && progress.completed_actions == 3
        {
            return Err(Error::FaultInjected("export_receipt_durable"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(
        error,
        Error::FaultInjected("export_receipt_durable")
    ));
    let inventory = export_logical_archive(&source, &archive).unwrap();
    assert_eq!(inspect_logical_archive(&archive).unwrap(), inventory);
}

#[test]
fn interrupted_export_after_final_footer_resumes_by_publication() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let uninterrupted = root.path().join("uninterrupted.rrd-archive");
    let resumed = root.path().join("finalized-resume.rrd-archive");
    let expected = export_logical_archive(&source, &uninterrupted).unwrap();

    let error = export_logical_archive_with_progress(&source, &resumed, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ArchiveFinalized {
            return Err(Error::FaultInjected("export_archive_finalized"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(
        error,
        Error::FaultInjected("export_archive_finalized")
    ));
    assert!(!resumed.exists());

    let actual = export_logical_archive(&source, &resumed).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        std::fs::read(resumed).unwrap(),
        std::fs::read(uninterrupted).unwrap()
    );
}

#[test]
fn tampered_export_receipt_is_denied() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("tamper.rrd-archive");
    export_logical_archive_with_progress(&source, &archive, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ReceiptDurable
            && progress.completed_actions == 1
        {
            return Err(Error::FaultInjected("leave_receipt"));
        }
        Ok(())
    })
    .unwrap_err();
    let receipt = root
        .path()
        .join(".tamper.rrd-archive.rrd-export.receipt.json");
    flip_last_byte(&receipt);
    assert!(export_logical_archive(&source, &archive).is_err());
    assert!(!archive.exists());
}

#[test]
fn missing_export_receipt_is_reconstructed_from_the_durable_prefix() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("missing-receipt.rrd-archive");
    export_logical_archive_with_progress(&source, &archive, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ActionDurable
            && progress.completed_actions == 2
        {
            return Err(Error::FaultInjected("remove_receipt"));
        }
        Ok(())
    })
    .unwrap_err();
    std::fs::remove_file(
        root.path()
            .join(".missing-receipt.rrd-archive.rrd-export.receipt.json"),
    )
    .unwrap();
    let inventory = export_logical_archive(&source, &archive).unwrap();
    assert_eq!(inspect_logical_archive(&archive).unwrap(), inventory);
}

#[test]
fn source_advancement_invalidates_an_unfinished_export_cut() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("advanced.rrd-archive");
    export_logical_archive_with_progress(&source, &archive, |progress| {
        if progress.checkpoint == LogicalArchiveCheckpoint::ReceiptDurable
            && progress.completed_actions == 1
        {
            return Err(Error::FaultInjected("source_will_advance"));
        }
        Ok(())
    })
    .unwrap_err();
    source.append_batch(&[claim("new-head", 60)]).unwrap();
    assert!(export_logical_archive(&source, &archive).is_err());
    assert!(!archive.exists());
    assert!(!root
        .path()
        .join(".advanced.rrd-archive.rrd-export.partial")
        .exists());
    assert!(!root
        .path()
        .join(".advanced.rrd-archive.rrd-export.receipt.json")
        .exists());
    export_logical_archive(&source, &archive).unwrap();
}

#[test]
fn interrupted_restore_reconciles_action_ahead_of_receipt() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("restored");
    export_logical_archive(&source, &archive).unwrap();
    let error = restore_logical_archive_to_new_root_with_progress(
        &archive,
        &target,
        100,
        |_| Ok(()),
        |progress| {
            if progress.checkpoint == LogicalArchiveCheckpoint::ActionDurable
                && progress.completed_actions == 2
            {
                return Err(Error::FaultInjected("restore_action_durable"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        Error::FaultInjected("restore_action_durable")
    ));
    assert!(!target.exists());

    let report = restore_logical_archive_to_new_root(&archive, &target, 101).unwrap();
    assert!(report.resumed);
    assert_same_runtime(&source, &NativeEngine::open(&target).unwrap());
}

#[test]
fn interrupted_restore_after_receipt_resumes_without_duplicate_mutations() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("restored");
    export_logical_archive(&source, &archive).unwrap();
    restore_logical_archive_to_new_root_with_progress(
        &archive,
        &target,
        100,
        |_| Ok(()),
        |progress| {
            if progress.checkpoint == LogicalArchiveCheckpoint::ReceiptDurable
                && progress.completed_actions == 3
            {
                return Err(Error::FaultInjected("restore_receipt_durable"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    restore_logical_archive_to_new_root(&archive, &target, 101).unwrap();
    assert_same_runtime(&source, &NativeEngine::open(&target).unwrap());
}

#[test]
fn tampered_restore_receipt_is_denied_before_target_publication() {
    let root = tempfile::tempdir().unwrap();
    let source = mixed_source(&root.path().join("source"));
    let archive = root.path().join("backup.rrd-archive");
    let target = root.path().join("restored");
    let inventory = export_logical_archive(&source, &archive).unwrap();
    restore_logical_archive_to_new_root_with_progress(
        &archive,
        &target,
        100,
        |_| Ok(()),
        |progress| {
            if progress.checkpoint == LogicalArchiveCheckpoint::ReceiptDurable
                && progress.completed_actions == 1
            {
                return Err(Error::FaultInjected("leave_restore_receipt"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    let receipt = restore_receipt(root.path(), &target, &inventory.archive_sha256);
    flip_last_byte(&receipt);
    assert!(restore_logical_archive_to_new_root(&archive, &target, 101).is_err());
    assert!(!target.exists());
}

#[test]
fn claim_stream_crosses_multiple_pages_without_reordering() {
    let root = tempfile::tempdir().unwrap();
    let source = NativeEngine::open(&root.path().join("source")).unwrap();
    let claims = (1..=2_100)
        .map(|index| claim(index, index))
        .collect::<Vec<_>>();
    source.append_batch(&claims).unwrap();
    let archive = root.path().join("claims.rrd-archive");
    let target = root.path().join("restored");
    let inventory = export_logical_archive(&source, &archive).unwrap();
    assert_eq!(inventory.standalone_claims, 2_100);
    restore_logical_archive_to_new_root(&archive, &target, 100).unwrap();
    assert_eq!(
        NativeEngine::open(&target)
            .unwrap()
            .claims_in_range(0, 2_100)
            .unwrap(),
        claims
    );
}

#[test]
fn one_commit_split_across_runtime_pages_remains_atomic() {
    let root = tempfile::tempdir().unwrap();
    let source = NativeEngine::open(&root.path().join("source")).unwrap();
    let mutations = (1..=1_100)
        .map(|index| RuntimeMutation::Claim {
            claim: claim(format!("runtime-{index}"), index),
        })
        .collect::<Vec<_>>();
    let outcome = source
        .commit_runtime(&RuntimeCommit {
            scope: ScopeId::new("instance:paged-commit").unwrap(),
            at: 1_100,
            actor: "agent:paged-commit".into(),
            expected_cursor: 0,
            mutations,
        })
        .unwrap();
    let archive = root.path().join("runtime.rrd-archive");
    let target = root.path().join("restored");
    let inventory = export_logical_archive(&source, &archive).unwrap();
    assert_eq!(inventory.runtime_commits, 1);
    assert_eq!(inventory.runtime_mutations, 1_100);
    restore_logical_archive_to_new_root(&archive, &target, 100).unwrap();
    let restored = NativeEngine::open(&target).unwrap();
    assert_eq!(
        restored.runtime_commit_outcome(&outcome.commit_id).unwrap(),
        Some(outcome)
    );
    assert_same_runtime(&source, &restored);
}

#[test]
fn every_canonical_runtime_family_and_transaction_audit_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let source_path = root.path().join("source");
    let objects = root.path().join("objects");
    let runtime = DataRuntime::new(
        NativeEngine::open(&source_path).unwrap(),
        LocalObjectStore::open(&objects).unwrap(),
    );
    let scope = ScopeId::new("instance:all-families").unwrap();
    runtime
        .engine()
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 10,
            actor: "agent:all-families".into(),
            expected_cursor: 0,
            mutations: bootstrap_mutations(),
        })
        .unwrap();
    let object = runtime
        .stage_object(
            "entity-a-source",
            Some(RuntimeRef::new("entity", "a").unwrap()),
            "text/plain",
            b"object bytes live in the application backup closure",
        )
        .unwrap();
    let read = runtime.engine().runtime_read_stamp(&scope).unwrap();
    let transaction = DataTransaction::new(
        read,
        RuntimeCommit {
            scope: scope.clone(),
            at: 20,
            actor: "agent:all-families".into(),
            expected_cursor: 3,
            mutations: all_family_mutations(object),
        },
    )
    .unwrap();
    let outcome = runtime.commit(&transaction).unwrap();
    let archive = root.path().join("all-families.rrd-archive");
    let target = root.path().join("restored");
    export_logical_archive(runtime.engine(), &archive).unwrap();
    restore_logical_archive_to_new_root(&archive, &target, 100).unwrap();
    let restored = NativeEngine::open(&target).unwrap();
    assert_same_runtime(runtime.engine(), &restored);
    assert_eq!(
        restored.runtime_audit(&outcome.commit_id).unwrap(),
        runtime.engine().runtime_audit(&outcome.commit_id).unwrap()
    );
}

fn bootstrap_mutations() -> Vec<RuntimeMutation> {
    let mut schema = RuntimeSchemaRegistry::empty(1, "archive all-family schema");
    schema.records.insert(
        RuntimeType::new("entity").unwrap(),
        RuntimeRecordSchema::default(),
    );
    schema.relations.insert(
        RuntimeType::new("links").unwrap(),
        RuntimeRelationSchema {
            from: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            to: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            ..RuntimeRelationSchema::default()
        },
    );
    schema.events.insert(
        RuntimeType::new("observed").unwrap(),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([RuntimeType::new("entity").unwrap()]),
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        },
    );
    vec![
        RuntimeMutation::Schema { registry: schema },
        RuntimeMutation::Record {
            record: record("a"),
        },
        RuntimeMutation::Record {
            record: record("b"),
        },
    ]
}

fn record(id: &str) -> RuntimeRecord {
    RuntimeRecord {
        reference: RuntimeRef::new("entity", id).unwrap(),
        valid_from: 10,
        valid_to: None,
        properties: RuntimeProperties::new(),
    }
}

fn all_family_mutations(object: rrd_core::ObjectReference) -> Vec<RuntimeMutation> {
    let subject = RuntimeRef::new("entity", "a").unwrap();
    vec![
        RuntimeMutation::Claim {
            claim: claim("all-families", 20),
        },
        RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: RuntimeRef::new("links", "a-b").unwrap(),
                from: subject.clone(),
                to: RuntimeRef::new("entity", "b").unwrap(),
                valid_from: 20,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new("observed").unwrap(),
                subject: Some(subject.clone()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", "a-title").unwrap(),
                subject: subject.clone(),
                collection: None,
                field: "title".into(),
                valid_from: 20,
                valid_to: None,
                value: VectorValue::Dense {
                    values: vec![0.0, 0.6, 0.8],
                },
                provenance: Some(EmbeddingProvenance {
                    source_digest: "11".repeat(32),
                    model: "fixture-embedder".into(),
                    model_digest: "22".repeat(32),
                    dimensions: 3,
                    normalization: VectorNormalization::UnitL2,
                    generation_parameters: RuntimeProperties::new(),
                }),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: RuntimeRef::new("sample", "a-temperature-20").unwrap(),
                series: subject.clone(),
                observed_at: 20,
                value: SeriesValue::Decimal("21.5".into()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: RuntimeRef::new("location", "a-current").unwrap(),
                subject,
                field: "position".into(),
                valid_from: 20,
                valid_to: None,
                value: GeoValue::Point {
                    point: GeoPoint {
                        longitude: -122.4194,
                        latitude: 37.7749,
                    },
                },
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Object { object },
    ]
}

fn restore_receipt(root: &Path, target: &Path, archive_sha256: &str) -> PathBuf {
    root.join(format!(
        ".{}.rrd-restore-{}.receipt.json",
        target.file_name().unwrap().to_string_lossy(),
        &archive_sha256[..16]
    ))
}

fn flip_last_byte(path: &Path) {
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(b"x").unwrap();
    file.sync_all().unwrap();
}
