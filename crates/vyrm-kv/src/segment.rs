use crate::{Error, Memtable, Result, SegmentDescriptor, VersionedValue};
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use vyrm_core::digest;

pub const SEGMENT_FORMAT_VERSION: u16 = 3;
pub const DEFAULT_BLOCK_CACHE_BYTES: usize = 4 * 1024 * 1024;
pub const SEGMENT_BLOCK_TARGET_BYTES: usize = 4 * 1024;
const SEGMENT_V1_MAGIC: &[u8; 8] = b"VYRSEG01";
const SEGMENT_V2_MAGIC: &[u8; 8] = b"VYRSEG02";
const SEGMENT_V3_MAGIC: &[u8; 8] = b"VYRSEG03";
const INDEX_V3_MAGIC: &[u8; 8] = b"VYRIX003";
const V1_HEADER_BYTES: usize = 40;
const V2_HEADER_BYTES: usize = 48;
const V3_HEADER_BYTES: usize = 64;
const INDEX_HEADER_BYTES: usize = 16;
const INDEX_ENTRY_BYTES: usize = 104;
const RECORD_HEADER_BYTES: usize = 20;
const FOOTER_BYTES: usize = 64;
const MAX_SEGMENT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 1024 * 1024;
const MAX_VALUE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DECODED_BLOCK_BYTES: usize = RECORD_HEADER_BYTES + MAX_KEY_BYTES + MAX_VALUE_BYTES;
const SPARSE_INDEX_STRIDE: usize = 4;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);
static CACHE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockCacheStats {
    pub capacity_bytes: usize,
    pub resident_bytes: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
pub(crate) struct BlockCache {
    capacity_bytes: usize,
    resident_bytes: usize,
    values: HashMap<(u64, usize), CacheEntry>,
    order: BinaryHeap<Reverse<(u64, (u64, usize))>>,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

#[derive(Debug)]
struct CacheEntry {
    value: Arc<DecodedBlock>,
    last_used: u64,
}

pub(crate) type SharedBlockCache = Arc<Mutex<BlockCache>>;

pub(crate) fn new_block_cache(capacity_bytes: usize) -> SharedBlockCache {
    Arc::new(Mutex::new(BlockCache {
        capacity_bytes,
        resident_bytes: 0,
        values: HashMap::new(),
        order: BinaryHeap::new(),
        clock: 0,
        hits: 0,
        misses: 0,
        evictions: 0,
    }))
}

pub(crate) fn block_cache_stats(cache: &SharedBlockCache) -> BlockCacheStats {
    let cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    BlockCacheStats {
        capacity_bytes: cache.capacity_bytes,
        resident_bytes: cache.resident_bytes,
        entries: cache.values.len(),
        hits: cache.hits,
        misses: cache.misses,
        evictions: cache.evictions,
    }
}

#[derive(Debug, Clone)]
enum BlockSource {
    File(Arc<Mutex<File>>),
    Bytes(Arc<Vec<u8>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockDescriptor {
    offset: u64,
    physical_bytes: usize,
    record_bytes: usize,
    entries: u64,
    digest: [u8; 32],
    last_key: Vec<u8>,
}

#[derive(Debug)]
struct DecodedBlock {
    bytes: Vec<u8>,
    record_offsets: Vec<u32>,
}

impl DecodedBlock {
    fn parse(bytes: Vec<u8>) -> Result<Self> {
        let mut record_offsets = Vec::new();
        let mut cursor = 0;
        while cursor < bytes.len() {
            record_offsets.push(
                u32::try_from(cursor).map_err(|_| {
                    Error::InvalidSegment("decoded block offset exceeds u32".into())
                })?,
            );
            cursor = parse_record(&bytes, cursor, bytes.len(), 1, u64::MAX)?.next;
        }
        Ok(Self {
            bytes,
            record_offsets,
        })
    }

    fn resident_bytes(&self) -> usize {
        self.bytes.len().saturating_add(
            self.record_offsets
                .len()
                .saturating_mul(std::mem::size_of::<u32>()),
        )
    }

    fn lower_bound(&self, key: &[u8]) -> Result<usize> {
        let mut left = 0;
        let mut right = self.record_offsets.len();
        while left < right {
            let middle = left + (right - left) / 2;
            let record = parse_record(
                &self.bytes,
                self.record_offsets[middle] as usize,
                self.bytes.len(),
                1,
                u64::MAX,
            )?;
            if record.key < key {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        Ok(left)
    }
}

#[derive(Debug)]
enum SegmentStorage {
    Legacy {
        records: Vec<u8>,
        sparse_index: Vec<SparseEntry>,
    },
    Blocked {
        source: BlockSource,
        blocks: Vec<BlockDescriptor>,
        cache: SharedBlockCache,
        cache_id: u64,
    },
}

pub struct Segment {
    pub descriptor: SegmentDescriptor,
    storage: SegmentStorage,
}

impl fmt::Debug for Segment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Segment")
            .field("descriptor", &self.descriptor)
            .field("block_count", &self.block_count())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SparseEntry {
    offset: usize,
    key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentVersion {
    pub sequence: u64,
    pub value: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
struct Record<'a> {
    key: &'a [u8],
    value: Option<&'a [u8]>,
    sequence: u64,
    next: usize,
}

impl Segment {
    pub fn write_from_memtable(directory: &Path, table: &Memtable) -> Result<(Self, PathBuf)> {
        Self::write_from_memtable_with_cache(
            directory,
            table,
            new_block_cache(DEFAULT_BLOCK_CACHE_BYTES),
        )
    }

    pub(crate) fn write_from_memtable_with_cache(
        directory: &Path,
        table: &Memtable,
        cache: SharedBlockCache,
    ) -> Result<(Self, PathBuf)> {
        let bytes = encode_v3(table)?;
        let digest = digest::sha256_hex(&bytes[..bytes.len() - FOOTER_BYTES]);
        let path = directory.join(format!("{digest}.seg"));
        std::fs::create_dir_all(directory)?;
        if path.exists() {
            let segment = Self::open_with_cache(&path, cache)?;
            if segment.descriptor.id != digest {
                return invalid("existing content-addressed segment has another identity");
            }
            return Ok((segment, path));
        }
        let temporary = directory.join(format!(
            ".{digest}.{}.{}.tmp",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Err(error) = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            File::open(directory)?.sync_all()
        })() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::Io(error));
        }
        Ok((Self::open_with_cache(&path, cache)?, path))
    }

    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_cache(path, new_block_cache(DEFAULT_BLOCK_CACHE_BYTES))
    }

    pub(crate) fn open_with_cache(path: &Path, cache: SharedBlockCache) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > MAX_SEGMENT_BYTES {
            return invalid("segment exceeds the 1 GiB physical safety limit");
        }
        if metadata.len() < (V1_HEADER_BYTES + FOOTER_BYTES) as u64 {
            return invalid("segment is shorter than its header and footer");
        }
        let mut file = File::open(path)?;
        let mut prefix = [0u8; 10];
        file.read_exact(&mut prefix)?;
        let version = u16::from_be_bytes(prefix[8..10].try_into().expect("fixed version"));
        if version == SEGMENT_FORMAT_VERSION && &prefix[..8] == SEGMENT_V3_MAGIC {
            decode_v3_file(path, metadata.len(), cache)
        } else {
            decode_legacy(std::fs::read(path)?)
        }
    }

    pub(crate) fn validate_snapshot_bytes(
        expected: &SegmentDescriptor,
        bytes: &[u8],
    ) -> Result<Self> {
        Self::validate_snapshot_owned(expected, bytes.to_vec())
    }

    pub(crate) fn validate_snapshot_owned(
        expected: &SegmentDescriptor,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        if bytes.len() as u64 > MAX_SEGMENT_BYTES {
            return invalid("snapshot segment exceeds the 1 GiB physical safety limit");
        }
        let mut segment = if bytes.starts_with(SEGMENT_V3_MAGIC) {
            decode_v3_bytes(bytes, new_block_cache(DEFAULT_BLOCK_CACHE_BYTES))?
        } else {
            decode_legacy(bytes)?
        };
        segment.descriptor.level = expected.level;
        if &segment.descriptor != expected {
            return Err(Error::InvalidSegment(format!(
                "snapshot segment {} differs from its descriptor",
                expected.id
            )));
        }
        Ok(segment)
    }

    fn validate_snapshot_descriptor(expected: &SegmentDescriptor, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > MAX_SEGMENT_BYTES {
            return invalid("snapshot segment exceeds the 1 GiB physical safety limit");
        }
        let mut descriptor = if bytes.starts_with(SEGMENT_V3_MAGIC) {
            validate_v3_slice(bytes)?
        } else {
            decode_legacy(bytes.to_vec())?.descriptor
        };
        descriptor.level = expected.level;
        if &descriptor != expected {
            return Err(Error::InvalidSegment(format!(
                "snapshot segment {} differs from its descriptor",
                expected.id
            )));
        }
        Ok(())
    }

    pub(crate) fn install_snapshot_bytes_with_cache(
        directory: &Path,
        expected: &SegmentDescriptor,
        bytes: &[u8],
        cache: SharedBlockCache,
    ) -> Result<Self> {
        Self::validate_snapshot_descriptor(expected, bytes)?;
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!("{}.seg", expected.id));
        if path.exists() {
            let mut existing = Self::open_with_cache(&path, cache)?;
            existing.descriptor.level = expected.level;
            if &existing.descriptor != expected || !file_equals_bytes(&path, bytes)? {
                return invalid(format!(
                    "existing snapshot segment {} has different bytes",
                    expected.id
                ));
            }
            return Ok(existing);
        }
        let temporary = directory.join(format!(
            ".{}.{}.{}.snapshot.tmp",
            expected.id,
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Err(error) = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            File::open(directory)?.sync_all()
        })() {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::Io(error));
        }
        let mut installed = Self::open_with_cache(&path, cache)?;
        installed.descriptor.level = expected.level;
        Ok(installed)
    }

    pub fn get(&self, key: &[u8], read_sequence: u64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get_version(key, read_sequence)?
            .and_then(|version| version.value))
    }

    pub(crate) fn get_version(
        &self,
        key: &[u8],
        read_sequence: u64,
    ) -> Result<Option<SegmentVersion>> {
        if key < self.descriptor.first_key.as_slice()
            || key > self.descriptor.last_key.as_slice()
            || read_sequence < self.descriptor.minimum_sequence
        {
            return Ok(None);
        }
        match &self.storage {
            SegmentStorage::Legacy {
                records,
                sparse_index,
            } => {
                let cursor = legacy_seek(records, sparse_index, key);
                select_version(records, cursor, records.len(), key, read_sequence)
            }
            SegmentStorage::Blocked { blocks, .. } => {
                let mut block_index =
                    blocks.partition_point(|block| block.last_key.as_slice() < key);
                let mut selected = None;
                while block_index < blocks.len() {
                    let block = self.load_block(block_index)?;
                    if let Some(version) = select_version_block(&block, key, read_sequence)? {
                        if selected
                            .as_ref()
                            .is_none_or(|prior: &SegmentVersion| version.sequence > prior.sequence)
                        {
                            selected = Some(version);
                        }
                    }
                    if blocks[block_index].last_key.as_slice() > key {
                        break;
                    }
                    block_index += 1;
                }
                Ok(selected)
            }
        }
    }

    pub(crate) fn get_versions(
        &self,
        keys: &[&[u8]],
        read_sequence: u64,
    ) -> Result<Vec<Option<SegmentVersion>>> {
        let mut output = vec![None; keys.len()];
        let mut order = (0..keys.len()).collect::<Vec<_>>();
        order.sort_by(|left, right| keys[*left].cmp(keys[*right]).then(left.cmp(right)));
        match &self.storage {
            SegmentStorage::Legacy { .. } => {
                for index in order {
                    output[index] = self.get_version(keys[index], read_sequence)?;
                }
            }
            SegmentStorage::Blocked { blocks, .. } => {
                let mut loaded: Option<(usize, Arc<DecodedBlock>)> = None;
                for index in order {
                    let key = keys[index];
                    if key < self.descriptor.first_key.as_slice()
                        || key > self.descriptor.last_key.as_slice()
                        || read_sequence < self.descriptor.minimum_sequence
                    {
                        continue;
                    }
                    let mut block_index =
                        blocks.partition_point(|block| block.last_key.as_slice() < key);
                    let mut selected = None;
                    while block_index < blocks.len() {
                        let block = match &loaded {
                            Some((loaded_index, block)) if *loaded_index == block_index => {
                                Arc::clone(block)
                            }
                            _ => {
                                let block = self.load_block(block_index)?;
                                loaded = Some((block_index, Arc::clone(&block)));
                                block
                            }
                        };
                        if let Some(version) = select_version_block(&block, key, read_sequence)? {
                            if selected.as_ref().is_none_or(|prior: &SegmentVersion| {
                                version.sequence > prior.sequence
                            }) {
                                selected = Some(version);
                            }
                        }
                        if blocks[block_index].last_key.as_slice() > key {
                            break;
                        }
                        block_index += 1;
                    }
                    output[index] = selected;
                }
            }
        }
        Ok(output)
    }

    pub fn scan(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let SegmentStorage::Blocked { blocks, .. } = &self.storage else {
            return Ok(self
                .visible_from(start, end, read_sequence)?
                .into_iter()
                .filter_map(|(key, version)| version.value.map(|value| (key, value)))
                .collect());
        };
        let mut output = Vec::new();
        let mut current_key = Vec::new();
        let mut selected = None::<SegmentVersion>;
        let mut collect = |record: Record<'_>| {
            if record.key < start || end.is_some_and(|end| record.key >= end) {
                return Ok(());
            }
            if current_key.as_slice() != record.key {
                if let Some(value) = selected.take().and_then(|version| version.value) {
                    output.push((std::mem::take(&mut current_key), value));
                } else {
                    current_key.clear();
                }
                current_key.extend_from_slice(record.key);
            }
            if record.sequence <= read_sequence
                && selected
                    .as_ref()
                    .is_none_or(|version| record.sequence > version.sequence)
            {
                selected = Some(SegmentVersion {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                });
            }
            Ok(())
        };
        let first = blocks.partition_point(|block| block.last_key.as_slice() < start);
        for (index, descriptor) in blocks.iter().enumerate().skip(first) {
            let block = self.load_block(index)?;
            let first_record = if index == first {
                block.lower_bound(start)?
            } else {
                0
            };
            visit_decoded_records(&block, first_record, &mut collect)?;
            if end.is_some_and(|end| descriptor.last_key.as_slice() >= end) {
                break;
            }
        }
        if let Some(value) = selected.and_then(|version| version.value) {
            output.push((current_key, value));
        }
        Ok(output)
    }

    pub fn visible_versions(&self, read_sequence: u64) -> Result<Vec<(Vec<u8>, VersionedValue)>> {
        self.visible_from(&[], None, read_sequence)
    }

    pub(crate) fn all_versions(&self) -> Result<Vec<(Vec<u8>, Vec<VersionedValue>)>> {
        let mut output = Vec::<(Vec<u8>, Vec<VersionedValue>)>::new();
        self.for_each_record(|record| {
            if output
                .last()
                .is_none_or(|(key, _)| key.as_slice() != record.key)
            {
                output.push((record.key.to_vec(), Vec::new()));
            }
            output
                .last_mut()
                .expect("record creates a version group")
                .1
                .push(VersionedValue {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                });
            Ok(())
        })?;
        Ok(output)
    }

    pub fn sparse_index_entries(&self) -> usize {
        match &self.storage {
            SegmentStorage::Legacy { sparse_index, .. } => sparse_index.len(),
            SegmentStorage::Blocked { blocks, .. } => blocks.len(),
        }
    }

    pub fn block_count(&self) -> usize {
        match &self.storage {
            SegmentStorage::Legacy { .. } => 1,
            SegmentStorage::Blocked { blocks, .. } => blocks.len(),
        }
    }

    pub(crate) fn visible_from(
        &self,
        start: &[u8],
        end: Option<&[u8]>,
        read_sequence: u64,
    ) -> Result<Vec<(Vec<u8>, VersionedValue)>> {
        let mut grouped = BTreeMap::<Vec<u8>, SegmentVersion>::new();
        let mut collect = |record: Record<'_>| {
            if record.key < start || end.is_some_and(|end| record.key >= end) {
                return Ok(());
            }
            if record.sequence <= read_sequence {
                let version = SegmentVersion {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                };
                if grouped
                    .get(record.key)
                    .is_none_or(|prior| version.sequence > prior.sequence)
                {
                    grouped.insert(record.key.to_vec(), version);
                }
            }
            Ok(())
        };
        match &self.storage {
            SegmentStorage::Legacy { records, .. } => visit_records(records, &mut collect)?,
            SegmentStorage::Blocked { blocks, .. } => {
                let first = blocks.partition_point(|block| block.last_key.as_slice() < start);
                for (index, descriptor) in blocks.iter().enumerate().skip(first) {
                    let block = self.load_block(index)?;
                    let first_record = if index == first {
                        block.lower_bound(start)?
                    } else {
                        0
                    };
                    visit_decoded_records(&block, first_record, &mut collect)?;
                    if end.is_some_and(|end| descriptor.last_key.as_slice() >= end) {
                        break;
                    }
                }
            }
        }
        Ok(grouped
            .into_iter()
            .map(|(key, version)| {
                (
                    key,
                    VersionedValue {
                        sequence: version.sequence,
                        value: version.value,
                    },
                )
            })
            .collect())
    }

    fn for_each_record(&self, mut visit: impl FnMut(Record<'_>) -> Result<()>) -> Result<()> {
        match &self.storage {
            SegmentStorage::Legacy { records, .. } => visit_records(records, &mut visit),
            SegmentStorage::Blocked { blocks, .. } => {
                for index in 0..blocks.len() {
                    let block = self.load_block(index)?;
                    visit_decoded_records(&block, 0, &mut visit)?;
                }
                Ok(())
            }
        }
    }

    fn load_block(&self, block_index: usize) -> Result<Arc<DecodedBlock>> {
        let SegmentStorage::Blocked {
            source,
            blocks,
            cache,
            cache_id,
        } = &self.storage
        else {
            return invalid("legacy segment has no block table");
        };
        let key = (*cache_id, block_index);
        {
            let mut cache = cache
                .lock()
                .map_err(|_| Error::InvalidSegment("block cache lock poisoned".into()))?;
            if cache.values.contains_key(&key) {
                cache.hits = cache.hits.saturating_add(1);
                cache.touch(key);
                let value = Arc::clone(&cache.values[&key].value);
                cache.maybe_rebuild_order();
                return Ok(value);
            }
            cache.misses = cache.misses.saturating_add(1);
        }
        let decoded = Arc::new(DecodedBlock::parse(read_and_decode_block(
            source,
            &blocks[block_index],
        )?)?);
        let mut cache = cache
            .lock()
            .map_err(|_| Error::InvalidSegment("block cache lock poisoned".into()))?;
        if cache.values.contains_key(&key) {
            cache.touch(key);
            let value = Arc::clone(&cache.values[&key].value);
            cache.maybe_rebuild_order();
            return Ok(value);
        }
        let decoded_bytes = decoded.resident_bytes();
        if decoded_bytes <= cache.capacity_bytes {
            while cache.resident_bytes.saturating_add(decoded_bytes) > cache.capacity_bytes {
                let Some(Reverse((stamp, oldest))) = cache.order.pop() else {
                    break;
                };
                if cache
                    .values
                    .get(&oldest)
                    .is_some_and(|entry| entry.last_used != stamp)
                {
                    continue;
                }
                if let Some(removed) = cache.values.remove(&oldest) {
                    cache.resident_bytes = cache
                        .resident_bytes
                        .saturating_sub(removed.value.resident_bytes());
                    cache.evictions = cache.evictions.saturating_add(1);
                }
            }
            cache.resident_bytes = cache.resident_bytes.saturating_add(decoded_bytes);
            let stamp = cache.next_stamp();
            cache.values.insert(
                key,
                CacheEntry {
                    value: Arc::clone(&decoded),
                    last_used: stamp,
                },
            );
            cache.order.push(Reverse((stamp, key)));
            cache.maybe_rebuild_order();
        }
        Ok(decoded)
    }
}

impl BlockCache {
    fn next_stamp(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        if self.clock == 0 {
            self.renumber();
        }
        self.clock
    }

    fn touch(&mut self, key: (u64, usize)) -> u64 {
        let stamp = self.next_stamp();
        self.values
            .get_mut(&key)
            .expect("cache key was checked")
            .last_used = stamp;
        self.order.push(Reverse((stamp, key)));
        stamp
    }

    fn maybe_rebuild_order(&mut self) {
        let maximum = self.values.len().saturating_mul(2).max(32);
        if self.order.len() > maximum {
            self.rebuild_order();
        }
    }

    fn rebuild_order(&mut self) {
        self.order = self
            .values
            .iter()
            .map(|(key, entry)| Reverse((entry.last_used, *key)))
            .collect();
    }

    fn renumber(&mut self) {
        let mut ordered = self
            .values
            .iter()
            .map(|(key, entry)| (entry.last_used, *key))
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        for (index, (_, key)) in ordered.into_iter().enumerate() {
            self.values
                .get_mut(&key)
                .expect("cache key exists")
                .last_used = u64::try_from(index + 1).expect("cache size fits u64");
        }
        self.clock = u64::try_from(self.values.len()).expect("cache size fits u64");
        self.rebuild_order();
    }
}

fn encode_v3(table: &Memtable) -> Result<Vec<u8>> {
    if table.version_count() == 0 {
        return invalid("cannot write an empty segment");
    }
    let entries = u64::try_from(table.version_count())
        .map_err(|_| Error::InvalidSegment("entry count exceeds u64".into()))?;
    let minimum_sequence = table
        .all_versions()
        .flat_map(|(_, versions)| versions.iter().map(|version| version.sequence))
        .min()
        .expect("non-empty table has a minimum sequence");
    let maximum_sequence = table.maximum_sequence();
    let mut blocks = Vec::<(Vec<u8>, u64, Vec<u8>)>::new();
    let mut current = Vec::with_capacity(SEGMENT_BLOCK_TARGET_BYTES);
    let mut current_entries = 0u64;
    let mut current_last_key = Vec::new();
    let mut total_record_bytes = 0u64;
    for (key, versions) in table.all_versions() {
        for version in versions {
            let record = encode_record(key, version)?;
            if !current.is_empty()
                && current.len().saturating_add(record.len()) > SEGMENT_BLOCK_TARGET_BYTES
            {
                blocks.push((
                    std::mem::take(&mut current),
                    current_entries,
                    std::mem::take(&mut current_last_key),
                ));
                current = Vec::with_capacity(SEGMENT_BLOCK_TARGET_BYTES);
                current_entries = 0;
            }
            total_record_bytes = total_record_bytes
                .checked_add(record.len() as u64)
                .ok_or_else(|| Error::InvalidSegment("record bytes overflow".into()))?;
            current.extend_from_slice(&record);
            current_entries += 1;
            current_last_key = key.to_vec();
        }
    }
    if !current.is_empty() {
        blocks.push((current, current_entries, current_last_key));
    }
    if total_record_bytes > MAX_SEGMENT_BYTES {
        return invalid("uncompressed records exceed the 1 GiB safety limit");
    }
    let mut output = vec![0u8; V3_HEADER_BYTES];
    let mut descriptors = Vec::with_capacity(blocks.len());
    for (records, block_entries, last_key) in blocks {
        let compressed = compress_prepend_size(&records);
        let offset = output.len() as u64;
        let block_digest = digest::sha256(&compressed);
        output.extend_from_slice(&compressed);
        descriptors.push(BlockDescriptor {
            offset,
            physical_bytes: compressed.len(),
            record_bytes: records.len(),
            entries: block_entries,
            digest: block_digest,
            last_key,
        });
    }
    let index_offset = output.len() as u64;
    output.extend_from_slice(INDEX_V3_MAGIC);
    output.extend_from_slice(&(descriptors.len() as u32).to_be_bytes());
    output.extend_from_slice(&0u32.to_be_bytes());
    for block in &descriptors {
        output.extend_from_slice(&block.offset.to_be_bytes());
        output.extend_from_slice(&(block.physical_bytes as u64).to_be_bytes());
        output.extend_from_slice(&(block.record_bytes as u64).to_be_bytes());
        output.extend_from_slice(&block.entries.to_be_bytes());
        output.extend_from_slice(&(block.last_key.len() as u32).to_be_bytes());
        output.extend_from_slice(&0u32.to_be_bytes());
        output.extend_from_slice(&encode_sha256_hex(block.digest));
        output.extend_from_slice(&block.last_key);
    }
    output[..8].copy_from_slice(SEGMENT_V3_MAGIC);
    output[8..10].copy_from_slice(&SEGMENT_FORMAT_VERSION.to_be_bytes());
    output[10..12].copy_from_slice(&(V3_HEADER_BYTES as u16).to_be_bytes());
    output[12..16].copy_from_slice(&1u32.to_be_bytes());
    output[16..24].copy_from_slice(&entries.to_be_bytes());
    output[24..32].copy_from_slice(&minimum_sequence.to_be_bytes());
    output[32..40].copy_from_slice(&maximum_sequence.to_be_bytes());
    output[40..48].copy_from_slice(&total_record_bytes.to_be_bytes());
    output[48..56].copy_from_slice(&index_offset.to_be_bytes());
    output[56..60].copy_from_slice(&(descriptors.len() as u32).to_be_bytes());
    output[60..64].copy_from_slice(&(SEGMENT_BLOCK_TARGET_BYTES as u32).to_be_bytes());
    if output.len() as u64 > MAX_SEGMENT_BYTES - FOOTER_BYTES as u64 {
        return invalid("encoded segment exceeds the 1 GiB safety limit");
    }
    let checksum = digest::sha256_hex(&output);
    output.extend_from_slice(checksum.as_bytes());
    Ok(output)
}

fn encode_record(key: &[u8], version: &VersionedValue) -> Result<Vec<u8>> {
    let value = version.value.as_deref().unwrap_or_default();
    let kind = if version.value.is_some() { 1 } else { 2 };
    let size = RECORD_HEADER_BYTES
        .checked_add(key.len())
        .and_then(|size| size.checked_add(value.len()))
        .ok_or_else(|| Error::InvalidSegment("record length overflow".into()))?;
    if key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || value.len() > MAX_VALUE_BYTES
        || size > MAX_DECODED_BLOCK_BYTES
    {
        return invalid("record exceeds the segment key/value contract");
    }
    let mut record = Vec::with_capacity(size);
    record.push(kind);
    record.extend_from_slice(&[0, 0, 0]);
    record.extend_from_slice(&(key.len() as u32).to_be_bytes());
    record.extend_from_slice(&(value.len() as u32).to_be_bytes());
    record.extend_from_slice(&version.sequence.to_be_bytes());
    record.extend_from_slice(key);
    record.extend_from_slice(value);
    Ok(record)
}

fn decode_v3_file(path: &Path, physical_bytes: u64, cache: SharedBlockCache) -> Result<Segment> {
    let actual = verify_file_digest(path, physical_bytes)?;
    let mut file = File::open(path)?;
    let (descriptor, blocks) = read_v3_metadata(&mut file, physical_bytes, actual)?;
    build_blocked_segment(
        descriptor,
        BlockSource::File(Arc::new(Mutex::new(file))),
        blocks,
        cache,
    )
}

fn decode_v3_bytes(bytes: Vec<u8>, cache: SharedBlockCache) -> Result<Segment> {
    if bytes.len() < V3_HEADER_BYTES + INDEX_HEADER_BYTES + FOOTER_BYTES {
        return invalid("v3 segment is shorter than its framing");
    }
    let content_end = bytes.len() - FOOTER_BYTES;
    let expected = std::str::from_utf8(&bytes[content_end..])
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = digest::sha256_hex(&bytes[..content_end]);
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    let mut cursor = std::io::Cursor::new(&bytes);
    let (descriptor, blocks) = read_v3_metadata(&mut cursor, bytes.len() as u64, actual)?;
    build_blocked_segment(
        descriptor,
        BlockSource::Bytes(Arc::new(bytes)),
        blocks,
        cache,
    )
}

fn validate_v3_slice(bytes: &[u8]) -> Result<SegmentDescriptor> {
    if bytes.len() < V3_HEADER_BYTES + INDEX_HEADER_BYTES + FOOTER_BYTES {
        return invalid("v3 segment is shorter than its framing");
    }
    let content_end = bytes.len() - FOOTER_BYTES;
    let expected = std::str::from_utf8(&bytes[content_end..])
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = digest::sha256_hex(&bytes[..content_end]);
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    let mut cursor = std::io::Cursor::new(bytes);
    let (descriptor, _) = read_v3_metadata(&mut cursor, bytes.len() as u64, actual)?;
    Ok(descriptor)
}

fn verify_file_digest(path: &Path, physical_bytes: u64) -> Result<String> {
    let content_bytes = physical_bytes
        .checked_sub(FOOTER_BYTES as u64)
        .ok_or_else(|| Error::InvalidSegment("segment has no checksum footer".into()))?;
    let mut file = File::open(path)?;
    let mut hasher = digest::Sha256::new();
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut remaining = content_bytes;
    while remaining != 0 {
        let take = usize::try_from(remaining.min(buffer.len() as u64)).expect("bounded copy");
        file.read_exact(&mut buffer[..take])?;
        hasher.update(&buffer[..take]);
        remaining -= take as u64;
    }
    let mut footer = [0u8; FOOTER_BYTES];
    file.read_exact(&mut footer)?;
    let expected = std::str::from_utf8(&footer)
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = hasher.finalize_hex();
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    Ok(actual)
}

fn file_equals_bytes(path: &Path, expected: &[u8]) -> Result<bool> {
    if std::fs::metadata(path)?.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut cursor = 0;
    while cursor < expected.len() {
        let end = cursor.saturating_add(buffer.len()).min(expected.len());
        file.read_exact(&mut buffer[..end - cursor])?;
        if buffer[..end - cursor] != expected[cursor..end] {
            return Ok(false);
        }
        cursor = end;
    }
    Ok(true)
}

fn read_v3_metadata(
    reader: &mut (impl Read + Seek),
    physical_bytes: u64,
    actual: String,
) -> Result<(SegmentDescriptor, Vec<BlockDescriptor>)> {
    let mut header = [0u8; V3_HEADER_BYTES];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut header)?;
    if &header[..8] != SEGMENT_V3_MAGIC
        || u16::from_be_bytes(header[8..10].try_into().unwrap()) != SEGMENT_FORMAT_VERSION
    {
        return invalid("v3 segment magic or version does not match");
    }
    if u16::from_be_bytes(header[10..12].try_into().unwrap()) as usize != V3_HEADER_BYTES
        || u32::from_be_bytes(header[12..16].try_into().unwrap()) != 1
    {
        return invalid("unknown v3 header length or compression flags");
    }
    let entries = u64::from_be_bytes(header[16..24].try_into().unwrap());
    let minimum_sequence = u64::from_be_bytes(header[24..32].try_into().unwrap());
    let maximum_sequence = u64::from_be_bytes(header[32..40].try_into().unwrap());
    let declared_record_bytes = u64::from_be_bytes(header[40..48].try_into().unwrap());
    let index_offset = u64::from_be_bytes(header[48..56].try_into().unwrap());
    let block_count = u32::from_be_bytes(header[56..60].try_into().unwrap()) as usize;
    let target = u32::from_be_bytes(header[60..64].try_into().unwrap()) as usize;
    let content_end = physical_bytes
        .checked_sub(FOOTER_BYTES as u64)
        .ok_or_else(|| Error::InvalidSegment("v3 footer underflow".into()))?;
    if entries == 0
        || minimum_sequence == 0
        || minimum_sequence > maximum_sequence
        || block_count == 0
        || target != SEGMENT_BLOCK_TARGET_BYTES
        || index_offset < V3_HEADER_BYTES as u64
        || index_offset >= content_end
        || declared_record_bytes > MAX_SEGMENT_BYTES
    {
        return invalid("invalid v3 header contract");
    }
    let index_bytes = usize::try_from(content_end - index_offset)
        .map_err(|_| Error::InvalidSegment("v3 index length exceeds usize".into()))?;
    if !(INDEX_HEADER_BYTES..=MAX_INDEX_BYTES).contains(&index_bytes) {
        return invalid("v3 index exceeds its bounded contract");
    }
    if block_count > (index_bytes - INDEX_HEADER_BYTES) / INDEX_ENTRY_BYTES {
        return invalid("v3 block count exceeds the bounded index");
    }
    reader.seek(SeekFrom::Start(index_offset))?;
    let mut index = vec![0u8; index_bytes];
    reader.read_exact(&mut index)?;
    if &index[..8] != INDEX_V3_MAGIC
        || u32::from_be_bytes(index[8..12].try_into().unwrap()) as usize != block_count
        || index[12..16] != [0, 0, 0, 0]
    {
        return invalid("invalid v3 index header");
    }
    let mut cursor = INDEX_HEADER_BYTES;
    let mut blocks = Vec::with_capacity(block_count);
    let mut prior_end = V3_HEADER_BYTES as u64;
    let mut prior_last_key: Option<Vec<u8>> = None;
    for _ in 0..block_count {
        let fixed_end = cursor
            .checked_add(INDEX_ENTRY_BYTES)
            .ok_or_else(|| Error::InvalidSegment("v3 index overflow".into()))?;
        let fixed = index
            .get(cursor..fixed_end)
            .ok_or_else(|| Error::InvalidSegment("truncated v3 index entry".into()))?;
        let offset = u64::from_be_bytes(fixed[0..8].try_into().unwrap());
        let physical = usize::try_from(u64::from_be_bytes(fixed[8..16].try_into().unwrap()))
            .map_err(|_| Error::InvalidSegment("v3 block length exceeds usize".into()))?;
        let record_bytes =
            usize::try_from(u64::from_be_bytes(fixed[16..24].try_into().unwrap()))
                .map_err(|_| Error::InvalidSegment("v3 record length exceeds usize".into()))?;
        let block_entries = u64::from_be_bytes(fixed[24..32].try_into().unwrap());
        let key_len = u32::from_be_bytes(fixed[32..36].try_into().unwrap()) as usize;
        if fixed[36..40] != [0, 0, 0, 0]
            || physical < 4
            || record_bytes == 0
            || record_bytes > MAX_DECODED_BLOCK_BYTES
            || block_entries == 0
            || key_len == 0
            || key_len > MAX_KEY_BYTES
            || offset != prior_end
            || offset
                .checked_add(physical as u64)
                .is_none_or(|end| end > index_offset)
        {
            return invalid("invalid v3 block descriptor");
        }
        let digest = decode_sha256_hex(&fixed[40..104])?;
        let key_end = fixed_end
            .checked_add(key_len)
            .ok_or_else(|| Error::InvalidSegment("v3 last key overflow".into()))?;
        let last_key = index
            .get(fixed_end..key_end)
            .ok_or_else(|| Error::InvalidSegment("truncated v3 last key".into()))?
            .to_vec();
        if prior_last_key
            .as_ref()
            .is_some_and(|prior| prior > &last_key)
        {
            return invalid("v3 block last keys are not ordered");
        }
        prior_end = offset + physical as u64;
        prior_last_key = Some(last_key.clone());
        blocks.push(BlockDescriptor {
            offset,
            physical_bytes: physical,
            record_bytes,
            entries: block_entries,
            digest,
            last_key,
        });
        cursor = key_end;
    }
    if cursor != index.len() || prior_end != index_offset {
        return invalid("v3 block table does not exactly cover the data region");
    }
    let descriptor = SegmentDescriptor {
        id: actual.clone(),
        level: 0,
        first_key: Vec::new(),
        last_key: Vec::new(),
        minimum_sequence,
        maximum_sequence,
        entries,
        bytes: physical_bytes,
        checksum: actual,
    };
    validate_v3_blocks(reader, descriptor, blocks, declared_record_bytes)
}

fn validate_v3_blocks(
    reader: &mut (impl Read + Seek),
    mut descriptor: SegmentDescriptor,
    blocks: Vec<BlockDescriptor>,
    declared_record_bytes: u64,
) -> Result<(SegmentDescriptor, Vec<BlockDescriptor>)> {
    let mut observed_entries = 0u64;
    let mut observed_bytes = 0u64;
    let mut previous: Option<(Vec<u8>, u64)> = None;
    let mut first_key = None;
    let mut last_key = None;
    for block in &blocks {
        reader.seek(SeekFrom::Start(block.offset))?;
        let mut compressed = vec![0u8; block.physical_bytes];
        reader.read_exact(&mut compressed)?;
        let records = decode_compressed_block(&compressed, block)?;
        let mut cursor = 0;
        let mut block_entries = 0u64;
        let mut block_last = None;
        while cursor < records.len() {
            let record = parse_record(
                &records,
                cursor,
                records.len(),
                descriptor.minimum_sequence,
                descriptor.maximum_sequence,
            )?;
            validate_order(&mut previous, &record)?;
            first_key.get_or_insert_with(|| record.key.to_vec());
            last_key = Some(record.key.to_vec());
            block_last = Some(record.key.to_vec());
            block_entries += 1;
            cursor = record.next;
        }
        if block_entries != block.entries
            || block_last.as_deref() != Some(block.last_key.as_slice())
        {
            return invalid("v3 block index disagrees with block contents");
        }
        observed_entries = observed_entries
            .checked_add(block_entries)
            .ok_or_else(|| Error::InvalidSegment("v3 entry count overflow".into()))?;
        observed_bytes = observed_bytes
            .checked_add(records.len() as u64)
            .ok_or_else(|| Error::InvalidSegment("v3 record byte count overflow".into()))?;
    }
    if observed_entries != descriptor.entries || observed_bytes != declared_record_bytes {
        return invalid("v3 header counts disagree with block contents");
    }
    descriptor.first_key = first_key.expect("validated v3 is non-empty");
    descriptor.last_key = last_key.expect("validated v3 is non-empty");
    Ok((descriptor, blocks))
}

fn build_blocked_segment(
    descriptor: SegmentDescriptor,
    source: BlockSource,
    blocks: Vec<BlockDescriptor>,
    cache: SharedBlockCache,
) -> Result<Segment> {
    Ok(Segment {
        descriptor,
        storage: SegmentStorage::Blocked {
            source,
            blocks,
            cache,
            cache_id: CACHE_ID.fetch_add(1, Ordering::Relaxed),
        },
    })
}

fn read_and_decode_block(source: &BlockSource, block: &BlockDescriptor) -> Result<Vec<u8>> {
    let mut compressed = vec![0u8; block.physical_bytes];
    match source {
        BlockSource::File(file) => {
            let mut file = file
                .lock()
                .map_err(|_| Error::InvalidSegment("segment file lock poisoned".into()))?;
            file.seek(SeekFrom::Start(block.offset))?;
            file.read_exact(&mut compressed)?;
        }
        BlockSource::Bytes(bytes) => {
            let start = usize::try_from(block.offset)
                .map_err(|_| Error::InvalidSegment("v3 block offset exceeds usize".into()))?;
            let end = start
                .checked_add(block.physical_bytes)
                .ok_or_else(|| Error::InvalidSegment("v3 block range overflow".into()))?;
            compressed.copy_from_slice(
                bytes
                    .get(start..end)
                    .ok_or_else(|| Error::InvalidSegment("v3 block range is absent".into()))?,
            );
        }
    }
    decode_compressed_block(&compressed, block)
}

fn decode_compressed_block(compressed: &[u8], block: &BlockDescriptor) -> Result<Vec<u8>> {
    if ring::digest::digest(&ring::digest::SHA256, compressed).as_ref() != block.digest {
        return invalid("v3 block checksum does not match");
    }
    let prefix = compressed
        .get(..4)
        .ok_or_else(|| Error::InvalidSegment("v3 compressed block has no size prefix".into()))?;
    let prefixed = u32::from_le_bytes(prefix.try_into().unwrap()) as usize;
    if prefixed != block.record_bytes || prefixed > MAX_DECODED_BLOCK_BYTES {
        return invalid("v3 declared and compressed block lengths differ");
    }
    let decoded = decompress_size_prepended(compressed)
        .map_err(|error| Error::InvalidSegment(format!("v3 LZ4 decode failed: {error}")))?;
    if decoded.len() != block.record_bytes {
        return invalid("v3 decoded block length differs from its index");
    }
    Ok(decoded)
}

fn encode_sha256_hex(digest: [u8; 32]) -> [u8; 64] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = [0u8; 64];
    for (index, byte) in digest.into_iter().enumerate() {
        encoded[index * 2] = HEX[(byte >> 4) as usize];
        encoded[index * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    encoded
}

fn decode_sha256_hex(encoded: &[u8]) -> Result<[u8; 32]> {
    if encoded.len() != 64 {
        return invalid("v3 block digest has the wrong length");
    }
    let mut digest = [0u8; 32];
    for (index, pair) in encoded.chunks_exact(2).enumerate() {
        digest[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Ok(digest)
}

fn decode_hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => invalid("v3 block digest is not lowercase hexadecimal"),
    }
}

fn decode_legacy(bytes: Vec<u8>) -> Result<Segment> {
    if bytes.len() < V1_HEADER_BYTES + FOOTER_BYTES {
        return invalid("segment is shorter than its header and footer");
    }
    let version = u16::from_be_bytes(bytes[8..10].try_into().unwrap());
    let (header_bytes, compressed) = match (&bytes[0..8], version) {
        (magic, 1) if magic == SEGMENT_V1_MAGIC => {
            let header_len = u16::from_be_bytes(bytes[10..12].try_into().unwrap());
            if header_len as usize != V1_HEADER_BYTES || bytes[12..16] != [0, 0, 0, 0] {
                return invalid("unknown v1 segment header length or flags");
            }
            (V1_HEADER_BYTES, false)
        }
        (magic, 2) if magic == SEGMENT_V2_MAGIC => {
            if bytes.len() < V2_HEADER_BYTES + FOOTER_BYTES {
                return invalid("v2 segment is shorter than its header and footer");
            }
            let header_len = u16::from_be_bytes(bytes[10..12].try_into().unwrap());
            if header_len as usize != V2_HEADER_BYTES || bytes[12..16] != [0, 0, 0, 1] {
                return invalid("unknown v2 segment header length or compression flags");
            }
            (V2_HEADER_BYTES, true)
        }
        (_, 1 | 2) => return invalid("segment magic does not match version"),
        _ => {
            return Err(Error::UnsupportedVersion {
                object: "segment",
                version,
            })
        }
    };
    let entries = u64::from_be_bytes(bytes[16..24].try_into().unwrap());
    let minimum_sequence = u64::from_be_bytes(bytes[24..32].try_into().unwrap());
    let maximum_sequence = u64::from_be_bytes(bytes[32..40].try_into().unwrap());
    if entries == 0 || minimum_sequence == 0 || minimum_sequence > maximum_sequence {
        return invalid("invalid segment count or sequence range");
    }
    let physical_content_end = bytes.len() - FOOTER_BYTES;
    let expected = std::str::from_utf8(&bytes[physical_content_end..])
        .map_err(|_| Error::InvalidSegment("segment footer is not ASCII".into()))?;
    let actual = digest::sha256_hex(&bytes[..physical_content_end]);
    if expected != actual {
        return invalid("segment content checksum does not match");
    }
    let records = if compressed {
        let declared = u64::from_be_bytes(bytes[40..48].try_into().unwrap());
        if declared > MAX_SEGMENT_BYTES {
            return invalid("v2 uncompressed records exceed the 1 GiB safety limit");
        }
        let compressed = &bytes[header_bytes..physical_content_end];
        let prefixed = compressed
            .get(..4)
            .ok_or_else(|| Error::InvalidSegment("v2 compressed body has no size prefix".into()))?;
        if u64::from(u32::from_le_bytes(prefixed.try_into().unwrap())) != declared {
            return invalid("v2 declared and compressed record lengths differ");
        }
        decompress_size_prepended(compressed)
            .map_err(|error| Error::InvalidSegment(format!("v2 LZ4 decode failed: {error}")))?
    } else {
        bytes[header_bytes..physical_content_end].to_vec()
    };
    let (first_key, last_key, sparse_index) =
        validate_legacy_records(&records, entries, minimum_sequence, maximum_sequence)?;
    Ok(Segment {
        descriptor: SegmentDescriptor {
            id: actual.clone(),
            level: 0,
            first_key,
            last_key,
            minimum_sequence,
            maximum_sequence,
            entries,
            bytes: bytes.len() as u64,
            checksum: actual,
        },
        storage: SegmentStorage::Legacy {
            records,
            sparse_index,
        },
    })
}

fn validate_legacy_records(
    records: &[u8],
    entries: u64,
    minimum_sequence: u64,
    maximum_sequence: u64,
) -> Result<(Vec<u8>, Vec<u8>, Vec<SparseEntry>)> {
    let mut cursor = 0;
    let mut previous = None;
    let mut sparse_index = Vec::new();
    let mut keys_since_index = SPARSE_INDEX_STRIDE;
    let mut first_key = None;
    let mut last_key = None;
    for _ in 0..entries {
        let offset = cursor;
        let record = parse_record(
            records,
            cursor,
            records.len(),
            minimum_sequence,
            maximum_sequence,
        )?;
        let new_key = previous
            .as_ref()
            .is_none_or(|(key, _): &(Vec<u8>, u64)| key.as_slice() != record.key);
        validate_order(&mut previous, &record)?;
        if new_key {
            first_key.get_or_insert_with(|| record.key.to_vec());
            last_key = Some(record.key.to_vec());
            if keys_since_index >= SPARSE_INDEX_STRIDE {
                sparse_index.push(SparseEntry {
                    offset,
                    key: record.key.to_vec(),
                });
                keys_since_index = 0;
            }
            keys_since_index += 1;
        }
        cursor = record.next;
    }
    if cursor != records.len() {
        return invalid("segment contains trailing bytes before its footer");
    }
    Ok((first_key.unwrap(), last_key.unwrap(), sparse_index))
}

fn validate_order(previous: &mut Option<(Vec<u8>, u64)>, record: &Record<'_>) -> Result<()> {
    if previous.as_ref().is_some_and(|(key, sequence)| {
        record.key < key.as_slice()
            || (record.key == key.as_slice() && record.sequence <= *sequence)
    }) {
        return invalid("segment records are not in canonical key/sequence order");
    }
    *previous = Some((record.key.to_vec(), record.sequence));
    Ok(())
}

fn legacy_seek(records: &[u8], sparse: &[SparseEntry], key: &[u8]) -> usize {
    let position = sparse.partition_point(|entry| entry.key.as_slice() <= key);
    sparse
        .get(position.saturating_sub(1))
        .map_or(0, |entry| entry.offset.min(records.len()))
}

fn select_version(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    key: &[u8],
    read_sequence: u64,
) -> Result<Option<SegmentVersion>> {
    let mut selected = None;
    while cursor < end {
        let record = parse_record(bytes, cursor, end, 1, u64::MAX)?;
        match record.key.cmp(key) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal if record.sequence <= read_sequence => {
                selected = Some(SegmentVersion {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                })
            }
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Greater => break,
        }
        cursor = record.next;
    }
    Ok(selected)
}

fn select_version_block(
    block: &DecodedBlock,
    key: &[u8],
    read_sequence: u64,
) -> Result<Option<SegmentVersion>> {
    let mut selected = None;
    for offset in block.record_offsets.iter().skip(block.lower_bound(key)?) {
        let record = parse_record(
            &block.bytes,
            *offset as usize,
            block.bytes.len(),
            1,
            u64::MAX,
        )?;
        match record.key.cmp(key) {
            std::cmp::Ordering::Equal if record.sequence <= read_sequence => {
                selected = Some(SegmentVersion {
                    sequence: record.sequence,
                    value: record.value.map(<[u8]>::to_vec),
                });
            }
            std::cmp::Ordering::Equal => {}
            _ => break,
        }
    }
    Ok(selected)
}

fn visit_records(bytes: &[u8], visit: &mut impl FnMut(Record<'_>) -> Result<()>) -> Result<()> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        let record = parse_record(bytes, cursor, bytes.len(), 1, u64::MAX)?;
        visit(record)?;
        cursor = record.next;
    }
    Ok(())
}

fn visit_decoded_records(
    block: &DecodedBlock,
    first: usize,
    visit: &mut impl FnMut(Record<'_>) -> Result<()>,
) -> Result<()> {
    for offset in block.record_offsets.iter().skip(first) {
        visit(parse_record(
            &block.bytes,
            *offset as usize,
            block.bytes.len(),
            1,
            u64::MAX,
        )?)?;
    }
    Ok(())
}

fn parse_record(
    bytes: &[u8],
    offset: usize,
    content_end: usize,
    minimum_sequence: u64,
    maximum_sequence: u64,
) -> Result<Record<'_>> {
    let header_end = offset
        .checked_add(RECORD_HEADER_BYTES)
        .ok_or_else(|| Error::InvalidSegment("record header overflow".into()))?;
    let header = bytes
        .get(offset..header_end)
        .filter(|_| header_end <= content_end)
        .ok_or_else(|| Error::InvalidSegment("incomplete segment record header".into()))?;
    let kind = header[0];
    if header[1..4] != [0, 0, 0] {
        return invalid("unknown segment record flags");
    }
    let key_len = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    let value_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let sequence = u64::from_be_bytes(header[12..20].try_into().unwrap());
    let end = header_end
        .checked_add(key_len)
        .and_then(|value| value.checked_add(value_len))
        .ok_or_else(|| Error::InvalidSegment("record length overflow".into()))?;
    let body = bytes
        .get(header_end..end)
        .filter(|_| end <= content_end)
        .ok_or_else(|| Error::InvalidSegment("incomplete segment record body".into()))?;
    if key_len == 0
        || key_len > MAX_KEY_BYTES
        || value_len > MAX_VALUE_BYTES
        || sequence < minimum_sequence
        || sequence > maximum_sequence
    {
        return invalid("invalid segment key, value, or record sequence");
    }
    let key = &body[..key_len];
    let stored_value = &body[key_len..];
    let value = match kind {
        1 => Some(stored_value),
        2 if stored_value.is_empty() => None,
        2 => return invalid("segment tombstone carries a value"),
        _ => return invalid(format!("unknown segment record kind {kind}")),
    };
    Ok(Record {
        key,
        value,
        sequence,
        next: end,
    })
}

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidSegment(reason.into()))
}
