use crate::{Error, Manifest, Result, Segment, SegmentDescriptor};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use vyrm_core::digest;

pub const SNAPSHOT_BUNDLE_FORMAT_VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"VYRSNP01";
const HEADER_BYTES: usize = 20;
const FOOTER_BYTES: usize = 64;
pub const SNAPSHOT_BUNDLE_MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SEGMENTS: usize = 1_000_000;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_DESCRIPTOR_BYTES: usize = 1024 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotExportBoundary {
    HeaderWritten,
    SegmentWritten,
    FileSynced,
}

impl SnapshotExportBoundary {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::HeaderWritten => "snapshot_export.header_written",
            Self::SegmentWritten => "snapshot_export.segment_written",
            Self::FileSynced => "snapshot_export.file_synced",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSegment {
    pub descriptor: SegmentDescriptor,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotBundle {
    pub format_version: u16,
    pub source_manifest: Manifest,
    pub segments: Vec<SnapshotSegment>,
    pub digest: String,
}

#[derive(Debug, Clone)]
struct FileSegment {
    descriptor: SegmentDescriptor,
    offset: u64,
    length: u64,
}

/// Authenticated snapshot metadata backed by one bounded on-disk bundle.
///
/// Opening performs an outer streaming digest pass and then validates each
/// physical segment one at a time. The underlying VyrmKV engine remains an
/// in-memory segment engine; this type removes whole-bundle duplication from
/// snapshot transport without claiming that separate engine limit is solved.
#[derive(Debug, Clone)]
pub struct SnapshotBundleFile {
    path: PathBuf,
    pub source_manifest: Manifest,
    segments: Vec<FileSegment>,
    pub digest: String,
    pub length: u64,
}

impl SnapshotBundleFile {
    pub(crate) fn create(
        source_manifest: Manifest,
        segment_directory: &Path,
        destination: impl AsRef<Path>,
    ) -> Result<Self> {
        Self::create_with_hook(source_manifest, segment_directory, destination, |_| Ok(()))
    }

    pub(crate) fn create_with_hook(
        source_manifest: Manifest,
        segment_directory: &Path,
        destination: impl AsRef<Path>,
        mut hook: impl FnMut(SnapshotExportBoundary) -> Result<()>,
    ) -> Result<Self> {
        validate_manifest_contract(&source_manifest)?;
        let destination = destination.as_ref();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let manifest = serde_json::to_vec(&source_manifest)?;
        if manifest.is_empty() || manifest.len() > MAX_MANIFEST_BYTES {
            return Err(Error::InvalidManifest(
                "snapshot manifest length is outside its physical contract".into(),
            ));
        }
        let manifest_len = u32::try_from(manifest.len())
            .map_err(|_| Error::InvalidManifest("snapshot manifest exceeds u32".into()))?;
        let segment_count = u32::try_from(source_manifest.segments.len())
            .map_err(|_| Error::InvalidManifest("snapshot segment count exceeds u32".into()))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let created = (|| -> Result<()> {
            let mut hash = digest::Sha256::new();
            let mut written = 0u64;
            for bytes in [
                MAGIC.as_slice(),
                &SNAPSHOT_BUNDLE_FORMAT_VERSION.to_be_bytes(),
                &0u16.to_be_bytes(),
                &manifest_len.to_be_bytes(),
                &segment_count.to_be_bytes(),
                manifest.as_slice(),
            ] {
                write_hashed(&mut output, &mut hash, &mut written, bytes)?;
            }
            hook(SnapshotExportBoundary::HeaderWritten)?;
            let mut buffer = [0u8; STREAM_BUFFER_BYTES];
            for descriptor in &source_manifest.segments {
                let encoded = serde_json::to_vec(descriptor)?;
                if encoded.is_empty() || encoded.len() > MAX_DESCRIPTOR_BYTES {
                    return Err(Error::InvalidManifest(
                        "snapshot segment descriptor length is outside its physical contract"
                            .into(),
                    ));
                }
                let descriptor_len = u32::try_from(encoded.len()).map_err(|_| {
                    Error::InvalidManifest("snapshot segment descriptor exceeds u32".into())
                })?;
                let source_path = segment_directory.join(format!("{}.seg", descriptor.id));
                let mut source = File::open(&source_path)?;
                let segment_len = source.metadata()?.len();
                if segment_len != descriptor.bytes {
                    return Err(Error::InvalidManifest(format!(
                        "snapshot segment {} length differs from its manifest",
                        descriptor.id
                    )));
                }
                write_hashed(
                    &mut output,
                    &mut hash,
                    &mut written,
                    &descriptor_len.to_be_bytes(),
                )?;
                write_hashed(
                    &mut output,
                    &mut hash,
                    &mut written,
                    &segment_len.to_be_bytes(),
                )?;
                write_hashed(&mut output, &mut hash, &mut written, &encoded)?;
                loop {
                    let read = source.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    write_hashed(&mut output, &mut hash, &mut written, &buffer[..read])?;
                }
                hook(SnapshotExportBoundary::SegmentWritten)?;
            }
            let encoded_digest = hash.finalize_hex();
            output.write_all(encoded_digest.as_bytes())?;
            written = written
                .checked_add(FOOTER_BYTES as u64)
                .ok_or_else(|| Error::InvalidManifest("snapshot length overflowed u64".into()))?;
            if written > SNAPSHOT_BUNDLE_MAX_BYTES {
                return Err(Error::InvalidManifest(format!(
                    "snapshot bundle exceeds {SNAPSHOT_BUNDLE_MAX_BYTES} bytes"
                )));
            }
            output.sync_all()?;
            if let Some(parent) = destination.parent() {
                File::open(parent)?.sync_all()?;
            }
            hook(SnapshotExportBoundary::FileSynced)?;
            Ok(())
        })();
        if let Err(error) = created {
            drop(output);
            let _ = std::fs::remove_file(destination);
            return Err(error);
        }
        drop(output);
        Self::open(destination)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let length = file.metadata()?.len();
        if length < (HEADER_BYTES + FOOTER_BYTES) as u64 || length > SNAPSHOT_BUNDLE_MAX_BYTES {
            return Err(Error::InvalidManifest(
                "snapshot bundle length is outside its physical contract".into(),
            ));
        }
        let content_end = length - FOOTER_BYTES as u64;
        let mut hash = digest::Sha256::new();
        let mut remaining = content_end;
        let mut buffer = [0u8; STREAM_BUFFER_BYTES];
        while remaining != 0 {
            let limit = usize::try_from(remaining.min(STREAM_BUFFER_BYTES as u64))
                .expect("bounded snapshot read size");
            file.read_exact(&mut buffer[..limit])?;
            hash.update(&buffer[..limit]);
            remaining -= limit as u64;
        }
        let mut footer = [0u8; FOOTER_BYTES];
        file.read_exact(&mut footer)?;
        let encoded_digest = std::str::from_utf8(&footer)
            .map_err(|_| Error::InvalidManifest("snapshot digest is not ASCII".into()))?
            .to_owned();
        let actual_digest = hash.finalize_hex();
        if encoded_digest != actual_digest {
            return Err(Error::InvalidManifest(
                "snapshot bundle digest does not authenticate its encoded bytes".into(),
            ));
        }

        file.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; HEADER_BYTES];
        file.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Err(Error::InvalidManifest(
                "snapshot bundle magic does not match".into(),
            ));
        }
        let version = u16::from_be_bytes(header[8..10].try_into().expect("fixed version field"));
        if version != SNAPSHOT_BUNDLE_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "snapshot bundle",
                version,
            });
        }
        if header[10..12] != [0, 0] {
            return Err(Error::InvalidManifest(
                "snapshot bundle has unknown flags".into(),
            ));
        }
        let manifest_len =
            u32::from_be_bytes(header[12..16].try_into().expect("fixed manifest length")) as usize;
        let segment_count =
            u32::from_be_bytes(header[16..20].try_into().expect("fixed segment count")) as usize;
        if manifest_len == 0 || manifest_len > MAX_MANIFEST_BYTES || segment_count > MAX_SEGMENTS {
            return Err(Error::InvalidManifest(
                "snapshot bundle manifest length or segment count is invalid".into(),
            ));
        }
        ensure_file_range(HEADER_BYTES as u64, manifest_len as u64, content_end)?;
        let mut manifest = vec![0u8; manifest_len];
        file.read_exact(&mut manifest)?;
        let source_manifest: Manifest = serde_json::from_slice(&manifest)?;
        validate_manifest_contract(&source_manifest)?;
        if source_manifest.segments.len() != segment_count {
            return Err(Error::InvalidManifest(
                "snapshot bundle segment inventory does not match its manifest".into(),
            ));
        }

        let mut segments = Vec::with_capacity(segment_count);
        for expected in &source_manifest.segments {
            let mut lengths = [0u8; 12];
            file.read_exact(&mut lengths)?;
            let descriptor_len =
                u32::from_be_bytes(lengths[..4].try_into().expect("fixed descriptor length"))
                    as usize;
            let segment_len =
                u64::from_be_bytes(lengths[4..].try_into().expect("fixed segment length"));
            if descriptor_len == 0 || descriptor_len > MAX_DESCRIPTOR_BYTES {
                return Err(Error::InvalidManifest(
                    "snapshot segment descriptor length is outside its physical contract".into(),
                ));
            }
            let descriptor_at = file.stream_position()?;
            ensure_file_range(descriptor_at, descriptor_len as u64, content_end)?;
            let mut encoded = vec![0u8; descriptor_len];
            file.read_exact(&mut encoded)?;
            let descriptor: SegmentDescriptor = serde_json::from_slice(&encoded)?;
            if &descriptor != expected || segment_len != descriptor.bytes {
                return Err(Error::InvalidManifest(
                    "snapshot segment order, descriptor, or length differs from its manifest"
                        .into(),
                ));
            }
            let offset = file.stream_position()?;
            ensure_file_range(offset, segment_len, content_end)?;
            segments.push(FileSegment {
                descriptor,
                offset,
                length: segment_len,
            });
            file.seek(SeekFrom::Start(offset + segment_len))?;
        }
        if file.stream_position()? != content_end {
            return Err(Error::InvalidManifest(
                "snapshot bundle has undeclared content bytes".into(),
            ));
        }
        let bundle = Self {
            path,
            source_manifest,
            segments,
            digest: encoded_digest,
            length,
        };
        for index in 0..bundle.segments.len() {
            bundle.validated_segment(index)?;
        }
        Ok(bundle)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get_many(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        let mut selected = vec![None; keys.len()];
        for index in 0..self.segments.len() {
            let segment = self.validated_segment(index)?;
            for (key_index, key) in keys.iter().enumerate() {
                if let Some(version) =
                    segment.get_version(key, self.source_manifest.durable_sequence)?
                {
                    if selected[key_index]
                        .as_ref()
                        .is_none_or(|(sequence, _)| version.sequence > *sequence)
                    {
                        selected[key_index] = Some((version.sequence, version.value));
                    }
                }
            }
        }
        Ok(selected
            .into_iter()
            .map(|version| version.and_then(|(_, value)| value))
            .collect())
    }

    pub(crate) fn segment_bytes(&self, index: usize) -> Result<Vec<u8>> {
        let segment = self
            .segments
            .get(index)
            .ok_or_else(|| Error::InvalidManifest("snapshot segment index is invalid".into()))?;
        let length = usize::try_from(segment.length).map_err(|_| {
            Error::InvalidManifest("snapshot segment length exceeds this platform".into())
        })?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(segment.offset))?;
        let mut bytes = vec![0u8; length];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn descriptors(&self) -> impl Iterator<Item = &SegmentDescriptor> {
        self.segments.iter().map(|segment| &segment.descriptor)
    }

    fn validated_segment(&self, index: usize) -> Result<Segment> {
        let segment = self
            .segments
            .get(index)
            .ok_or_else(|| Error::InvalidManifest("snapshot segment index is invalid".into()))?;
        Segment::validate_snapshot_owned(&segment.descriptor, self.segment_bytes(index)?)
    }
}

impl SnapshotBundle {
    pub(crate) fn new(source_manifest: Manifest, segments: Vec<SnapshotSegment>) -> Result<Self> {
        let mut bundle = Self {
            format_version: SNAPSHOT_BUNDLE_FORMAT_VERSION,
            source_manifest,
            segments,
            digest: String::new(),
        };
        bundle.digest = digest::sha256_hex(&bundle.content_bytes()?);
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<()> {
        self.validated_segments().map(|_| ())
    }

    fn validated_segments(&self) -> Result<Vec<Segment>> {
        if self.format_version != SNAPSHOT_BUNDLE_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "snapshot bundle",
                version: self.format_version,
            });
        }
        self.source_manifest.validate()?;
        if self.source_manifest.wal_start_sequence
            != self.source_manifest.durable_sequence.saturating_add(1)
        {
            return Err(Error::InvalidManifest(
                "snapshot manifest is not flush-bounded to an empty successor WAL".into(),
            ));
        }
        if self.segments.len() > MAX_SEGMENTS
            || self.segments.len() != self.source_manifest.segments.len()
        {
            return Err(Error::InvalidManifest(
                "snapshot bundle segment inventory does not match its manifest".into(),
            ));
        }
        let mut segments = Vec::with_capacity(self.segments.len());
        for (bundled, expected) in self.segments.iter().zip(&self.source_manifest.segments) {
            if &bundled.descriptor != expected {
                return Err(Error::InvalidManifest(
                    "snapshot segment order or descriptor differs from its manifest".into(),
                ));
            }
            segments.push(Segment::validate_snapshot_bytes(expected, &bundled.bytes)?);
        }
        let content = self.content_bytes()?;
        if content.len().saturating_add(FOOTER_BYTES) > SNAPSHOT_BUNDLE_MAX_BYTES as usize {
            return Err(Error::InvalidManifest(format!(
                "snapshot bundle exceeds {SNAPSHOT_BUNDLE_MAX_BYTES} bytes"
            )));
        }
        let actual = digest::sha256_hex(&content);
        if self.digest != actual {
            return Err(Error::InvalidManifest(
                "snapshot bundle digest does not match its content".into(),
            ));
        }
        Ok(segments)
    }

    /// Reads one key from the authenticated physical closure without
    /// installing it. Snapshot consumers use this to bind transfer metadata to
    /// the exact state-machine bytes before the target manifest can advance.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get_many(&[key])?
            .pop()
            .expect("one requested key produces one result"))
    }

    /// Reads several keys with one bundle-validation and segment-decode pass.
    pub fn get_many(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        let segments = self.validated_segments()?;
        let mut selected = vec![None; keys.len()];
        for segment in &segments {
            for (index, key) in keys.iter().enumerate() {
                if let Some(version) =
                    segment.get_version(key, self.source_manifest.durable_sequence)?
                {
                    if selected[index]
                        .as_ref()
                        .is_none_or(|(sequence, _)| version.sequence > *sequence)
                    {
                        selected[index] = Some((version.sequence, version.value));
                    }
                }
            }
        }
        Ok(selected
            .into_iter()
            .map(|version| version.and_then(|(_, value)| value))
            .collect())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut output = self.content_bytes()?;
        output.extend_from_slice(self.digest.as_bytes());
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_BYTES + FOOTER_BYTES
            || bytes.len() > SNAPSHOT_BUNDLE_MAX_BYTES as usize
        {
            return Err(Error::InvalidManifest(
                "snapshot bundle length is outside its physical contract".into(),
            ));
        }
        if &bytes[..8] != MAGIC {
            return Err(Error::InvalidManifest(
                "snapshot bundle magic does not match".into(),
            ));
        }
        let version = u16::from_be_bytes(bytes[8..10].try_into().expect("fixed version field"));
        if version != SNAPSHOT_BUNDLE_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "snapshot bundle",
                version,
            });
        }
        if bytes[10..12] != [0, 0] {
            return Err(Error::InvalidManifest(
                "snapshot bundle has unknown flags".into(),
            ));
        }
        let content_end = bytes.len() - FOOTER_BYTES;
        let encoded_digest = std::str::from_utf8(&bytes[content_end..])
            .map_err(|_| Error::InvalidManifest("snapshot digest is not ASCII".into()))?
            .to_owned();
        let actual_digest = digest::sha256_hex(&bytes[..content_end]);
        if encoded_digest != actual_digest {
            return Err(Error::InvalidManifest(
                "snapshot bundle digest does not authenticate its encoded bytes".into(),
            ));
        }
        let manifest_len =
            u32::from_be_bytes(bytes[12..16].try_into().expect("fixed manifest length")) as usize;
        let segment_count =
            u32::from_be_bytes(bytes[16..20].try_into().expect("fixed segment count")) as usize;
        if manifest_len == 0 || segment_count > MAX_SEGMENTS {
            return Err(Error::InvalidManifest(
                "snapshot bundle manifest length or segment count is invalid".into(),
            ));
        }
        let mut cursor = HEADER_BYTES;
        let manifest_end = checked_end(cursor, manifest_len, content_end)?;
        let source_manifest: Manifest = serde_json::from_slice(&bytes[cursor..manifest_end])?;
        cursor = manifest_end;
        let mut segments = Vec::with_capacity(segment_count);
        for _ in 0..segment_count {
            let descriptor_len = read_u32(bytes, &mut cursor, content_end)? as usize;
            let segment_len = read_u64(bytes, &mut cursor, content_end)?;
            let segment_len = usize::try_from(segment_len).map_err(|_| {
                Error::InvalidManifest("snapshot segment length exceeds this platform".into())
            })?;
            let descriptor_end = checked_end(cursor, descriptor_len, content_end)?;
            let descriptor: SegmentDescriptor =
                serde_json::from_slice(&bytes[cursor..descriptor_end])?;
            cursor = descriptor_end;
            let segment_end = checked_end(cursor, segment_len, content_end)?;
            segments.push(SnapshotSegment {
                descriptor,
                bytes: bytes[cursor..segment_end].to_vec(),
            });
            cursor = segment_end;
        }
        if cursor != content_end {
            return Err(Error::InvalidManifest(format!(
                "snapshot bundle has {} undeclared content bytes",
                content_end - cursor
            )));
        }
        let bundle = Self {
            format_version: version,
            source_manifest,
            segments,
            digest: encoded_digest,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    fn content_bytes(&self) -> Result<Vec<u8>> {
        let manifest = serde_json::to_vec(&self.source_manifest)?;
        let manifest_len = u32::try_from(manifest.len())
            .map_err(|_| Error::InvalidManifest("snapshot manifest exceeds u32".into()))?;
        let segment_count = u32::try_from(self.segments.len())
            .map_err(|_| Error::InvalidManifest("snapshot segment count exceeds u32".into()))?;
        let mut output = Vec::with_capacity(HEADER_BYTES + manifest.len());
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&self.format_version.to_be_bytes());
        output.extend_from_slice(&0u16.to_be_bytes());
        output.extend_from_slice(&manifest_len.to_be_bytes());
        output.extend_from_slice(&segment_count.to_be_bytes());
        output.extend_from_slice(&manifest);
        for segment in &self.segments {
            let descriptor = serde_json::to_vec(&segment.descriptor)?;
            let descriptor_len = u32::try_from(descriptor.len()).map_err(|_| {
                Error::InvalidManifest("snapshot segment descriptor exceeds u32".into())
            })?;
            let segment_len = u64::try_from(segment.bytes.len()).map_err(|_| {
                Error::InvalidManifest("snapshot segment length exceeds u64".into())
            })?;
            output.extend_from_slice(&descriptor_len.to_be_bytes());
            output.extend_from_slice(&segment_len.to_be_bytes());
            output.extend_from_slice(&descriptor);
            output.extend_from_slice(&segment.bytes);
            if output.len().saturating_add(FOOTER_BYTES) > SNAPSHOT_BUNDLE_MAX_BYTES as usize {
                return Err(Error::InvalidManifest(format!(
                    "snapshot bundle exceeds {SNAPSHOT_BUNDLE_MAX_BYTES} bytes"
                )));
            }
        }
        Ok(output)
    }
}

fn read_u32(bytes: &[u8], cursor: &mut usize, end: usize) -> Result<u32> {
    let next = checked_end(*cursor, 4, end)?;
    let value = u32::from_be_bytes(bytes[*cursor..next].try_into().expect("fixed u32 field"));
    *cursor = next;
    Ok(value)
}

fn read_u64(bytes: &[u8], cursor: &mut usize, end: usize) -> Result<u64> {
    let next = checked_end(*cursor, 8, end)?;
    let value = u64::from_be_bytes(bytes[*cursor..next].try_into().expect("fixed u64 field"));
    *cursor = next;
    Ok(value)
}

fn checked_end(cursor: usize, length: usize, bound: usize) -> Result<usize> {
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| Error::InvalidManifest("snapshot field length overflowed".into()))?;
    if end > bound {
        return Err(Error::InvalidManifest(
            "snapshot field exceeds the declared bundle".into(),
        ));
    }
    Ok(end)
}

fn validate_manifest_contract(manifest: &Manifest) -> Result<()> {
    manifest.validate()?;
    if manifest.wal_start_sequence != manifest.durable_sequence.saturating_add(1) {
        return Err(Error::InvalidManifest(
            "snapshot manifest is not flush-bounded to an empty successor WAL".into(),
        ));
    }
    if manifest.segments.len() > MAX_SEGMENTS {
        return Err(Error::InvalidManifest(
            "snapshot bundle has too many segments".into(),
        ));
    }
    Ok(())
}

fn write_hashed(
    output: &mut File,
    digest: &mut digest::Sha256,
    written: &mut u64,
    bytes: &[u8],
) -> Result<()> {
    *written = written
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| Error::InvalidManifest("snapshot length overflowed u64".into()))?;
    if written.saturating_add(FOOTER_BYTES as u64) > SNAPSHOT_BUNDLE_MAX_BYTES {
        return Err(Error::InvalidManifest(format!(
            "snapshot bundle exceeds {SNAPSHOT_BUNDLE_MAX_BYTES} bytes"
        )));
    }
    output.write_all(bytes)?;
    digest.update(bytes);
    Ok(())
}

fn ensure_file_range(offset: u64, length: u64, bound: u64) -> Result<()> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::InvalidManifest("snapshot field length overflowed".into()))?;
    if end > bound {
        return Err(Error::InvalidManifest(
            "snapshot field exceeds the declared bundle".into(),
        ));
    }
    Ok(())
}
