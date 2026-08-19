use crate::{
    recover, AppendReceipt, Durability, Memtable, RecoveredBatch, Result, WalWriter, WriteBatch,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ACTIVE_WAL: &str = "active.wal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub sequence: u64,
}

/// Single-writer native database during the M3 memtable phase. Immutable
/// segments join this composition next; the API already makes every read's
/// MVCC boundary explicit.
pub struct Database {
    root: PathBuf,
    wal: WalWriter,
    memtable: Memtable,
}

impl Database {
    pub fn create(root: &Path) -> Result<Self> {
        std::fs::create_dir(root)?;
        let wal = WalWriter::create(&root.join(ACTIVE_WAL))?;
        Ok(Self {
            root: root.to_owned(),
            wal,
            memtable: Memtable::default(),
        })
    }

    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join(ACTIVE_WAL);
        let recovery = recover(&path)?;
        if let Some(offset) = recovery.torn_tail {
            return Err(crate::Error::TornTail { offset });
        }
        let memtable = Memtable::recover(&recovery.batches)?;
        let wal = WalWriter::open(&path)?;
        Ok(Self {
            root: root.to_owned(),
            wal,
            memtable,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            sequence: self.memtable.maximum_sequence(),
        }
    }

    pub fn write(&mut self, batch: &WriteBatch, durability: Durability) -> Result<AppendReceipt> {
        let payload = batch.encode()?;
        let receipt = self.wal.append_write_batch(batch, durability)?;
        self.memtable.apply(&RecoveredBatch {
            offset: receipt.offset,
            first_sequence: receipt.first_sequence,
            last_sequence: receipt.last_sequence,
            checksum: receipt.checksum,
            payload,
        })?;
        Ok(receipt)
    }

    pub fn sync(&mut self) -> Result<u64> {
        self.wal.sync()
    }

    pub fn get(&self, key: &[u8], snapshot: Snapshot) -> Option<&[u8]> {
        self.memtable.get(key, snapshot.sequence)
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        snapshot: Snapshot,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.memtable.scan(start, end, snapshot.sequence)
    }

    pub fn memtable(&self) -> &Memtable {
        &self.memtable
    }
}
