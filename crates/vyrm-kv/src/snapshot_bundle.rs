use crate::{Error, Manifest, Result, Segment, SegmentDescriptor};
use serde::{Deserialize, Serialize};
use vyrm_core::digest;

pub const SNAPSHOT_BUNDLE_FORMAT_VERSION: u16 = 1;
const MAGIC: &[u8; 8] = b"VYRSNP01";
const HEADER_BYTES: usize = 20;
const FOOTER_BYTES: usize = 64;
const MAX_BUNDLE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_SEGMENTS: usize = 1_000_000;

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
        for (bundled, expected) in self.segments.iter().zip(&self.source_manifest.segments) {
            if &bundled.descriptor != expected {
                return Err(Error::InvalidManifest(
                    "snapshot segment order or descriptor differs from its manifest".into(),
                ));
            }
            Segment::validate_snapshot_bytes(expected, &bundled.bytes)?;
        }
        let content = self.content_bytes()?;
        if content.len().saturating_add(FOOTER_BYTES) > MAX_BUNDLE_BYTES {
            return Err(Error::InvalidManifest(format!(
                "snapshot bundle exceeds {MAX_BUNDLE_BYTES} bytes"
            )));
        }
        let actual = digest::sha256_hex(&content);
        if self.digest != actual {
            return Err(Error::InvalidManifest(
                "snapshot bundle digest does not match its content".into(),
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut output = self.content_bytes()?;
        output.extend_from_slice(self.digest.as_bytes());
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_BYTES + FOOTER_BYTES || bytes.len() > MAX_BUNDLE_BYTES {
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
            if output.len().saturating_add(FOOTER_BYTES) > MAX_BUNDLE_BYTES {
                return Err(Error::InvalidManifest(format!(
                    "snapshot bundle exceeds {MAX_BUNDLE_BYTES} bytes"
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
