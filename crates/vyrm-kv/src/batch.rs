use crate::{Error, Result, WAL_MAX_PAYLOAD_BYTES};
use serde::{Deserialize, Deserializer, Serialize};

pub const BATCH_FORMAT_VERSION: u16 = 2;
const BATCH_V1_MAGIC: &[u8; 8] = b"VYRBAT01";
const BATCH_V2_MAGIC: &[u8; 8] = b"VYRBAT02";
const BATCH_HEADER_BYTES: usize = 16;
const V1_OP_HEADER_BYTES: usize = 12;
const V2_OP_HEADER_BYTES: usize = 8;
const V2_DELETE_MASK: u32 = 1 << 31;
const V2_VALUE_LEN_MASK: u32 = !V2_DELETE_MASK;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteBatch {
    pub(crate) operations: Vec<Mutation>,
    #[serde(skip)]
    encoded_len: usize,
}

impl WriteBatch {
    pub fn new(operations: Vec<Mutation>) -> Result<Self> {
        let encoded_len = validate_operations(&operations)?;
        Ok(Self {
            operations,
            encoded_len,
        })
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn operations(&self) -> &[Mutation] {
        &self.operations
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let count = u32::try_from(self.operations.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u32".into()))?;
        let mut output = Vec::with_capacity(self.encoded_len);
        output.extend_from_slice(BATCH_V2_MAGIC);
        output.extend_from_slice(&BATCH_FORMAT_VERSION.to_be_bytes());
        output.extend_from_slice(&0u16.to_be_bytes());
        output.extend_from_slice(&count.to_be_bytes());
        for operation in &self.operations {
            let (key, value, tagged_value_len): (&[u8], &[u8], u32) = match operation {
                Mutation::Put { key, value } => (
                    key,
                    value,
                    u32::try_from(value.len())
                        .map_err(|_| Error::InvalidBatch("value length exceeds u32".into()))?,
                ),
                Mutation::Delete { key } => (key, &[], V2_DELETE_MASK),
            };
            output.extend_from_slice(
                &u32::try_from(key.len())
                    .map_err(|_| Error::InvalidBatch("key length exceeds u32".into()))?
                    .to_be_bytes(),
            );
            output.extend_from_slice(&tagged_value_len.to_be_bytes());
            output.extend_from_slice(key);
            output.extend_from_slice(value);
        }
        debug_assert_eq!(output.len(), self.encoded_len);
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < BATCH_HEADER_BYTES {
            return invalid("incomplete batch header");
        }
        if bytes.len() > WAL_MAX_PAYLOAD_BYTES {
            return invalid(format!(
                "encoded batch exceeds {WAL_MAX_PAYLOAD_BYTES} bytes"
            ));
        }
        let version = u16::from_be_bytes(bytes[8..10].try_into().expect("fixed version field"));
        if version != 1 && version != BATCH_FORMAT_VERSION {
            return Err(Error::UnsupportedVersion {
                object: "write batch",
                version,
            });
        }
        let operation_header_bytes = match (&bytes[0..8], version) {
            (magic, 1) if magic == BATCH_V1_MAGIC => V1_OP_HEADER_BYTES,
            (magic, BATCH_FORMAT_VERSION) if magic == BATCH_V2_MAGIC => V2_OP_HEADER_BYTES,
            _ => return invalid("batch magic/version pair does not match"),
        };
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
                .checked_add(operation_header_bytes)
                .ok_or_else(|| Error::InvalidBatch("operation header length overflow".into()))?;
            let header = bytes
                .get(cursor..header_end)
                .ok_or_else(|| Error::InvalidBatch("incomplete operation header".into()))?;
            let (kind, key_len, value_len) = if version == 1 {
                if header[1..4] != [0, 0, 0] {
                    return invalid("unknown operation flags");
                }
                (
                    header[0],
                    u32::from_be_bytes(header[4..8].try_into().expect("fixed key length")) as usize,
                    u32::from_be_bytes(header[8..12].try_into().expect("fixed value length"))
                        as usize,
                )
            } else {
                let tagged_value_len =
                    u32::from_be_bytes(header[4..8].try_into().expect("fixed tagged value length"));
                let delete = tagged_value_len & V2_DELETE_MASK != 0;
                let value_len = (tagged_value_len & V2_VALUE_LEN_MASK) as usize;
                if delete && value_len != 0 {
                    return invalid("delete operation carries a value length");
                }
                (
                    if delete { 2 } else { 1 },
                    u32::from_be_bytes(header[0..4].try_into().expect("fixed key length")) as usize,
                    value_len,
                )
            };
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
}

impl<'de> Deserialize<'de> for WriteBatch {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedWriteBatch {
            operations: Vec<Mutation>,
        }

        let serialized = SerializedWriteBatch::deserialize(deserializer)?;
        Self::new(serialized.operations).map_err(serde::de::Error::custom)
    }
}

fn validate_operations(operations: &[Mutation]) -> Result<usize> {
    if operations.is_empty() || operations.len() > MAX_OPERATIONS {
        return Err(Error::InvalidBatch(format!(
            "operation count must be in 1..={MAX_OPERATIONS}"
        )));
    }
    let mut encoded_len = BATCH_HEADER_BYTES;
    for operation in operations {
        operation.validate()?;
        let value_len = match operation {
            Mutation::Put { value, .. } => value.len(),
            Mutation::Delete { .. } => 0,
        };
        encoded_len = encoded_len
            .checked_add(V2_OP_HEADER_BYTES)
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

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidBatch(reason.into()))
}
