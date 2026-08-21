use crate::{Error, Result, WAL_MAX_PAYLOAD_BYTES};
use serde::{Deserialize, Serialize};

pub const BATCH_FORMAT_VERSION: u16 = 1;
const BATCH_MAGIC: &[u8; 8] = b"VYRBAT01";
const BATCH_HEADER_BYTES: usize = 16;
const OP_HEADER_BYTES: usize = 12;
const MAX_OPERATIONS: usize = 1_000_000;
const MAX_KEY_BYTES: usize = 1024 * 1024;
const MAX_VALUE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Mutation {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

impl Mutation {
    pub fn key(&self) -> &[u8] {
        match self {
            Self::Put { key, .. } | Self::Delete { key } => key,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.key().is_empty() || self.key().len() > MAX_KEY_BYTES {
            return Err(Error::InvalidBatch(format!(
                "key length must be in 1..={MAX_KEY_BYTES} bytes"
            )));
        }
        if let Self::Put { value, .. } = self {
            if value.len() > MAX_VALUE_BYTES {
                return Err(Error::InvalidBatch(format!(
                    "value length exceeds {MAX_VALUE_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteBatch {
    pub operations: Vec<Mutation>,
}

impl WriteBatch {
    pub fn new(operations: Vec<Mutation>) -> Result<Self> {
        let batch = Self { operations };
        batch.validate()?;
        Ok(batch)
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let encoded_len = self.encoded_len()?;
        let count = u32::try_from(self.operations.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u32".into()))?;
        let mut output = Vec::with_capacity(encoded_len);
        output.extend_from_slice(BATCH_MAGIC);
        output.extend_from_slice(&BATCH_FORMAT_VERSION.to_be_bytes());
        output.extend_from_slice(&0u16.to_be_bytes());
        output.extend_from_slice(&count.to_be_bytes());
        for operation in &self.operations {
            let (kind, key, value): (u8, &[u8], &[u8]) = match operation {
                Mutation::Put { key, value } => (1, key, value),
                Mutation::Delete { key } => (2, key, &[]),
            };
            output.push(kind);
            output.extend_from_slice(&[0, 0, 0]);
            output.extend_from_slice(
                &u32::try_from(key.len())
                    .map_err(|_| Error::InvalidBatch("key length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            output.extend_from_slice(
                &u32::try_from(value.len())
                    .map_err(|_| Error::InvalidBatch("value length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            output.extend_from_slice(key);
            output.extend_from_slice(value);
        }
        debug_assert_eq!(output.len(), encoded_len);
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < BATCH_HEADER_BYTES {
            return invalid("incomplete batch header");
        }
        if &bytes[0..8] != BATCH_MAGIC {
            return invalid("batch magic does not match");
        }
        let version = u16::from_be_bytes(bytes[8..10].try_into().expect("fixed version field"));
        if version != BATCH_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "write batch",
                version,
            });
        }
        if bytes[10..12] != [0, 0] {
            return invalid("unknown batch flags");
        }
        let count =
            u32::from_be_bytes(bytes[12..16].try_into().expect("fixed count field")) as usize;
        if count == 0 || count > MAX_OPERATIONS {
            return invalid(format!("invalid operation count {count}"));
        }
        let mut cursor = BATCH_HEADER_BYTES;
        let mut operations = Vec::with_capacity(count);
        for _ in 0..count {
            let header_end = cursor
                .checked_add(OP_HEADER_BYTES)
                .ok_or_else(|| Error::InvalidBatch("operation header length overflow".into()))?;
            let header = bytes
                .get(cursor..header_end)
                .ok_or_else(|| Error::InvalidBatch("incomplete operation header".into()))?;
            let kind = header[0];
            if header[1..4] != [0, 0, 0] {
                return invalid("unknown operation flags");
            }
            let key_len =
                u32::from_be_bytes(header[4..8].try_into().expect("fixed key length")) as usize;
            let value_len =
                u32::from_be_bytes(header[8..12].try_into().expect("fixed value length")) as usize;
            if key_len == 0 || key_len > MAX_KEY_BYTES || value_len > MAX_VALUE_BYTES {
                return invalid("operation key/value length exceeds its contract");
            }
            cursor = header_end;
            let end = cursor
                .checked_add(key_len)
                .and_then(|value| value.checked_add(value_len))
                .ok_or_else(|| Error::InvalidBatch("operation length overflow".into()))?;
            let body = bytes
                .get(cursor..end)
                .ok_or_else(|| Error::InvalidBatch("incomplete operation body".into()))?;
            let key = body[..key_len].to_vec();
            let value = body[key_len..].to_vec();
            let operation = match kind {
                1 => Mutation::Put { key, value },
                2 if value.is_empty() => Mutation::Delete { key },
                2 => return invalid("delete operation carries a value"),
                _ => return invalid(format!("unknown operation kind {kind}")),
            };
            operations.push(operation);
            cursor = end;
        }
        if cursor != bytes.len() {
            return invalid(format!(
                "{} trailing byte(s) after the declared batch",
                bytes.len() - cursor
            ));
        }
        Self::new(operations)
    }

    fn validate(&self) -> Result<()> {
        self.encoded_len().map(|_| ())
    }

    fn encoded_len(&self) -> Result<usize> {
        if self.operations.is_empty() || self.operations.len() > MAX_OPERATIONS {
            return Err(Error::InvalidBatch(format!(
                "operation count must be in 1..={MAX_OPERATIONS}"
            )));
        }
        let mut encoded_len = BATCH_HEADER_BYTES;
        for operation in &self.operations {
            operation.validate()?;
            let value_len = match operation {
                Mutation::Put { value, .. } => value.len(),
                Mutation::Delete { .. } => 0,
            };
            encoded_len = encoded_len
                .checked_add(OP_HEADER_BYTES)
                .and_then(|length| length.checked_add(operation.key().len()))
                .and_then(|length| length.checked_add(value_len))
                .ok_or_else(|| Error::InvalidBatch("encoded batch length overflow".into()))?;
            if encoded_len > WAL_MAX_PAYLOAD_BYTES {
                return Err(Error::InvalidBatch(format!(
                    "encoded batch exceeds {WAL_MAX_PAYLOAD_BYTES} bytes"
                )));
            }
        }
        Ok(encoded_len)
    }
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidBatch(reason.into()))
}
