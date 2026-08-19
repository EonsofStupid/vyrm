use crate::{Error, Result, WriteBatch};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const WAL_FORMAT_VERSION: u16 = 1;
pub const WAL_MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const FILE_MAGIC: &[u8; 8] = b"VYRWAL01";
const RECORD_MAGIC: &[u8; 4] = b"VYR1";
const FILE_HEADER_BYTES: usize = 16;
const RECORD_HEADER_BYTES: usize = 32;
const RECORD_KIND_BATCH: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Durability {
    Buffered,
    Authoritative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalBatch<'a> {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub payload: &'a [u8],
}

impl WalBatch<'_> {
    fn validate(&self) -> Result<()> {
        if self.first_sequence == 0
            || self.last_sequence < self.first_sequence
            || self.last_sequence == u64::MAX
        {
            return Err(Error::InvalidBatch(
                "sequence range must be non-zero, ordered, and leave room for its successor".into(),
            ));
        }
        if self.payload.is_empty() {
            return Err(Error::InvalidBatch("payload must not be empty".into()));
        }
        if self.payload.len() > WAL_MAX_PAYLOAD_BYTES {
            return Err(Error::InvalidBatch(format!(
                "payload is {} bytes; maximum is {WAL_MAX_PAYLOAD_BYTES}",
                self.payload.len()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendReceipt {
    pub offset: u64,
    pub end_offset: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub checksum: u32,
    pub durable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveredBatch {
    pub offset: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub checksum: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovery {
    pub batches: Vec<RecoveredBatch>,
    pub recovered_through: u64,
    pub valid_bytes: u64,
    pub torn_tail: Option<u64>,
}

pub struct WalWriter {
    path: PathBuf,
    file: File,
    next_sequence: u64,
    poisoned: bool,
}

impl WalWriter {
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&file_header())?;
        file.sync_all()?;
        Ok(Self {
            path: path.to_owned(),
            file,
            next_sequence: 1,
            poisoned: false,
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let recovery = recover(path)?;
        if let Some(offset) = recovery.torn_tail {
            return Err(Error::TornTail { offset });
        }
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            next_sequence: recovery
                .recovered_through
                .checked_add(1)
                .ok_or_else(|| Error::InvalidBatch("sequence overflow".into()))?,
            poisoned: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn append(
        &mut self,
        batch: &WalBatch<'_>,
        durability: Durability,
    ) -> Result<AppendReceipt> {
        if self.poisoned {
            return Err(Error::PoisonedWriter);
        }
        batch.validate()?;
        if batch.first_sequence != self.next_sequence {
            return Err(Error::InvalidBatch(format!(
                "expected first sequence {}, received {}",
                self.next_sequence, batch.first_sequence
            )));
        }
        let header = record_header(batch)?;
        let offset = self.file.seek(SeekFrom::End(0))?;
        let write = (|| -> std::io::Result<()> {
            self.file.write_all(&header)?;
            self.file.write_all(batch.payload)?;
            if durability == Durability::Authoritative {
                self.file.sync_data()?;
            }
            Ok(())
        })();
        if let Err(error) = write {
            self.poisoned = true;
            return Err(Error::Io(error));
        }
        self.next_sequence = batch
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidBatch("sequence overflow".into()))?;
        Ok(AppendReceipt {
            offset,
            end_offset: offset + RECORD_HEADER_BYTES as u64 + batch.payload.len() as u64,
            first_sequence: batch.first_sequence,
            last_sequence: batch.last_sequence,
            checksum: u32::from_be_bytes(header[28..32].try_into().expect("fixed checksum field")),
            durable: durability == Durability::Authoritative,
        })
    }

    /// Allocates one MVCC sequence per operation and writes the full batch as
    /// one indivisible recovery frame.
    pub fn append_write_batch(
        &mut self,
        batch: &WriteBatch,
        durability: Durability,
    ) -> Result<AppendReceipt> {
        let payload = batch.encode()?;
        let count = u64::try_from(batch.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u64".into()))?;
        let last_sequence = self
            .next_sequence
            .checked_add(count - 1)
            .ok_or_else(|| Error::InvalidBatch("sequence range overflow".into()))?;
        self.append(
            &WalBatch {
                first_sequence: self.next_sequence,
                last_sequence,
                payload: &payload,
            },
            durability,
        )
    }

    pub fn sync(&mut self) -> Result<u64> {
        if self.poisoned {
            return Err(Error::PoisonedWriter);
        }
        self.file.sync_data()?;
        Ok(self.next_sequence.saturating_sub(1))
    }
}

pub fn recover(path: &Path) -> Result<Recovery> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length < FILE_HEADER_BYTES as u64 {
        return Err(Error::Corruption {
            offset: 0,
            reason: "incomplete WAL file header".into(),
        });
    }
    let mut file_header_bytes = [0u8; FILE_HEADER_BYTES];
    file.read_exact(&mut file_header_bytes)?;
    validate_file_header(&file_header_bytes)?;

    let mut batches = Vec::new();
    let mut offset = FILE_HEADER_BYTES as u64;
    let mut expected_sequence = 1u64;
    let mut torn_tail = None;
    while offset < length {
        let remaining = length - offset;
        if remaining < RECORD_HEADER_BYTES as u64 {
            torn_tail = Some(offset);
            break;
        }
        let mut header = [0u8; RECORD_HEADER_BYTES];
        file.read_exact(&mut header)?;
        let decoded = decode_record_header(&header, offset)?;
        if decoded.first_sequence != expected_sequence {
            return Err(Error::Corruption {
                offset,
                reason: format!(
                    "non-contiguous sequence: expected {expected_sequence}, found {}",
                    decoded.first_sequence
                ),
            });
        }
        let end = offset
            .checked_add(RECORD_HEADER_BYTES as u64)
            .and_then(|value| value.checked_add(decoded.payload_len as u64))
            .ok_or_else(|| Error::Corruption {
                offset,
                reason: "record length overflow".into(),
            })?;
        if end > length {
            torn_tail = Some(offset);
            break;
        }
        let mut payload = vec![0u8; decoded.payload_len];
        file.read_exact(&mut payload)?;
        let actual = crc32c(&[&header[4..28], &payload]);
        if actual != decoded.checksum {
            return Err(Error::Corruption {
                offset,
                reason: format!(
                    "checksum mismatch: expected {:08x}, computed {actual:08x}",
                    decoded.checksum
                ),
            });
        }
        batches.push(RecoveredBatch {
            offset,
            first_sequence: decoded.first_sequence,
            last_sequence: decoded.last_sequence,
            checksum: decoded.checksum,
            payload,
        });
        expected_sequence =
            decoded
                .last_sequence
                .checked_add(1)
                .ok_or_else(|| Error::Corruption {
                    offset,
                    reason: "sequence overflow".into(),
                })?;
        offset = end;
    }
    Ok(Recovery {
        recovered_through: expected_sequence.saturating_sub(1),
        batches,
        valid_bytes: offset,
        torn_tail,
    })
}

/// Truncates only an incomplete final frame previously classified by recovery.
/// Complete corrupt frames are never converted into apparent data loss.
pub fn repair_torn_tail(path: &Path) -> Result<Recovery> {
    let recovery = recover(path)?;
    let Some(_) = recovery.torn_tail else {
        return Ok(recovery);
    };
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(recovery.valid_bytes)?;
    file.sync_all()?;
    recover(path)
}

struct DecodedHeader {
    payload_len: usize,
    first_sequence: u64,
    last_sequence: u64,
    checksum: u32,
}

fn file_header() -> [u8; FILE_HEADER_BYTES] {
    let mut header = [0u8; FILE_HEADER_BYTES];
    header[0..8].copy_from_slice(FILE_MAGIC);
    header[8..10].copy_from_slice(&WAL_FORMAT_VERSION.to_be_bytes());
    header[10..12].copy_from_slice(&(FILE_HEADER_BYTES as u16).to_be_bytes());
    let checksum = crc32c(&[&header[0..12]]);
    header[12..16].copy_from_slice(&checksum.to_be_bytes());
    header
}

fn validate_file_header(header: &[u8; FILE_HEADER_BYTES]) -> Result<()> {
    if &header[0..8] != FILE_MAGIC {
        return Err(Error::Corruption {
            offset: 0,
            reason: "WAL file magic does not match".into(),
        });
    }
    let version = u16::from_be_bytes(header[8..10].try_into().expect("fixed version field"));
    if version != WAL_FORMAT_VERSION {
        return Err(Error::UnsupportedVersion {
            object: "WAL",
            version,
        });
    }
    let header_len = u16::from_be_bytes(header[10..12].try_into().expect("fixed length field"));
    if header_len as usize != FILE_HEADER_BYTES {
        return Err(Error::Corruption {
            offset: 10,
            reason: format!("unexpected WAL header length {header_len}"),
        });
    }
    let expected = u32::from_be_bytes(header[12..16].try_into().expect("fixed checksum field"));
    let actual = crc32c(&[&header[0..12]]);
    if actual != expected {
        return Err(Error::Corruption {
            offset: 0,
            reason: "WAL header checksum mismatch".into(),
        });
    }
    Ok(())
}

fn record_header(batch: &WalBatch<'_>) -> Result<[u8; RECORD_HEADER_BYTES]> {
    let payload_len = u32::try_from(batch.payload.len())
        .map_err(|_| Error::InvalidBatch("payload length exceeds u32".into()))?;
    let mut header = [0u8; RECORD_HEADER_BYTES];
    header[0..4].copy_from_slice(RECORD_MAGIC);
    header[4..6].copy_from_slice(&WAL_FORMAT_VERSION.to_be_bytes());
    header[6] = RECORD_KIND_BATCH;
    header[7] = 0;
    header[8..12].copy_from_slice(&payload_len.to_be_bytes());
    header[12..20].copy_from_slice(&batch.first_sequence.to_be_bytes());
    header[20..28].copy_from_slice(&batch.last_sequence.to_be_bytes());
    let checksum = crc32c(&[&header[4..28], batch.payload]);
    header[28..32].copy_from_slice(&checksum.to_be_bytes());
    Ok(header)
}

fn decode_record_header(header: &[u8; RECORD_HEADER_BYTES], offset: u64) -> Result<DecodedHeader> {
    if &header[0..4] != RECORD_MAGIC {
        return Err(Error::Corruption {
            offset,
            reason: "record magic does not match".into(),
        });
    }
    let version = u16::from_be_bytes(header[4..6].try_into().expect("fixed version field"));
    if version != WAL_FORMAT_VERSION {
        return Err(Error::UnsupportedVersion {
            object: "WAL record",
            version,
        });
    }
    if header[6] != RECORD_KIND_BATCH || header[7] != 0 {
        return Err(Error::Corruption {
            offset,
            reason: "unknown WAL record kind or flags".into(),
        });
    }
    let payload_len =
        u32::from_be_bytes(header[8..12].try_into().expect("fixed payload length")) as usize;
    if payload_len == 0 || payload_len > WAL_MAX_PAYLOAD_BYTES {
        return Err(Error::Corruption {
            offset,
            reason: format!("invalid payload length {payload_len}"),
        });
    }
    let first_sequence =
        u64::from_be_bytes(header[12..20].try_into().expect("fixed sequence field"));
    let last_sequence =
        u64::from_be_bytes(header[20..28].try_into().expect("fixed sequence field"));
    if first_sequence == 0 || last_sequence < first_sequence {
        return Err(Error::Corruption {
            offset,
            reason: "invalid record sequence range".into(),
        });
    }
    Ok(DecodedHeader {
        payload_len,
        first_sequence,
        last_sequence,
        checksum: u32::from_be_bytes(header[28..32].try_into().expect("fixed checksum field")),
    })
}

fn crc32c(chunks: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for chunk in chunks {
        for byte in *chunk {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0x82f6_3b78 & 0u32.wrapping_sub(crc & 1));
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::crc32c;

    #[test]
    fn crc32c_matches_the_published_check_value() {
        assert_eq!(crc32c(&[b"123456789"]), 0xe306_9283);
    }
}
