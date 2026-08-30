use super::*;
use rrd_contract::{
    DataGeoPoint, DataGeoValue, DataLogicalModel, DataPropertySchema, DataRecordSchema,
    DataReference, DataSchemaMode, DataSchemaRegistry, DataTableSchema, DataValueType,
    EnsureQueryIndex, ExecuteQuery, QueryBudget, QueryFullTextConfiguration, QueryIndexKind,
    QueryTextStemmer, QueryValue,
};
use std::collections::{BTreeMap, BTreeSet};

fn reference(kind: &str, value: &str) -> DataReference {
    DataReference {
        kind: CanonicalId::new(kind).unwrap(),
        id: CanonicalId::new(value).unwrap(),
    }
}

fn commit(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    mutations: Vec<TransactionMutation>,
    at: u64,
    suffix: &str,
) -> std::result::Result<rrd_contract::CommitReceipt, ServiceError> {
    let transaction = engine.begin_transaction(
        &lease.session_id,
        &lease.token,
        &BeginTransaction {
            scope: CanonicalId::new("data").unwrap(),
            timeout_ms: 1_000,
        },
        &mutation_context(
            &id(&format!("begin-index-{suffix}")),
            &format!("request-begin-index-{suffix}"),
            &format!("operation-begin-index-{suffix}"),
        ),
        at,
    )?;
    let request = CommitTransaction {
        operation_sha256: rrd_contract::transaction_operation_sha256(&mutations),
        mutations,
    };
    engine.commit_transaction(
        &lease.session_id,
        &lease.token,
        &transaction.transaction_id,
        &id(&format!("commit-index-{suffix}")),
        &request,
        at,
        &format!("request-commit-index-{suffix}"),
        &format!("operation-commit-index-{suffix}"),
    )
}

#[allow(clippy::too_many_arguments)]
fn ensure(
    engine: &RrdEngine,
    lease: &rrd_contract::SessionLease,
    ordinal: u64,
    index_id: &str,
    definition_query: &str,
    unique: bool,
    kind: QueryIndexKind,
    full_text: Option<QueryFullTextConfiguration>,
) -> rrd_contract::EnsureQueryIndexResult {
    engine
        .ensure_query_index(
            &lease.session_id,
            &lease.token,
            &id(&format!("ensure-index-{ordinal}")),
            &EnsureQueryIndex {
                scope: format!("instance:{}", instance()),
                index_id: CanonicalId::new(index_id).unwrap(),
                definition_query: definition_query.into(),
                unique,
                kind,
                full_text,
                budget: QueryBudget::default(),
            },
            ordinal,
            &format!("request-ensure-index-{ordinal}"),
            &format!("operation-ensure-index-{ordinal}"),
        )
        .unwrap()
}

#[test]
fn public_engine_owns_every_index_family_and_enforces_unique_commits() {
    let (_root, engine) = isolated_engine();
    let lease = engine
        .create_session(
            &session_request(9_000, 4),
            &id("index-foundation-session"),
            10,
            "request-index-foundation-session",
            "operation-index-foundation-session",
        )
        .unwrap();
    let document = reference("document", "alpha");
    commit(
        &engine,
        &lease,
        vec![
            TransactionMutation::PutSchema {
                registry: DataSchemaRegistry {
                    revision: 1,
                    migration: "install index foundation fixture".into(),
                    catalogue: DataCatalogueIdentity::default(),
                    tables: BTreeMap::from([(
                        CanonicalId::new("location").unwrap(),
                        DataTableSchema {
                            model: DataLogicalModel::Geo,
                            mode: DataSchemaMode::Schemaless,
                            properties: BTreeMap::new(),
                            allow_additional_properties: false,
                        },
                    )]),
                    records: BTreeMap::from([(
                        CanonicalId::new("document").unwrap(),
                        DataRecordSchema {
                            properties: BTreeMap::from([
                                (
                                    "category".into(),
                                    DataPropertySchema {
                                        value_type: DataValueType::String,
                                        required: true,
                                    },
                                ),
                                (
                                    "body".into(),
                                    DataPropertySchema {
                                        value_type: DataValueType::String,
                                        required: true,
                                    },
                                ),
                            ]),
                            ..DataRecordSchema::default()
                        },
                    )]),
                    relations: BTreeMap::new(),
                    events: BTreeMap::new(),
                },
            },
            TransactionMutation::PutRecord {
                reference: document.clone(),
                valid_from: 1,
                valid_to: None,
                properties: BTreeMap::from([
                    ("category".into(), QueryValue::String("news".into())),
                    (
                        "body".into(),
                        QueryValue::String("The cafés running".into()),
                    ),
                ]),
            },
            TransactionMutation::PutRecord {
                reference: reference("document", "beta"),
                valid_from: 1,
                valid_to: None,
                properties: BTreeMap::from([
                    ("category".into(), QueryValue::String("archive".into())),
                    ("body".into(), QueryValue::String("quiet history".into())),
                ]),
            },
            TransactionMutation::PutGeo {
                reference: reference("location", "alpha"),
                subject: document,
                field: CanonicalId::new("position").unwrap(),
                valid_from: 1,
                valid_to: None,
                value: DataGeoValue::Point {
                    point: DataGeoPoint {
                        longitude: -122.4,
                        latitude: 37.8,
                    },
                },
                properties: BTreeMap::new(),
            },
        ],
        20,
        "seed",
    )
    .unwrap();

    let count = ensure(
        &engine,
        &lease,
        30,
        "document-count",
        "FROM record:document AT VALID 1 KNOWN HEAD PROJECT *",
        false,
        QueryIndexKind::Count,
        None,
    );
    assert_eq!(count.index.analytics_total_count, Some(2));
    assert_eq!(count.index.analytics_group_count, Some(0));
    assert_eq!(count.index.maintenance.as_ref().unwrap().mode, "full_build");

    let grouped = ensure(
        &engine,
        &lease,
        31,
        "document-category-count",
        "FROM record:document AT VALID 1 KNOWN HEAD PROJECT category",
        false,
        QueryIndexKind::AggregateCount,
        None,
    );
    assert_eq!(grouped.index.analytics_total_count, Some(2));
    assert_eq!(grouped.index.analytics_group_count, Some(2));

    let view = ensure(
        &engine,
        &lease,
        32,
        "news-documents",
        "FROM record:document AT VALID 1 KNOWN HEAD WHERE category = \"news\" PROJECT category, body",
        false,
        QueryIndexKind::MaterializedView,
        None,
    );
    assert_eq!(view.index.artifact_rows, Some(1));

    let geo = ensure(
        &engine,
        &lease,
        33,
        "document-location",
        "FROM geo:location AT VALID 1 KNOWN HEAD PROJECT geometry_kind, longitude, latitude",
        false,
        QueryIndexKind::Geo,
        None,
    );
    assert_eq!(geo.index.artifact_rows, Some(1));

    let text_config = QueryFullTextConfiguration {
        ascii_folding: true,
        stop_words: BTreeSet::from(["the".into()]),
        stemmer: QueryTextStemmer::English,
        ..QueryFullTextConfiguration::default()
    };
    let text = ensure(
        &engine,
        &lease,
        34,
        "document-body-bm25",
        "FROM record:document AT VALID 1 KNOWN HEAD PROJECT body",
        false,
        QueryIndexKind::Bm25,
        Some(text_config.clone()),
    );
    assert_eq!(text.index.full_text, Some(text_config));

    let search = engine
        .execute_query(
            &lease.session_id,
            &lease.token,
            &ExecuteQuery {
                scope: format!("instance:{}", instance()),
                query: "FROM record:document AT VALID 1 KNOWN HEAD WHERE body MATCH \"CAFE running\" PROJECT *".into(),
                parameters: BTreeMap::new(),
                budget: QueryBudget::default(),
            },
            35,
            "request-index-search",
            "operation-index-search",
        )
        .unwrap();
    let values = &search.rows[0].values;
    assert!(values.contains_key("_score"));
    assert!(values.contains_key("_matched_terms"));
    assert!(values.contains_key("_highlight_offsets"));
    assert_eq!(
        values["_highlight"],
        QueryValue::String("The <em>cafés</em> <em>running</em>".into())
    );

    ensure(
        &engine,
        &lease,
        36,
        "document-body-unique",
        "FROM record:document AT VALID 1 KNOWN HEAD PROJECT body",
        true,
        QueryIndexKind::Scalar,
        None,
    );
    let cursor_before = engine.readiness(37).unwrap().runtime_cursor;
    let duplicate = commit(
        &engine,
        &lease,
        vec![TransactionMutation::PutRecord {
            reference: reference("document", "gamma"),
            valid_from: 1,
            valid_to: None,
            properties: BTreeMap::from([
                ("category".into(), QueryValue::String("news".into())),
                (
                    "body".into(),
                    QueryValue::String("The cafés running".into()),
                ),
            ]),
        }],
        38,
        "duplicate",
    );
    assert!(
        matches!(duplicate, Err(ServiceError::Query(message)) if message.contains("rejects overlapping duplicate records"))
    );
    assert_eq!(engine.readiness(39).unwrap().runtime_cursor, cursor_before);
}
