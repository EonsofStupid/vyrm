//! Deterministic physical storage differential required before Fjall removal.
//!
//! This intentionally exercises raw put/update/delete semantics: the typed
//! runtime contract does not yet define referentially safe entity deletion.

use fjall::{KeyspaceCreateOptions, PersistMode, Readable, SingleWriterTxDatabase};
use std::collections::BTreeMap;
use vyrm_core::digest::Sha256;
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const OPERATIONS: usize = 20_000;
const BATCH: usize = 250;
const KEY_CARDINALITY: u64 = 2_048;
const SEED: u64 = 0x6a09_e667_f3bc_c909;

#[test]
fn mixed_put_update_delete_reopen_compaction_matches_fjall_and_model() {
    let root = tempfile::tempdir().unwrap();
    let native_path = root.path().join("native");
    let fjall_path = root.path().join("fjall");
    let mut native = Database::create(&native_path).unwrap();
    let mut fjall = open_fjall(&fjall_path);
    let mut keyspace = fjall
        .keyspace("soak", KeyspaceCreateOptions::default)
        .unwrap();
    let mut model = BTreeMap::<Vec<u8>, Vec<u8>>::new();
    let mut random = SEED;
    let mut puts = 0u64;
    let mut deletes = 0u64;
    let mut updates = 0u64;
    let mut reopen_count = 0u64;
    let mut compaction_count = 0u64;

    for (epoch, offset) in (0..OPERATIONS).step_by(BATCH).enumerate() {
        let mut native_ops = Vec::with_capacity(BATCH);
        let mut fjall_tx = fjall.write_tx().durability(Some(PersistMode::SyncAll));
        for operation in offset..(offset + BATCH).min(OPERATIONS) {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let key = format!("key/{:04}", random % KEY_CARDINALITY).into_bytes();
            if (random >> 17).is_multiple_of(5) {
                model.remove(&key);
                fjall_tx.remove(&keyspace, &key);
                native_ops.push(Mutation::Delete { key });
                deletes += 1;
            } else {
                if model.contains_key(&key) {
                    updates += 1;
                } else {
                    puts += 1;
                }
                let value = format!("value/{operation:08}/{random:016x}").into_bytes();
                model.insert(key.clone(), value.clone());
                fjall_tx.insert(&keyspace, &key, &value);
                native_ops.push(Mutation::Put { key, value });
            }
        }
        fjall_tx.commit().unwrap();
        native
            .write_owned(
                WriteBatch::new(native_ops).unwrap(),
                Durability::Authoritative,
            )
            .unwrap();
        native.flush_memtable((epoch + 1) as u64).unwrap();

        if (epoch + 1) % 10 == 0 {
            native.compact(&[], (epoch + 1) as u64).unwrap();
            compaction_count += 1;
        }
        if (epoch + 1) % 8 == 0 {
            drop(keyspace);
            drop(fjall);
            drop(native);
            fjall = open_fjall(&fjall_path);
            keyspace = fjall
                .keyspace("soak", KeyspaceCreateOptions::default)
                .unwrap();
            native = Database::open(&native_path).unwrap();
            reopen_count += 1;
        }
        if (epoch + 1) % 5 == 0 {
            assert_all_equal(&native, &fjall, &keyspace, &model);
        }
    }

    assert_all_equal(&native, &fjall, &keyspace, &model);
    let digest = model_digest(&model);
    let actual = serde_json::json!({
        "contract": "vyrmkv-fjall-mixed-storage-soak-v1",
        "seed": format!("0x{SEED:016x}"),
        "operations": OPERATIONS,
        "inserts": puts,
        "updates": updates,
        "deletes": deletes,
        "key_cardinality": KEY_CARDINALITY,
        "visible_keys": model.len(),
        "reopens": reopen_count,
        "compactions": compaction_count,
        "final_sha256": digest,
        "result": "identical",
    });
    eprintln!("{actual}");
    let checked: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/evidence/m4-storage-mixed-soak.json"
    ))
    .unwrap();
    for field in [
        "contract",
        "seed",
        "operations",
        "inserts",
        "updates",
        "deletes",
        "key_cardinality",
        "visible_keys",
        "reopens",
        "compactions",
        "final_sha256",
        "result",
    ] {
        assert_eq!(
            checked[field], actual[field],
            "stale evidence field {field}"
        );
    }
}

fn open_fjall(path: &std::path::Path) -> SingleWriterTxDatabase {
    SingleWriterTxDatabase::builder(path)
        .manual_journal_persist(true)
        .open()
        .unwrap()
}

fn assert_all_equal(
    native: &Database,
    fjall: &SingleWriterTxDatabase,
    keyspace: &fjall::SingleWriterTxKeyspace,
    model: &BTreeMap<Vec<u8>, Vec<u8>>,
) {
    let native_values: BTreeMap<_, _> = native
        .scan(&[], None, native.snapshot())
        .unwrap()
        .into_iter()
        .collect();
    let fjall_values: BTreeMap<_, _> = fjall
        .read_tx()
        .iter(keyspace)
        .map(|item| {
            let (key, value) = item.into_inner().unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect();
    assert_eq!(&native_values, model, "vyrmKV differs from reference model");
    assert_eq!(&fjall_values, model, "Fjall differs from reference model");
}

fn model_digest(model: &BTreeMap<Vec<u8>, Vec<u8>>) -> String {
    let mut digest = Sha256::new();
    for (key, value) in model {
        digest.update(&(key.len() as u64).to_be_bytes());
        digest.update(key);
        digest.update(&(value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    digest.finalize_hex()
}
