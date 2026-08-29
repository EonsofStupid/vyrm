use std::collections::BTreeMap;

use rrd_contract::{
    CanonicalId, DataCatalogueIdentity, DataLogicalModel, DataPropertySchema, DataRecordSchema,
    DataSchemaMode, DataSchemaRegistry, DataTableSchema, DataValueType,
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
