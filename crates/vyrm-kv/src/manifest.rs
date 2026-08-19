use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest;

pub const MANIFEST_FORMAT_VERSION: u16 = 1;
const CURRENT_FILE: &str = "CURRENT";
const MANIFEST_DIRECTORY: &str = "manifests";
static POINTER_TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentDescriptor {
    pub id: String,
    pub level: u8,
    pub first_key: Vec<u8>,
    pub last_key: Vec<u8>,
    pub minimum_sequence: u64,
    pub maximum_sequence: u64,
    pub entries: u64,
    pub bytes: u64,
    pub checksum: String,
}

impl SegmentDescriptor {
    fn validate(&self) -> Result<()> {
        if !is_sha256(&self.id) || !is_sha256(&self.checksum) {
            return Err(Error::InvalidManifest(
                "segment identity and checksum must be content-addressed SHA-256 values".into(),
            ));
        }
        if self.first_key > self.last_key {
            return Err(Error::InvalidManifest(format!(
                "segment {} has an inverted key range",
                self.id
            )));
        }
        if self.minimum_sequence == 0 || self.minimum_sequence > self.maximum_sequence {
            return Err(Error::InvalidManifest(format!(
                "segment {} has an invalid sequence range",
                self.id
            )));
        }
        if self.entries == 0 || self.bytes == 0 {
            return Err(Error::InvalidManifest(format!(
                "segment {} must contain entries and bytes",
                self.id
            )));
        }
        Ok(())
    }
}

/// Immutable physical state published by a single compare-and-swap update of
/// the `CURRENT` pointer. The digest excludes itself and is the manifest id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u16,
    pub generation: u64,
    pub parent: Option<String>,
    pub created_at: u64,
    pub durable_sequence: u64,
    pub wal_start_sequence: u64,
    pub segments: Vec<SegmentDescriptor>,
    pub digest: String,
}

impl Manifest {
    pub fn new(
        generation: u64,
        parent: Option<String>,
        created_at: u64,
        durable_sequence: u64,
        wal_start_sequence: u64,
        mut segments: Vec<SegmentDescriptor>,
    ) -> Result<Self> {
        segments.sort_by(|left, right| {
            left.level
                .cmp(&right.level)
                .then_with(|| left.first_key.cmp(&right.first_key))
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut manifest = Self {
            format_version: MANIFEST_FORMAT_VERSION,
            generation,
            parent,
            created_at,
            durable_sequence,
            wal_start_sequence,
            segments,
            digest: String::new(),
        };
        manifest.validate_components()?;
        manifest.digest = digest::sha256_hex(&manifest.bytes_without_digest()?);
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "manifest",
                version: self.format_version,
            });
        }
        self.validate_components()?;
        let expected = digest::sha256_hex(&self.bytes_without_digest()?);
        if self.digest != expected {
            return Err(Error::InvalidManifest(
                "manifest digest does not match its fields".into(),
            ));
        }
        Ok(())
    }

    fn validate_components(&self) -> Result<()> {
        if self.generation == 0 {
            return Err(Error::InvalidManifest(
                "manifest generation must be greater than zero".into(),
            ));
        }
        if self.wal_start_sequence > self.durable_sequence.saturating_add(1) {
            return Err(Error::InvalidManifest(
                "WAL start cannot skip beyond the durable sequence".into(),
            ));
        }
        if self.generation == 1 && self.parent.is_some() {
            return Err(Error::InvalidManifest(
                "the first manifest cannot name a parent".into(),
            ));
        }
        if self.generation > 1 && self.parent.as_deref().is_none_or(str::is_empty) {
            return Err(Error::InvalidManifest(
                "later manifests require a parent digest".into(),
            ));
        }
        if self
            .parent
            .as_deref()
            .is_some_and(|parent| !is_sha256(parent))
        {
            return Err(Error::InvalidManifest(
                "manifest parent must be a SHA-256 digest".into(),
            ));
        }
        let mut identities = BTreeSet::new();
        for segment in &self.segments {
            segment.validate()?;
            if !identities.insert(&segment.id) {
                return Err(Error::InvalidManifest(format!(
                    "segment {} appears more than once",
                    segment.id
                )));
            }
            if segment.maximum_sequence > self.durable_sequence {
                return Err(Error::InvalidManifest(format!(
                    "segment {} extends beyond the durable sequence",
                    segment.id
                )));
            }
        }
        Ok(())
    }

    fn bytes_without_digest(&self) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct Content<'a> {
            format_version: u16,
            generation: u64,
            parent: &'a Option<String>,
            created_at: u64,
            durable_sequence: u64,
            wal_start_sequence: u64,
            segments: &'a [SegmentDescriptor],
        }
        Ok(serde_json::to_vec(&Content {
            format_version: self.format_version,
            generation: self.generation,
            parent: &self.parent,
            created_at: self.created_at,
            durable_sequence: self.durable_sequence,
            wal_start_sequence: self.wal_start_sequence,
            segments: &self.segments,
        })?)
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentPointer {
    pub format_version: u16,
    pub generation: u64,
    pub manifest: String,
    pub checksum: String,
}

impl CurrentPointer {
    fn new(manifest: &Manifest) -> Self {
        let mut pointer = Self {
            format_version: MANIFEST_FORMAT_VERSION,
            generation: manifest.generation,
            manifest: manifest.digest.clone(),
            checksum: String::new(),
        };
        pointer.checksum = pointer.expected_checksum();
        pointer
    }

    fn validate(&self) -> Result<()> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "CURRENT pointer",
                version: self.format_version,
            });
        }
        if self.generation == 0 || !is_sha256(&self.manifest) || !is_sha256(&self.checksum) {
            return Err(Error::InvalidManifest(
                "invalid CURRENT pointer fields".into(),
            ));
        }
        if self.checksum != self.expected_checksum() {
            return Err(Error::InvalidManifest(
                "CURRENT pointer checksum mismatch".into(),
            ));
        }
        Ok(())
    }

    fn expected_checksum(&self) -> String {
        let mut bytes = b"vyrm-current-v1\0".to_vec();
        bytes.extend_from_slice(&self.format_version.to_be_bytes());
        bytes.extend_from_slice(&self.generation.to_be_bytes());
        bytes.extend_from_slice(self.manifest.as_bytes());
        digest::sha256_hex(&bytes)
    }
}

/// Holds the process lock for the full publication session. Manifest bytes are
/// synced before `CURRENT`, then the parent directory is synced after rename.
pub struct ManifestStore {
    root: PathBuf,
    _lock: File,
}

impl ManifestStore {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join(MANIFEST_DIRECTORY))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("MANIFEST.LOCK"))?;
        lock.lock()?;
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
        })
    }

    pub fn current(&self) -> Result<Option<(CurrentPointer, Manifest)>> {
        let path = self.root.join(CURRENT_FILE);
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let pointer: CurrentPointer = serde_json::from_slice(&bytes)?;
        pointer.validate()?;
        let manifest = self.load(&pointer.manifest)?;
        if manifest.generation != pointer.generation {
            return Err(Error::InvalidManifest(
                "CURRENT generation differs from its manifest".into(),
            ));
        }
        Ok(Some((pointer, manifest)))
    }

    pub fn load(&self, digest: &str) -> Result<Manifest> {
        if !is_sha256(digest) {
            return Err(Error::InvalidManifest(
                "manifest lookup requires a SHA-256 identity".into(),
            ));
        }
        let bytes = std::fs::read(
            self.root
                .join(MANIFEST_DIRECTORY)
                .join(format!("{digest}.json")),
        )?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        manifest.validate()?;
        if manifest.digest != digest {
            return Err(Error::InvalidManifest(
                "manifest filename differs from its content identity".into(),
            ));
        }
        Ok(manifest)
    }

    pub fn publish(&self, manifest: &Manifest, expected: Option<&str>) -> Result<CurrentPointer> {
        manifest.validate()?;
        let current = self.current()?;
        let actual = current
            .as_ref()
            .map(|(pointer, _)| pointer.manifest.clone());
        if actual.as_deref() != expected {
            return Err(Error::ManifestConflict {
                expected: expected.map(str::to_owned),
                actual,
            });
        }
        match current {
            None if manifest.generation == 1 && manifest.parent.is_none() => {}
            Some((pointer, _))
                if manifest.generation == pointer.generation + 1
                    && manifest.parent.as_deref() == Some(pointer.manifest.as_str()) => {}
            _ => {
                return Err(Error::InvalidManifest(
                    "manifest generation/parent does not extend CURRENT".into(),
                ))
            }
        }
        self.persist_manifest(manifest)?;
        let pointer = CurrentPointer::new(manifest);
        self.publish_pointer(&pointer)?;
        Ok(pointer)
    }

    fn persist_manifest(&self, manifest: &Manifest) -> Result<()> {
        let directory = self.root.join(MANIFEST_DIRECTORY);
        let path = directory.join(format!("{}.json", manifest.digest));
        let bytes = serde_json::to_vec(manifest)?;
        if path.exists() {
            if std::fs::read(&path)? != bytes {
                return Err(Error::InvalidManifest(
                    "existing manifest identity has different bytes".into(),
                ));
            }
            return Ok(());
        }
        let temporary = directory.join(format!(
            ".{}.{}.tmp",
            manifest.digest,
            POINTER_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        persist_rename(&directory, &temporary, &path, &bytes)
    }

    fn publish_pointer(&self, pointer: &CurrentPointer) -> Result<()> {
        let path = self.root.join(CURRENT_FILE);
        let temporary = self.root.join(format!(
            ".CURRENT.{}.tmp",
            POINTER_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        persist_rename(&self.root, &temporary, &path, &serde_json::to_vec(pointer)?)
    }
}

fn persist_rename(directory: &Path, temporary: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    if let Err(error) = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(temporary, target)?;
        File::open(directory)?.sync_all()
    })() {
        let _ = std::fs::remove_file(temporary);
        return Err(Error::Io(error));
    }
    Ok(())
}
