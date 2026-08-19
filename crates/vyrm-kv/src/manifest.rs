use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use vyrm_core::digest;

pub const MANIFEST_FORMAT_VERSION: u16 = 1;

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
