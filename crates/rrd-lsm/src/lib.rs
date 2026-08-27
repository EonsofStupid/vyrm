//! Native storage substrate for Rrd.
//!
//! This crate begins at the durable boundary. One accepted atomic batch is one
//! checksummed WAL frame. Recovery accepts only a contiguous valid prefix;
//! incomplete tail bytes are reported for explicit repair, while corruption in
//! a complete frame fails closed.

mod batch;
mod database;
mod error;
mod manifest;
mod memtable;
mod segment;
mod snapshot_bundle;
mod wal;

use std::path::Path;

#[cfg(unix)]
/// Requests durable directory-entry metadata on platforms that expose a
/// directory `fsync` operation.
pub fn sync_directory(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
/// Completes the portable directory-publication boundary on platforms without
/// a directory `fsync` operation. Windows publication durability is supplied
/// by [`publish_rename`]'s write-through rename instead.
pub fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
/// Atomically publishes a completed temporary file or directory and requests
/// durable parent-directory metadata where the platform supports it.
pub fn publish_rename(directory: &Path, temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)?;
    sync_directory(directory)
}

#[cfg(windows)]
/// Atomically replaces a target using write-through Windows rename semantics.
pub fn publish_rename(_directory: &Path, temporary: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
/// Publishes a completed temporary path on other supported platforms.
pub fn publish_rename(_directory: &Path, temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

pub use batch::{Mutation, WriteBatch, BATCH_FORMAT_VERSION};
pub use database::{
    CompactionBoundary, CompactionOutcome, CompactionPolicy, Database, DatabaseOptions,
    FailureMode, FlushBoundary, GarbageCollectionReport, MaintenancePolicy, MaintenanceStats,
    Snapshot, SnapshotInstallBoundary, DEFAULT_COMPACTION_MAX_INPUT_SEGMENTS,
    DEFAULT_COMPACTION_TARGET_SEGMENT_BYTES, DEFAULT_L0_COMPACTION_TRIGGER,
    DEFAULT_MAX_COMPACTION_LEVEL, DEFAULT_MEMTABLE_MAX_VERSIONS, DEFAULT_WAL_PAYLOAD_MAX_BYTES,
};
pub use error::{Error, Result};
pub use manifest::{
    Checkpoint, CurrentPointer, Manifest, ManifestStore, SegmentDescriptor, MANIFEST_FORMAT_VERSION,
};
pub use memtable::{Memtable, MemtableProfile, VersionedValue};
pub use segment::{
    BlockCacheStats, Segment, DEFAULT_BLOCK_CACHE_BYTES, SEGMENT_BLOCK_TARGET_BYTES,
    SEGMENT_FORMAT_VERSION,
};
pub use snapshot_bundle::{
    SnapshotBundle, SnapshotBundleFile, SnapshotExportBoundary, SnapshotSegment,
    SNAPSHOT_BUNDLE_FORMAT_VERSION, SNAPSHOT_BUNDLE_MAX_BYTES,
};
pub use wal::{
    recover, recover_from, repair_torn_tail, AppendReceipt, Durability, RecoveredBatch, Recovery,
    WalBatch, WalWriter, WAL_FORMAT_VERSION, WAL_MAX_PAYLOAD_BYTES,
};

#[cfg(test)]
mod durable_fs_tests {
    use super::*;

    #[test]
    fn directory_sync_uses_the_platform_durability_boundary() {
        let root = tempfile::tempdir().unwrap();
        sync_directory(root.path()).unwrap();
    }

    #[test]
    fn durable_rename_publishes_files_and_directories() {
        let root = tempfile::tempdir().unwrap();

        let temporary_file = root.path().join("file.pending");
        let target_file = root.path().join("file.ready");
        std::fs::write(&temporary_file, b"ready").unwrap();
        std::fs::File::open(&temporary_file)
            .unwrap()
            .sync_all()
            .unwrap();
        publish_rename(root.path(), &temporary_file, &target_file).unwrap();
        assert_eq!(std::fs::read(&target_file).unwrap(), b"ready");
        assert!(!temporary_file.exists());

        let temporary_directory = root.path().join("directory.pending");
        let target_directory = root.path().join("directory.ready");
        std::fs::create_dir(&temporary_directory).unwrap();
        std::fs::write(temporary_directory.join("entry"), b"durable").unwrap();
        publish_rename(root.path(), &temporary_directory, &target_directory).unwrap();
        assert_eq!(
            std::fs::read(target_directory.join("entry")).unwrap(),
            b"durable"
        );
        assert!(!temporary_directory.exists());
    }
}
