use std::collections::BTreeMap;

use rrd_contract::{
    CanonicalId, DataCatalogueIdentity, DataLogicalModel, DataPropertySchema, DataRecordSchema,
    DataReference, DataSchemaMode, DataSchemaRegistry, DataTableSchema, DataTarget, DataValueType,
    DataVectorValue, TransactionMutation,
};

#[test]
fn public_and_canonical_catalogues_round_trip_without_losing_model_or_mode() {
    let document = CanonicalId::new("document").unwrap();
    let embedding = CanonicalId::new("embedding").unwrap();
    let public = DataSchemaRegistry {
        revision: 7,
        migration: "round-trip unified catalogue".into(),
        catalogue: DataCatalogueIdentity {
            namespace: CanonicalId::new("project").unwrap(),
            database: CanonicalId::new("runtime").unwrap(),
        },
        tables: BTreeMap::from([
            (
                document.clone(),
                DataTableSchema {
                    model: DataLogicalModel::Document,
                    mode: DataSchemaMode::Strict,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
            (
                embedding,
                DataTableSchema {
                    model: DataLogicalModel::Vector,
                    mode: DataSchemaMode::Schemaless,
                    properties: BTreeMap::new(),
                    allow_additional_properties: false,
                },
            ),
        ]),
        records: BTreeMap::from([(
            document,
            DataRecordSchema {
                properties: BTreeMap::from([(
                    "title".into(),
                    DataPropertySchema {
                        value_type: DataValueType::String,
                        required: true,
                    },
                )]),
                ..DataRecordSchema::default()
            },
        )]),
        relations: BTreeMap::new(),
        events: BTreeMap::new(),
    };

    let canonical = super::super::runtime_schema(&public).unwrap();
    canonical.validate().unwrap();
    assert_eq!(super::super::public_schema(&canonical).unwrap(), public);
}

#[test]
fn public_event_retirement_round_trips_through_the_cursor_typed_canonical_target() {
    let public = TransactionMutation::RetireData {
        model: DataLogicalModel::Event,
        target: DataTarget::Event {
            kind: CanonicalId::new("observed").unwrap(),
            cursor: 17,
        },
        effective_at: 1_000,
    };
    let canonical = super::super::public_runtime_mutation(
        &public,
        &rrd_contract::CorrelationId::new("session-catalogue").unwrap(),
    )
    .unwrap();
    assert_eq!(
        super::super::public_data_mutation(&canonical).unwrap(),
        public
    );
}

#[test]
fn public_vector_round_trip_preserves_its_named_collection_address() {
    let public = TransactionMutation::PutVector {
        reference: DataReference {
            kind: CanonicalId::new("embedding").unwrap(),
            id: CanonicalId::new("alpha").unwrap(),
        },
        subject: DataReference {
            kind: CanonicalId::new("document").unwrap(),
            id: CanonicalId::new("alpha").unwrap(),
        },
        collection_id: Some(CanonicalId::new("documents").unwrap()),
        vector_name: Some(CanonicalId::new("body").unwrap()),
        field: CanonicalId::new("body-embedding").unwrap(),
        valid_from: 1_000,
        valid_to: None,
        value: DataVectorValue::Dense {
            values: vec![1.0, 0.0],
        },
        provenance: None,
        properties: BTreeMap::new(),
    };
    let canonical = super::super::public_runtime_mutation(
        &public,
        &rrd_contract::CorrelationId::new("session-vector-address").unwrap(),
    )
    .unwrap();
    assert_eq!(
        super::super::public_data_mutation(&canonical).unwrap(),
        public
    );
}
