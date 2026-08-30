//! Child process for the durability test in `tests/durability.rs`.
//!
//! Writes claims through a [`Writer`], optionally flushes, then announces
//! readiness and blocks. The parent terminates it with SIGKILL, so no
//! destructor, no `Drop`, and no shutdown path can contribute to what survives.
//!
//! ```text
//! durability-child <db-path> <count> <flush|noflush|native-transaction|native-lock>
//! ```

use rrd_core::{
    Claim, DataTransaction, GeoPoint, GeoValue, Predicate, Producer, RuntimeCommit, RuntimeEvent,
    RuntimeEventSchema, RuntimeGeo, RuntimeMutation, RuntimeProperties, RuntimeRecord,
    RuntimeRecordSchema, RuntimeRef, RuntimeRelation, RuntimeRelationSchema, RuntimeSchemaRegistry,
    RuntimeSeriesSample, RuntimeType, RuntimeVector, ScopeId, SeriesValue, Subject, VectorValue,
};
use rrd_store::{Engine, NativeEngine, Store, Writer, WriterConfig};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("db path");
    let count: usize = args.next().expect("count").parse().expect("count");
    let mode = args
        .next()
        .expect("flush|noflush|native-transaction|native-lock");
    let path = Path::new(&path);

    match mode.as_str() {
        "native-transaction" => native_transaction(path),
        "native-lock" => hold_native_lock(path),
        "flush" | "noflush" => {}
        other => panic!("unknown durability child mode {other:?}"),
    }

    let store = Arc::new(Store::open(path).expect("open store"));

    // In `noflush` mode the delay must exceed the lifetime of the process, so
    // that an absent claim proves the contract rather than losing a race with
    // the interval timer.
    let flush_delay = if mode == "flush" {
        Duration::from_millis(5)
    } else {
        Duration::from_secs(3600)
    };

    let writer = Writer::spawn(
        Arc::clone(&store),
        WriterConfig {
            flush_delay,
            max_batch: 1024,
            queue_capacity: 8192,
        },
    );

    for i in 0..count {
        let claim = Claim::new(
            Subject::new(format!("s{i}")).unwrap(),
            Predicate::new("status").unwrap(),
            "in_progress",
            100 + i as u64,
            100 + i as u64,
            Producer {
                actor: "durability-child".into(),
                on_behalf_of: None,
                session: None,
            },
        );
        writer.submit(claim).expect("submit");
    }

    if mode == "flush" {
        writer.flush().expect("flush");
    }

    // Leak the writer so that no unwinding or destructor can flush after the
    // readiness signal. The parent kills this process.
    std::mem::forget(writer);
    ready_and_wait()
}

fn native_transaction(path: &Path) -> ! {
    let engine = NativeEngine::open(path).expect("open native engine");
    let scope = ScopeId::new("instance:native-durability").unwrap();
    let read = engine.runtime_read_stamp(&scope).unwrap();
    let transaction = DataTransaction::new(
        read,
        RuntimeCommit {
            scope,
            at: 100,
            actor: "agent:durability-child".into(),
            expected_cursor: 0,
            mutations: native_mixed_family_mutations(),
        },
    )
    .unwrap();
    let outcome = engine.commit_data_transaction(&transaction).unwrap();
    assert_eq!(outcome.count, 9);
    ready_and_wait()
}

fn native_mixed_family_mutations() -> Vec<RuntimeMutation> {
    let entity = RuntimeType::new("entity").unwrap();
    let mut registry = RuntimeSchemaRegistry::empty(1, "native durability schema");
    registry
        .records
        .insert(entity.clone(), RuntimeRecordSchema::default());
    registry.relations.insert(
        RuntimeType::new("links").unwrap(),
        RuntimeRelationSchema {
            from: BTreeSet::from([entity.clone()]),
            to: BTreeSet::from([entity.clone()]),
            ..RuntimeRelationSchema::default()
        },
    );
    registry.events.insert(
        RuntimeType::new("observed").unwrap(),
        RuntimeEventSchema {
            subject_required: true,
            subject_types: BTreeSet::from([entity]),
            properties: BTreeMap::new(),
            allow_additional_properties: false,
        },
    );
    let a = RuntimeRef::new("entity", "a").unwrap();
    let b = RuntimeRef::new("entity", "b").unwrap();
    let record = |reference| RuntimeRecord {
        reference,
        valid_from: 100,
        valid_to: None,
        properties: RuntimeProperties::new(),
    };
    vec![
        RuntimeMutation::Schema { registry },
        RuntimeMutation::Record {
            record: record(a.clone()),
        },
        RuntimeMutation::Record {
            record: record(b.clone()),
        },
        RuntimeMutation::Relation {
            relation: RuntimeRelation {
                reference: RuntimeRef::new("links", "a-b").unwrap(),
                from: a.clone(),
                to: b,
                valid_from: 100,
                valid_to: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Event {
            event: RuntimeEvent {
                kind: RuntimeType::new("observed").unwrap(),
                subject: Some(a.clone()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Vector {
            vector: RuntimeVector {
                reference: RuntimeRef::new("embedding", "a-title").unwrap(),
                subject: a.clone(),
                collection: None,
                field: "title".into(),
                valid_from: 100,
                valid_to: None,
                value: VectorValue::Dense {
                    values: vec![0.0, 0.6, 0.8],
                },
                provenance: None,
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::SeriesSample {
            sample: RuntimeSeriesSample {
                reference: RuntimeRef::new("sample", "a-temperature-100").unwrap(),
                series: a.clone(),
                observed_at: 100,
                value: SeriesValue::Decimal("21.5".into()),
                properties: RuntimeProperties::new(),
            },
        },
        RuntimeMutation::Geo {
            geo: RuntimeGeo {
                reference: RuntimeRef::new("location", "a-current").unwrap(),
                subject: a,
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
        RuntimeMutation::Claim {
            claim: Claim::new(
                Subject::new("entity:a").unwrap(),
                Predicate::new("status").unwrap(),
                "durable",
                100,
                100,
                Producer {
                    actor: "agent:durability-child".into(),
                    on_behalf_of: None,
                    session: Some("native-sigkill".into()),
                },
            ),
        },
    ]
}

fn hold_native_lock(path: &Path) -> ! {
    let _engine = NativeEngine::open(path).expect("open native lock owner");
    ready_and_wait()
}

fn ready_and_wait() -> ! {
    println!("READY");
    std::io::stdout().flush().expect("flush stdout");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
