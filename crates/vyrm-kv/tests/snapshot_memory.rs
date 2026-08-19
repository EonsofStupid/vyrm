#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const SEGMENTS: usize = 20;
const VALUE_BYTES: usize = 1024 * 1024;
const MAX_EXPORT_RSS_GROWTH_BYTES: u64 = 16 * 1024 * 1024;

#[test]
fn file_snapshot_export_does_not_grow_with_the_whole_bundle() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path()).unwrap();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for index in 0..SEGMENTS {
        let mut value = vec![0u8; VALUE_BYTES];
        for chunk in value.chunks_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let bytes = state.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
        database
            .write_owned(
                WriteBatch::new(vec![Mutation::Put {
                    key: format!("segment-{index:02}").into_bytes(),
                    value,
                }])
                .unwrap(),
                Durability::Authoritative,
            )
            .unwrap();
        database.flush_memtable(index as u64 + 1).unwrap();
    }

    let baseline = resident_bytes();
    let running = Arc::new(AtomicBool::new(true));
    let peak = Arc::new(AtomicU64::new(baseline));
    let sampler_running = Arc::clone(&running);
    let sampler_peak = Arc::clone(&peak);
    let sampler = std::thread::spawn(move || {
        while sampler_running.load(Ordering::Acquire) {
            sampler_peak.fetch_max(resident_bytes(), Ordering::AcqRel);
            std::thread::sleep(Duration::from_micros(200));
        }
        sampler_peak.fetch_max(resident_bytes(), Ordering::AcqRel);
    });

    let path = directory.path().join("bounded.snapshot");
    let snapshot = database.export_snapshot_file(100, &path).unwrap();
    running.store(false, Ordering::Release);
    sampler.join().unwrap();

    assert!(
        snapshot.length > MAX_EXPORT_RSS_GROWTH_BYTES,
        "fixture must exceed the allowed incremental RSS"
    );
    let growth = peak.load(Ordering::Acquire).saturating_sub(baseline);
    assert!(
        growth <= MAX_EXPORT_RSS_GROWTH_BYTES,
        "file snapshot export grew RSS by {growth} bytes for a {} byte bundle",
        snapshot.length
    );
}

fn resident_bytes() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .expect("Linux proc status carries VmRSS");
    kib * 1024
}
