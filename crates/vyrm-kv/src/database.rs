use crate::segment::{block_cache_stats, new_block_cache, SharedBlockCache};
use crate::{
    recover_from, AppendReceipt, Checkpoint, Durability, Error, Manifest, ManifestStore, Memtable,
    Result, Segment, SnapshotBundle, SnapshotBundleFile, SnapshotExportBoundary, SnapshotSegment,
    VersionedValue, WalWriter, WriteBatch,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const WAL_DIRECTORY: &str = "wal";
const SEGMENT_DIRECTORY: &str = "segments";
pub const DEFAULT_WAL_PAYLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MEMTABLE_MAX_VERSIONS: usize = 524_288;
pub const DEFAULT_L0_COMPACTION_TRIGGER: usize = 8;
pub const DEFAULT_COMPACTION_MAX_INPUT_SEGMENTS: usize = 16;
pub const DEFAULT_COMPACTION_TARGET_SEGMENT_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_COMPACTION_LEVEL: u8 = 6;

/// Synchronous write-path maintenance limits. Crossing either limit stalls the
/// next writer while the existing WAL-backed memtable is published. This keeps
/// recovery work and resident mutable state bounded without acknowledging a
/// write whose maintenance publication failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenancePolicy {
    pub wal_payload_max_bytes: usize,
    pub memtable_max_versions: usize,
}

impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            wal_payload_max_bytes: DEFAULT_WAL_PAYLOAD_MAX_BYTES,
            memtable_max_versions: DEFAULT_MEMTABLE_MAX_VERSIONS,
        }
    }
}

impl MaintenancePolicy {
    fn validate(self) -> Result<Self> {
        if self.wal_payload_max_bytes == 0 || self.memtable_max_versions == 0 {
            return Err(Error::InvalidConfiguration(
                "WAL payload and memtable version limits must be greater than zero".into(),
            ));
        }
        Ok(self)
    }
}

/// Deterministic bounds for one immutable maintenance step. The input bound
/// prevents a growing database from turning one compaction into an unbounded
/// all-segment rewrite; the output target is enforced at key boundaries so all
/// MVCC versions for one key remain together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionPolicy {
    pub l0_compaction_trigger: usize,
    pub max_input_segments: usize,
    pub target_segment_bytes: usize,
    pub max_level: u8,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            l0_compaction_trigger: DEFAULT_L0_COMPACTION_TRIGGER,
            max_input_segments: DEFAULT_COMPACTION_MAX_INPUT_SEGMENTS,
            target_segment_bytes: DEFAULT_COMPACTION_TARGET_SEGMENT_BYTES,
            max_level: DEFAULT_MAX_COMPACTION_LEVEL,
        }
    }
}

impl CompactionPolicy {
    fn validate(self) -> Result<Self> {
        if self.l0_compaction_trigger < 2
            || self.max_input_segments < 2
            || self.target_segment_bytes == 0
            || self.max_level == 0
        {
            return Err(Error::InvalidConfiguration(
                "compaction requires an L0 trigger and input bound of at least two, a non-zero output target, and at least one level".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseOptions {
    pub block_cache_bytes: usize,
    pub maintenance: MaintenancePolicy,
    pub compaction: CompactionPolicy,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            block_cache_bytes: crate::DEFAULT_BLOCK_CACHE_BYTES,
            maintenance: MaintenancePolicy::default(),
            compaction: CompactionPolicy::default(),
        }
    }
}

impl DatabaseOptions {
    fn validate(self) -> Result<Self> {
        self.maintenance.validate()?;
        self.compaction.validate()?;
        Ok(self)
    }
}

/// Process-local maintenance counters. Canonical state lives in the WAL,
/// segments, and manifests; these counters intentionally reset on reopen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceStats {
    pub automatic_flushes: u64,
    pub write_stalls: u64,
    pub failed_flushes: u64,
    pub oversized_batches: u64,
    pub peak_wal_payload_bytes: usize,
    pub peak_memtable_versions: usize,
    pub automatic_compactions: u64,
    pub failed_compactions: u64,
    pub compaction_input_bytes: u64,
    pub compaction_output_bytes: u64,
    pub peak_compaction_buffer_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionOutcome {
    pub previous_manifest: String,
    pub manifest: String,
    pub input_segments: usize,
    pub output_segments: usize,
    pub input_versions: u64,
    pub output_versions: u64,
    pub protected_sequences: Vec<u64>,
    pub source_level: u8,
    pub target_level: u8,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub peak_buffer_bytes: usize,
    pub history_pruned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactionCandidate {
    indices: Vec<usize>,
    source_level: u8,
    target_level: u8,
}

struct CompactionMerge {
    segments: Vec<Segment>,
    output_segments: usize,
    input_versions: u64,
    output_versions: u64,
    output_bytes: u64,
    peak_buffer_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GarbageCollectionReport {
    pub retained_manifests: Vec<String>,
    pub retained_segments: Vec<String>,
    pub retained_wals: Vec<String>,
    pub removed_manifests: Vec<String>,
    pub removed_segments: Vec<String>,
    pub removed_wals: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureMode {
    Crash,
    StorageFull,
}

impl FailureMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::StorageFull => "storage-full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushBoundary {
    WalSynced,
    SegmentSynced,
    SuccessorWalSynced,
    ManifestPublished,
}

impl FlushBoundary {
    fn as_str(self) -> &'static str {
        match self {
            Self::WalSynced => "flush.wal_synced",
            Self::SegmentSynced => "flush.segment_synced",
            Self::SuccessorWalSynced => "flush.successor_wal_synced",
            Self::ManifestPublished => "flush.manifest_published",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionBoundary {
    SegmentSynced,
    ManifestPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotInstallBoundary {
    SegmentsSynced,
    SuccessorWalSynced,
    ManifestPublished,
}

impl SnapshotInstallBoundary {
    fn as_str(self) -> &'static str {
        match self {
            Self::SegmentsSynced => "snapshot.segments_synced",
            Self::SuccessorWalSynced => "snapshot.successor_wal_synced",
            Self::ManifestPublished => "snapshot.manifest_published",
        }
    }
}

impl CompactionBoundary {
    fn as_str(self) -> &'static str {
        match self {
            Self::SegmentSynced => "compaction.segment_synced",
            Self::ManifestPublished => "compaction.manifest_published",
        }
    }
}

/// Single-writer native database with crash-ordered WAL → segment → manifest
/// publication. Fjall remains the compatibility oracle until the Engine-level
/// differential and performance gates close.
pub struct Database {
    root: PathBuf,
    manifests: ManifestStore,
    manifest: Manifest,
    wal: WalWriter,
    memtable: Memtable,
    segments: Vec<Segment>,
    block_cache: SharedBlockCache,
    maintenance: MaintenancePolicy,
    compaction: CompactionPolicy,
    maintenance_stats: MaintenanceStats,
    wal_payload_bytes: usize,
}

impl Database {
    pub fn create(root: &Path) -> Result<Self> {
        Self::create_with_options(root, DatabaseOptions::default())
    }

    pub fn create_with_block_cache(root: &Path, block_cache_bytes: usize) -> Result<Self> {
        Self::create_with_options(
            root,
            DatabaseOptions {
                block_cache_bytes,
                ..DatabaseOptions::default()
            },
        )
    }

    pub fn create_with_options(root: &Path, options: DatabaseOptions) -> Result<Self> {
        let options = options.validate()?;
        if root.exists() {
            if !root.is_dir() || std::fs::read_dir(root)?.next().is_some() {
                return Err(Error::InvalidManifest(
                    "new database path exists and is not an empty directory".into(),
                ));
            }
        } else {
            std::fs::create_dir(root)?;
        }
        let manifests = ManifestStore::open(root)?;
        std::fs::create_dir(root.join(WAL_DIRECTORY))?;
        std::fs::create_dir(root.join(SEGMENT_DIRECTORY))?;
        let manifest = Manifest::new(1, None, 0, 0, 1, Vec::new())?;
        let wal = WalWriter::create_at(&wal_path(root, 1), 1)?;
        manifests.publish(&manifest, None)?;
        Ok(Self {
            root: root.to_owned(),
            manifests,
            manifest,
            wal,
            memtable: Memtable::default(),
            segments: Vec::new(),
            block_cache: new_block_cache(options.block_cache_bytes),
            maintenance: options.maintenance,
            compaction: options.compaction,
            maintenance_stats: MaintenanceStats::default(),
            wal_payload_bytes: 0,
        })
    }

    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with_options(root, DatabaseOptions::default())
    }

    pub fn open_with_block_cache(root: &Path, block_cache_bytes: usize) -> Result<Self> {
        Self::open_with_options(
            root,
            DatabaseOptions {
                block_cache_bytes,
                ..DatabaseOptions::default()
            },
        )
    }

    pub fn open_with_options(root: &Path, options: DatabaseOptions) -> Result<Self> {
        let options = options.validate()?;
        let manifests = ManifestStore::open(root)?;
        let (_, manifest) = manifests
            .current()?
            .ok_or_else(|| Error::InvalidManifest("database has no CURRENT manifest".into()))?;
        let block_cache = new_block_cache(options.block_cache_bytes);
        let mut segments = Vec::with_capacity(manifest.segments.len());
        for expected in &manifest.segments {
            let mut segment = Segment::open_with_cache(
                &root
                    .join(SEGMENT_DIRECTORY)
                    .join(format!("{}.seg", expected.id)),
                Arc::clone(&block_cache),
            )?;
            segment.descriptor.level = expected.level;
            if &segment.descriptor != expected {
                return Err(Error::InvalidManifest(format!(
                    "segment {} does not match its manifest descriptor",
                    expected.id
                )));
            }
            segments.push(segment);
        }
        let path = wal_path(root, manifest.wal_start_sequence);
        let recovery = recover_from(&path, manifest.wal_start_sequence)?;
        if let Some(offset) = recovery.torn_tail {
            return Err(Error::TornTail { offset });
        }
        let memtable = Memtable::recover_from(&recovery.batches, manifest.durable_sequence)?;
        let wal_payload_bytes = recovery.batches.iter().fold(0usize, |bytes, batch| {
            bytes.saturating_add(batch.payload.len())
        });
        let maintenance_stats = MaintenanceStats {
            peak_wal_payload_bytes: wal_payload_bytes,
            peak_memtable_versions: memtable.version_count(),
            ..MaintenanceStats::default()
        };
        let wal = WalWriter::open_at(&path, manifest.wal_start_sequence)?;
        Ok(Self {
            root: root.to_owned(),
            manifests,
            manifest,
            wal,
            memtable,
            segments,
            block_cache,
            maintenance: options.maintenance,
            compaction: options.compaction,
            maintenance_stats,
            wal_payload_bytes,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            sequence: self.memtable.maximum_sequence(),
        }
    }

    pub fn write(&mut self, batch: &WriteBatch, durability: Durability) -> Result<AppendReceipt> {
        let payload = batch.encode()?;
        self.prepare_write(batch, payload.len())?;
        let receipt = self
            .wal
            .append_encoded_write_batch(batch, &payload, durability)?;
        self.memtable
            .apply_write_batch(batch, receipt.first_sequence, receipt.last_sequence)?;
        self.wal_payload_bytes = self.wal_payload_bytes.saturating_add(payload.len());
        self.record_memtable_peak();
        Ok(receipt)
    }

    /// Owned fast path used by adapters that build a batch for immediate
    /// commit. Keys and values move into the memtable after WAL encoding rather
    /// than being cloned a second time.
    pub fn write_owned(
        &mut self,
        batch: WriteBatch,
        durability: Durability,
    ) -> Result<AppendReceipt> {
        let payload = batch.encode()?;
        self.prepare_write(&batch, payload.len())?;
        let receipt = self
            .wal
            .append_encoded_write_batch(&batch, &payload, durability)?;
        self.memtable.apply_owned_write_batch(
            batch,
            receipt.first_sequence,
            receipt.last_sequence,
        )?;
        self.wal_payload_bytes = self.wal_payload_bytes.saturating_add(payload.len());
        self.record_memtable_peak();
        Ok(receipt)
    }

    fn prepare_write(&mut self, batch: &WriteBatch, payload_bytes: usize) -> Result<()> {
        let projected_bytes = self.wal_payload_bytes.saturating_add(payload_bytes);
        let projected_versions = self.memtable.version_count().saturating_add(batch.len());
        let active = self.memtable.version_count() != 0;
        if active
            && (projected_bytes > self.maintenance.wal_payload_max_bytes
                || projected_versions > self.maintenance.memtable_max_versions)
        {
            self.maintenance_stats.write_stalls =
                self.maintenance_stats.write_stalls.saturating_add(1);
            match self.flush_memtable(self.manifest.created_at) {
                Ok(Some(_)) => {
                    self.maintenance_stats.automatic_flushes =
                        self.maintenance_stats.automatic_flushes.saturating_add(1);
                    match self.compact_if_needed(self.manifest.created_at) {
                        Ok(Some(outcome)) => {
                            self.maintenance_stats.automatic_compactions = self
                                .maintenance_stats
                                .automatic_compactions
                                .saturating_add(1);
                            self.maintenance_stats.compaction_input_bytes = self
                                .maintenance_stats
                                .compaction_input_bytes
                                .saturating_add(outcome.input_bytes);
                            self.maintenance_stats.compaction_output_bytes = self
                                .maintenance_stats
                                .compaction_output_bytes
                                .saturating_add(outcome.output_bytes);
                            self.maintenance_stats.peak_compaction_buffer_bytes = self
                                .maintenance_stats
                                .peak_compaction_buffer_bytes
                                .max(outcome.peak_buffer_bytes);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            self.maintenance_stats.failed_compactions =
                                self.maintenance_stats.failed_compactions.saturating_add(1);
                            return Err(error);
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    self.maintenance_stats.failed_flushes =
                        self.maintenance_stats.failed_flushes.saturating_add(1);
                    return Err(error);
                }
            }
        }
        if payload_bytes > self.maintenance.wal_payload_max_bytes
            || batch.len() > self.maintenance.memtable_max_versions
        {
            self.maintenance_stats.oversized_batches =
                self.maintenance_stats.oversized_batches.saturating_add(1);
        }
        Ok(())
    }

    fn record_memtable_peak(&mut self) {
        self.maintenance_stats.peak_wal_payload_bytes = self
            .maintenance_stats
            .peak_wal_payload_bytes
            .max(self.wal_payload_bytes);
        self.maintenance_stats.peak_memtable_versions = self
            .maintenance_stats
            .peak_memtable_versions
            .max(self.memtable.version_count());
    }

    pub fn sync(&mut self) -> Result<u64> {
        self.wal.sync()
    }

    /// Flushes the current WAL-backed memtable into one immutable segment.
    /// Publication order makes either the old or new manifest fully recoverable
    /// at every crash boundary.
    pub fn flush_memtable(&mut self, at: u64) -> Result<Option<Manifest>> {
        self.flush_memtable_inner(at, None)
    }

    /// Deterministic fault-injection entry point for recovery matrices. A
    /// `ManifestPublished` failure updates in-memory state before returning so
    /// callers that do not immediately simulate process death still fail safe.
    pub fn flush_memtable_with_failure(
        &mut self,
        at: u64,
        boundary: FlushBoundary,
        mode: FailureMode,
    ) -> Result<Option<Manifest>> {
        self.flush_memtable_inner(at, Some((boundary, mode)))
    }

    fn flush_memtable_inner(
        &mut self,
        at: u64,
        failure: Option<(FlushBoundary, FailureMode)>,
    ) -> Result<Option<Manifest>> {
        if self.memtable.version_count() == 0 {
            self.wal.sync()?;
            return Ok(None);
        }
        let sequence = self.wal.sync()?;
        inject_failure(failure, FlushBoundary::WalSynced)?;
        let (segment, _) = Segment::write_from_memtable_with_cache(
            &self.root.join(SEGMENT_DIRECTORY),
            &self.memtable,
            Arc::clone(&self.block_cache),
        )?;
        inject_failure(failure, FlushBoundary::SegmentSynced)?;
        let next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidBatch("sequence overflow at flush".into()))?;
        let next_path = wal_path(&self.root, next_sequence);
        let successor = if next_path.exists() {
            let recovery = recover_from(&next_path, next_sequence)?;
            if !recovery.batches.is_empty() || recovery.torn_tail.is_some() {
                return Err(Error::Corruption {
                    offset: 0,
                    reason: "unpublished successor WAL contains data".into(),
                });
            }
            WalWriter::open_at(&next_path, next_sequence)?
        } else {
            WalWriter::create_at(&next_path, next_sequence)?
        };
        inject_failure(failure, FlushBoundary::SuccessorWalSynced)?;
        let mut descriptors = self.manifest.segments.clone();
        descriptors.push(segment.descriptor.clone());
        let next = Manifest::new(
            self.manifest
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidManifest("manifest generation overflow".into()))?,
            Some(self.manifest.digest.clone()),
            at,
            sequence,
            next_sequence,
            descriptors,
        )?;
        self.manifests.publish(&next, Some(&self.manifest.digest))?;
        self.wal = successor;
        self.memtable = Memtable::at_sequence(sequence);
        self.wal_payload_bytes = 0;
        self.segments.push(segment);
        self.manifest = next.clone();
        inject_failure(failure, FlushBoundary::ManifestPublished)?;
        Ok(Some(next))
    }

    pub fn checkpoint(&self, name: &str, at: u64) -> Result<Checkpoint> {
        self.manifests.checkpoint(name, &self.manifest.digest, at)
    }

    pub fn checkpoints(&self) -> Result<Vec<Checkpoint>> {
        self.manifests.checkpoints()
    }

    pub fn release_checkpoint(&self, name: &str) -> Result<bool> {
        self.manifests.release_checkpoint(name)
    }

    /// Flushes the active WAL-backed memtable and captures the resulting
    /// immutable manifest closure. The published manifest points at an empty
    /// successor WAL, so every state byte required by the snapshot is carried
    /// by the authenticated segment inventory.
    pub fn export_snapshot_bundle(&mut self, at: u64) -> Result<SnapshotBundle> {
        self.flush_memtable(at)?;
        let segments = self
            .manifest
            .segments
            .iter()
            .map(|descriptor| {
                let bytes = std::fs::read(
                    self.root
                        .join(SEGMENT_DIRECTORY)
                        .join(format!("{}.seg", descriptor.id)),
                )?;
                Ok(SnapshotSegment {
                    descriptor: descriptor.clone(),
                    bytes,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        SnapshotBundle::new(self.manifest.clone(), segments)
    }

    /// Flushes and writes the authenticated snapshot bundle directly to a new
    /// file, retaining only a fixed copy buffer plus one segment during deep
    /// validation.
    pub fn export_snapshot_file(
        &mut self,
        at: u64,
        destination: impl AsRef<Path>,
    ) -> Result<SnapshotBundleFile> {
        self.flush_memtable(at)?;
        SnapshotBundleFile::create(
            self.manifest.clone(),
            &self.root.join(SEGMENT_DIRECTORY),
            destination,
        )
    }

    pub fn export_snapshot_file_with_failure(
        &mut self,
        at: u64,
        destination: impl AsRef<Path>,
        boundary: SnapshotExportBoundary,
        mode: FailureMode,
    ) -> Result<SnapshotBundleFile> {
        self.flush_memtable(at)?;
        SnapshotBundleFile::create_with_hook(
            self.manifest.clone(),
            &self.root.join(SEGMENT_DIRECTORY),
            destination,
            |reached| {
                if reached == boundary {
                    Err(Error::InjectedFailure {
                        mode: mode.as_str(),
                        boundary: boundary.as_str(),
                    })
                } else {
                    Ok(())
                }
            },
        )
    }

    /// Replaces the current logical database image with a newer authenticated
    /// snapshot. Segment files and an empty continuation WAL are durable before
    /// a single manifest-pointer publication makes the imported state visible.
    pub fn install_snapshot_bundle(
        &mut self,
        bundle: &SnapshotBundle,
        at: u64,
    ) -> Result<Manifest> {
        self.install_snapshot_bundle_inner(bundle, at, None)
    }

    /// Installs a file-backed bundle without decoding the whole transfer into
    /// one allocation. VyrmKV currently retains opened segments in memory, so
    /// the resident engine image remains a separately measured bound.
    pub fn install_snapshot_file(
        &mut self,
        bundle: &SnapshotBundleFile,
        at: u64,
    ) -> Result<Manifest> {
        self.install_snapshot_file_inner(bundle, at, None)
    }

    pub fn install_snapshot_file_with_failure(
        &mut self,
        bundle: &SnapshotBundleFile,
        at: u64,
        boundary: SnapshotInstallBoundary,
        mode: FailureMode,
    ) -> Result<Manifest> {
        self.install_snapshot_file_inner(bundle, at, Some((boundary, mode)))
    }

    fn install_snapshot_file_inner(
        &mut self,
        bundle: &SnapshotBundleFile,
        at: u64,
        failure: Option<(SnapshotInstallBoundary, FailureMode)>,
    ) -> Result<Manifest> {
        let source_sequence = bundle.source_manifest.durable_sequence;
        let current_sequence = self.snapshot().sequence;
        if source_sequence == current_sequence
            && self.manifest.segments == bundle.source_manifest.segments
        {
            return Ok(self.manifest.clone());
        }
        if source_sequence <= current_sequence {
            return Err(Error::InvalidManifest(format!(
                "snapshot sequence {source_sequence} does not advance local sequence {current_sequence}"
            )));
        }

        let segment_directory = self.root.join(SEGMENT_DIRECTORY);
        let mut imported = Vec::with_capacity(bundle.source_manifest.segments.len());
        for (index, descriptor) in bundle.descriptors().enumerate() {
            let bytes = bundle.segment_bytes(index)?;
            imported.push(Segment::install_snapshot_bytes_with_cache(
                &segment_directory,
                descriptor,
                &bytes,
                Arc::clone(&self.block_cache),
            )?);
        }
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::SegmentsSynced)?;

        let next_sequence = source_sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidManifest("snapshot sequence overflowed".into()))?;
        let next_path = wal_path(&self.root, next_sequence);
        let successor = if next_path.exists() {
            let recovery = recover_from(&next_path, next_sequence)?;
            if !recovery.batches.is_empty() || recovery.torn_tail.is_some() {
                return Err(Error::InvalidManifest(
                    "snapshot continuation WAL already contains data".into(),
                ));
            }
            WalWriter::open_at(&next_path, next_sequence)?
        } else {
            WalWriter::create_at(&next_path, next_sequence)?
        };
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::SuccessorWalSynced)?;

        let previous = self.manifest.digest.clone();
        let manifest = Manifest::new(
            self.manifest
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidManifest("manifest generation overflowed".into()))?,
            Some(previous.clone()),
            at.max(self.manifest.created_at)
                .max(bundle.source_manifest.created_at),
            source_sequence,
            next_sequence,
            bundle.source_manifest.segments.clone(),
        )?;
        self.manifests.publish(&manifest, Some(&previous))?;
        self.wal = successor;
        self.memtable = Memtable::at_sequence(source_sequence);
        self.wal_payload_bytes = 0;
        self.segments = imported;
        self.manifest = manifest.clone();
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::ManifestPublished)?;
        Ok(manifest)
    }

    pub fn install_snapshot_bundle_with_failure(
        &mut self,
        bundle: &SnapshotBundle,
        at: u64,
        boundary: SnapshotInstallBoundary,
        mode: FailureMode,
    ) -> Result<Manifest> {
        self.install_snapshot_bundle_inner(bundle, at, Some((boundary, mode)))
    }

    fn install_snapshot_bundle_inner(
        &mut self,
        bundle: &SnapshotBundle,
        at: u64,
        failure: Option<(SnapshotInstallBoundary, FailureMode)>,
    ) -> Result<Manifest> {
        bundle.validate()?;
        let source_sequence = bundle.source_manifest.durable_sequence;
        let current_sequence = self.snapshot().sequence;
        if source_sequence == current_sequence
            && self.manifest.segments == bundle.source_manifest.segments
        {
            return Ok(self.manifest.clone());
        }
        if source_sequence <= current_sequence {
            return Err(Error::InvalidManifest(format!(
                "snapshot sequence {source_sequence} does not advance local sequence {current_sequence}"
            )));
        }

        let segment_directory = self.root.join(SEGMENT_DIRECTORY);
        let mut imported = Vec::with_capacity(bundle.segments.len());
        for bundled in &bundle.segments {
            imported.push(Segment::install_snapshot_bytes_with_cache(
                &segment_directory,
                &bundled.descriptor,
                &bundled.bytes,
                Arc::clone(&self.block_cache),
            )?);
        }
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::SegmentsSynced)?;

        let next_sequence = source_sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidManifest("snapshot sequence overflowed".into()))?;
        let next_path = wal_path(&self.root, next_sequence);
        let successor = if next_path.exists() {
            let recovery = recover_from(&next_path, next_sequence)?;
            if !recovery.batches.is_empty() || recovery.torn_tail.is_some() {
                return Err(Error::InvalidManifest(
                    "snapshot continuation WAL already contains data".into(),
                ));
            }
            WalWriter::open_at(&next_path, next_sequence)?
        } else {
            WalWriter::create_at(&next_path, next_sequence)?
        };
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::SuccessorWalSynced)?;

        let previous = self.manifest.digest.clone();
        let manifest = Manifest::new(
            self.manifest
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidManifest("manifest generation overflowed".into()))?,
            Some(previous.clone()),
            at.max(self.manifest.created_at)
                .max(bundle.source_manifest.created_at),
            source_sequence,
            next_sequence,
            bundle.source_manifest.segments.clone(),
        )?;
        self.manifests.publish(&manifest, Some(&previous))?;
        self.wal = successor;
        self.memtable = Memtable::at_sequence(source_sequence);
        self.wal_payload_bytes = 0;
        self.segments = imported;
        self.manifest = manifest.clone();
        inject_snapshot_install_failure(failure, SnapshotInstallBoundary::ManifestPublished)?;
        Ok(manifest)
    }

    /// Executes one deterministic leveled compaction step while retaining
    /// exactly the versions observable at `protected` snapshots and at the
    /// current durable head. Input selection is bounded and records are merged
    /// through forward-only segment cursors. Output is partitioned at key
    /// boundaries, so the working set is bounded by the configured target plus
    /// one key's complete MVCC history.
    ///
    /// Named checkpoints keep their original manifests and segments and are
    /// handled independently by garbage collection.
    pub fn compact(
        &mut self,
        protected: &[Snapshot],
        at: u64,
    ) -> Result<Option<CompactionOutcome>> {
        self.compact_inner(protected, at, None, true, false)
    }

    pub fn compact_with_failure(
        &mut self,
        protected: &[Snapshot],
        at: u64,
        boundary: CompactionBoundary,
        mode: FailureMode,
    ) -> Result<Option<CompactionOutcome>> {
        self.compact_inner(protected, at, Some((boundary, mode)), true, false)
    }

    /// Runs one leveled step only when the configured L0 threshold or a lower
    /// level debt is present. Automatic maintenance conservatively retains all
    /// MVCC versions because a plain [`Snapshot`] is intentionally copyable and
    /// cannot act as a lifetime-tracked reclamation lease. Explicit compaction
    /// supplies the precise protected snapshot set and may prune history.
    pub fn compact_if_needed(&mut self, at: u64) -> Result<Option<CompactionOutcome>> {
        self.flush_memtable_inner(at, None)?;
        self.compact_inner(&[], at, None, false, true)
    }

    fn compact_inner(
        &mut self,
        protected: &[Snapshot],
        at: u64,
        failure: Option<(CompactionBoundary, FailureMode)>,
        force: bool,
        retain_all_versions: bool,
    ) -> Result<Option<CompactionOutcome>> {
        self.flush_memtable_inner(at, None)?;
        let Some(candidate) = self.select_compaction_candidate(force) else {
            return Ok(None);
        };
        let durable = self.manifest.durable_sequence;
        let mut protected_sequences = protected
            .iter()
            .map(|snapshot| snapshot.sequence)
            .filter(|sequence| *sequence <= durable)
            .collect::<BTreeSet<_>>();
        protected_sequences.insert(durable);
        let previous_manifest = self.manifest.digest.clone();
        let input_segments = candidate.indices.len();
        let input_bytes = candidate.indices.iter().try_fold(0u64, |total, index| {
            total
                .checked_add(self.segments[*index].descriptor.bytes)
                .ok_or_else(|| Error::InvalidSegment("compaction input bytes overflow".into()))
        })?;
        let merge = self.merge_compaction_candidate(
            &candidate,
            durable,
            &protected_sequences,
            retain_all_versions,
        )?;
        let CompactionMerge {
            segments: compacted_segments,
            output_segments,
            input_versions,
            output_versions,
            output_bytes,
            peak_buffer_bytes,
        } = merge;
        inject_compaction_failure(failure, CompactionBoundary::SegmentSynced)?;
        let selected = candidate.indices.iter().copied().collect::<BTreeSet<_>>();
        let mut descriptors = self
            .segments
            .iter()
            .enumerate()
            .filter(|(index, _)| !selected.contains(index))
            .map(|(_, segment)| segment.descriptor.clone())
            .collect::<Vec<_>>();
        descriptors.extend(
            compacted_segments
                .iter()
                .map(|segment| segment.descriptor.clone()),
        );
        let next = Manifest::new(
            self.manifest
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidManifest("manifest generation overflow".into()))?,
            Some(previous_manifest.clone()),
            at,
            durable,
            self.manifest.wal_start_sequence,
            descriptors,
        )?;
        self.manifests
            .publish(&next, Some(previous_manifest.as_str()))?;
        let mut retained_segments = std::mem::take(&mut self.segments)
            .into_iter()
            .enumerate()
            .filter_map(|(index, segment)| (!selected.contains(&index)).then_some(segment))
            .collect::<Vec<_>>();
        retained_segments.extend(compacted_segments);
        self.segments = retained_segments;
        self.manifest = next.clone();
        inject_compaction_failure(failure, CompactionBoundary::ManifestPublished)?;
        Ok(Some(CompactionOutcome {
            previous_manifest,
            manifest: next.digest,
            input_segments,
            output_segments,
            input_versions,
            output_versions,
            protected_sequences: protected_sequences.into_iter().collect(),
            source_level: candidate.source_level,
            target_level: candidate.target_level,
            input_bytes,
            output_bytes,
            peak_buffer_bytes,
            history_pruned: !retain_all_versions,
        }))
    }

    fn select_compaction_candidate(&self, force: bool) -> Option<CompactionCandidate> {
        let mut l0 = self
            .segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| segment.descriptor.level == 0)
            .map(|(index, segment)| (index, &segment.descriptor))
            .collect::<Vec<_>>();
        l0.sort_by(|left, right| {
            left.1
                .maximum_sequence
                .cmp(&right.1.maximum_sequence)
                .then_with(|| left.1.first_key.cmp(&right.1.first_key))
                .then_with(|| left.1.id.cmp(&right.1.id))
        });
        if !l0.is_empty() && (force || l0.len() >= self.compaction.l0_compaction_trigger) {
            let target_level = 1.min(self.compaction.max_level);
            let mut selected_l0 = vec![l0[0].0];
            let mut first_key = l0[0].1.first_key.clone();
            let mut last_key = l0[0].1.last_key.clone();
            for (index, descriptor) in l0.into_iter().skip(1) {
                let candidate_first = first_key.as_slice().min(descriptor.first_key.as_slice());
                let candidate_last = last_key.as_slice().max(descriptor.last_key.as_slice());
                let overlap_count = self
                    .segments
                    .iter()
                    .enumerate()
                    .filter(|(other, segment)| {
                        !selected_l0.contains(other)
                            && *other != index
                            && segment.descriptor.level == target_level
                            && ranges_overlap(
                                candidate_first,
                                candidate_last,
                                &segment.descriptor.first_key,
                                &segment.descriptor.last_key,
                            )
                    })
                    .count();
                if selected_l0.len() + 1 + overlap_count > self.compaction.max_input_segments {
                    break;
                }
                selected_l0.push(index);
                first_key = candidate_first.to_vec();
                last_key = candidate_last.to_vec();
            }
            let mut indices = selected_l0;
            let overlaps = self
                .segments
                .iter()
                .enumerate()
                .filter(|(index, segment)| {
                    !indices.contains(index)
                        && segment.descriptor.level == target_level
                        && ranges_overlap(
                            &first_key,
                            &last_key,
                            &segment.descriptor.first_key,
                            &segment.descriptor.last_key,
                        )
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            indices.extend(overlaps);
            if (force || indices.len() >= 2) && indices.len() <= self.compaction.max_input_segments
            {
                indices.sort_unstable();
                return Some(CompactionCandidate {
                    indices,
                    source_level: 0,
                    target_level,
                });
            }
            return None;
        }

        for source_level in 1..self.compaction.max_level {
            let mut at_level = self
                .segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| segment.descriptor.level == source_level)
                .map(|(index, segment)| (index, &segment.descriptor))
                .collect::<Vec<_>>();
            if at_level.is_empty() || (!force && at_level.len() < 2) {
                continue;
            }
            at_level.sort_by(|left, right| {
                left.1
                    .first_key
                    .cmp(&right.1.first_key)
                    .then_with(|| left.1.id.cmp(&right.1.id))
            });
            let target_level = source_level.saturating_add(1);
            for take in (1..=at_level.len().min(self.compaction.max_input_segments)).rev() {
                let first_key = &at_level[0].1.first_key;
                let last_key = &at_level[take - 1].1.last_key;
                let mut indices = at_level
                    .iter()
                    .take(take)
                    .map(|(index, _)| *index)
                    .collect::<Vec<_>>();
                for (index, segment) in self.segments.iter().enumerate() {
                    if segment.descriptor.level == target_level
                        && ranges_overlap(
                            first_key,
                            last_key,
                            &segment.descriptor.first_key,
                            &segment.descriptor.last_key,
                        )
                    {
                        indices.push(index);
                    }
                }
                indices.sort_unstable();
                indices.dedup();
                if indices.len() <= self.compaction.max_input_segments
                    && (force || indices.len() >= 2)
                {
                    return Some(CompactionCandidate {
                        indices,
                        source_level,
                        target_level,
                    });
                }
            }
        }
        None
    }

    fn merge_compaction_candidate(
        &self,
        candidate: &CompactionCandidate,
        durable: u64,
        protected_sequences: &BTreeSet<u64>,
        retain_all_versions: bool,
    ) -> Result<CompactionMerge> {
        let selected = candidate.indices.iter().copied().collect::<BTreeSet<_>>();
        let mut cursors = candidate
            .indices
            .iter()
            .map(|index| self.segments[*index].record_cursor())
            .collect::<Vec<_>>();
        let mut current = cursors
            .iter_mut()
            .map(|cursor| cursor.next_record())
            .collect::<Result<Vec<_>>>()?;
        let mut buffered = BTreeMap::<Vec<u8>, Vec<VersionedValue>>::new();
        let mut buffered_bytes = 0usize;
        let mut peak_buffer_bytes = 0usize;
        let mut compacted_segments = Vec::new();
        let mut input_versions = 0u64;
        let mut output_versions = 0u64;
        let mut output_bytes = 0u64;

        while let Some(key) = current
            .iter()
            .filter_map(|record| record.as_ref().map(|record| record.key.as_slice()))
            .min()
            .map(<[u8]>::to_vec)
        {
            let mut versions = BTreeMap::<u64, Option<Vec<u8>>>::new();
            for (cursor, record) in cursors.iter_mut().zip(current.iter_mut()) {
                while record.as_ref().is_some_and(|record| record.key == key) {
                    let observed = record.take().expect("record was checked");
                    match versions.get(&observed.version.sequence) {
                        Some(existing) if existing != &observed.version.value => {
                            return Err(Error::InvalidSegment(format!(
                                "segments disagree for sequence {}",
                                observed.version.sequence
                            )));
                        }
                        Some(_) => {}
                        None => {
                            input_versions = input_versions.saturating_add(1);
                            versions.insert(observed.version.sequence, observed.version.value);
                        }
                    }
                    *record = cursor.next_record()?;
                }
            }
            let retained = if retain_all_versions {
                versions
                    .into_iter()
                    .map(|(sequence, value)| VersionedValue { sequence, value })
                    .collect::<Vec<_>>()
            } else {
                protected_sequences
                    .iter()
                    .filter_map(|sequence| {
                        versions
                            .range(..=*sequence)
                            .next_back()
                            .map(|(sequence, value)| {
                                (
                                    *sequence,
                                    VersionedValue {
                                        sequence: *sequence,
                                        value: value.clone(),
                                    },
                                )
                            })
                    })
                    .collect::<BTreeMap<_, _>>()
                    .into_values()
                    .collect::<Vec<_>>()
            };
            let tombstone_only = retained.iter().all(|version| version.value.is_none());
            let shadows_unselected = self.segments.iter().enumerate().any(|(index, segment)| {
                !selected.contains(&index)
                    && key >= segment.descriptor.first_key
                    && key <= segment.descriptor.last_key
            });
            if retained.is_empty()
                || (!retain_all_versions && tombstone_only && !shadows_unselected)
            {
                continue;
            }
            let group_bytes = version_group_bytes(&key, &retained);
            if !buffered.is_empty()
                && buffered_bytes.saturating_add(group_bytes) > self.compaction.target_segment_bytes
            {
                let segment = self.write_compaction_partition(
                    std::mem::take(&mut buffered),
                    durable,
                    candidate.target_level,
                )?;
                output_bytes = output_bytes.saturating_add(segment.descriptor.bytes);
                compacted_segments.push(segment);
                buffered_bytes = 0;
            }
            output_versions = output_versions.saturating_add(retained.len() as u64);
            buffered_bytes = buffered_bytes.saturating_add(group_bytes);
            peak_buffer_bytes = peak_buffer_bytes.max(buffered_bytes);
            buffered.insert(key, retained);
        }
        if !buffered.is_empty() {
            let segment =
                self.write_compaction_partition(buffered, durable, candidate.target_level)?;
            output_bytes = output_bytes.saturating_add(segment.descriptor.bytes);
            compacted_segments.push(segment);
        }
        Ok(CompactionMerge {
            output_segments: compacted_segments.len(),
            segments: compacted_segments,
            input_versions,
            output_versions,
            output_bytes,
            peak_buffer_bytes,
        })
    }

    fn write_compaction_partition(
        &self,
        versions: BTreeMap<Vec<u8>, Vec<VersionedValue>>,
        durable: u64,
        target_level: u8,
    ) -> Result<Segment> {
        let table = Memtable::from_versions(versions, durable)?;
        let (mut segment, _) = Segment::write_from_memtable_with_cache(
            &self.root.join(SEGMENT_DIRECTORY),
            &table,
            Arc::clone(&self.block_cache),
        )?;
        segment.descriptor.level = target_level;
        Ok(segment)
    }

    /// Removes physical objects unreachable from `CURRENT` or a named
    /// checkpoint. The inventory is fully validated before the first delete.
    pub fn garbage_collect(&self) -> Result<GarbageCollectionReport> {
        let mut manifests = BTreeMap::new();
        manifests.insert(self.manifest.digest.clone(), self.manifest.clone());
        for checkpoint in self.manifests.checkpoints()? {
            manifests
                .entry(checkpoint.manifest.clone())
                .or_insert(self.manifests.load(&checkpoint.manifest)?);
        }
        let retained_manifests = manifests.keys().cloned().collect::<BTreeSet<_>>();
        let retained_segments = manifests
            .values()
            .flat_map(|manifest| manifest.segments.iter().map(|segment| segment.id.clone()))
            .collect::<BTreeSet<_>>();
        let retained_wals = manifests
            .values()
            .map(|manifest| format!("{:020}.wal", manifest.wal_start_sequence))
            .collect::<BTreeSet<_>>();

        let manifest_directory = self.root.join("manifests");
        let segment_directory = self.root.join(SEGMENT_DIRECTORY);
        let wal_directory = self.root.join(WAL_DIRECTORY);
        let mut report = GarbageCollectionReport {
            retained_manifests: retained_manifests.iter().cloned().collect(),
            retained_segments: retained_segments.iter().cloned().collect(),
            retained_wals: retained_wals.iter().cloned().collect(),
            ..GarbageCollectionReport::default()
        };
        let manifest_candidates =
            unreachable_files(&manifest_directory, "json", &retained_manifests)?;
        let segment_candidates = unreachable_files(&segment_directory, "seg", &retained_segments)?;
        let wal_candidates = unreachable_files(&wal_directory, "wal", &retained_wals)?;
        remove_candidates(
            &manifest_directory,
            manifest_candidates,
            &mut report.removed_manifests,
        )?;
        remove_candidates(
            &segment_directory,
            segment_candidates,
            &mut report.removed_segments,
        )?;
        remove_candidates(&wal_directory, wal_candidates, &mut report.removed_wals)?;
        Ok(report)
    }

    pub fn get(&self, key: &[u8], snapshot: Snapshot) -> Result<Option<Vec<u8>>> {
        // The active memtable always contains sequences newer than every
        // published segment. A visible value (including a tombstone) is
        // therefore authoritative for this snapshot and lets hot runtime keys
        // avoid immutable-block I/O entirely.
        if let Some(version) = self.memtable.get_version(key, snapshot.sequence) {
            return Ok(version.value.clone());
        }
        let mut best_sequence = 0;
        let mut best_value = None;
        for segment in &self.segments {
            if let Some(version) = segment.get_version(key, snapshot.sequence)? {
                if version.sequence > best_sequence {
                    best_sequence = version.sequence;
                    best_value = version.value;
                }
            }
        }
        Ok(best_value)
    }

    pub fn get_many(&self, keys: &[Vec<u8>], snapshot: Snapshot) -> Result<Vec<Option<Vec<u8>>>> {
        let mut values = vec![None; keys.len()];
        let mut unresolved = Vec::with_capacity(keys.len());
        for (index, key) in keys.iter().enumerate() {
            if let Some(version) = self.memtable.get_version(key, snapshot.sequence) {
                values[index] = version.value.clone();
            } else {
                unresolved.push(index);
            }
        }
        if unresolved.is_empty() {
            return Ok(values);
        }
        let unresolved_keys = unresolved
            .iter()
            .map(|index| keys[*index].as_slice())
            .collect::<Vec<_>>();
        if self.segments.len() == 1 && self.memtable.version_count() == 0 {
            for (position, version) in self.segments[0]
                .get_versions(&unresolved_keys, snapshot.sequence)?
                .into_iter()
                .enumerate()
            {
                values[unresolved[position]] = version.and_then(|version| version.value);
            }
            return Ok(values);
        }
        let mut best_sequences = vec![0u64; unresolved.len()];
        for segment in &self.segments {
            for (position, version) in segment
                .get_versions(&unresolved_keys, snapshot.sequence)?
                .into_iter()
                .enumerate()
            {
                if let Some(version) = version {
                    if version.sequence > best_sequences[position] {
                        best_sequences[position] = version.sequence;
                        values[unresolved[position]] = version.value;
                    }
                }
            }
        }
        Ok(values)
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        snapshot: Snapshot,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        if self.segments.len() == 1 && self.memtable.version_count() == 0 {
            return self.segments[0].scan(start, end, snapshot.sequence);
        }
        let mut visible = BTreeMap::<Vec<u8>, VersionedValue>::new();
        for segment in &self.segments {
            for (key, version) in segment.visible_from(start, end, snapshot.sequence)? {
                if visible
                    .get(&key)
                    .is_none_or(|current| version.sequence > current.sequence)
                {
                    visible.insert(key, version);
                }
            }
        }
        for (key, version) in self.memtable.visible_from(start, end, snapshot.sequence) {
            if visible
                .get(&key)
                .is_none_or(|current| version.sequence > current.sequence)
            {
                visible.insert(key, version);
            }
        }
        Ok(visible
            .into_iter()
            .filter_map(|(key, version)| version.value.map(|value| (key, value)))
            .collect())
    }

    pub fn memtable(&self) -> &Memtable {
        &self.memtable
    }

    pub fn maintenance_policy(&self) -> MaintenancePolicy {
        self.maintenance
    }

    pub fn compaction_policy(&self) -> CompactionPolicy {
        self.compaction
    }

    pub fn l0_segment_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| segment.descriptor.level == 0)
            .count()
    }

    pub fn compaction_debt_segments(&self) -> usize {
        self.l0_segment_count()
            .saturating_sub(self.compaction.l0_compaction_trigger.saturating_sub(1))
    }

    pub fn maintenance_stats(&self) -> MaintenanceStats {
        self.maintenance_stats
    }

    pub fn wal_payload_bytes(&self) -> usize {
        self.wal_payload_bytes
    }

    /// Current shared immutable-block residency and effectiveness counters.
    pub fn block_cache_stats(&self) -> crate::BlockCacheStats {
        block_cache_stats(&self.block_cache)
    }
}

fn wal_path(root: &Path, starting_sequence: u64) -> PathBuf {
    root.join(WAL_DIRECTORY)
        .join(format!("{starting_sequence:020}.wal"))
}

fn ranges_overlap(
    left_first: &[u8],
    left_last: &[u8],
    right_first: &[u8],
    right_last: &[u8],
) -> bool {
    left_first <= right_last && right_first <= left_last
}

fn version_group_bytes(key: &[u8], versions: &[VersionedValue]) -> usize {
    versions.iter().fold(0usize, |bytes, version| {
        bytes
            .saturating_add(key.len())
            .saturating_add(version.value.as_ref().map_or(0, Vec::len))
            .saturating_add(std::mem::size_of::<VersionedValue>())
    })
}

fn inject_failure(
    configured: Option<(FlushBoundary, FailureMode)>,
    reached: FlushBoundary,
) -> Result<()> {
    if let Some((boundary, mode)) = configured.filter(|(boundary, _)| *boundary == reached) {
        return Err(Error::InjectedFailure {
            mode: mode.as_str(),
            boundary: boundary.as_str(),
        });
    }
    Ok(())
}

fn inject_snapshot_install_failure(
    configured: Option<(SnapshotInstallBoundary, FailureMode)>,
    reached: SnapshotInstallBoundary,
) -> Result<()> {
    if let Some((boundary, mode)) = configured.filter(|(boundary, _)| *boundary == reached) {
        return Err(Error::InjectedFailure {
            mode: mode.as_str(),
            boundary: boundary.as_str(),
        });
    }
    Ok(())
}

fn inject_compaction_failure(
    configured: Option<(CompactionBoundary, FailureMode)>,
    reached: CompactionBoundary,
) -> Result<()> {
    if let Some((boundary, mode)) = configured.filter(|(boundary, _)| *boundary == reached) {
        return Err(Error::InjectedFailure {
            mode: mode.as_str(),
            boundary: boundary.as_str(),
        });
    }
    Ok(())
}

fn unreachable_files(
    directory: &Path,
    extension: &str,
    retained: &BTreeSet<String>,
) -> Result<Vec<(PathBuf, String)>> {
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some(extension)
        {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let identity = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| Error::InvalidManifest("non-UTF-8 storage object name".into()))?
            .to_owned();
        let retained_identity = if extension == "wal" {
            retained.contains(&file_name)
        } else {
            retained.contains(&identity)
        };
        if !retained_identity {
            candidates.push((entry.path(), file_name));
        }
    }
    candidates.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(candidates)
}

fn remove_candidates(
    directory: &Path,
    candidates: Vec<(PathBuf, String)>,
    removed: &mut Vec<String>,
) -> Result<()> {
    for (path, file_name) in candidates {
        std::fs::remove_file(path)?;
        removed.push(file_name);
    }
    if !removed.is_empty() {
        File::open(directory)?.sync_all()?;
    }
    Ok(())
}
