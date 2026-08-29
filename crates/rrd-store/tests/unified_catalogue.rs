use std::collections::{BTreeMap, BTreeSet};

use rrd_core::{
    digest, GeoPoint, GeoValue, ObjectReceipt, ObjectReference, RuntimeCatalogueIdentity,
    RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo, RuntimeLogicalModel,
    RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema,
    RuntimeRef, RuntimeRelation, RuntimeRelationSchema, RuntimeRetirement, RuntimeSchemaMode,
    RuntimeSchemaRegistry, RuntimeSeriesSample, RuntimeTableSchema, RuntimeType, RuntimeValue,
    RuntimeValueType, RuntimeVector, ScopeId, SeriesValue, VectorValue,
};
use rrd_store::{Engine, MemoryEngine, NativeEngine, Store};

fn kind(value: &str) -> RuntimeType {
    RuntimeType::new(value).unwrap()
}

fn reference(kind: &str, id: &str) -> RuntimeRef {
    RuntimeRef::new(kind, id).unwrap()
}

fn schemaless(model: RuntimeLogicalModel) -> RuntimeTableSchema {
    RuntimeTableSchema::schemaless(model)
}

fn strict(
    model: RuntimeLogicalModel,
    properties: BTreeMap<String, RuntimePropertySchema>,
) -> RuntimeTableSchema {
    RuntimeTableSchema {
        model,
        mode: RuntimeSchemaMode::Strict,
        properties,
        allow_additional_properties: false,
    }
}

fn catalogue(revision: u64, vector_quality_required: bool) -> RuntimeSchemaRegistry {
    let mut registry = RuntimeSchemaRegistry::empty(
        revision,
        if revision == 1 {
            "install one multi-model catalogue"
        } else {
            "require vector quality across the catalogue"
        },
    );
    registry.catalogue = RuntimeCatalogueIdentity {
        namespace: kind("project"),
        database: kind("runtime"),
    };
    registry.tables = BTreeMap::from([
        (
            kind("entity"),
            RuntimeTableSchema::strict(RuntimeLogicalModel::Relational),
        ),
        (kind("document"), schemaless(RuntimeLogicalModel::Document)),
        (kind("kv"), schemaless(RuntimeLogicalModel::KeyValue)),
        (
            kind("reasoning"),
            schemaless(RuntimeLogicalModel::ReasoningRecord),
        ),
        (
            kind("claim"),
            RuntimeTableSchema::strict(RuntimeLogicalModel::ReasoningClaim),
        ),
        (
            kind("links"),
            RuntimeTableSchema::strict(RuntimeLogicalModel::GraphRelation),
        ),
        (
            kind("observed"),
            RuntimeTableSchema::strict(RuntimeLogicalModel::Event),
        ),
        (
            kind("embedding"),
            strict(
                RuntimeLogicalModel::Vector,
                BTreeMap::from([(
                    "quality".into(),
                    RuntimePropertySchema {
                        value_type: RuntimeValueType::Unsigned,
                        required: vector_quality_required,
                    },
                )]),
            ),
        ),
        (
            kind("sample"),
            strict(
                RuntimeLogicalModel::TimeSeries,
                BTreeMap::from([(
                    "unit".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                )]),
            ),
        ),
        (
            kind("location"),
            strict(RuntimeLogicalModel::Geo, BTreeMap::new()),
        ),
        (
            kind("object"),
            strict(
                RuntimeLogicalModel::Object,
                BTreeMap::from([(
                    "class".into(),
                    RuntimePropertySchema::required(RuntimeValueType::String),
                )]),
            ),
        ),
        (
            kind("lifecycle"),
            schemaless(RuntimeLogicalModel::LifecycleEvent),
        ),
    ]);
    registry.records.insert(
        kind("entity"),
        RuntimeRecordSchema {
            properties: BTreeMap::from([(
                "name".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            ..RuntimeRecordSchema::default()
        },
    );
    registry.relations.insert(
        kind("links"),
        RuntimeRelationSchema {
            from: BTreeSet::from([kind("entity")]),
            to: BTreeSet::from([kind("document")]),
            unique_pair: true,
            ..RuntimeRelationSchema::default()
        },
    );
    registry.events.insert(
        kind("observed"),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([kind("entity")]),
            properties: BTreeMap::from([(
                "stage".into(),
                RuntimePropertySchema::required(RuntimeValueType::String),
            )]),
            allow_additional_properties: false,
        },
    );
    registry
}

fn record(kind: &str, id: &str, properties: RuntimeProperties) -> RuntimeRecord {
    record_at(kind, id, 100, properties)
}

fn record_at(
    kind: &str,
    id: &str,
    valid_from: u64,
    properties: RuntimeProperties,
) -> RuntimeRecord {
    RuntimeRecord {
        reference: reference(kind, id),
        valid_from,
        valid_to: None,
        properties,
    }
}

fn vector(quality: Option<u64>) -> RuntimeVector {
    RuntimeVector {
        reference: reference("embedding", "entity-a-title"),
        subject: reference("entity", "a"),
        collection: None,
        field: "title".into(),
        valid_from: 100,
        valid_to: None,
        value: VectorValue::Dense {
            values: vec![1.0, 0.0],
        },
        provenance: None,
        properties: quality
            .map(|quality| BTreeMap::from([("quality".into(), RuntimeValue::Unsigned(quality))]))
            .unwrap_or_default(),
    }
}

fn object() -> ObjectReference {
    object_version(b"catalogued object", "1")
}

fn object_version(bytes: &[u8], version: &str) -> ObjectReference {
    let sha256 = digest::sha256_hex(bytes);
    let mut object = ObjectReference::for_bytes(
        "artifact",
        Some(reference("entity", "a")),
        "text/plain",
        bytes,
        ObjectReceipt {
            backend: "fixture".into(),
            key: ObjectReference::canonical_key(&sha256).unwrap(),
            version: Some(version.into()),
            etag: None,
        },
    )
    .unwrap();
    object
        .properties
        .insert("class".into(), RuntimeValue::String("evidence".into()));
    object
}

fn retire(model: RuntimeLogicalModel, kind: &str, id: &str, effective_at: u64) -> RuntimeMutation {
    RuntimeMutation::Retire {
        retirement: RuntimeRetirement {
            model,
            reference: reference(kind, id),
            effective_at,
        },
    }
}

fn initial_mutations() -> Vec<RuntimeMutation> {
    let entity = reference("entity", "a");
    let document = reference("document", "doc-a");
    vec![
        RuntimeMutation::Schema {
            registry: catalogue(1, false),
        },
        RuntimeMutation::Record {
            record: record(
                "entity",
                "a",
                BTreeMap::from([("name".into(), RuntimeValue::String("alpha".into()))]),
            ),
        },
        RuntimeMutation::Record {
            record: record(
                "document",
                "doc-a",
                BTreeMap::from([(
                    "arbitrary".into(),
                    RuntimeValue::Map(BTreeMap::from([(
                        "nested".into(),
                        RuntimeValue::Bool(true),
                    )])),
                )]),
            ),
        },
        RuntimeMutation::Record {
            record: record(
                "kv",
                "feature-flag",
                BTreeMap::from([("value".into(), RuntimeValue::String("on".into()))]),
            ),
        },
        RuntimeMutation::Record {
            record: record(
                "reasoning",
                "run-a",
                BTreeMap::from([("state".into(), RuntimeValue::String("active".into()))]),
            ),
        },
        RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: reference("links", "a-doc"),
                from: entity.clone(),
                to: document,
                valid_from: 100,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: kind("observed"),
                subject: Some(entity.clone()),
                properties: BTreeMap::from([(
                    "stage".into(),
                    RuntimeValue::String("created".into()),
                )]),
            },
        },
        RuntimeMutation::Vector {
            vector: vector(None),
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: reference("sample", "temperature-100"),
                series: entity.clone(),
                observed_at: 100,
                value: SeriesValue::Decimal("21.5".into()),
                properties: BTreeMap::from([(
                    "unit".into(),
                    RuntimeValue::String("celsius".into()),
                )]),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: reference("location", "current"),
                subject: entity.clone(),
                field: "position".into(),
                valid_from: 100,
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
        RuntimeMutation::Object { object: object() },
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: kind("lifecycle"),
                subject: Some(entity),
                properties: BTreeMap::from([(
                    "untyped_extension".into(),
                    RuntimeValue::String("ready".into()),
                )]),
            },
        },
    ]
}

fn exercise(engine: &dyn Engine) -> (ScopeId, u64) {
    let scope = ScopeId::new("instance:unified-catalogue").unwrap();
    let initial = RuntimeCommit {
        scope: scope.clone(),
        at: 100,
        actor: "agent:catalogue-test".into(),
        expected_cursor: 0,
        mutations: initial_mutations(),
    };
    let outcome = engine.commit_runtime(&initial).unwrap();
    let cursor = outcome.last_cursor;
    let installed = engine.runtime_schema(&scope).unwrap().unwrap();
    assert_eq!(installed.catalogue.namespace, kind("project"));
    assert_eq!(installed.catalogue.database, kind("runtime"));
    assert_eq!(installed.catalogue_tables().unwrap().len(), 12);
    assert_eq!(
        installed.tables[&kind("document")].mode,
        RuntimeSchemaMode::Schemaless
    );

    let rejected = RuntimeCommit {
        scope: scope.clone(),
        at: 101,
        actor: "agent:catalogue-test".into(),
        expected_cursor: cursor,
        mutations: vec![
            RuntimeMutation::Schema {
                registry: catalogue(2, true),
            },
            RuntimeMutation::Vector {
                vector: vector(None),
            },
        ],
    };
    assert!(engine.commit_runtime(&rejected).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), cursor);
    assert_eq!(engine.runtime_schema(&scope).unwrap().unwrap().revision, 1);

    let accepted = RuntimeCommit {
        scope: scope.clone(),
        at: 102,
        actor: "agent:catalogue-test".into(),
        expected_cursor: cursor,
        mutations: vec![
            RuntimeMutation::Schema {
                registry: catalogue(2, true),
            },
            RuntimeMutation::Vector {
                vector: vector(Some(100)),
            },
        ],
    };
    let migrated = engine.commit_runtime(&accepted).unwrap();
    assert_eq!(engine.runtime_schema(&scope).unwrap().unwrap().revision, 2);
    (scope, migrated.last_cursor)
}

#[test]
fn cross_model_catalogue_changes_are_atomic_and_equal_across_engines() {
    exercise(&MemoryEngine::new());

    let compatibility = tempfile::tempdir().unwrap();
    exercise(&Store::open(compatibility.path()).unwrap());

    let native = tempfile::tempdir().unwrap();
    exercise(&NativeEngine::open(native.path()).unwrap());
}

#[test]
fn unified_catalogue_survives_compatibility_and_native_reopen() {
    let compatibility = tempfile::tempdir().unwrap();
    let (scope, cursor) = {
        let engine = Store::open(compatibility.path()).unwrap();
        exercise(&engine)
    };
    let reopened = Store::open(compatibility.path()).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), cursor);
    assert_eq!(
        reopened.runtime_schema(&scope).unwrap().unwrap(),
        catalogue(2, true)
    );

    let native = tempfile::tempdir().unwrap();
    let path = native.path().join("native");
    let (scope, cursor) = {
        let engine = NativeEngine::open(&path).unwrap();
        exercise(&engine)
    };
    let reopened = NativeEngine::open(&path).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), cursor);
    assert_eq!(
        reopened.runtime_schema(&scope).unwrap().unwrap(),
        catalogue(2, true)
    );
}

fn exercise_crud(engine: &dyn Engine) -> (ScopeId, u64, rrd_core::RuntimeDataSnapshot) {
    let (scope, cursor) = exercise(engine);
    let mut updated_vector = vector(Some(101));
    updated_vector.valid_from = 103;
    let update_mutations = vec![
        RuntimeMutation::Record {
            record: record_at(
                "entity",
                "a",
                103,
                BTreeMap::from([("name".into(), RuntimeValue::String("beta".into()))]),
            ),
        },
        RuntimeMutation::Record {
            record: record_at(
                "document",
                "doc-a",
                103,
                BTreeMap::from([("revision".into(), RuntimeValue::Unsigned(2))]),
            ),
        },
        RuntimeMutation::Record {
            record: record_at(
                "kv",
                "feature-flag",
                103,
                BTreeMap::from([("value".into(), RuntimeValue::String("off".into()))]),
            ),
        },
        RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: reference("links", "a-doc"),
                from: reference("entity", "a"),
                to: reference("document", "doc-a"),
                valid_from: 103,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        },
        retire(RuntimeLogicalModel::Event, "observed", "cursor:7", 103),
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: kind("observed"),
                subject: Some(reference("entity", "a")),
                properties: BTreeMap::from([(
                    "stage".into(),
                    RuntimeValue::String("corrected".into()),
                )]),
            },
        },
        RuntimeMutation::Vector {
            vector: updated_vector,
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: reference("sample", "temperature-100"),
                series: reference("entity", "a"),
                observed_at: 103,
                value: SeriesValue::Decimal("22.0".into()),
                properties: BTreeMap::from([(
                    "unit".into(),
                    RuntimeValue::String("celsius".into()),
                )]),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: reference("location", "current"),
                subject: reference("entity", "a"),
                field: "position".into(),
                valid_from: 103,
                valid_to: None,
                value: GeoValue::Point {
                    point: GeoPoint {
                        longitude: -73.9857,
                        latitude: 40.7484,
                    },
                },
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Object {
            object: object_version(b"catalogued object revision two", "2"),
        },
    ];
    let update = RuntimeCommit {
        scope: scope.clone(),
        at: 103,
        actor: "agent:crud-test".into(),
        expected_cursor: cursor,
        mutations: update_mutations,
    };
    let updated = engine.commit_runtime(&update).unwrap();
    let replacement_event_cursor = cursor + 6;
    let (_, snapshot) = engine.runtime_data_snapshot(&scope, 103, 4_096).unwrap();
    assert_eq!(snapshot.records.len(), 4);
    assert_eq!(snapshot.relations.len(), 1);
    assert_eq!(snapshot.events.len(), 2);
    assert_eq!(snapshot.vectors.len(), 1);
    assert_eq!(snapshot.series.len(), 1);
    assert_eq!(snapshot.geo.len(), 1);
    assert_eq!(snapshot.objects.len(), 1);
    assert!(snapshot.contains(
        RuntimeLogicalModel::Event,
        &reference("observed", &format!("cursor:{replacement_event_cursor}"))
    ));
    assert!(!snapshot.contains(
        RuntimeLogicalModel::Event,
        &reference("observed", "cursor:7")
    ));

    let retirements = vec![
        retire(RuntimeLogicalModel::Relational, "entity", "a", 104),
        retire(RuntimeLogicalModel::Document, "document", "doc-a", 104),
        retire(RuntimeLogicalModel::KeyValue, "kv", "feature-flag", 104),
        retire(
            RuntimeLogicalModel::ReasoningRecord,
            "reasoning",
            "run-a",
            104,
        ),
        retire(RuntimeLogicalModel::GraphRelation, "links", "a-doc", 104),
        retire(
            RuntimeLogicalModel::Event,
            "observed",
            &format!("cursor:{replacement_event_cursor}"),
            104,
        ),
        retire(
            RuntimeLogicalModel::LifecycleEvent,
            "lifecycle",
            "cursor:12",
            104,
        ),
        retire(
            RuntimeLogicalModel::Vector,
            "embedding",
            "entity-a-title",
            104,
        ),
        retire(
            RuntimeLogicalModel::TimeSeries,
            "sample",
            "temperature-100",
            104,
        ),
        retire(RuntimeLogicalModel::Geo, "location", "current", 104),
        retire(RuntimeLogicalModel::Object, "object", "artifact", 104),
    ];
    let retired = engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 104,
            actor: "agent:crud-test".into(),
            expected_cursor: updated.last_cursor,
            mutations: retirements,
        })
        .unwrap();
    let (_, empty) = engine.runtime_data_snapshot(&scope, 104, 4_096).unwrap();
    assert!(empty.records.is_empty());
    assert!(empty.relations.is_empty());
    assert!(empty.events.is_empty());
    assert!(empty.vectors.is_empty());
    assert!(empty.series.is_empty());
    assert!(empty.geo.is_empty());
    assert!(empty.objects.is_empty());

    let (_, historical) = engine.runtime_data_snapshot(&scope, 103, 4_096).unwrap();
    assert_eq!(historical.records.len(), 4);
    assert_eq!(historical.events.len(), 2);

    let recreated = engine
        .commit_runtime(&RuntimeCommit {
            scope: scope.clone(),
            at: 105,
            actor: "agent:crud-test".into(),
            expected_cursor: retired.last_cursor,
            mutations: vec![RuntimeMutation::Record {
                record: record_at(
                    "document",
                    "doc-a",
                    105,
                    BTreeMap::from([("revision".into(), RuntimeValue::Unsigned(3))]),
                ),
            }],
        })
        .unwrap();

    let rejected = RuntimeCommit {
        scope: scope.clone(),
        at: 106,
        actor: "agent:crud-test".into(),
        expected_cursor: recreated.last_cursor,
        mutations: vec![
            RuntimeMutation::Record {
                record: record_at(
                    "document",
                    "doc-a",
                    106,
                    BTreeMap::from([("revision".into(), RuntimeValue::Unsigned(4))]),
                ),
            },
            retire(RuntimeLogicalModel::Document, "document", "missing", 106),
        ],
    };
    assert!(engine.commit_runtime(&rejected).is_err());
    assert_eq!(engine.runtime_cursor().unwrap(), recreated.last_cursor);
    let (_, final_snapshot) = engine.runtime_data_snapshot(&scope, 106, 4_096).unwrap();
    assert_eq!(final_snapshot.records.len(), 1);
    assert_eq!(
        final_snapshot.records[0].value.properties["revision"],
        RuntimeValue::Unsigned(3)
    );
    (scope, recreated.last_cursor, final_snapshot)
}

#[test]
fn multi_model_create_update_retire_and_recreate_are_identical_across_engines() {
    let (_, _, memory) = exercise_crud(&MemoryEngine::new());

    let compatibility = tempfile::tempdir().unwrap();
    let (_, _, fjall) = exercise_crud(&Store::open(compatibility.path()).unwrap());
    assert_eq!(memory, fjall);

    let native = tempfile::tempdir().unwrap();
    let (_, _, native) = exercise_crud(&NativeEngine::open(native.path()).unwrap());
    assert_eq!(memory, native);
}

#[test]
fn mixed_model_crud_snapshot_is_exact_after_fjall_and_native_reopen() {
    let compatibility = tempfile::tempdir().unwrap();
    let (scope, cursor, expected) = {
        let engine = Store::open(compatibility.path()).unwrap();
        exercise_crud(&engine)
    };
    let reopened = Store::open(compatibility.path()).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), cursor);
    assert_eq!(
        reopened
            .runtime_data_snapshot(&scope, 106, 4_096)
            .unwrap()
            .1,
        expected
    );

    let native = tempfile::tempdir().unwrap();
    let path = native.path().join("native");
    let (scope, cursor, expected) = {
        let engine = NativeEngine::open(&path).unwrap();
        exercise_crud(&engine)
    };
    let reopened = NativeEngine::open(&path).unwrap();
    assert_eq!(reopened.runtime_cursor().unwrap(), cursor);
    assert_eq!(
        reopened
            .runtime_data_snapshot(&scope, 106, 4_096)
            .unwrap()
            .1,
        expected
    );
}

#[test]
fn concurrent_updates_from_one_read_stamp_allow_exactly_one_writer() {
    let engine = std::sync::Arc::new(MemoryEngine::new());
    let (scope, cursor) = exercise(engine.as_ref());
    let commits = ["writer-a", "writer-b"].map(|actor| RuntimeCommit {
        scope: scope.clone(),
        at: 103,
        actor: actor.into(),
        expected_cursor: cursor,
        mutations: vec![RuntimeMutation::Record {
            record: record_at(
                "document",
                "doc-a",
                103,
                BTreeMap::from([("writer".into(), RuntimeValue::String(actor.into()))]),
            ),
        }],
    });
    let results = std::thread::scope(|threads| {
        commits
            .into_iter()
            .map(|commit| {
                let engine = engine.clone();
                threads.spawn(move || engine.commit_runtime(&commit))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}
