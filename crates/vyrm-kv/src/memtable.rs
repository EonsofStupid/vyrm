use crate::{Error, Mutation, RecoveredBatch, Result, WriteBatch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedValue {
    pub sequence: u64,
    pub value: Option<Vec<u8>>,
}

/// Ordered MVCC reference memtable. Versions remain until snapshot-aware
/// flush/compaction proves they are unreachable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memtable {
    versions: BTreeMap<Vec<u8>, Vec<VersionedValue>>,
    maximum_sequence: u64,
    approximate_bytes: usize,
}

impl Memtable {
    pub fn recover(batches: &[RecoveredBatch]) -> Result<Self> {
        Self::recover_from(batches, 0)
    }

    pub fn recover_from(batches: &[RecoveredBatch], previous_sequence: u64) -> Result<Self> {
        let mut table = Self::at_sequence(previous_sequence);
        for batch in batches {
            table.apply(batch)?;
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
                    .saturating_add(value.value.as_ref().map_or(0, Vec::len))
                    .saturating_add(std::mem::size_of::<VersionedValue>());
            }
        }
        Ok(Self {
            versions,
            maximum_sequence,
            approximate_bytes,
        })
    }

    pub fn apply(&mut self, recovered: &RecoveredBatch) -> Result<()> {
        let batch = WriteBatch::decode(&recovered.payload)?;
        self.apply_write_batch(
            &batch,
            recovered.first_sequence,
            recovered.last_sequence,
        )
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
        for (index, operation) in batch.operations.iter().enumerate() {
            let sequence = first_sequence + index as u64;
            let (key, value) = match operation {
                Mutation::Put { key, value } => (key.clone(), Some(value.clone())),
                Mutation::Delete { key } => (key.clone(), None),
            };
            self.approximate_bytes = self
                .approximate_bytes
                .saturating_add(key.len())
                .saturating_add(value.as_ref().map_or(0, Vec::len))
                .saturating_add(std::mem::size_of::<VersionedValue>());
            self.versions
                .entry(key)
                .or_default()
                .push(VersionedValue { sequence, value });
        }
        self.maximum_sequence = last_sequence;
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
        for (index, operation) in batch.operations.into_iter().enumerate() {
            let sequence = first_sequence + index as u64;
            let (key, value) = match operation {
                Mutation::Put { key, value } => (key, Some(value)),
                Mutation::Delete { key } => (key, None),
            };
            self.approximate_bytes = self
                .approximate_bytes
                .saturating_add(key.len())
                .saturating_add(value.as_ref().map_or(0, Vec::len))
                .saturating_add(std::mem::size_of::<VersionedValue>());
            self.versions
                .entry(key)
                .or_default()
                .push(VersionedValue { sequence, value });
        }
        self.maximum_sequence = last_sequence;
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

    pub fn maximum_sequence(&self) -> u64 {
        self.maximum_sequence
    }

    pub fn key_count(&self) -> usize {
        self.versions.len()
    }

    pub fn version_count(&self) -> usize {
        self.versions.values().map(Vec::len).sum()
    }

    pub fn approximate_bytes(&self) -> usize {
        self.approximate_bytes
    }

    pub fn all_versions(&self) -> impl Iterator<Item = (&[u8], &[VersionedValue])> {
        self.versions
            .iter()
            .map(|(key, versions)| (key.as_slice(), versions.as_slice()))
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
}
