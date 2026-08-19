use crate::{
    recover_from, AppendReceipt, Checkpoint, Durability, Error, Manifest, ManifestStore, Memtable,
    RecoveredBatch, Result, Segment, VersionedValue, WalWriter, WriteBatch,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const WAL_DIRECTORY: &str = "wal";
const SEGMENT_DIRECTORY: &str = "segments";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub sequence: u64,
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
        if self.memtable.version_count() == 0 {
            self.wal.sync()?;
            return Ok(None);
        }
        let sequence = self.wal.sync()?;
        let (segment, _) =
            Segment::write_from_memtable(&self.root.join(SEGMENT_DIRECTORY), &self.memtable)?;
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
