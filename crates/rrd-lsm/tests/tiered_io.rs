use rrd_lsm::{
    Database, DatabaseOptions, Durability, Mutation, SegmentIoMode, SegmentIoPolicy,
    SegmentIoStats, WriteBatch,
};

const CACHE_BYTES: usize = 16 * 1024;
const REQUEST_BYTES: usize = 16 * 1024 * 1024;

fn write_fixture(root: &std::path::Path) {
    let mut database = Database::create(root).unwrap();
    let mutations = (0..96)
        .map(|index| Mutation::Put {
            key: format!("key-{index:03}").into_bytes(),
            value: vec![u8::try_from(index).unwrap(); 2_049],
        })
        .collect::<Vec<_>>();
    database
        .write(
            &WriteBatch::new(mutations).unwrap(),
            Durability::Authoritative,
        )
        .unwrap();
    database.flush_memtable(100).unwrap();
}

fn read_with_mode(root: &std::path::Path, mode: SegmentIoMode) -> (Vec<Vec<u8>>, SegmentIoStats) {
    let database = Database::open_with_options(
        root,
        DatabaseOptions {
            block_cache_bytes: CACHE_BYTES,
            segment_io: SegmentIoPolicy {
                mode,
                allow_fallback: true,
                max_request_bytes: REQUEST_BYTES,
            },
            ..DatabaseOptions::default()
        },
    )
    .unwrap();
    let snapshot = database.snapshot();
    let values = [0, 17, 51, 95]
        .into_iter()
        .map(|index| {
            database
                .get(format!("key-{index:03}").as_bytes(), snapshot)
                .unwrap()
                .unwrap()
        })
        .collect();
    let cache = database.block_cache_stats();
    assert!(cache.resident_bytes <= CACHE_BYTES);
    let stats = database.segment_io_stats();
    assert_eq!(stats.configured_max_request_bytes, REQUEST_BYTES);
    assert!(stats.read_operations > 0);
    assert!(stats.bytes_read > 0);
    assert!(stats.peak_request_bytes <= REQUEST_BYTES);
    (values, stats)
}

#[test]
fn mmap_and_bounded_reads_are_identical_and_measured_separately_from_cache() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("native");
    write_fixture(&root);

    let (bounded, bounded_stats) = read_with_mode(&root, SegmentIoMode::Bounded);
    let (mapped, mapped_stats) = read_with_mode(&root, SegmentIoMode::Mmap);
    assert_eq!(mapped, bounded);
    assert!(bounded_stats.bounded_segments > 0);
    assert!(bounded_stats.bounded_read_operations > 0);
    assert_eq!(bounded_stats.mmap_segments, 0);
    assert!(mapped_stats.mmap_segments > 0);
    assert!(mapped_stats.mmap_read_operations > 0);
    assert_eq!(mapped_stats.bounded_segments, 0);
}

#[test]
fn io_uring_runs_when_the_kernel_admits_it_or_records_the_bounded_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("native");
    write_fixture(&root);

    let (expected, _) = read_with_mode(&root, SegmentIoMode::Bounded);
    let (actual, stats) = read_with_mode(&root, SegmentIoMode::IoUring);
    assert_eq!(actual, expected);
    if stats.io_uring_segments > 0 {
        assert_eq!(stats.bounded_segments, 0);
        assert!(stats.io_uring_read_operations > 0 || stats.bounded_read_operations > 0);
        if stats.bounded_read_operations > 0 {
            assert!(stats.fallback_count > 0);
            assert!(stats
                .last_fallback_reason
                .as_deref()
                .is_some_and(|reason| !reason.is_empty()));
        } else {
            assert_eq!(stats.fallback_count, 0);
            assert_eq!(stats.last_fallback_reason, None);
        }
    } else {
        assert!(stats.bounded_segments > 0);
        assert!(stats.fallback_count > 0);
        assert!(stats
            .last_fallback_reason
            .as_deref()
            .is_some_and(|reason| !reason.is_empty()));
    }
}
