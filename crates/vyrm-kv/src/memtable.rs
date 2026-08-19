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
        let mut table = Self::default();
        for batch in batches {
            table.apply(batch)?;
        }
        Ok(table)
    }

    pub fn apply(&mut self, recovered: &RecoveredBatch) -> Result<()> {
        let batch = WriteBatch::decode(&recovered.payload)?;
        let operation_count = u64::try_from(batch.len())
            .map_err(|_| Error::InvalidBatch("operation count exceeds u64".into()))?;
        let expected_last = recovered
            .first_sequence
            .checked_add(operation_count - 1)
            .ok_or_else(|| Error::InvalidBatch("batch sequence range overflow".into()))?;
        if recovered.first_sequence != self.maximum_sequence.saturating_add(1)
            || recovered.last_sequence != expected_last
        {
            return Err(Error::InvalidBatch(format!(
                "batch sequence range {}..={} does not match {} operation(s) after sequence {}",
                recovered.first_sequence,
                recovered.last_sequence,
                batch.len(),
                self.maximum_sequence
            )));
        }
        for (index, operation) in batch.operations.into_iter().enumerate() {
            let sequence = recovered.first_sequence + index as u64;
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
        self.maximum_sequence = recovered.last_sequence;
        Ok(())
    }

    pub fn get(&self, key: &[u8], read_sequence: u64) -> Option<&[u8]> {
        self.versions
            .get(key)?
            .iter()
            .rev()
            .find(|version| version.sequence <= read_sequence)?
            .value
            .as_deref()
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
}
