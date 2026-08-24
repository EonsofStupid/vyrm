//! Capability-explicit S3-compatible immutable object adapter.
//!
//! Authentication, HTTP, retries, and endpoint policy stay in a transport
//! implementation. This layer owns Vyrm semantics: deterministic keys,
//! conditional creation, post-write verification, and no read-check-write
//! fallback when the backend cannot provide `If-None-Match` behavior.

use crate::{
    Error, ImmutableObjectStore, ObjectInventory, ObjectInventoryEntry, ObjectInventoryState,
    Result, VerifiedObject,
};
use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use vyrm_core::{digest, ObjectReceipt, ObjectReference};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObjectMetadata {
    pub key: String,
    pub length: u64,
    pub etag: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalPut {
    Created(S3ObjectMetadata),
    AlreadyExists(S3ObjectMetadata),
}

/// Small synchronous transport port. Implementations must map `put_if_absent`
/// to an actual conditional request and must never emulate it with HEAD+PUT.
pub trait S3ObjectClient: Send + Sync {
    fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<ConditionalPut>;
    fn head(&self, key: &str) -> Result<Option<S3ObjectMetadata>>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn list(&self, prefix: &str) -> Result<Vec<S3ObjectMetadata>>;
    fn delete(&self, key: &str) -> Result<()>;
}

pub struct S3CompatibleObjectStore<C> {
    client: C,
    backend_name: String,
}

impl<C: S3ObjectClient> S3CompatibleObjectStore<C> {
    pub fn new(client: C, backend_name: impl Into<String>) -> Result<Self> {
        let backend_name = backend_name.into();
        if backend_name.trim().is_empty() || backend_name.as_bytes().contains(&0) {
            return Err(Error::Object(
                "S3-compatible backend name must be non-empty and contain no NUL bytes".into(),
            ));
        }
        Ok(Self {
            client,
            backend_name,
        })
    }

    pub fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        let sha256 = digest::sha256_hex(bytes);
        let key = ObjectReference::canonical_key(&sha256).map_err(Error::from)?;
        let metadata = match self.client.put_if_absent(&key, bytes)? {
            ConditionalPut::Created(metadata) | ConditionalPut::AlreadyExists(metadata) => metadata,
        };
        validate_metadata(&key, bytes.len() as u64, &metadata)?;
        let verified = self.verify(&sha256)?;
        if verified.length != bytes.len() as u64 {
            return Err(Error::ObjectLengthMismatch {
                expected: bytes.len() as u64,
                actual: verified.length,
            });
        }
        Ok(verified)
    }

    pub fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(Cursor::new(self.get(reference)?)))
    }

    /// The current synchronous S3 transport exposes whole-object PUT, so this
    /// compatibility adapter validates the stream before materializing that
    /// required request body. Multipart streaming belongs behind a future S3
    /// transport capability, not in the portable object contract.
    pub fn put_verified_stream(
        &self,
        expected_sha256: &str,
        expected_length: u64,
        reader: &mut dyn Read,
    ) -> Result<VerifiedObject> {
        ObjectReference::canonical_key(expected_sha256).map_err(Error::from)?;
        let limit = expected_length
            .checked_add(1)
            .ok_or_else(|| Error::Object("object length overflowed u64".into()))?;
        let mut bytes = Vec::new();
        reader.take(limit).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != expected_length {
            return Err(Error::ObjectLengthMismatch {
                expected: expected_length,
                actual: bytes.len() as u64,
            });
        }
        let actual = digest::sha256_hex(&bytes);
        if actual != expected_sha256 {
            return Err(Error::ObjectCorrupt {
                expected: expected_sha256.to_owned(),
                actual,
            });
        }
        self.put(&bytes)
    }

    pub fn verify(&self, sha256: &str) -> Result<VerifiedObject> {
        let key = ObjectReference::canonical_key(sha256).map_err(Error::from)?;
        let metadata = self
            .client
            .head(&key)?
            .ok_or_else(|| Error::ObjectMissing(sha256.to_owned()))?;
        validate_metadata(&key, metadata.length, &metadata)?;
        let bytes = self
            .client
            .get(&key)?
            .ok_or_else(|| Error::ObjectMissing(sha256.to_owned()))?;
        if bytes.len() as u64 != metadata.length {
            return Err(Error::ObjectLengthMismatch {
                expected: metadata.length,
                actual: bytes.len() as u64,
            });
        }
        let actual = digest::sha256_hex(&bytes);
        if actual != sha256 {
            return Err(Error::ObjectCorrupt {
                expected: sha256.to_owned(),
                actual,
            });
        }
        Ok(VerifiedObject {
            sha256: sha256.to_owned(),
            length: metadata.length,
            receipt: ObjectReceipt {
                backend: self.backend_name.clone(),
                key,
                version: metadata.version,
                etag: metadata.etag,
            },
        })
    }

    pub fn get(&self, reference: &ObjectReference) -> Result<Vec<u8>> {
        reference.validate().map_err(Error::from)?;
        let verified = self.verify(&reference.sha256)?;
        if verified.length != reference.length {
            return Err(Error::ObjectLengthMismatch {
                expected: reference.length,
                actual: verified.length,
            });
        }
        self.client
            .get(&reference.receipt.key)?
            .ok_or_else(|| Error::ObjectMissing(reference.sha256.clone()))
    }

    pub fn inventory(&self, reachable: &BTreeSet<String>) -> Result<ObjectInventory> {
        let mut entries = self
            .client
            .list("objects/sha256/")?
            .into_iter()
            .map(|metadata| {
                let sha256 = metadata
                    .key
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let bytes = self
                    .client
                    .get(&metadata.key)?
                    .ok_or_else(|| Error::ObjectMissing(sha256.clone()))?;
                let actual = digest::sha256_hex(&bytes);
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
                    length: bytes.len() as u64,
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
            if self.client.head(&key)?.is_some() {
                self.client.delete(&key)?;
                removed.push(sha256.clone());
            }
        }
        removed.sort();
        Ok(removed)
    }

    pub fn into_client(self) -> C {
        self.client
    }
}

impl<C: S3ObjectClient> ImmutableObjectStore for S3CompatibleObjectStore<C> {
    fn put(&self, bytes: &[u8]) -> Result<VerifiedObject> {
        S3CompatibleObjectStore::put(self, bytes)
    }

    fn open_verified(&self, reference: &ObjectReference) -> Result<Box<dyn Read + Send>> {
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

fn validate_metadata(key: &str, expected_length: u64, metadata: &S3ObjectMetadata) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalObjectStore;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MemoryS3Client {
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl S3ObjectClient for MemoryS3Client {
        fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<ConditionalPut> {
            let mut objects = self.objects.lock().expect("memory S3 mutex");
            let created = !objects.contains_key(key);
            let stored = objects
                .entry(key.to_owned())
                .or_insert_with(|| bytes.to_vec());
            let metadata = S3ObjectMetadata {
                key: key.to_owned(),
                length: stored.len() as u64,
                etag: Some(digest::sha256_hex(stored)),
                version: Some("1".into()),
            };
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
                .expect("memory S3 mutex")
                .get(key)
                .map(|bytes| S3ObjectMetadata {
                    key: key.to_owned(),
                    length: bytes.len() as u64,
                    etag: Some(digest::sha256_hex(bytes)),
                    version: Some("1".into()),
                }))
        }

        fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .objects
                .lock()
                .expect("memory S3 mutex")
                .get(key)
                .cloned())
        }

        fn list(&self, prefix: &str) -> Result<Vec<S3ObjectMetadata>> {
            let mut values = self
                .objects
                .lock()
                .expect("memory S3 mutex")
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(key, bytes)| S3ObjectMetadata {
                    key: key.clone(),
                    length: bytes.len() as u64,
                    etag: Some(digest::sha256_hex(bytes)),
                    version: Some("1".into()),
                })
                .collect::<Vec<_>>();
            values.sort_by(|left, right| left.key.cmp(&right.key));
            Ok(values)
        }

        fn delete(&self, key: &str) -> Result<()> {
            self.objects.lock().expect("memory S3 mutex").remove(key);
            Ok(())
        }
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
        let local_inventory = local.inventory(&BTreeSet::new()).unwrap();
        let s3_inventory = s3.inventory(&BTreeSet::new()).unwrap();
        assert_eq!(local_inventory.entries, s3_inventory.entries);
    }
}
