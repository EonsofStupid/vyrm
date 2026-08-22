use serde::Serialize;
use std::collections::BTreeMap;
use vyrm_core::{
    ReadStamp, RuntimeEventSchema, RuntimeRecordSchema, RuntimeSchemaRegistry, RuntimeType, ScopeId,
};
use vyrm_mx::{bind, plan, Catalog, Parameters, SchemaVersion};
use vyrm_ql::parse;

#[derive(Serialize)]
struct Vector {
    source: &'static str,
    plan: vyrm_mx::PhysicalPlan,
}

#[test]
fn physical_plan_matches_golden_vector() {
    let scope = ScopeId::new("instance:golden").unwrap();
    let read = ReadStamp::new(scope, Some(1), 1, 4, Some("11".repeat(32))).unwrap();
    let mut schema = RuntimeSchemaRegistry::empty(1, "golden query contract");
    schema.records.insert(
        RuntimeType::new("document").unwrap(),
        RuntimeRecordSchema {
            allow_additional_properties: false,
            properties: BTreeMap::new(),
            ..RuntimeRecordSchema::default()
        },
    );
    schema.events.insert(
        RuntimeType::new("tool_result").unwrap(),
        RuntimeEventSchema::default(),
    );
    let catalog = Catalog {
        read,
        schemas: vec![SchemaVersion {
            cursor: 1,
            registry: schema,
        }],
    };
    let vectors = [
        "FROM record:document AT VALID 100 KNOWN 4 PROJECT id LIMIT 5 EXPLAIN CONTRACT",
        "FROM event:tool_result AT VALID 100 KNOWN 4 WHERE cursor = 4 PROJECT cursor EXPLAIN CONTRACT",
    ]
    .into_iter()
    .map(|source| {
        let query = parse(source).unwrap();
        Vector {
            source,
            plan: plan(&bind(&query, &Parameters::new(), &catalog).unwrap()).unwrap(),
        }
    })
    .collect::<Vec<_>>();
    let actual = format!("{}\n", serde_json::to_string_pretty(&vectors).unwrap());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/plan-vectors.json");
    if std::env::var_os("VYRM_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))).unwrap();
        std::fs::write(path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("missing {path}; run with VYRM_UPDATE_GOLDENS=1"));
    assert_eq!(actual, expected);
}
