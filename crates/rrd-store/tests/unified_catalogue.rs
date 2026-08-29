use std::collections::{BTreeMap, BTreeSet};

use rrd_core::{
    digest, GeoPoint, GeoValue, ObjectReceipt, ObjectReference, RuntimeCatalogueIdentity,
    RuntimeCommit, RuntimeEvent, RuntimeEventSchema, RuntimeGeo, RuntimeLogicalModel,
    RuntimeMutation, RuntimeProperties, RuntimePropertySchema, RuntimeRecord, RuntimeRecordSchema,
    RuntimeRef, RuntimeRelation, RuntimeRelationSchema, RuntimeSchemaMode, RuntimeSchemaRegistry,
    RuntimeSeriesSample, RuntimeTableSchema, RuntimeType, RuntimeValue, RuntimeValueType,
    RuntimeVector, ScopeId, SeriesValue, VectorValue,
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
    RuntimeRecord {
        reference: reference(kind, id),
        valid_from: 100,
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
    let bytes = b"catalogued object";
    let sha256 = digest::sha256_hex(bytes);
    let mut object = ObjectReference::for_bytes(
        "artifact",
        Some(reference("entity", "a")),
        "text/plain",
        bytes,
        ObjectReceipt {
            backend: "fixture".into(),
            key: ObjectReference::canonical_key(&sha256).unwrap(),
            version: Some("1".into()),
            etag: None,
        },
    )
    .unwrap();
    object
        .properties
        .insert("class".into(), RuntimeValue::String("evidence".into()));
    object
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
