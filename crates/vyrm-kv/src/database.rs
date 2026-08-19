use crate::{
    recover_from, AppendReceipt, Checkpoint, Durability, Error, Manifest, ManifestStore, Memtable,
    RecoveredBatch, Result, Segment, VersionedValue, WalWriter, WriteBatch,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};

const WAL_DIRECTORY: &str = "wal";
const SEGMENT_DIRECTORY: &str = "segments";

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
}

impl Database {
    pub fn create(root: &Path) -> Result<Self> {
        std::fs::create_dir(root)?;
        std::fs::create_dir(root.join(WAL_DIRECTORY))?;
        std::fs::create_dir(root.join(SEGMENT_DIRECTORY))?;
        let manifests = ManifestStore::open(root)?;
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
        })
    }

    pub fn open(root: &Path) -> Result<Self> {
        let manifests = ManifestStore::open(root)?;
        let (_, manifest) = manifests
            .current()?
            .ok_or_else(|| Error::InvalidManifest("database has no CURRENT manifest".into()))?;
        let mut segments = Vec::with_capacity(manifest.segments.len());
        for expected in &manifest.segments {
            let mut segment = Segment::open(
                &root
                    .join(SEGMENT_DIRECTORY)
                    .join(format!("{}.seg", expected.id)),
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
        let wal = WalWriter::open_at(&path, manifest.wal_start_sequence)?;
        Ok(Self {
            root: root.to_owned(),
            manifests,
            manifest,
            wal,
            memtable,
            segments,
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
        let receipt = self.wal.append_write_batch(batch, durability)?;
        self.memtable.apply(&RecoveredBatch {
            offset: receipt.offset,
            first_sequence: receipt.first_sequence,
            last_sequence: receipt.last_sequence,
            checksum: receipt.checksum,
            payload,
        })?;
        Ok(receipt)
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
        let (segment, _) =
            Segment::write_from_memtable(&self.root.join(SEGMENT_DIRECTORY), &self.memtable)?;
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

    /// Rewrites every current immutable segment into one canonical level while
    /// retaining exactly the versions observable at `protected` snapshots and
    /// at the current durable head. Named checkpoints keep their original
    /// manifests/segments and are handled independently by garbage collection.
    pub fn compact(
        &mut self,
        protected: &[Snapshot],
        at: u64,
    ) -> Result<Option<CompactionOutcome>> {
        self.compact_inner(protected, at, None)
    }

    pub fn compact_with_failure(
        &mut self,
        protected: &[Snapshot],
        at: u64,
        boundary: CompactionBoundary,
        mode: FailureMode,
    ) -> Result<Option<CompactionOutcome>> {
        self.compact_inner(protected, at, Some((boundary, mode)))
    }

    fn compact_inner(
        &mut self,
        protected: &[Snapshot],
        at: u64,
        failure: Option<(CompactionBoundary, FailureMode)>,
    ) -> Result<Option<CompactionOutcome>> {
        self.flush_memtable(at)?;
        if self.segments.is_empty() {
            return Ok(None);
        }
        let durable = self.manifest.durable_sequence;
        let mut protected_sequences = protected
            .iter()
            .map(|snapshot| snapshot.sequence)
            .filter(|sequence| *sequence <= durable)
            .collect::<BTreeSet<_>>();
        protected_sequences.insert(durable);

        let mut merged = BTreeMap::<Vec<u8>, BTreeMap<u64, Option<Vec<u8>>>>::new();
        for segment in &self.segments {
            for (key, versions) in segment.all_versions() {
                let target = merged.entry(key.to_vec()).or_default();
                for version in versions {
                    match target.get(&version.sequence) {
                        Some(existing) if existing != &version.value => {
                            return Err(Error::InvalidSegment(format!(
                                "segments disagree for sequence {}",
                                version.sequence
                            )));
                        }
                        Some(_) => {}
                        None => {
                            target.insert(version.sequence, version.value.clone());
                        }
                    }
                }
            }
        }
        let input_versions = merged.values().map(BTreeMap::len).sum::<usize>() as u64;
        let mut retained = BTreeMap::<Vec<u8>, Vec<VersionedValue>>::new();
        for (key, versions) in merged {
            let selected = protected_sequences
                .iter()
                .filter_map(|sequence| {
                    versions
                        .range(..=*sequence)
                        .next_back()
                        .map(|(sequence, value)| (*sequence, value.clone()))
                })
                .collect::<BTreeMap<_, _>>();
            if selected.values().all(Option::is_none) {
                continue;
            }
            retained.insert(
                key,
                selected
                    .into_iter()
                    .map(|(sequence, value)| VersionedValue { sequence, value })
                    .collect(),
            );
        }
        let output_versions = retained.values().map(Vec::len).sum::<usize>() as u64;
        let previous_manifest = self.manifest.digest.clone();
        let input_segments = self.segments.len();
        let mut compacted_segments = Vec::new();
        if !retained.is_empty() {
            let table = Memtable::from_versions(retained, durable)?;
            let (mut segment, _) =
                Segment::write_from_memtable(&self.root.join(SEGMENT_DIRECTORY), &table)?;
            segment.descriptor.level = self
                .segments
                .iter()
                .map(|segment| segment.descriptor.level)
                .max()
                .unwrap_or_default()
                .saturating_add(1);
            compacted_segments.push(segment);
        }
        inject_compaction_failure(failure, CompactionBoundary::SegmentSynced)?;
        let next = Manifest::new(
            self.manifest
                .generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidManifest("manifest generation overflow".into()))?,
            Some(previous_manifest.clone()),
            at,
            durable,
            self.manifest.wal_start_sequence,
            compacted_segments
                .iter()
                .map(|segment| segment.descriptor.clone())
                .collect(),
        )?;
        self.manifests
            .publish(&next, Some(previous_manifest.as_str()))?;
        self.segments = compacted_segments;
        self.manifest = next.clone();
        inject_compaction_failure(failure, CompactionBoundary::ManifestPublished)?;
        Ok(Some(CompactionOutcome {
            previous_manifest,
            manifest: next.digest,
            input_segments,
            output_segments: self.segments.len(),
            input_versions,
            output_versions,
            protected_sequences: protected_sequences.into_iter().collect(),
        }))
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
        let segment_candidates =
            unreachable_files(&segment_directory, "seg", &retained_segments)?;
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
        remove_candidates(
            &wal_directory,
            wal_candidates,
            &mut report.removed_wals,
        )?;
        Ok(report)
    }

    pub fn get(&self, key: &[u8], snapshot: Snapshot) -> Option<&[u8]> {
        self.visible_version(key, snapshot.sequence)?
            .value
            .as_deref()
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        snapshot: Snapshot,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut visible = BTreeMap::<Vec<u8>, VersionedValue>::new();
        for (key, version) in self
            .segments
            .iter()
            .flat_map(|segment| segment.visible_versions(snapshot.sequence))
            .chain(self.memtable.visible_versions(snapshot.sequence))
        {
            if key.as_slice() < start || end.is_some_and(|end| key.as_slice() >= end) {
                continue;
            }
            if visible
                .get(&key)
                .is_none_or(|current| version.sequence > current.sequence)
            {
                visible.insert(key, version);
            }
        }
        visible
            .into_iter()
            .filter_map(|(key, version)| version.value.map(|value| (key, value)))
            .collect()
    }

    pub fn memtable(&self) -> &Memtable {
        &self.memtable
    }

    fn visible_version(&self, key: &[u8], sequence: u64) -> Option<&VersionedValue> {
        self.segments
            .iter()
            .filter_map(|segment| segment.get_version(key, sequence))
            .chain(self.memtable.get_version(key, sequence))
            .max_by_key(|version| version.sequence)
    }
}

fn wal_path(root: &Path, starting_sequence: u64) -> PathBuf {
    root.join(WAL_DIRECTORY)
        .join(format!("{starting_sequence:020}.wal"))
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
