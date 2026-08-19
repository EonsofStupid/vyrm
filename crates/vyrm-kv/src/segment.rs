use crate::{Error, Memtable, Result, SegmentDescriptor, VersionedValue};
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use vyrm_core::digest;

pub const SEGMENT_FORMAT_VERSION: u16 = 2;
const SEGMENT_V1_MAGIC: &[u8; 8] = b"VYRSEG01";
const SEGMENT_V2_MAGIC: &[u8; 8] = b"VYRSEG02";
const V1_HEADER_BYTES: usize = 40;
const V2_HEADER_BYTES: usize = 48;
const RECORD_HEADER_BYTES: usize = 20;
const FOOTER_BYTES: usize = 64;
const MAX_SEGMENT_BYTES: u64 = 1024 * 1024 * 1024;
const SPARSE_INDEX_STRIDE: usize = 4;
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub descriptor: SegmentDescriptor,
    bytes: Vec<u8>,
    content_end: usize,
    sparse_index: Vec<SparseEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SparseEntry {
    offset: usize,
    key_start: usize,
    key_end: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SegmentVersion<'a> {
    pub sequence: u64,
    pub value: Option<&'a [u8]>,
}

#[derive(Debug, Clone, Copy)]
struct Record<'a> {
    key: &'a [u8],
    value: Option<&'a [u8]>,
    sequence: u64,
    next: usize,
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
            return invalid("segment exceeds the 1 GiB physical safety limit");
        }
        decode(std::fs::read(path)?)
    }

    pub(crate) fn validate_snapshot_bytes(
        expected: &SegmentDescriptor,
        bytes: &[u8],
    ) -> Result<Self> {
        if bytes.len() as u64 > MAX_SEGMENT_BYTES {
            return invalid("snapshot segment exceeds the 1 GiB physical safety limit");
        }
        let mut segment = decode(bytes.to_vec())?;
        segment.descriptor.level = expected.level;
        if &segment.descriptor != expected {
            return Err(Error::InvalidSegment(format!(
                "snapshot segment {} differs from its descriptor",
                expected.id
            )));
        }
        Ok(segment)
    }

    pub(crate) fn install_snapshot_bytes(
        directory: &Path,
        expected: &SegmentDescriptor,
        bytes: &[u8],
    ) -> Result<Self> {
        let segment = Self::validate_snapshot_bytes(expected, bytes)?;
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!("{}.seg", expected.id));
        if path.exists() {
            let existing = Self::open(&path)?;
            if existing.descriptor.id != expected.id || std::fs::read(&path)? != bytes {
                return Err(Error::InvalidSegment(format!(
                    "existing snapshot segment {} has different bytes",
                    expected.id
                )));
            }
            return Ok(segment);
        }
        let temporary = directory.join(format!(
            ".{}.{}.{}.snapshot.tmp",
            expected.id,
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Err(error) = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            File::open(directory)?.sync_all()
        })() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::Io(error));
        }
        Ok(segment)
    }

    pub fn get(&self, key: &[u8], read_sequence: u64) -> Option<&[u8]> {
        self.get_version(key, read_sequence)?.value
    }

    pub(crate) fn get_version(&self, key: &[u8], read_sequence: u64) -> Option<SegmentVersion<'_>> {
        if key < self.descriptor.first_key.as_slice()
            || key > self.descriptor.last_key.as_slice()
            || read_sequence < self.descriptor.minimum_sequence
        {
            return None;
        }
        let mut cursor = self.seek_offset(key);
        let mut selected = None;
        while cursor < self.content_end {
            let record = self.record_at(cursor)?;
            match record.key.cmp(key) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal if record.sequence <= read_sequence => {
                    selected = Some(SegmentVersion {
                        sequence: record.sequence,
                        value: record.value,
                    });
                }
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => break,
            }
            cursor = record.next;
        }
        selected
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.visible_from(start, end, read_sequence)
            .into_iter()
            .filter_map(|(key, version)| version.value.map(|value| (key, value)))
            .collect()
    }

    pub fn visible_versions(&self, read_sequence: u64) -> Vec<(Vec<u8>, VersionedValue)> {
        self.visible_from(&[], None, read_sequence)
    }

    pub(crate) fn all_versions(&self) -> Vec<(Vec<u8>, Vec<VersionedValue>)> {
        let mut output = Vec::<(Vec<u8>, Vec<VersionedValue>)>::new();
        let mut cursor = 0;
        while cursor < self.content_end {
            let record = self
                .record_at(cursor)
                .expect("validated segment contains complete records");
            if output
                .last()
                .is_none_or(|(key, _)| key.as_slice() != record.key)
            {
                output.push((record.key.to_vec(), Vec::new()));
            }
            output
                .last_mut()
                .expect("record creates a version group")
                .1
                .push(VersionedValue {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                });
            cursor = record.next;
        }
        output
    }

    pub fn sparse_index_entries(&self) -> usize {
        self.sparse_index.len()
    }

    fn visible_from(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Vec<(Vec<u8>, VersionedValue)> {
        let mut output = Vec::new();
        let mut cursor = self.seek_offset(start);
        let mut current_key: Option<&[u8]> = None;
        let mut selected: Option<SegmentVersion<'_>> = None;
        while cursor < self.content_end {
            let record = self
                .record_at(cursor)
                .expect("validated segment contains complete records");
            if current_key.is_some_and(|key| key != record.key) {
                let key = current_key.expect("a changed key has a prior group");
                if key >= start && end.is_none_or(|end| key < end) {
                    if let Some(version) = selected {
                        output.push((
                            key.to_vec(),
                            VersionedValue {
                                sequence: version.sequence,
                                value: version.value.map(<[u8]>::to_vec),
                            },
                        ));
                    }
                }
                if end.is_some_and(|end| record.key >= end) {
                    return output;
                }
                selected = None;
            }
            current_key = Some(record.key);
            if record.sequence <= read_sequence {
                selected = Some(SegmentVersion {
                    sequence: record.sequence,
                    value: record.value,
                });
            }
            cursor = record.next;
        }
        if let (Some(key), Some(version)) = (current_key, selected) {
            if key >= start && end.is_none_or(|end| key < end) {
                output.push((
                    key.to_vec(),
                    VersionedValue {
                        sequence: version.sequence,
                        value: version.value.map(<[u8]>::to_vec),
                    },
                ));
            }
        }
        output
    }

    fn seek_offset(&self, key: &[u8]) -> usize {
        let position = self
            .sparse_index
            .partition_point(|entry| &self.bytes[entry.key_start..entry.key_end] <= key);
        self.sparse_index[position.saturating_sub(1)].offset
    }

    fn record_at(&self, offset: usize) -> Option<Record<'_>> {
        parse_record(
            &self.bytes,
            offset,
            self.content_end,
            self.descriptor.minimum_sequence,
            self.descriptor.maximum_sequence,
        )
        .ok()
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
    let mut records = Vec::with_capacity(table.approximate_bytes());
    for (key, versions) in table.all_versions() {
        for version in versions {
            let (kind, value): (u8, &[u8]) = match &version.value {
                Some(value) => (1, value),
                None => (2, &[]),
            };
            records.push(kind);
            records.extend_from_slice(&[0, 0, 0]);
            records.extend_from_slice(
                &u32::try_from(key.len())
                    .map_err(|_| Error::InvalidSegment("key length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            records.extend_from_slice(
                &u32::try_from(value.len())
                    .map_err(|_| Error::InvalidSegment("value length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            records.extend_from_slice(&version.sequence.to_be_bytes());
            records.extend_from_slice(key);
            records.extend_from_slice(value);
        }
    }
    let record_bytes = u64::try_from(records.len())
        .map_err(|_| Error::InvalidSegment("record bytes exceed u64".into()))?;
    if record_bytes > MAX_SEGMENT_BYTES {
        return invalid("uncompressed records exceed the 1 GiB safety limit");
    }
    let compressed = compress_prepend_size(&records);
    let mut bytes = Vec::with_capacity(V2_HEADER_BYTES + compressed.len() + FOOTER_BYTES);
    bytes.extend_from_slice(SEGMENT_V2_MAGIC);
    bytes.extend_from_slice(&SEGMENT_FORMAT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&(V2_HEADER_BYTES as u16).to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&entries.to_be_bytes());
    bytes.extend_from_slice(&minimum_sequence.to_be_bytes());
    bytes.extend_from_slice(&maximum_sequence.to_be_bytes());
    bytes.extend_from_slice(&record_bytes.to_be_bytes());
    bytes.extend_from_slice(&compressed);
    let checksum = digest::sha256_hex(&bytes);
    bytes.extend_from_slice(checksum.as_bytes());
    Ok(bytes)
}

fn decode(bytes: Vec<u8>) -> Result<Segment> {
    if bytes.len() < V1_HEADER_BYTES + FOOTER_BYTES {
        return invalid("segment is shorter than its header and footer");
    }
    let version = u16::from_be_bytes(bytes[8..10].try_into().expect("fixed version field"));
    let (header_bytes, compressed) = match (&bytes[0..8], version) {
        (magic, 1) if magic == SEGMENT_V1_MAGIC => {
            let header_len =
                u16::from_be_bytes(bytes[10..12].try_into().expect("fixed header length"));
            if header_len as usize != V1_HEADER_BYTES || bytes[12..16] != [0, 0, 0, 0] {
                return invalid("unknown v1 segment header length or flags");
            }
            (V1_HEADER_BYTES, false)
        }
        (magic, SEGMENT_FORMAT_VERSION) if magic == SEGMENT_V2_MAGIC => {
            if bytes.len() < V2_HEADER_BYTES + FOOTER_BYTES {
                return invalid("v2 segment is shorter than its header and footer");
            }
            let header_len =
                u16::from_be_bytes(bytes[10..12].try_into().expect("fixed header length"));
            if header_len as usize != V2_HEADER_BYTES || bytes[12..16] != [0, 0, 0, 1] {
                return invalid("unknown v2 segment header length or compression flags");
            }
            (V2_HEADER_BYTES, true)
        }
        (_, 1 | SEGMENT_FORMAT_VERSION) => return invalid("segment magic does not match version"),
        _ => {
            return Err(Error::UnsupportedVersion {
                object: "segment",
                version,
            });
        }
    };
    let entries = u64::from_be_bytes(bytes[16..24].try_into().expect("fixed entry count"));
    let minimum_sequence = u64::from_be_bytes(bytes[24..32].try_into().expect("fixed sequence"));
    let maximum_sequence = u64::from_be_bytes(bytes[32..40].try_into().expect("fixed sequence"));
    if entries == 0 || minimum_sequence == 0 || minimum_sequence > maximum_sequence {
        return invalid("invalid segment count or sequence range");
    }
    let physical_content_end = bytes.len() - FOOTER_BYTES;
    let expected = std::str::from_utf8(&bytes[physical_content_end..])
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = digest::sha256_hex(&bytes[..physical_content_end]);
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    let records = if compressed {
        let declared = u64::from_be_bytes(bytes[40..48].try_into().expect("fixed record length"));
        if declared > MAX_SEGMENT_BYTES {
            return invalid("v2 uncompressed records exceed the 1 GiB safety limit");
        }
        let compressed = &bytes[header_bytes..physical_content_end];
        let prefixed = compressed
            .get(..4)
            .ok_or_else(|| Error::InvalidSegment("v2 compressed body has no size prefix".into()))?;
        let prefixed_size = u32::from_le_bytes(prefixed.try_into().expect("fixed size prefix"));
        if u64::from(prefixed_size) != declared {
            return invalid("v2 declared and compressed record lengths differ");
        }
        decompress_size_prepended(compressed)
            .map_err(|error| Error::InvalidSegment(format!("v2 LZ4 decode failed: {error}")))?
    } else {
        bytes[header_bytes..physical_content_end].to_vec()
    };
    let content_end = records.len();
    let mut cursor = 0;
    let mut previous: Option<(Vec<u8>, u64)> = None;
    let mut sparse_index = Vec::new();
    let mut keys_since_index = SPARSE_INDEX_STRIDE;
    let mut first_key = None;
    let mut last_key = None;
    for _ in 0..entries {
        let offset = cursor;
        let record = parse_record(
            &records,
            cursor,
            content_end,
            minimum_sequence,
            maximum_sequence,
        )?;
        let key = record.key.to_vec();
        let new_key = previous
            .as_ref()
            .is_none_or(|(prior_key, _)| prior_key != &key);
        if previous
            .as_ref()
            .is_some_and(|(prior_key, prior_sequence)| {
                key < *prior_key || (key == *prior_key && record.sequence <= *prior_sequence)
            })
        {
            return invalid("segment records are not in canonical key/sequence order");
        }
        if new_key {
            if first_key.is_none() {
                first_key = Some(key.clone());
            }
            last_key = Some(key.clone());
            if keys_since_index >= SPARSE_INDEX_STRIDE {
                sparse_index.push(SparseEntry {
                    offset,
                    key_start: offset + RECORD_HEADER_BYTES,
                    key_end: offset + RECORD_HEADER_BYTES + record.key.len(),
                });
                keys_since_index = 0;
            }
            keys_since_index += 1;
        }
        previous = Some((key, record.sequence));
        cursor = record.next;
    }
    if cursor != content_end {
        return invalid("segment contains trailing bytes before its footer");
    }
    let first_key = first_key.expect("entries are non-empty");
    let last_key = last_key.expect("entries are non-empty");
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
        bytes: records,
        content_end,
        sparse_index,
    })
}

fn parse_record(
    bytes: &[u8],
    offset: usize,
    content_end: usize,
    minimum_sequence: u64,
    maximum_sequence: u64,
) -> Result<Record<'_>> {
    let header_end = offset
        .checked_add(RECORD_HEADER_BYTES)
        .ok_or_else(|| Error::InvalidSegment("record header overflow".into()))?;
    let header = bytes
        .get(offset..header_end)
        .filter(|_| header_end <= content_end)
        .ok_or_else(|| Error::InvalidSegment("incomplete segment record header".into()))?;
    let kind = header[0];
    if header[1..4] != [0, 0, 0] {
        return invalid("unknown segment record flags");
    }
    let key_len = u32::from_be_bytes(header[4..8].try_into().expect("fixed key length")) as usize;
    let value_len =
        u32::from_be_bytes(header[8..12].try_into().expect("fixed value length")) as usize;
    let sequence = u64::from_be_bytes(header[12..20].try_into().expect("fixed sequence"));
    let end = header_end
        .checked_add(key_len)
        .and_then(|value| value.checked_add(value_len))
        .ok_or_else(|| Error::InvalidSegment("record length overflow".into()))?;
    let body = bytes
        .get(header_end..end)
        .filter(|_| end <= content_end)
        .ok_or_else(|| Error::InvalidSegment("incomplete segment record body".into()))?;
    if key_len == 0 || sequence < minimum_sequence || sequence > maximum_sequence {
        return invalid("invalid segment key or record sequence");
    }
    let key = &body[..key_len];
    let stored_value = &body[key_len..];
    let value = match kind {
        1 => Some(stored_value),
        2 if stored_value.is_empty() => None,
        2 => return invalid("segment tombstone carries a value"),
        _ => return invalid(format!("unknown segment record kind {kind}")),
    };
    Ok(Record {
        key,
        value,
        sequence,
        next: end,
    })
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidSegment(reason.into()))
}
