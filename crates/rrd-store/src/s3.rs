//! Authenticated, resumable S3-compatible immutable object adapter.
//!
//! The transport owns HTTP, TLS, credentials, and SigV4/mTLS mechanics. This
//! layer admits only a transport that declares the exact S3 capabilities RRD
//! requires, then owns deterministic keys, bounded retry, multipart resume,
//! checksums, conditional completion, range-streamed verification, and RRD
//! object receipts.

use crate::{
    Error, ImmutableObjectStore, ObjectInventory, ObjectInventoryEntry, ObjectInventoryState,
    Result, VerifiedObject,
};
use rrd_core::{digest, ObjectReceipt, ObjectReference};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

pub const S3_MIN_PART_BYTES: usize = 5 * 1024 * 1024;
pub const S3_MAX_PART_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const S3_MAX_PARTS: u32 = 10_000;
pub const DEFAULT_S3_RANGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Authentication {
    Anonymous,
    AwsSignatureV4,
    MutualTls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S3TransportCapabilities {
    pub authentication: S3Authentication,
    pub signed_payloads: bool,
    pub conditional_writes: bool,
    pub resumable_multipart: bool,
    pub ranged_reads: bool,
    pub sha256_checksums: bool,
    pub paginated_listing: bool,
}

impl S3TransportCapabilities {
    pub fn authenticated_v4() -> Self {
        Self {
            authentication: S3Authentication::AwsSignatureV4,
            signed_payloads: true,
            conditional_writes: true,
            resumable_multipart: true,
            ranged_reads: true,
            sha256_checksums: true,
            paginated_listing: true,
        }
    }

    fn validate(self) -> Result<()> {
        if self.authentication == S3Authentication::Anonymous {
            return Err(Error::Object(
                "S3 transport must authenticate every object operation".into(),
            ));
        }
        if !self.signed_payloads
            || !self.conditional_writes
            || !self.resumable_multipart
            || !self.ranged_reads
            || !self.sha256_checksums
            || !self.paginated_listing
        {
            return Err(Error::Object(
                "S3 transport lacks signed payload, conditional write, resumable multipart, ranged read, SHA-256 checksum, or paginated listing capability".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S3TransferPolicy {
    pub part_bytes: usize,
    pub range_bytes: usize,
    pub max_attempts: u8,
}

impl Default for S3TransferPolicy {
    fn default() -> Self {
        Self {
            part_bytes: S3_MIN_PART_BYTES,
            range_bytes: DEFAULT_S3_RANGE_BYTES,
            max_attempts: 3,
        }
    }
}

impl S3TransferPolicy {
    fn validate(self) -> Result<Self> {
        let part_bytes = u64::try_from(self.part_bytes)
            .map_err(|_| Error::Object("S3 part size exceeds u64".into()))?;
        if !(S3_MIN_PART_BYTES as u64..=S3_MAX_PART_BYTES).contains(&part_bytes) {
            return Err(Error::Object(format!(
                "S3 part size must be in {S3_MIN_PART_BYTES}..={S3_MAX_PART_BYTES} bytes"
            )));
        }
        if self.range_bytes == 0 || self.range_bytes > self.part_bytes {
            return Err(Error::Object(
                "S3 range size must be positive and no larger than one part".into(),
            ));
        }
        if self.max_attempts == 0 || self.max_attempts > 10 {
            return Err(Error::Object("S3 remote attempts must be in 1..=10".into()));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObjectMetadata {
    pub key: String,
    pub length: u64,
    pub etag: Option<String>,
    pub version: Option<String>,
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalPut {
    Created(S3ObjectMetadata),
    AlreadyExists(S3ObjectMetadata),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3MultipartUpload {
    pub key: String,
    pub upload_id: String,
    pub expected_sha256: String,
    pub expected_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3UploadedPart {
    pub part_number: u32,
    pub length: u64,
    pub sha256: String,
    pub etag: String,
}

/// Synchronous S3 transport port. Implementations map these methods to signed,
/// bounded-time S3 requests. `put_if_absent` and multipart completion must use
/// real conditional requests, never HEAD-then-PUT emulation. `find_multipart`
/// and `list_parts` must consume all service pages before returning.
pub trait S3ObjectClient: Send + Sync {
    fn capabilities(&self) -> S3TransportCapabilities;
    fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<ConditionalPut>;
    fn head(&self, key: &str) -> Result<Option<S3ObjectMetadata>>;
    fn get(&self, key: &str, version: Option<&str>) -> Result<Option<Vec<u8>>>;
    fn get_range(
        &self,
        key: &str,
        version: Option<&str>,
        offset: u64,
        length: usize,
    ) -> Result<Option<Vec<u8>>>;
    fn list(&self, prefix: &str) -> Result<Vec<S3ObjectMetadata>>;
    fn delete(&self, key: &str) -> Result<()>;
    fn find_multipart(
        &self,
        key: &str,
        expected_sha256: &str,
        expected_length: u64,
    ) -> Result<Option<S3MultipartUpload>>;
    fn create_multipart(
        &self,
        key: &str,
        expected_sha256: &str,
        expected_length: u64,
    ) -> Result<S3MultipartUpload>;
    fn list_parts(&self, upload: &S3MultipartUpload) -> Result<Vec<S3UploadedPart>>;
    fn upload_part(
        &self,
        upload: &S3MultipartUpload,
        part_number: u32,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<S3UploadedPart>;
    fn complete_multipart_if_absent(
        &self,
        upload: &S3MultipartUpload,
        parts: &[S3UploadedPart],
    ) -> Result<ConditionalPut>;
    fn abort_multipart(&self, upload: &S3MultipartUpload) -> Result<()>;
}

pub struct S3CompatibleObjectStore<C> {
    client: C,
    backend_name: String,
    policy: S3TransferPolicy,
}

impl<C: S3ObjectClient> S3CompatibleObjectStore<C> {
    pub fn new(client: C, backend_name: impl Into<String>) -> Result<Self> {
        Self::with_policy(client, backend_name, S3TransferPolicy::default())
    }

    pub fn with_policy(
        client: C,
        backend_name: impl Into<String>,
        policy: S3TransferPolicy,
    ) -> Result<Self> {
        client.capabilities().validate()?;
        let backend_name = backend_name.into();
        if backend_name.trim().is_empty() || backend_name.as_bytes().contains(&0) {
            return Err(Error::Object(
                "S3-compatible backend name must be non-empty and contain no NUL bytes".into(),
            ));
        }
        Ok(Self {
            client,
            backend_name,
            policy: policy.validate()?,
        })
    }

    pub fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        let sha256 = digest::sha256_hex(bytes);
        let key = ObjectReference::canonical_key(&sha256).map_err(Error::from)?;
        let metadata = match self.retry(|| self.client.put_if_absent(&key, bytes))? {
            ConditionalPut::Created(metadata) | ConditionalPut::AlreadyExists(metadata) => metadata,
        };
        validate_metadata(&key, bytes.len() as u64, &sha256, &metadata)?;
        self.verify_expected(&sha256, Some(bytes.len() as u64))
    }

    pub fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        reference.validate().map_err(Error::from)?;
        self.verify_expected(&reference.sha256, Some(reference.length))?;
        Ok(Box::new(S3RangeReader {
            store: self,
            key: reference.receipt.key.clone(),
            version: reference.receipt.version.clone(),
            length: reference.length,
            offset: 0,
            buffered: Cursor::new(Vec::new()),
        }))
    }

    /// Uploads one known content address with bounded memory. A retry discovers
    /// the prior multipart upload and reuses only parts whose exact bytes match
    /// the restarted source stream.
    pub fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        let key = ObjectReference::canonical_key(expected_sha256).map_err(Error::from)?;
        if expected_length == 0 {
            ensure_empty(reader)?;
            let actual = digest::sha256_hex(b"");
            if expected_sha256 != actual {
                return Err(Error::ObjectCorrupt {
                    expected: expected_sha256.to_owned(),
                    actual,
                });
            }
            return self.put(b"");
        }
        let part_count = expected_length.div_ceil(self.policy.part_bytes as u64);
        if part_count > u64::from(S3_MAX_PARTS) {
            return Err(Error::Object(format!(
                "object requires {part_count} parts beyond the S3 limit {S3_MAX_PARTS}"
            )));
        }

        let upload = match self.retry(|| {
            self.client
                .find_multipart(&key, expected_sha256, expected_length)
        })? {
            Some(upload) => upload,
            None => self.retry(|| {
                self.client
                    .create_multipart(&key, expected_sha256, expected_length)
            })?,
        };
        validate_upload(&key, expected_sha256, expected_length, &upload)?;
        let existing = indexed_parts(self.retry(|| self.client.list_parts(&upload))?)?;
        if existing.keys().next_back().copied().unwrap_or(0) > part_count as u32 {
            return Err(Error::Object(
                "remote multipart upload contains a part beyond the expected object".into(),
            ));
        }

        let mut full_digest = digest::Sha256::new();
        let mut completed = Vec::with_capacity(part_count as usize);
        let mut remaining = expected_length;
        for part_number in 1..=part_count as u32 {
            let expected_part_length = remaining.min(self.policy.part_bytes as u64) as usize;
            let bytes = read_exact_part(reader, expected_part_length)?;
            remaining -= bytes.len() as u64;
            full_digest.update(&bytes);
            let sha256 = digest::sha256_hex(&bytes);
            let part = match existing.get(&part_number) {
                Some(part) if part.length == bytes.len() as u64 && part.sha256 == sha256 => {
                    part.clone()
                }
                _ => self.retry(|| {
                    self.client
                        .upload_part(&upload, part_number, &bytes, &sha256)
                })?,
            };
            validate_part(part_number, bytes.len() as u64, &sha256, &part)?;
            completed.push(part);
        }
        ensure_empty(reader)?;
        let actual = full_digest.finalize_hex();
        if actual != expected_sha256 {
            return Err(Error::ObjectCorrupt {
                expected: expected_sha256.to_owned(),
                actual,
            });
        }
        validate_completed_parts(&completed, expected_length)?;
        let metadata = match self.retry(|| {
            self.client
                .complete_multipart_if_absent(&upload, &completed)
        })? {
            ConditionalPut::Created(metadata) | ConditionalPut::AlreadyExists(metadata) => metadata,
        };
        validate_metadata(&key, expected_length, expected_sha256, &metadata)?;
        self.verify_expected(expected_sha256, Some(expected_length))
    }

    pub fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        self.verify_expected(sha256, None)
    }

    pub fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        reference.validate().map_err(Error::from)?;
        self.verify_expected(&reference.sha256, Some(reference.length))?;
        let bytes = self
            .retry(|| {
                self.client
                    .get(&reference.receipt.key, reference.receipt.version.as_deref())
            })?
            .ok_or_else(|| Error::ObjectMissing(reference.sha256.clone()))?;
        if bytes.len() as u64 != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: bytes.len() as u64,
            });
        }
        let actual = digest::sha256_hex(&bytes);
        if actual != reference.sha256 {
            return Err(Error::ObjectCorrupt {
                expected: reference.sha256.clone(),
                actual,
            });
        }
        Ok(bytes)
    }

    pub fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        let mut entries = self
            .retry(|| self.client.list("objects/sha256/"))?
            .into_iter()
            .map(|metadata| {
                let sha256 = metadata
                    .key
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let (actual, length) = self.digest_remote(&metadata)?;
                let state = if actual != sha256 {
                    ObjectInventoryState::Corrupt {
                        actual_sha256: actual,
                    }
                } else if reachable.contains(&sha256) {
                    ObjectInventoryState::Reachable
                } else {
                    ObjectInventoryState::Orphan
                };
                Ok(ObjectInventoryEntry {
                    sha256,
                    length,
                    state,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by(|left, right| left.sha256.cmp(&right.sha256));
        Ok(ObjectInventory {
            entries,
            staging_files: Vec::new(),
            quarantined_files: Vec::new(),
        })
    }

    pub fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        let mut removed = Vec::new();
        for sha256 in unreachable {
            let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
            if self.retry(|| self.client.head(&key))?.is_some() {
                self.retry(|| self.client.delete(&key))?;
                removed.push(sha256.clone());
            }
        }
        removed.sort();
        Ok(removed)
    }

    pub fn into_client(self) -> C {
        self.client
    }

    fn verify_expected(&self, sha256: &str, length: Option<u64>) -> Result<VerifiedObject> {
        let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
        let metadata = self
            .retry(|| self.client.head(&key))?
            .ok_or_else(|| Error::ObjectMissing(sha256.to_owned()))?;
        let expected_length = length.unwrap_or(metadata.length);
        validate_metadata(&key, expected_length, sha256, &metadata)?;
        let (actual, actual_length) = self.digest_remote(&metadata)?;
        if actual_length != expected_length {
            return Err(Error::ObjectLengthMismatch {
                expected: expected_length,
                actual: actual_length,
            });
        }
        if actual != sha256 {
            return Err(Error::ObjectCorrupt {
                expected: sha256.to_owned(),
                actual,
            });
        }
        Ok(VerifiedObject {
            sha256: sha256.to_owned(),
            length: actual_length,
            receipt: ObjectReceipt {
                backend: self.backend_name.clone(),
                key,
                version: metadata.version,
                etag: metadata.etag,
            },
        })
    }

    fn digest_remote(&self, metadata: &S3ObjectMetadata) -> Result<(String, u64)> {
        let mut hasher = digest::Sha256::new();
        let mut offset = 0u64;
        while offset < metadata.length {
            let take =
                usize::try_from((metadata.length - offset).min(self.policy.range_bytes as u64))
                    .expect("range is bounded by usize policy");
            let bytes = self
                .retry(|| {
                    self.client
                        .get_range(&metadata.key, metadata.version.as_deref(), offset, take)
                })?
                .ok_or_else(|| Error::ObjectMissing(metadata.key.clone()))?;
            if bytes.len() != take {
                return Err(Error::ObjectLengthMismatch {
                    expected: take as u64,
                    actual: bytes.len() as u64,
                });
            }
            hasher.update(&bytes);
            offset += bytes.len() as u64;
        }
        Ok((hasher.finalize_hex(), offset))
    }

    fn retry<T>(&self, mut operation: impl FnMut() -> Result<T>) -> Result<T> {
        for attempt in 1..=self.policy.max_attempts {
            match operation() {
                Err(Error::RemoteObjectTransient(_)) if attempt < self.policy.max_attempts => {}
                result => return result,
            }
        }
        unreachable!("positive bounded attempt count returns from its final attempt")
    }
}

impl<C: S3ObjectClient> ImmutableObjectStore for S3CompatibleObjectStore<C> {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        S3CompatibleObjectStore::put(self, bytes)
    }

    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send + '_>> {
        S3CompatibleObjectStore::open_verified(self, reference)
    }

    fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        S3CompatibleObjectStore::put_verified_stream(self, expected_sha256, expected_length, reader)
    }

    fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        S3CompatibleObjectStore::verify(self, sha256)
    }

    fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        S3CompatibleObjectStore::get(self, reference)
    }

    fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        S3CompatibleObjectStore::inventory(self, reachable)
    }

    fn reclaim_orphans(&self, unreachable: &BTreeSet<String>) -> Result<Vec<String>> {
        S3CompatibleObjectStore::reclaim_orphans(self, unreachable)
    }
}

struct S3RangeReader<'a, C> {
    store: &'a S3CompatibleObjectStore<C>,
    key: String,
    version: Option<String>,
    length: u64,
    offset: u64,
    buffered: Cursor<Vec<u8>>,
}

impl<C: S3ObjectClient> Read for S3RangeReader<'_, C> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let buffered = self.buffered.read(output)?;
        if buffered != 0 {
            return Ok(buffered);
        }
        if self.offset >= self.length {
            return Ok(0);
        }
        let take =
            usize::try_from((self.length - self.offset).min(self.store.policy.range_bytes as u64))
                .expect("range is bounded by usize policy");
        let bytes = self
            .store
            .retry(|| {
                self.store
                    .client
                    .get_range(&self.key, self.version.as_deref(), self.offset, take)
            })
            .map_err(std::io::Error::other)?
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "object missing"))?;
        if bytes.len() != take {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("range returned {} of {take} bytes", bytes.len()),
            ));
        }
        self.offset += bytes.len() as u64;
        self.buffered = Cursor::new(bytes);
        self.buffered.read(output)
    }
}

fn validate_metadata(
    key: &str,
    expected_length: u64,
    expected_sha256: &str,
    metadata: &S3ObjectMetadata,
) -> Result<()> {
    if metadata.key != key {
        return Err(Error::Object(format!(
            "S3 response key {:?} differs from requested key {key:?}",
            metadata.key
        )));
    }
    if metadata.length != expected_length {
        return Err(Error::ObjectLengthMismatch {
            expected: expected_length,
            actual: metadata.length,
        });
    }
    if metadata.checksum_sha256.as_deref() != Some(expected_sha256) {
        return Err(Error::Object(format!(
            "S3 response for {key} lacks the exact expected SHA-256 checksum"
        )));
    }
    Ok(())
}

fn validate_upload(
    key: &str,
    expected_sha256: &str,
    expected_length: u64,
    upload: &S3MultipartUpload,
) -> Result<()> {
    if upload.key != key
        || upload.expected_sha256 != expected_sha256
        || upload.expected_length != expected_length
        || upload.upload_id.trim().is_empty()
    {
        return Err(Error::Object(
            "S3 multipart identity differs from the requested content address".into(),
        ));
    }
    Ok(())
}

fn indexed_parts(parts: Vec<S3UploadedPart>) -> Result<BTreeMap<u32, S3UploadedPart>> {
    let mut indexed = BTreeMap::new();
    for part in parts {
        if part.part_number == 0 || part.part_number > S3_MAX_PARTS {
            return Err(Error::Object("S3 multipart part number is invalid".into()));
        }
        let number = part.part_number;
        if indexed.insert(number, part).is_some() {
            return Err(Error::Object(
                "S3 multipart listing returned a duplicate part".into(),
            ));
        }
    }
    Ok(indexed)
}

fn validate_part(
    part_number: u32,
    expected_length: u64,
    expected_sha256: &str,
    part: &S3UploadedPart,
) -> Result<()> {
    if part.part_number != part_number
        || part.length != expected_length
        || part.sha256 != expected_sha256
        || part.etag.trim().is_empty()
    {
        return Err(Error::Object(format!(
            "S3 multipart part {part_number} returned divergent checksum, length, number, or ETag"
        )));
    }
    Ok(())
}

fn validate_completed_parts(parts: &[S3UploadedPart], expected_length: u64) -> Result<()> {
    let mut length = 0u64;
    for (index, part) in parts.iter().enumerate() {
        if part.part_number != u32::try_from(index + 1).expect("S3 part count fits u32") {
            return Err(Error::Object(
                "S3 multipart completion requires consecutive parts beginning at one".into(),
            ));
        }
        length = length
            .checked_add(part.length)
            .ok_or_else(|| Error::Object("S3 multipart length overflowed".into()))?;
    }
    if length != expected_length {
        return Err(Error::ObjectLengthMismatch {
            expected: expected_length,
            actual: length,
        });
    }
    Ok(())
}

fn read_exact_part(reader: &mut dyn Read, length: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0; length];
    let mut offset = 0;
    while offset < length {
        let read = reader.read(&mut bytes[offset..])?;
        if read == 0 {
            return Err(Error::ObjectLengthMismatch {
                expected: length as u64,
                actual: offset as u64,
            });
        }
        offset += read;
    }
    Ok(bytes)
}

fn ensure_empty(reader: &mut dyn Read) -> Result<()> {
    let mut extra = [0u8; 1];
    if reader.read(&mut extra)? != 0 {
        return Err(Error::Object(
            "object stream contains bytes beyond its declared length".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export_logical_archive, LocalObjectStore, MemoryEngine};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use tempfile::tempdir;

    #[derive(Default)]
    struct PendingUpload {
        upload: Option<S3MultipartUpload>,
        parts: BTreeMap<u32, (S3UploadedPart, Vec<u8>)>,
    }

    struct MemoryS3Client {
        capabilities: S3TransportCapabilities,
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
        pending: Mutex<PendingUpload>,
        transient_part_failures: Mutex<BTreeMap<u32, u32>>,
        transient_range_failures: Mutex<u32>,
        part_uploads: Mutex<BTreeMap<u32, u32>>,
        max_range_bytes: AtomicU64,
        upload_id: AtomicU64,
    }

    impl Default for MemoryS3Client {
        fn default() -> Self {
            Self {
                capabilities: S3TransportCapabilities::authenticated_v4(),
                objects: Mutex::new(BTreeMap::new()),
                pending: Mutex::new(PendingUpload::default()),
                transient_part_failures: Mutex::new(BTreeMap::new()),
                transient_range_failures: Mutex::new(0),
                part_uploads: Mutex::new(BTreeMap::new()),
                max_range_bytes: AtomicU64::new(0),
                upload_id: AtomicU64::new(1),
            }
        }
    }

    impl MemoryS3Client {
        fn metadata(key: &str, bytes: &[u8]) -> S3ObjectMetadata {
            S3ObjectMetadata {
                key: key.to_owned(),
                length: bytes.len() as u64,
                etag: Some(format!("etag-{}", digest::sha256_hex(bytes))),
                version: Some("1".into()),
                checksum_sha256: Some(digest::sha256_hex(bytes)),
            }
        }

        fn fail_part(&self, part_number: u32, failures: u32) {
            self.transient_part_failures
                .lock()
                .unwrap()
                .insert(part_number, failures);
        }

        fn fail_ranges(&self, failures: u32) {
            *self.transient_range_failures.lock().unwrap() = failures;
        }
    }

    impl S3ObjectClient for MemoryS3Client {
        fn capabilities(&self) -> S3TransportCapabilities {
            self.capabilities
        }

        fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<ConditionalPut> {
            let mut objects = self.objects.lock().unwrap();
            let created = !objects.contains_key(key);
            let stored = objects.entry(key.into()).or_insert_with(|| bytes.to_vec());
            let metadata = Self::metadata(key, stored);
            Ok(if created {
                ConditionalPut::Created(metadata)
            } else {
                ConditionalPut::AlreadyExists(metadata)
            })
        }

        fn head(&self, key: &str) -> Result<Option<S3ObjectMetadata>> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .get(key)
                .map(|bytes| Self::metadata(key, bytes)))
        }

        fn get(&self, key: &str, version: Option<&str>) -> Result<Option<Vec<u8>>> {
            if version.is_some_and(|version| version != "1") {
                return Ok(None);
            }
            Ok(self.objects.lock().unwrap().get(key).cloned())
        }

        fn get_range(
            &self,
            key: &str,
            version: Option<&str>,
            offset: u64,
            length: usize,
        ) -> Result<Option<Vec<u8>>> {
            let mut failures = self.transient_range_failures.lock().unwrap();
            if *failures != 0 {
                *failures -= 1;
                return Err(Error::RemoteObjectTransient(
                    "ranged read unavailable".into(),
                ));
            }
            drop(failures);
            self.max_range_bytes
                .fetch_max(length as u64, Ordering::Relaxed);
            if version.is_some_and(|version| version != "1") {
                return Ok(None);
            }
            let objects = self.objects.lock().unwrap();
            let Some(bytes) = objects.get(key) else {
                return Ok(None);
            };
            let start =
                usize::try_from(offset).map_err(|_| Error::Object("offset overflow".into()))?;
            let end = start.saturating_add(length).min(bytes.len());
            Ok(Some(bytes.get(start..end).unwrap_or_default().to_vec()))
        }

        fn list(&self, prefix: &str) -> Result<Vec<S3ObjectMetadata>> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(key, bytes)| Self::metadata(key, bytes))
                .collect())
        }

        fn delete(&self, key: &str) -> Result<()> {
            self.objects.lock().unwrap().remove(key);
            Ok(())
        }

        fn find_multipart(
            &self,
            key: &str,
            expected_sha256: &str,
            expected_length: u64,
        ) -> Result<Option<S3MultipartUpload>> {
            Ok(self
                .pending
                .lock()
                .unwrap()
                .upload
                .clone()
                .filter(|upload| {
                    upload.key == key
                        && upload.expected_sha256 == expected_sha256
                        && upload.expected_length == expected_length
                }))
        }

        fn create_multipart(
            &self,
            key: &str,
            expected_sha256: &str,
            expected_length: u64,
        ) -> Result<S3MultipartUpload> {
            let upload = S3MultipartUpload {
                key: key.into(),
                upload_id: format!("upload-{}", self.upload_id.fetch_add(1, Ordering::Relaxed)),
                expected_sha256: expected_sha256.into(),
                expected_length,
            };
            let mut pending = self.pending.lock().unwrap();
            pending.upload = Some(upload.clone());
            pending.parts.clear();
            Ok(upload)
        }

        fn list_parts(&self, upload: &S3MultipartUpload) -> Result<Vec<S3UploadedPart>> {
            let pending = self.pending.lock().unwrap();
            if pending.upload.as_ref() != Some(upload) {
                return Err(Error::Object("unknown multipart upload".into()));
            }
            Ok(pending
                .parts
                .values()
                .map(|(part, _)| part.clone())
                .collect())
        }

        fn upload_part(
            &self,
            upload: &S3MultipartUpload,
            part_number: u32,
            bytes: &[u8],
            sha256: &str,
        ) -> Result<S3UploadedPart> {
            let mut failures = self.transient_part_failures.lock().unwrap();
            if failures.get(&part_number).copied().unwrap_or(0) > 0 {
                *failures.get_mut(&part_number).unwrap() -= 1;
                return Err(Error::RemoteObjectTransient(format!(
                    "part {part_number} unavailable"
                )));
            }
            drop(failures);
            *self
                .part_uploads
                .lock()
                .unwrap()
                .entry(part_number)
                .or_default() += 1;
            let mut pending = self.pending.lock().unwrap();
            if pending.upload.as_ref() != Some(upload) {
                return Err(Error::Object("unknown multipart upload".into()));
            }
            if digest::sha256_hex(bytes) != sha256 {
                return Err(Error::Object("part checksum is invalid".into()));
            }
            let part = S3UploadedPart {
                part_number,
                length: bytes.len() as u64,
                sha256: sha256.into(),
                etag: format!("part-{part_number}-{sha256}"),
            };
            pending
                .parts
                .insert(part_number, (part.clone(), bytes.to_vec()));
            Ok(part)
        }

        fn complete_multipart_if_absent(
            &self,
            upload: &S3MultipartUpload,
            parts: &[S3UploadedPart],
        ) -> Result<ConditionalPut> {
            let mut pending = self.pending.lock().unwrap();
            if pending.upload.as_ref() != Some(upload) {
                return Err(Error::Object("unknown multipart upload".into()));
            }
            let mut bytes = Vec::new();
            for part in parts {
                let (stored, value) = pending
                    .parts
                    .get(&part.part_number)
                    .ok_or_else(|| Error::Object("completion part is absent".into()))?;
                if stored != part {
                    return Err(Error::Object("completion part diverged".into()));
                }
                bytes.extend_from_slice(value);
            }
            if bytes.len() as u64 != upload.expected_length
                || digest::sha256_hex(&bytes) != upload.expected_sha256
            {
                return Err(Error::Object("full multipart checksum diverged".into()));
            }
            let mut objects = self.objects.lock().unwrap();
            let created = !objects.contains_key(&upload.key);
            let stored = objects.entry(upload.key.clone()).or_insert(bytes);
            let metadata = Self::metadata(&upload.key, stored);
            pending.upload = None;
            pending.parts.clear();
            Ok(if created {
                ConditionalPut::Created(metadata)
            } else {
                ConditionalPut::AlreadyExists(metadata)
            })
        }

        fn abort_multipart(&self, upload: &S3MultipartUpload) -> Result<()> {
            let mut pending = self.pending.lock().unwrap();
            if pending.upload.as_ref() == Some(upload) {
                pending.upload = None;
                pending.parts.clear();
            }
            Ok(())
        }
    }

    fn object_reference(verified: &VerifiedObject) -> ObjectReference {
        ObjectReference::for_verified(
            "archive",
            None,
            "application/vnd.rrflow.archive",
            verified.sha256.clone(),
            verified.length,
            verified.receipt.clone(),
        )
        .unwrap()
    }

    #[test]
    fn unauthenticated_or_incomplete_transport_is_refused_before_io() {
        let anonymous = MemoryS3Client {
            capabilities: S3TransportCapabilities {
                authentication: S3Authentication::Anonymous,
                ..S3TransportCapabilities::authenticated_v4()
            },
            ..MemoryS3Client::default()
        };
        assert!(S3CompatibleObjectStore::new(anonymous, "s3:test").is_err());

        let unsigned = MemoryS3Client {
            capabilities: S3TransportCapabilities {
                signed_payloads: false,
                ..S3TransportCapabilities::authenticated_v4()
            },
            ..MemoryS3Client::default()
        };
        assert!(S3CompatibleObjectStore::new(unsigned, "s3:test").is_err());
    }

    #[test]
    fn local_and_s3_compatible_adapters_have_identical_content_semantics() {
        let directory = tempdir().unwrap();
        let local = LocalObjectStore::open(directory.path()).unwrap();
        let s3 = S3CompatibleObjectStore::new(MemoryS3Client::default(), "s3:test").unwrap();
        for bytes in [
            b"".as_slice(),
            b"one".as_slice(),
            b"two-two".as_slice(),
            b"one".as_slice(),
        ] {
            let local_value = local.put(bytes).unwrap();
            let s3_value = s3.put(bytes).unwrap();
            assert_eq!(local_value.sha256, s3_value.sha256);
            assert_eq!(local_value.length, s3_value.length);
        }
        assert_eq!(
            local.inventory(&BTreeSet::new()).unwrap().entries,
            s3.inventory(&BTreeSet::new()).unwrap().entries
        );
    }

    #[test]
    fn logical_archive_uses_the_same_authenticated_resumable_object_path() {
        let temporary = tempdir().unwrap();
        let archive = temporary.path().join("empty.rrd-archive");
        export_logical_archive(&MemoryEngine::new(), &archive).unwrap();
        let bytes = std::fs::read(&archive).unwrap();
        let sha256 = digest::sha256_hex(&bytes);
        let remote = S3CompatibleObjectStore::new(MemoryS3Client::default(), "s3:archive").unwrap();
        let mut source = std::fs::File::open(archive).unwrap();
        let verified = remote
            .put_verified_stream(&sha256, bytes.len() as u64, &mut source)
            .unwrap();
        let reference = object_reference(&verified);
        remote.client.fail_ranges(2);
        let mut restored = Vec::new();
        remote
            .open_verified(&reference)
            .unwrap()
            .read_to_end(&mut restored)
            .unwrap();
        assert_eq!(restored, bytes);
    }

    #[test]
    fn interrupted_upload_resumes_parts_and_transient_errors_are_bounded() {
        let bytes = (0..(S3_MIN_PART_BYTES * 2 + 31))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let sha256 = digest::sha256_hex(&bytes);
        let client = MemoryS3Client::default();
        client.fail_part(2, 1);
        let store = S3CompatibleObjectStore::with_policy(
            client,
            "s3:test",
            S3TransferPolicy {
                max_attempts: 1,
                ..S3TransferPolicy::default()
            },
        )
        .unwrap();
        assert!(matches!(
            store.put_verified_stream(&sha256, bytes.len() as u64, &mut Cursor::new(&bytes)),
            Err(Error::RemoteObjectTransient(_))
        ));
        let verified = store
            .put_verified_stream(&sha256, bytes.len() as u64, &mut Cursor::new(&bytes))
            .unwrap();
        assert_eq!(verified.sha256, sha256);
        let uploads = store.client.part_uploads.lock().unwrap();
        assert_eq!(uploads.get(&1), Some(&1));
        assert_eq!(uploads.get(&2), Some(&1));
        assert_eq!(uploads.get(&3), Some(&1));
    }

    #[test]
    fn ranged_remote_recovery_restores_local_loss_and_corruption_fails_closed() {
        let bytes = vec![0x5a; S3_MIN_PART_BYTES + 19];
        let sha256 = digest::sha256_hex(&bytes);
        let client = MemoryS3Client::default();
        client.fail_part(1, 2);
        let remote = S3CompatibleObjectStore::new(client, "s3:test").unwrap();
        let verified = remote
            .put_verified_stream(&sha256, bytes.len() as u64, &mut Cursor::new(&bytes))
            .unwrap();
        let reference = object_reference(&verified);

        let temporary = tempdir().unwrap();
        let local = LocalObjectStore::open(temporary.path()).unwrap();
        local.put(&bytes).unwrap();
        std::fs::remove_file(local.verified_path(&reference).unwrap()).unwrap();
        assert!(matches!(
            local.verify(&sha256),
            Err(Error::ObjectMissing(_))
        ));
        let mut reader = remote.open_verified(&reference).unwrap();
        local
            .put_verified_stream(&sha256, bytes.len() as u64, reader.as_mut())
            .unwrap();
        assert_eq!(local.get(&reference).unwrap(), bytes);
        assert!(
            remote.client.max_range_bytes.load(Ordering::Relaxed) <= DEFAULT_S3_RANGE_BYTES as u64
        );

        let mut objects = remote.client.objects.lock().unwrap();
        objects.get_mut(&reference.receipt.key).unwrap()[17] ^= 0x40;
        drop(objects);
        assert!(matches!(
            remote.verify(&sha256),
            Err(Error::Object(_)) | Err(Error::ObjectCorrupt { .. })
        ));
    }
}
