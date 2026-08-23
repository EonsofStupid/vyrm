use crate::{Error, Mutation, RecoveredBatch, Result, WriteBatch};
use serde::{Deserialize, Serialize};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Included, Unbounded};

#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionChain {
    One(VersionedValue),
    Many(Vec<VersionedValue>),
}

impl VersionChain {
    fn from_vec(mut versions: Vec<VersionedValue>) -> Self {
        if versions.len() == 1 {
            Self::One(versions.pop().expect("one checked version"))
        } else {
            Self::Many(versions)
        }
    }

    fn push(&mut self, version: VersionedValue) {
        match self {
            Self::Many(versions) => versions.push(version),
            Self::One(_) => {
                let Self::One(previous) =
                    std::mem::replace(self, Self::Many(Vec::with_capacity(2)))
                else {
                    unreachable!("one-version branch changed during replacement")
                };
                let Self::Many(versions) = self else {
                    unreachable!("replacement created a multi-version chain")
                };
                versions.push(previous);
                versions.push(version);
            }
        }
    }

    fn as_slice(&self) -> &[VersionedValue] {
        match self {
            Self::One(version) => std::slice::from_ref(version),
            Self::Many(versions) => versions,
        }
    }

    fn iter(&self) -> std::slice::Iter<'_, VersionedValue> {
        self.as_slice().iter()
    }

    fn spilled_capacity(&self) -> Option<usize> {
        match self {
            Self::One(_) => None,
            Self::Many(versions) => Some(versions.capacity()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedValue {
    pub sequence: u64,
    /// Exact-length payload storage keeps the value descriptor two machine
    /// words wide instead of carrying `Vec`'s unused capacity word. The full
    /// record is three words on 64-bit targets; WAL/segment encodings are
    /// unchanged.
    pub value: Option<Box<[u8]>>,
}

/// Attributable mutable-memory components. `owned_bytes_lower_bound` excludes
/// B-tree node metadata, allocator headers/fragmentation, and process pages;
/// it is intentionally not presented as an RSS estimate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemtableProfile {
    pub key_count: usize,
    pub version_count: usize,
    pub key_payload_bytes: usize,
    pub value_payload_bytes: usize,
    pub tombstones: usize,
    pub spilled_chains: usize,
    pub spilled_version_capacity: usize,
    pub key_handle_bytes: usize,
    pub version_chain_bytes: usize,
    pub version_record_bytes: usize,
    pub owned_bytes_lower_bound: usize,
}

/// Ordered MVCC reference memtable. Versions remain until snapshot-aware
/// flush/compaction proves they are unreachable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memtable {
    // AI storage workloads overwhelmingly create one live version per logical
    // key. Keep that version inline in the tree node and spill only actual
    // MVCC history, avoiding one heap allocation for every new key.
    versions: BTreeMap<Box<[u8]>, VersionChain>,
    maximum_sequence: u64,
    version_count: usize,
    approximate_bytes: usize,
}

impl Memtable {
    pub fn recover(batches: &[RecoveredBatch]) -> Result<Self> {
        Self::recover_from(batches, 0)
    }

    pub fn recover_from(batches: &[RecoveredBatch], previous_sequence: u64) -> Result<Self> {
        let mut table = Self::at_sequence(previous_sequence);
        for recovered in batches {
            let batch = WriteBatch::decode(&recovered.payload)?;
            table.apply_owned_write_batch(
                batch,
                recovered.first_sequence,
                recovered.last_sequence,
            )?;
        }
        Ok(table)
    }

    pub(crate) fn at_sequence(sequence: u64) -> Self {
        Self {
            maximum_sequence: sequence,
            ..Self::default()
        }
    }

    pub(crate) fn from_versions(
        versions: BTreeMap<Vec<u8>, Vec<VersionedValue>>,
        maximum_sequence: u64,
    ) -> Result<Self> {
        let mut approximate_bytes = 0usize;
        let mut version_count = 0usize;
        for (key, values) in &versions {
            if key.is_empty() || values.is_empty() {
                return Err(Error::InvalidSegment(
                    "compacted memtable contains an empty key/version set".into(),
                ));
            }
            let mut previous = 0;
            for value in values {
                if value.sequence == 0
                    || value.sequence <= previous
                    || value.sequence > maximum_sequence
                {
                    return Err(Error::InvalidSegment(
                        "compacted versions are not strictly ordered".into(),
                    ));
                }
                previous = value.sequence;
                approximate_bytes = approximate_bytes
                    .saturating_add(key.len())
                    .saturating_add(value.value.as_ref().map_or(0, |value| value.len()))
                    .saturating_add(std::mem::size_of::<VersionedValue>());
            }
            version_count = version_count
                .checked_add(values.len())
                .ok_or_else(|| Error::InvalidSegment("compacted version count overflow".into()))?;
        }
        Ok(Self {
            versions: versions
                .into_iter()
                .map(|(key, values)| (key.into_boxed_slice(), VersionChain::from_vec(values)))
                .collect(),
            maximum_sequence,
            version_count,
            approximate_bytes,
        })
    }

    pub fn apply(&mut self, recovered: &RecoveredBatch) -> Result<()> {
        let batch = WriteBatch::decode(&recovered.payload)?;
        self.apply_write_batch(&batch, recovered.first_sequence, recovered.last_sequence)
    }

    pub(crate) fn apply_owned_recovered(&mut self, recovered: RecoveredBatch) -> Result<()> {
        let batch = WriteBatch::decode(&recovered.payload)?;
        self.apply_owned_write_batch(batch, recovered.first_sequence, recovered.last_sequence)
    }

    pub(crate) fn apply_write_batch(
        &mut self,
        batch: &WriteBatch,
        first_sequence: u64,
        last_sequence: u64,
    ) -> Result<()> {
        let operation_count = u64::try_from(batch.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u64".into()))?;
        let expected_last = first_sequence
            .checked_add(operation_count - 1)
            .ok_or_else(|| Error::InvalidBatch("batch sequence range overflow".into()))?;
        if first_sequence != self.maximum_sequence.saturating_add(1)
            || last_sequence != expected_last
        {
            return Err(Error::InvalidBatch(format!(
                "batch sequence range {}..={} does not match {} operation(s) after sequence {}",
                first_sequence,
                last_sequence,
                batch.len(),
                self.maximum_sequence
            )));
        }
        let next_version_count = self
            .version_count
            .checked_add(batch.len())
            .ok_or_else(|| Error::InvalidBatch("memtable version count overflow".into()))?;
        for (index, operation) in batch.operations.iter().enumerate() {
            let sequence = first_sequence + index as u64;
            let (key, value) = match operation {
                Mutation::Put { key, value } => {
                    (key.clone(), Some(value.clone().into_boxed_slice()))
                }
                Mutation::Delete { key } => (key.clone(), None),
            };
            self.approximate_bytes = self
                .approximate_bytes
                .saturating_add(key.len())
                .saturating_add(value.as_ref().map_or(0, |value| value.len()))
                .saturating_add(std::mem::size_of::<VersionedValue>());
            self.insert_version(key.into_boxed_slice(), VersionedValue { sequence, value });
        }
        self.maximum_sequence = last_sequence;
        self.version_count = next_version_count;
        Ok(())
    }

    pub(crate) fn apply_owned_write_batch(
        &mut self,
        batch: WriteBatch,
        first_sequence: u64,
        last_sequence: u64,
    ) -> Result<()> {
        let operation_count = u64::try_from(batch.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u64".into()))?;
        let expected_last = first_sequence
            .checked_add(operation_count - 1)
            .ok_or_else(|| Error::InvalidBatch("batch sequence range overflow".into()))?;
        if first_sequence != self.maximum_sequence.saturating_add(1)
            || last_sequence != expected_last
        {
            return Err(Error::InvalidBatch(format!(
                "batch sequence range {first_sequence}..={last_sequence} does not match {} operation(s) after sequence {}",
                batch.len(), self.maximum_sequence
            )));
        }
        let next_version_count = self
            .version_count
            .checked_add(batch.len())
            .ok_or_else(|| Error::InvalidBatch("memtable version count overflow".into()))?;
        for (index, operation) in batch.operations.into_iter().enumerate() {
            let sequence = first_sequence + index as u64;
            let (key, value) = match operation {
                Mutation::Put { key, value } => (key, Some(value.into_boxed_slice())),
                Mutation::Delete { key } => (key, None),
            };
            self.approximate_bytes = self
                .approximate_bytes
                .saturating_add(key.len())
                .saturating_add(value.as_ref().map_or(0, |value| value.len()))
                .saturating_add(std::mem::size_of::<VersionedValue>());
            self.insert_version(key.into_boxed_slice(), VersionedValue { sequence, value });
        }
        self.maximum_sequence = last_sequence;
        self.version_count = next_version_count;
        Ok(())
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
        self.visible_from(start, end, read_sequence)
            .into_iter()
            .filter_map(|(key, version)| version.value.map(|value| (key, value.into_vec())))
            .collect()
    }

    /// Returns only visible versions inside the requested ordered interval.
    /// Tombstones are retained so callers merging immutable and mutable layers
    /// cannot resurrect an older value.
    pub(crate) fn visible_from(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Vec<(Vec<u8>, VersionedValue)> {
        let bounds = (Included(start), end.map_or(Unbounded, Excluded));
        self.versions
            .range::<[u8], _>(bounds)
            .filter_map(|(key, versions)| {
                versions
                    .iter()
                    .rev()
                    .find(|version| version.sequence <= read_sequence)
                    .cloned()
                    .map(|version| (key.to_vec(), version))
            })
            .collect()
    }

    pub fn maximum_sequence(&self) -> u64 {
        self.maximum_sequence
    }

    pub fn key_count(&self) -> usize {
        self.versions.len()
    }

    pub fn version_count(&self) -> usize {
        self.version_count
    }

    pub fn approximate_bytes(&self) -> usize {
        self.approximate_bytes
    }

    pub fn profile(&self) -> MemtableProfile {
        let mut profile = MemtableProfile {
            key_count: self.key_count(),
            version_count: self.version_count(),
            key_handle_bytes: std::mem::size_of::<Box<[u8]>>(),
            version_chain_bytes: std::mem::size_of::<VersionChain>(),
            version_record_bytes: std::mem::size_of::<VersionedValue>(),
            ..MemtableProfile::default()
        };
        for (key, versions) in &self.versions {
            profile.key_payload_bytes = profile.key_payload_bytes.saturating_add(key.len());
            if let Some(capacity) = versions.spilled_capacity() {
                profile.spilled_chains = profile.spilled_chains.saturating_add(1);
                profile.spilled_version_capacity = profile
                    .spilled_version_capacity
                    .saturating_add(capacity);
            }
            for version in versions.iter() {
                match &version.value {
                    Some(value) => {
                        profile.value_payload_bytes =
                            profile.value_payload_bytes.saturating_add(value.len());
                    }
                    None => profile.tombstones = profile.tombstones.saturating_add(1),
                }
            }
        }
        profile.owned_bytes_lower_bound = profile
            .key_count
            .saturating_mul(
                profile
                    .key_handle_bytes
                    .saturating_add(profile.version_chain_bytes),
            )
            .saturating_add(profile.key_payload_bytes)
            .saturating_add(profile.value_payload_bytes)
            .saturating_add(
                profile
                    .spilled_version_capacity
                    .saturating_mul(profile.version_record_bytes),
            );
        profile
    }

    pub fn all_versions(&self) -> impl Iterator<Item = (&[u8], &[VersionedValue])> {
        self.versions
            .iter()
            .map(|(key, versions)| (key.as_ref(), versions.as_slice()))
    }

    pub fn visible_versions(&self, read_sequence: u64) -> Vec<(Vec<u8>, VersionedValue)> {
        self.visible_from(&[], None, read_sequence)
    }

    fn insert_version(&mut self, key: Box<[u8]>, version: VersionedValue) {
        match self.versions.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(VersionChain::One(version));
            }
            Entry::Occupied(mut entry) => entry.get_mut().push(version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_separates_attributable_bytes_from_rss() {
        let mut table = Memtable::default();
        let first = WriteBatch::new(vec![
            Mutation::Put {
                key: b"a".to_vec(),
                value: b"one".to_vec(),
            },
            Mutation::Put {
                key: b"a".to_vec(),
                value: b"two".to_vec(),
            },
            Mutation::Delete { key: b"b".to_vec() },
        ])
        .unwrap();
        table.apply_owned_write_batch(first, 1, 3).unwrap();

        let profile = table.profile();
        assert_eq!(profile.key_count, 2);
        assert_eq!(profile.version_count, 3);
        assert_eq!(profile.key_payload_bytes, 2);
        assert_eq!(profile.value_payload_bytes, 6);
        assert_eq!(profile.tombstones, 1);
        assert_eq!(profile.spilled_chains, 1);
        assert!(profile.spilled_version_capacity >= 2);
        assert!(profile.owned_bytes_lower_bound >= 8);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(profile.version_record_bytes, 24);
    }

    #[test]
    fn exact_length_value_keeps_the_existing_serde_shape() {
        let value = VersionedValue {
            sequence: 7,
            value: Some(vec![1, 2, 3].into_boxed_slice()),
        };
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(encoded, r#"{"sequence":7,"value":[1,2,3]}"#);
        assert_eq!(serde_json::from_str::<VersionedValue>(&encoded).unwrap(), value);
    }
}
