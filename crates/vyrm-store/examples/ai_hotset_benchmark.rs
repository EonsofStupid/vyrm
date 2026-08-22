//! Isolated Fjall/vyrmKV read profiles for an AI runtime's control and metadata sets.
//!
//! Setup is deliberately excluded from measurement: a cold immutable corpus is
//! published first, then a small set of routing, lease, cursor, and outcome-like
//! keys is overwritten in the active memtable. The measured phase selects one
//! explicit point or metadata-fan-out workload. These are physical profiles,
//! not a general database benchmark.

use fjall::{
    KeyspaceCreateOptions, PersistMode, Readable, SingleWriterTxDatabase, SingleWriterTxKeyspace,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};
use vyrm_store::{measure_storage_footprint, FootprintBytes, StorageFootprint};

const FORMAT_VERSION: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Workload {
    CurrentHotHit,
    ColdHit,
    PointMiss,
    HistoricalHotHit,
    MetadataFanout,
}

impl Workload {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "current-hot-hit" => Ok(Self::CurrentHotHit),
            "cold-hit" => Ok(Self::ColdHit),
            "point-miss" => Ok(Self::PointMiss),
            "historical-hot-hit" => Ok(Self::HistoricalHotHit),
            "metadata-fanout" => Ok(Self::MetadataFanout),
            _ => Err(format!(
                "unknown workload {value:?}; expected current-hot-hit, cold-hit, point-miss, historical-hot-hit, or metadata-fanout"
            )),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::CurrentHotHit => "current_hot_hit",
            Self::ColdHit => "cold_hit",
            Self::PointMiss => "point_miss",
            Self::HistoricalHotHit => "historical_hot_hit",
            Self::MetadataFanout => "metadata_fanout",
        }
    }

    const fn cli_str(self) -> &'static str {
        match self {
            Self::CurrentHotHit => "current-hot-hit",
            Self::ColdHit => "cold-hit",
            Self::PointMiss => "point-miss",
            Self::HistoricalHotHit => "historical-hot-hit",
            Self::MetadataFanout => "metadata-fanout",
        }
    }

    const fn uses_historical_snapshot(self) -> bool {
        matches!(self, Self::HistoricalHotHit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PayloadProfile {
    RepeatedByte,
    StructuredJson,
    DeterministicEntropy,
    EmbeddingF32,
}

impl PayloadProfile {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "repeated-byte" => Ok(Self::RepeatedByte),
            "structured-json" => Ok(Self::StructuredJson),
            "deterministic-entropy" => Ok(Self::DeterministicEntropy),
            "embedding-f32" => Ok(Self::EmbeddingF32),
            _ => Err(format!(
                "unknown payload profile {value:?}; expected repeated-byte, structured-json, deterministic-entropy, or embedding-f32"
            )),
        }
    }

    const fn cli_str(self) -> &'static str {
        match self {
            Self::RepeatedByte => "repeated-byte",
            Self::StructuredJson => "structured-json",
            Self::DeterministicEntropy => "deterministic-entropy",
            Self::EmbeddingF32 => "embedding-f32",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    workload: Workload,
    payload_profile: PayloadProfile,
    trials: usize,
    cold_keys: usize,
    hot_keys: usize,
    reads: usize,
    batch_size: usize,
    value_bytes: usize,
    fanout_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workload: Workload::CurrentHotHit,
            payload_profile: PayloadProfile::RepeatedByte,
            trials: 5,
            cold_keys: 8_192,
            hot_keys: 128,
            reads: 65_536,
            batch_size: 128,
            value_bytes: 128,
            fanout_width: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Latency {
    samples: usize,
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
    maximum_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Trial {
    backend: String,
    correctness_verified: bool,
    reads_per_second: f64,
    items_per_sample: usize,
    latency: Latency,
    footprint: LifecycleFootprint,
}

struct ReadMeasurement {
    correctness_verified: bool,
    reads_per_second: f64,
    items_per_sample: usize,
    latency: Latency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LifecycleFootprint {
    logical_live_payload_bytes: u64,
    logical_written_payload_bytes: u64,
    active: StorageFootprint,
    reopened: StorageFootprint,
    maintained: StorageFootprint,
    maintained_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Evidence {
    format_version: u16,
    measured_at_unix_ms: u128,
    architecture: String,
    operating_system: String,
    workload: String,
    aggregation: &'static str,
    throughput_unit: &'static str,
    latency_unit: &'static str,
    footprint_contract: &'static str,
    config: Config,
    fjall_version: &'static str,
    fjall_trials: Vec<Trial>,
    native_trials: Vec<Trial>,
    fjall: Trial,
    native: Trial,
    native_to_fjall_read_throughput: f64,
    native_to_fjall_p95_latency: f64,
    footprint_comparison: FootprintComparison,
    promotion: PromotionVerdict,
}

#[derive(Debug, Clone, Serialize)]
struct FootprintComparison {
    promotion_state: &'static str,
    native_to_fjall_reopened_apparent: f64,
    native_to_fjall_reopened_allocated: Option<f64>,
    fjall_reopened_allocated_to_live_payload: Option<f64>,
    native_reopened_allocated_to_live_payload: Option<f64>,
    maintained_cross_backend_comparable: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PromotionVerdict {
    passes: bool,
    failures: Vec<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("AI hot-set benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|value| value == "--child") {
        return run_child(&arguments);
    }
    let (config, output, require_promotion) = parse(&arguments)?;
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut fjall_trials = Vec::with_capacity(config.trials);
    let mut native_trials = Vec::with_capacity(config.trials);
    for trial in 0..config.trials {
        let trial_root = root.path().join(format!("trial-{trial}"));
        fs::create_dir_all(&trial_root).map_err(|error| error.to_string())?;
        let mut measure = |backend: &str| -> Result<(), String> {
            let result = launch_child(backend, &trial_root.join(backend), &config)?;
            if backend == "fjall" {
                fjall_trials.push(result);
            } else {
                native_trials.push(result);
            }
            Ok(())
        };
        if trial % 2 == 0 {
            measure("fjall")?;
            measure("native")?;
        } else {
            measure("native")?;
            measure("fjall")?;
        }
    }
    let fjall = aggregate("fjall", &fjall_trials);
    let native = aggregate("native", &native_trials);
    if !fjall.correctness_verified || !native.correctness_verified {
        return Err("one or more isolated trials failed correctness".into());
    }
    let throughput_ratio = native.reads_per_second / fjall.reads_per_second;
    let p95_ratio = native.latency.p95_ns as f64 / fjall.latency.p95_ns.max(1) as f64;
    let footprint_comparison = footprint_comparison(&fjall, &native);
    let promotion = promotion(
        &fjall,
        &native,
        throughput_ratio,
        p95_ratio,
        &footprint_comparison,
    );
    let evidence = Evidence {
        format_version: FORMAT_VERSION,
        measured_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis(),
        architecture: std::env::consts::ARCH.into(),
        operating_system: std::env::consts::OS.into(),
        workload: config.workload.as_str().into(),
        aggregation:
            "median of per-trial metrics; latency percentiles are medians of each isolated trial's percentile",
        throughput_unit: "resolved key items per second",
        latency_unit: "nanoseconds per point request or complete fan-out request",
        footprint_contract: "active is measured while the read snapshot is open; reopened follows a clean close/open with no explicit maintenance; maintained follows backend-native flush, major compaction, unreachable-file collection where available, and a second clean reopen; promotion does not compare backend-native maintained states",
        config,
        fjall_version: "3.1.8",
        native_to_fjall_read_throughput: throughput_ratio,
        native_to_fjall_p95_latency: p95_ratio,
        footprint_comparison,
        promotion,
        fjall_trials,
        native_trials,
        fjall,
        native,
    };
    let bytes = serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?;
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(output, &bytes).map_err(|error| error.to_string())?;
    }
    println!(
        "{}",
        String::from_utf8(bytes).map_err(|error| error.to_string())?
    );
    if require_promotion && !evidence.promotion.passes {
        return Err(format!(
            "strict AI-read promotion gate failed: {}",
            evidence.promotion.failures.join("; ")
        ));
    }
    Ok(())
}

fn parse(arguments: &[String]) -> Result<(Config, Option<PathBuf>, bool), String> {
    let mut config = Config::default();
    let mut output = None;
    let mut require_promotion = false;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--require-promotion" {
            require_promotion = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("missing value after {}", arguments[index]))?;
        let parsed = || {
            value
                .parse::<usize>()
                .map_err(|error| format!("invalid value for {}: {error}", arguments[index]))
        };
        match arguments[index].as_str() {
            "--workload" => config.workload = Workload::parse(value)?,
            "--payload-profile" => config.payload_profile = PayloadProfile::parse(value)?,
            "--trials" => config.trials = parsed()?,
            "--cold-keys" => config.cold_keys = parsed()?,
            "--hot-keys" => config.hot_keys = parsed()?,
            "--reads" => config.reads = parsed()?,
            "--batch-size" => config.batch_size = parsed()?,
            "--value-bytes" => config.value_bytes = parsed()?,
            "--fanout-width" => config.fanout_width = parsed()?,
            "--output" => output = Some(PathBuf::from(value)),
            unknown => return Err(format!("unknown argument {unknown}")),
        }
        index += 2;
    }
    if [
        config.trials,
        config.cold_keys,
        config.hot_keys,
        config.reads,
        config.batch_size,
        config.value_bytes,
        config.fanout_width,
    ]
    .contains(&0)
    {
        return Err("all numeric values must be greater than zero".into());
    }
    if config.hot_keys > config.cold_keys || config.batch_size > config.cold_keys {
        return Err("hot keys and batch size cannot exceed the cold corpus".into());
    }
    if config.workload == Workload::ColdHit && config.hot_keys == config.cold_keys {
        return Err("cold-hit requires at least one immutable key outside the hot set".into());
    }
    if config.workload == Workload::MetadataFanout && config.hot_keys == config.cold_keys {
        return Err("metadata-fanout requires immutable keys outside the hot set".into());
    }
    if config.payload_profile == PayloadProfile::StructuredJson && config.value_bytes < 64 {
        return Err("structured-json requires at least 64 value bytes".into());
    }
    if config.payload_profile == PayloadProfile::EmbeddingF32 && config.value_bytes % 4 != 0 {
        return Err("embedding-f32 value bytes must be divisible by four".into());
    }
    Ok((config, output, require_promotion))
}

fn launch_child(backend: &str, path: &Path, config: &Config) -> Result<Trial, String> {
    let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args(["--child", backend, "--path"])
        .arg(path)
        .args(["--trials", "1"])
        .args(["--workload", config.workload.cli_str()])
        .args(["--payload-profile", config.payload_profile.cli_str()])
        .args(["--cold-keys", &config.cold_keys.to_string()])
        .args(["--hot-keys", &config.hot_keys.to_string()])
        .args(["--reads", &config.reads.to_string()])
        .args(["--batch-size", &config.batch_size.to_string()])
        .args(["--value-bytes", &config.value_bytes.to_string()])
        .args(["--fanout-width", &config.fanout_width.to_string()])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{backend} child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn run_child(arguments: &[String]) -> Result<(), String> {
    let backend = arguments
        .get(1)
        .ok_or_else(|| "child backend is required".to_owned())?;
    if arguments.get(2).map(String::as_str) != Some("--path") {
        return Err("child path is required".into());
    }
    let path = PathBuf::from(
        arguments
            .get(3)
            .ok_or_else(|| "child path value is required".to_owned())?,
    );
    let (config, output, require_promotion) = parse(&arguments[4..])?;
    if output.is_some() {
        return Err("child output path is not supported".into());
    }
    if require_promotion {
        return Err("child cannot require a promotion verdict".into());
    }
    let trial = match backend.as_str() {
        "fjall" => run_fjall(&path, &config)?,
        "native" => run_native(&path, &config)?,
        _ => return Err(format!("unknown child backend {backend}")),
    };
    println!(
        "{}",
        serde_json::to_string(&trial).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn run_fjall(path: &Path, config: &Config) -> Result<Trial, String> {
    let database = SingleWriterTxDatabase::builder(path)
        .manual_journal_persist(true)
        .open()
        .map_err(|error| error.to_string())?;
    let keyspace = database
        .keyspace("runtime", KeyspaceCreateOptions::default)
        .map_err(|error| error.to_string())?;
    for start in (0..config.cold_keys).step_by(config.batch_size) {
        let mut transaction = database.write_tx().durability(Some(PersistMode::SyncAll));
        for index in start..(start + config.batch_size).min(config.cold_keys) {
            transaction.insert(&keyspace, key(index), value(config, index, false));
        }
        transaction.commit().map_err(|error| error.to_string())?;
    }
    keyspace
        .as_ref()
        .rotate_memtable_and_wait()
        .map_err(|error| error.to_string())?;
    let historical = database.read_tx();
    let mut transaction = database.write_tx().durability(Some(PersistMode::SyncAll));
    for index in 0..config.hot_keys {
        transaction.insert(&keyspace, key(index), value(config, index, true));
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let current = database.read_tx();
    let snapshot = if config.workload.uses_historical_snapshot() {
        &historical
    } else {
        &current
    };
    let measured = if config.workload == Workload::MetadataFanout {
        measure_fanout(config, |keys| {
            keys.iter()
                .map(|key| {
                    snapshot
                        .get(&keyspace, key)
                        .map(|value| value.map(|value| value.to_vec()))
                        .map_err(|error| error.to_string())
                })
                .collect()
        })
    } else {
        measure(config, |index| {
            Ok(snapshot
                .get(&keyspace, key(index))
                .map_err(|error| error.to_string())?
                .map(|value| value.to_vec()))
        })
    }?;
    let active = storage_footprint(path)?;
    drop(current);
    drop(historical);
    drop(keyspace);
    drop(database);

    let reopened_database = SingleWriterTxDatabase::builder(path)
        .manual_journal_persist(true)
        .open()
        .map_err(|error| error.to_string())?;
    let reopened_keyspace = reopened_database
        .keyspace("runtime", KeyspaceCreateOptions::default)
        .map_err(|error| error.to_string())?;
    verify_fjall_current(&reopened_database, &reopened_keyspace, config)?;
    let reopened = storage_footprint(path)?;
    reopened_keyspace
        .as_ref()
        .rotate_memtable_and_wait()
        .map_err(|error| error.to_string())?;
    reopened_keyspace
        .as_ref()
        .major_compact()
        .map_err(|error| error.to_string())?;
    reopened_database
        .persist(PersistMode::SyncAll)
        .map_err(|error| error.to_string())?;
    drop(reopened_keyspace);
    drop(reopened_database);

    let maintained_database = SingleWriterTxDatabase::builder(path)
        .manual_journal_persist(true)
        .open()
        .map_err(|error| error.to_string())?;
    let maintained_keyspace = maintained_database
        .keyspace("runtime", KeyspaceCreateOptions::default)
        .map_err(|error| error.to_string())?;
    verify_fjall_current(&maintained_database, &maintained_keyspace, config)?;
    let maintained = storage_footprint(path)?;

    Ok(finish_trial(
        "fjall",
        measured,
        lifecycle_footprint(
            config,
            active,
            reopened,
            maintained,
            vec![
                "flush_active_memtable".into(),
                "major_compact".into(),
                "persist_and_clean_reopen".into(),
            ],
        )?,
    ))
}

fn run_native(path: &Path, config: &Config) -> Result<Trial, String> {
    let mut database = Database::create(path).map_err(|error| error.to_string())?;
    for start in (0..config.cold_keys).step_by(config.batch_size) {
        let operations = (start..(start + config.batch_size).min(config.cold_keys))
            .map(|index| Mutation::Put {
                key: key(index),
                value: value(config, index, false),
            })
            .collect();
        database
            .write_owned(
                WriteBatch::new(operations).map_err(|error| error.to_string())?,
                Durability::Authoritative,
            )
            .map_err(|error| error.to_string())?;
    }
    database
        .flush_memtable(1)
        .map_err(|error| error.to_string())?;
    let historical = database.snapshot();
    let operations = (0..config.hot_keys)
        .map(|index| Mutation::Put {
            key: key(index),
            value: value(config, index, true),
        })
        .collect();
    database
        .write_owned(
            WriteBatch::new(operations).map_err(|error| error.to_string())?,
            Durability::Authoritative,
        )
        .map_err(|error| error.to_string())?;
    let current = database.snapshot();
    let snapshot = if config.workload.uses_historical_snapshot() {
        historical
    } else {
        current
    };
    let measured = if config.workload == Workload::MetadataFanout {
        measure_fanout(config, |keys| {
            database
                .get_many(keys, snapshot)
                .map_err(|error| error.to_string())
        })
    } else {
        measure(config, |index| {
            database
                .get(&key(index), snapshot)
                .map_err(|error| error.to_string())
        })
    }?;
    let active = storage_footprint(path)?;
    drop(database);

    let mut reopened_database = Database::open(path).map_err(|error| error.to_string())?;
    verify_native_current(&reopened_database, config)?;
    let reopened = storage_footprint(path)?;
    reopened_database
        .flush_memtable(2)
        .map_err(|error| error.to_string())?;
    reopened_database
        .compact(&[], 3)
        .map_err(|error| error.to_string())?;
    reopened_database
        .garbage_collect()
        .map_err(|error| error.to_string())?;
    drop(reopened_database);

    let maintained_database = Database::open(path).map_err(|error| error.to_string())?;
    verify_native_current(&maintained_database, config)?;
    let maintained = storage_footprint(path)?;

    Ok(finish_trial(
        "native",
        measured,
        lifecycle_footprint(
            config,
            active,
            reopened,
            maintained,
            vec![
                "flush_active_memtable".into(),
                "compact_unpinned_history".into(),
                "collect_unreachable_files".into(),
                "clean_reopen".into(),
            ],
        )?,
    ))
}

fn measure(
    config: &Config,
    mut get: impl FnMut(usize) -> Result<Option<Vec<u8>>, String>,
) -> Result<ReadMeasurement, String> {
    for iteration in 0..config.hot_keys * 4 {
        black_box(get(workload_index(config, iteration))?);
    }
    let started = Instant::now();
    let mut samples = Vec::with_capacity(config.reads);
    let mut correctness_verified = true;
    for iteration in 0..config.reads {
        let index = workload_index(config, iteration.wrapping_mul(7_919));
        let read_started = Instant::now();
        let value = get(index)?;
        samples.push(read_started.elapsed());
        correctness_verified &= value == expected_value(config, index);
        black_box(value);
    }
    let elapsed = started.elapsed();
    Ok(ReadMeasurement {
        correctness_verified,
        reads_per_second: config.reads as f64 / elapsed.as_secs_f64(),
        items_per_sample: 1,
        latency: summarize(samples),
    })
}

fn measure_fanout(
    config: &Config,
    mut get_many: impl FnMut(&[Vec<u8>]) -> Result<Vec<Option<Vec<u8>>>, String>,
) -> Result<ReadMeasurement, String> {
    for iteration in 0..config.hot_keys * 4 {
        let keys = fanout_keys(config, iteration);
        black_box(get_many(&keys)?);
    }
    let started = Instant::now();
    let mut samples = Vec::with_capacity(config.reads);
    let mut correctness_verified = true;
    for iteration in 0..config.reads {
        let keys = fanout_keys(config, iteration.wrapping_mul(7_919));
        let expected = keys
            .iter()
            .map(|key| expected_fanout_value(config, key))
            .collect::<Vec<_>>();
        let read_started = Instant::now();
        let values = get_many(&keys)?;
        samples.push(read_started.elapsed());
        correctness_verified &= values == expected;
        black_box(values);
    }
    let elapsed = started.elapsed();
    Ok(ReadMeasurement {
        correctness_verified,
        reads_per_second: (config.reads * config.fanout_width) as f64 / elapsed.as_secs_f64(),
        items_per_sample: config.fanout_width,
        latency: summarize(samples),
    })
}

fn workload_index(config: &Config, iteration: usize) -> usize {
    match config.workload {
        Workload::CurrentHotHit | Workload::HistoricalHotHit => iteration % config.hot_keys,
        Workload::ColdHit => config.hot_keys + iteration % (config.cold_keys - config.hot_keys),
        Workload::PointMiss => config.cold_keys + 1 + iteration % config.cold_keys,
        Workload::MetadataFanout => unreachable!("metadata fan-out uses batched keys"),
    }
}

fn expected_value(config: &Config, index: usize) -> Option<Vec<u8>> {
    match config.workload {
        Workload::CurrentHotHit => Some(value(config, index, true)),
        Workload::ColdHit | Workload::HistoricalHotHit => Some(value(config, index, false)),
        Workload::PointMiss => None,
        Workload::MetadataFanout => unreachable!("metadata fan-out validates each batched key"),
    }
}

fn fanout_keys(config: &Config, iteration: usize) -> Vec<Vec<u8>> {
    let cold_span = config.cold_keys - config.hot_keys;
    (0..config.fanout_width)
        .map(|lane| {
            let mixed = iteration.wrapping_add(lane.wrapping_mul(1_009));
            let index = match lane % 4 {
                0 => mixed % config.hot_keys,
                1 | 2 => config.hot_keys + mixed % cold_span,
                _ => config.cold_keys + 1 + mixed % config.cold_keys,
            };
            key(index)
        })
        .collect()
}

fn expected_fanout_value(config: &Config, key_bytes: &[u8]) -> Option<Vec<u8>> {
    let suffix = key_bytes
        .strip_prefix(b"runtime:control:")
        .expect("benchmark generated key has its canonical prefix");
    let index = std::str::from_utf8(suffix)
        .expect("benchmark generated key suffix is UTF-8")
        .parse::<usize>()
        .expect("benchmark generated key suffix is numeric");
    if index < config.hot_keys {
        Some(value(config, index, true))
    } else if index < config.cold_keys {
        Some(value(config, index, false))
    } else {
        None
    }
}

fn key(index: usize) -> Vec<u8> {
    format!("runtime:control:{index:012}").into_bytes()
}

fn value(config: &Config, index: usize, hot: bool) -> Vec<u8> {
    match config.payload_profile {
        PayloadProfile::RepeatedByte => {
            vec![((index + if hot { 97 } else { 0 }) % 251) as u8; config.value_bytes]
        }
        PayloadProfile::StructuredJson => structured_json_value(config, index, hot),
        PayloadProfile::DeterministicEntropy => deterministic_bytes(config, index, hot),
        PayloadProfile::EmbeddingF32 => embedding_value(config, index, hot),
    }
}

fn structured_json_value(config: &Config, index: usize, hot: bool) -> Vec<u8> {
    let prefix = format!(
        "{{\"id\":{index:012},\"generation\":{},\"payload\":\"",
        u8::from(hot)
    );
    let suffix = b"\"}";
    let mut output = prefix.into_bytes();
    assert!(output.len() + suffix.len() <= config.value_bytes);
    let mut state = value_seed(index, hot);
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    while output.len() + suffix.len() < config.value_bytes {
        state = next_state(state);
        output.push(ALPHABET[(state as usize) % ALPHABET.len()]);
    }
    output.extend_from_slice(suffix);
    output
}

fn deterministic_bytes(config: &Config, index: usize, hot: bool) -> Vec<u8> {
    let mut state = value_seed(index, hot);
    (0..config.value_bytes)
        .map(|_| {
            state = next_state(state);
            (state >> 56) as u8
        })
        .collect()
}

fn embedding_value(config: &Config, index: usize, hot: bool) -> Vec<u8> {
    let mut state = value_seed(index, hot);
    let mut output = Vec::with_capacity(config.value_bytes);
    for _ in 0..config.value_bytes / 4 {
        state = next_state(state);
        let unit = ((state >> 40) as u32) as f32 / ((1u32 << 24) - 1) as f32;
        output.extend_from_slice(&(unit * 2.0 - 1.0).to_le_bytes());
    }
    output
}

fn value_seed(index: usize, hot: bool) -> u64 {
    (index as u64)
        .wrapping_add(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ if hot {
            0xD1B5_4A32_D192_ED03
        } else {
            0x94D0_49BB_1331_11EB
        }
}

fn next_state(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

fn finish_trial(backend: &str, measured: ReadMeasurement, footprint: LifecycleFootprint) -> Trial {
    Trial {
        backend: backend.into(),
        correctness_verified: measured.correctness_verified,
        reads_per_second: measured.reads_per_second,
        items_per_sample: measured.items_per_sample,
        latency: measured.latency,
        footprint,
    }
}

fn lifecycle_footprint(
    config: &Config,
    active: StorageFootprint,
    reopened: StorageFootprint,
    maintained: StorageFootprint,
    maintained_actions: Vec<String>,
) -> Result<LifecycleFootprint, String> {
    let logical_live_payload_bytes = logical_payload_bytes(config, config.cold_keys)?;
    let hot_written = logical_payload_bytes(config, config.hot_keys)?;
    let logical_written_payload_bytes = logical_live_payload_bytes
        .checked_add(hot_written)
        .ok_or_else(|| "logical written payload bytes overflowed".to_owned())?;
    Ok(LifecycleFootprint {
        logical_live_payload_bytes,
        logical_written_payload_bytes,
        active,
        reopened,
        maintained,
        maintained_actions,
    })
}

fn logical_payload_bytes(config: &Config, keys: usize) -> Result<u64, String> {
    (0..keys).try_fold(0u64, |total, index| {
        let item = key(index)
            .len()
            .checked_add(config.value_bytes)
            .ok_or_else(|| "logical payload item bytes overflowed".to_owned())?;
        total
            .checked_add(item as u64)
            .ok_or_else(|| "logical payload bytes overflowed".to_owned())
    })
}

fn storage_footprint(path: &Path) -> Result<StorageFootprint, String> {
    measure_storage_footprint(path).map_err(|error| error.to_string())
}

fn verify_fjall_current(
    database: &SingleWriterTxDatabase,
    keyspace: &SingleWriterTxKeyspace,
    config: &Config,
) -> Result<(), String> {
    let snapshot = database.read_tx();
    for index in 0..config.cold_keys {
        let actual = snapshot
            .get(keyspace, key(index))
            .map_err(|error| error.to_string())?
            .map(|bytes| bytes.to_vec());
        let expected = Some(value(config, index, index < config.hot_keys));
        if actual != expected {
            return Err(format!("fjall current value differs at key {index}"));
        }
    }
    Ok(())
}

fn verify_native_current(database: &Database, config: &Config) -> Result<(), String> {
    let snapshot = database.snapshot();
    for index in 0..config.cold_keys {
        let actual = database
            .get(&key(index), snapshot)
            .map_err(|error| error.to_string())?;
        let expected = Some(value(config, index, index < config.hot_keys));
        if actual != expected {
            return Err(format!("native current value differs at key {index}"));
        }
    }
    Ok(())
}

fn summarize(mut samples: Vec<Duration>) -> Latency {
    samples.sort_unstable();
    Latency {
        samples: samples.len(),
        p50_ns: percentile(&samples, 0.50),
        p95_ns: percentile(&samples, 0.95),
        p99_ns: percentile(&samples, 0.99),
        maximum_ns: percentile(&samples, 1.0),
    }
}

fn percentile(samples: &[Duration], quantile: f64) -> u64 {
    let index = ((samples.len() - 1) as f64 * quantile).ceil() as usize;
    u64::try_from(samples[index].as_nanos()).unwrap_or(u64::MAX)
}

fn aggregate(backend: &str, trials: &[Trial]) -> Trial {
    Trial {
        backend: backend.into(),
        correctness_verified: trials.iter().all(|trial| trial.correctness_verified),
        reads_per_second: median_f64(trials.iter().map(|trial| trial.reads_per_second).collect()),
        items_per_sample: trials[0].items_per_sample,
        latency: Latency {
            samples: trials.iter().map(|trial| trial.latency.samples).sum(),
            p50_ns: median_u64(trials.iter().map(|trial| trial.latency.p50_ns).collect()),
            p95_ns: median_u64(trials.iter().map(|trial| trial.latency.p95_ns).collect()),
            p99_ns: median_u64(trials.iter().map(|trial| trial.latency.p99_ns).collect()),
            maximum_ns: median_u64(
                trials
                    .iter()
                    .map(|trial| trial.latency.maximum_ns)
                    .collect(),
            ),
        },
        footprint: aggregate_lifecycle_footprint(trials),
    }
}

fn aggregate_lifecycle_footprint(trials: &[Trial]) -> LifecycleFootprint {
    LifecycleFootprint {
        logical_live_payload_bytes: median_u64(
            trials
                .iter()
                .map(|trial| trial.footprint.logical_live_payload_bytes)
                .collect(),
        ),
        logical_written_payload_bytes: median_u64(
            trials
                .iter()
                .map(|trial| trial.footprint.logical_written_payload_bytes)
                .collect(),
        ),
        active: aggregate_storage_footprint(
            trials.iter().map(|trial| &trial.footprint.active).collect(),
        ),
        reopened: aggregate_storage_footprint(
            trials
                .iter()
                .map(|trial| &trial.footprint.reopened)
                .collect(),
        ),
        maintained: aggregate_storage_footprint(
            trials
                .iter()
                .map(|trial| &trial.footprint.maintained)
                .collect(),
        ),
        maintained_actions: trials[0].footprint.maintained_actions.clone(),
    }
}

fn aggregate_storage_footprint(values: Vec<&StorageFootprint>) -> StorageFootprint {
    let classes = values
        .iter()
        .flat_map(|value| value.by_class.keys().cloned())
        .collect::<BTreeSet<_>>();
    let by_class = classes
        .into_iter()
        .map(|class| {
            let measurements = values
                .iter()
                .map(|value| value.by_class.get(&class).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            (class, aggregate_footprint_bytes(&measurements))
        })
        .collect();
    let allocated = values
        .iter()
        .filter_map(|value| value.allocated_bytes)
        .collect::<Vec<_>>();
    StorageFootprint {
        apparent_bytes: median_u64(values.iter().map(|value| value.apparent_bytes).collect()),
        allocated_bytes: (allocated.len() == values.len()).then(|| median_u64(allocated)),
        allocated_bytes_source: values[0].allocated_bytes_source.clone(),
        files: median_u64(values.iter().map(|value| value.files).collect()),
        by_class,
    }
}

fn aggregate_footprint_bytes(values: &[FootprintBytes]) -> FootprintBytes {
    let allocated = values
        .iter()
        .filter_map(|value| value.allocated_bytes)
        .collect::<Vec<_>>();
    FootprintBytes {
        apparent_bytes: median_u64(values.iter().map(|value| value.apparent_bytes).collect()),
        allocated_bytes: (allocated.len() == values.len()).then(|| median_u64(allocated)),
        files: median_u64(values.iter().map(|value| value.files).collect()),
    }
}

fn promotion(
    fjall: &Trial,
    native: &Trial,
    throughput_ratio: f64,
    p95_ratio: f64,
    footprint: &FootprintComparison,
) -> PromotionVerdict {
    let mut failures = Vec::new();
    if !fjall.correctness_verified || !native.correctness_verified {
        failures.push("one or more backends failed correctness".into());
    }
    if throughput_ratio < 1.0 {
        failures.push(format!(
            "native item throughput ratio {throughput_ratio:.3} is below 1.000"
        ));
    }
    if p95_ratio > 1.0 {
        failures.push(format!(
            "native p95 request-latency ratio {p95_ratio:.3} exceeds 1.000"
        ));
    }
    if footprint
        .native_to_fjall_reopened_allocated
        .is_some_and(|ratio| ratio > 1.0)
    {
        failures.push(format!(
            "native clean-reopen allocated-footprint ratio {:.3} exceeds 1.000",
            footprint
                .native_to_fjall_reopened_allocated
                .expect("ratio was checked above")
        ));
    }
    PromotionVerdict {
        passes: failures.is_empty(),
        failures,
    }
}

fn footprint_comparison(fjall: &Trial, native: &Trial) -> FootprintComparison {
    let fjall_reopened = &fjall.footprint.reopened;
    let native_reopened = &native.footprint.reopened;
    FootprintComparison {
        promotion_state: "clean_reopen_without_explicit_maintenance",
        native_to_fjall_reopened_apparent: native_reopened.apparent_bytes as f64
            / fjall_reopened.apparent_bytes.max(1) as f64,
        native_to_fjall_reopened_allocated: native_reopened
            .allocated_bytes
            .zip(fjall_reopened.allocated_bytes)
            .map(|(native, fjall)| native as f64 / fjall.max(1) as f64),
        fjall_reopened_allocated_to_live_payload: fjall_reopened
            .allocated_bytes
            .map(|bytes| bytes as f64 / fjall.footprint.logical_live_payload_bytes.max(1) as f64),
        native_reopened_allocated_to_live_payload: native_reopened
            .allocated_bytes
            .map(|bytes| bytes as f64 / native.footprint.logical_live_payload_bytes.max(1) as f64),
        maintained_cross_backend_comparable: false,
    }
}

fn median_u64(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
