#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const SEGMENTS: usize = 20;
const VALUE_BYTES: usize = 1024 * 1024;
const MAX_EXPORT_RSS_GROWTH_BYTES: u64 = 16 * 1024 * 1024;
const READ_CACHE_BYTES: usize = 4 * 1024 * 1024;

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

#[test]
fn disk_resident_reopen_and_reads_are_bounded_by_the_block_cache() {
    if let Some(path) = std::env::var_os("VYRM_SEGMENT_MEMORY_CHILD") {
        let baseline = resident_bytes();
        let database =
            Database::open_with_block_cache(std::path::Path::new(&path), READ_CACHE_BYTES).unwrap();
        let snapshot = database.snapshot();
        for index in 0..SEGMENTS {
            assert!(database
                .get(format!("segment-{index:02}").as_bytes(), snapshot)
                .unwrap()
                .is_some());
        }
        let growth = resident_bytes().saturating_sub(baseline);
        let stats = database.block_cache_stats();
        assert!(stats.resident_bytes <= READ_CACHE_BYTES);
        assert!(stats.evictions > 0);
        assert!(
            growth <= MAX_EXPORT_RSS_GROWTH_BYTES,
            "opening and reading {} MiB of segments grew RSS by {growth} bytes",
            SEGMENTS
        );
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    populate_large_segments(directory.path());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("disk_resident_reopen_and_reads_are_bounded_by_the_block_cache")
        .arg("--nocapture")
        .env("VYRM_SEGMENT_MEMORY_CHILD", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "memory child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn populate_large_segments(path: &std::path::Path) {
    let mut database = Database::create(path).unwrap();
    let mut state = 0x243f_6a88_85a3_08d3u64;
    for index in 0..SEGMENTS {
        let mut value = vec![0u8; VALUE_BYTES];
        for chunk in value.chunks_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_le_bytes()[..chunk.len()]);
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
