use crate::{Error, Memtable, Result, SegmentDescriptor, VersionedValue};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest;

pub const SEGMENT_FORMAT_VERSION: u16 = 1;
const SEGMENT_MAGIC: &[u8; 8] = b"VYRSEG01";
const HEADER_BYTES: usize = 40;
const RECORD_HEADER_BYTES: usize = 20;
const FOOTER_BYTES: usize = 64;
const MAX_SEGMENT_BYTES: u64 = 1024 * 1024 * 1024;
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub descriptor: SegmentDescriptor,
    versions: BTreeMap<Vec<u8>, Vec<VersionedValue>>,
}

impl Segment {
    pub fn write_from_memtable(directory: &Path, table: &Memtable) -> Result<(Self, PathBuf)> {
        let bytes = encode(table)?;
        let digest = digest::sha256_hex(&bytes[..bytes.len() - FOOTER_BYTES]);
        let path = directory.join(format!("{digest}.seg"));
        std::fs::create_dir_all(directory)?;
        if path.exists() {
            let segment = Self::open(&path)?;
            if segment.descriptor.id != digest {
                return Err(Error::InvalidSegment(
                    "existing content-addressed segment has another identity".into(),
                ));
            }
            return Ok((segment, path));
        }
        let temporary = directory.join(format!(
            ".{digest}.{}.{}.tmp",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Err(error) = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            File::open(directory)?.sync_all()
        })() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::Io(error));
        }
        Ok((Self::open(&path)?, path))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > MAX_SEGMENT_BYTES {
            return invalid("segment exceeds the 1 GiB v1 safety limit");
        }
        let bytes = std::fs::read(path)?;
        decode(&bytes)
    }

    pub fn get(&self, key: &[u8], read_sequence: u64) -> Option<&[u8]> {
        self.get_version(key, read_sequence)?.value.as_deref()
    }

    pub fn get_version(&self, key: &[u8], read_sequence: u64) -> Option<&VersionedValue> {
        self.versions
            .get(key)?
            .iter()
            .rev()
            .find(|version| version.sequence <= read_sequence)
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.versions
            .iter()
            .filter(|(key, _)| {
                key.as_slice() >= start && end.is_none_or(|end| key.as_slice() < end)
            })
            .filter_map(|(key, versions)| {
                versions
                    .iter()
                    .rev()
                    .find(|version| version.sequence <= read_sequence)
                    .and_then(|version| version.value.as_ref())
                    .map(|value| (key.clone(), value.clone()))
            })
            .collect()
    }

    pub fn visible_versions(&self, read_sequence: u64) -> Vec<(Vec<u8>, VersionedValue)> {
        self.versions
            .iter()
            .filter_map(|(key, versions)| {
                versions
                    .iter()
                    .rev()
                    .find(|version| version.sequence <= read_sequence)
                    .cloned()
                    .map(|version| (key.clone(), version))
            })
            .collect()
    }

    pub(crate) fn all_versions(&self) -> impl Iterator<Item = (&[u8], &[VersionedValue])> {
        self.versions
            .iter()
            .map(|(key, versions)| (key.as_slice(), versions.as_slice()))
    }
}

fn encode(table: &Memtable) -> Result<Vec<u8>> {
    if table.version_count() == 0 {
        return invalid("cannot write an empty segment");
    }
    let entries = u64::try_from(table.version_count())
        .map_err(|_| Error::InvalidSegment("entry count exceeds u64".into()))?;
    let minimum_sequence = table
        .all_versions()
        .flat_map(|(_, versions)| versions.iter().map(|version| version.sequence))
        .min()
        .expect("non-empty table has a minimum sequence");
    let maximum_sequence = table.maximum_sequence();
    let mut bytes = Vec::with_capacity(HEADER_BYTES + table.approximate_bytes() + FOOTER_BYTES);
    bytes.extend_from_slice(SEGMENT_MAGIC);
    bytes.extend_from_slice(&SEGMENT_FORMAT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&(HEADER_BYTES as u16).to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&entries.to_be_bytes());
    bytes.extend_from_slice(&minimum_sequence.to_be_bytes());
    bytes.extend_from_slice(&maximum_sequence.to_be_bytes());
    for (key, versions) in table.all_versions() {
        for version in versions {
            let (kind, value): (u8, &[u8]) = match &version.value {
                Some(value) => (1, value),
                None => (2, &[]),
            };
            bytes.push(kind);
            bytes.extend_from_slice(&[0, 0, 0]);
            bytes.extend_from_slice(
                &u32::try_from(key.len())
                    .map_err(|_| Error::InvalidSegment("key length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(
                &u32::try_from(value.len())
                    .map_err(|_| Error::InvalidSegment("value length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(&version.sequence.to_be_bytes());
            bytes.extend_from_slice(key);
            bytes.extend_from_slice(value);
        }
    }
    let checksum = digest::sha256_hex(&bytes);
    bytes.extend_from_slice(checksum.as_bytes());
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<Segment> {
    if bytes.len() < HEADER_BYTES + FOOTER_BYTES {
        return invalid("segment is shorter than its header and footer");
    }
    if &bytes[0..8] != SEGMENT_MAGIC {
        return invalid("segment magic does not match");
    }
    let version = u16::from_be_bytes(bytes[8..10].try_into().expect("fixed version field"));
    if version != SEGMENT_FORMAT_VERSION {
        return Err(Error::UnsupportedVersion {
            object: "segment",
            version,
        });
    }
    let header_len = u16::from_be_bytes(bytes[10..12].try_into().expect("fixed header length"));
    if header_len as usize != HEADER_BYTES || bytes[12..16] != [0, 0, 0, 0] {
        return invalid("unknown segment header length or flags");
    }
    let entries = u64::from_be_bytes(bytes[16..24].try_into().expect("fixed entry count"));
    let minimum_sequence = u64::from_be_bytes(bytes[24..32].try_into().expect("fixed sequence"));
    let maximum_sequence = u64::from_be_bytes(bytes[32..40].try_into().expect("fixed sequence"));
    if entries == 0 || minimum_sequence == 0 || minimum_sequence > maximum_sequence {
        return invalid("invalid segment count or sequence range");
    }
    let content_end = bytes.len() - FOOTER_BYTES;
    let expected = std::str::from_utf8(&bytes[content_end..])
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = digest::sha256_hex(&bytes[..content_end]);
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    let mut cursor = HEADER_BYTES;
    let mut versions = BTreeMap::<Vec<u8>, Vec<VersionedValue>>::new();
    let mut previous: Option<(Vec<u8>, u64)> = None;
    for _ in 0..entries {
        let header_end = cursor
            .checked_add(RECORD_HEADER_BYTES)
            .ok_or_else(|| Error::InvalidSegment("record header overflow".into()))?;
        let header = bytes
            .get(cursor..header_end)
            .filter(|_| header_end <= content_end)
            .ok_or_else(|| Error::InvalidSegment("incomplete segment record header".into()))?;
        let kind = header[0];
        if header[1..4] != [0, 0, 0] {
            return invalid("unknown segment record flags");
        }
        let key_len =
            u32::from_be_bytes(header[4..8].try_into().expect("fixed key length")) as usize;
        let value_len =
            u32::from_be_bytes(header[8..12].try_into().expect("fixed value length")) as usize;
        let sequence = u64::from_be_bytes(header[12..20].try_into().expect("fixed sequence"));
        cursor = header_end;
        let end = cursor
            .checked_add(key_len)
            .and_then(|value| value.checked_add(value_len))
            .ok_or_else(|| Error::InvalidSegment("record length overflow".into()))?;
        let body = bytes
            .get(cursor..end)
            .filter(|_| end <= content_end)
            .ok_or_else(|| Error::InvalidSegment("incomplete segment record body".into()))?;
        if key_len == 0 || sequence < minimum_sequence || sequence > maximum_sequence {
            return invalid("invalid segment key or record sequence");
        }
        let key = body[..key_len].to_vec();
        let value = body[key_len..].to_vec();
        if previous
            .as_ref()
            .is_some_and(|(prior_key, prior_sequence)| {
                key < *prior_key || (key == *prior_key && sequence <= *prior_sequence)
            })
        {
            return invalid("segment records are not in canonical key/sequence order");
        }
        let value = match kind {
            1 => Some(value),
            2 if value.is_empty() => None,
            2 => return invalid("segment tombstone carries a value"),
            _ => return invalid(format!("unknown segment record kind {kind}")),
        };
        previous = Some((key.clone(), sequence));
        versions
            .entry(key)
            .or_default()
            .push(VersionedValue { sequence, value });
        cursor = end;
    }
    if cursor != content_end {
        return invalid("segment contains trailing bytes before its footer");
    }
    let first_key = versions
        .first_key_value()
        .expect("entries are non-empty")
        .0
        .clone();
    let last_key = versions
        .last_key_value()
        .expect("entries are non-empty")
        .0
        .clone();
    let id = actual;
    Ok(Segment {
        descriptor: SegmentDescriptor {
            id: id.clone(),
            level: 0,
            first_key,
            last_key,
            minimum_sequence,
            maximum_sequence,
            entries,
            bytes: bytes.len() as u64,
            checksum: id,
        },
        versions,
    })
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidSegment(reason.into()))
}
