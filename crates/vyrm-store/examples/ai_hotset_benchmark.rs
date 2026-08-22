//! Isolated Fjall/vyrmKV read profiles for an AI runtime's control and metadata sets.
//!
//! Setup is deliberately excluded from measurement: a cold immutable corpus is
//! published first, then a small set of routing, lease, cursor, and outcome-like
//! keys is overwritten in the active memtable. The measured phase selects one
//! explicit point or metadata-fan-out workload. These are physical profiles,
//! not a general database benchmark.

use fjall::{KeyspaceCreateOptions, PersistMode, Readable, SingleWriterTxDatabase};
use serde::{Deserialize, Serialize};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const FORMAT_VERSION: u16 = 2;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    workload: Workload,
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
    disk_bytes: u64,
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
    config: Config,
    fjall_version: &'static str,
    fjall_trials: Vec<Trial>,
    native_trials: Vec<Trial>,
    fjall: Trial,
    native: Trial,
    native_to_fjall_read_throughput: f64,
    native_to_fjall_p95_latency: f64,
    promotion: PromotionVerdict,
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
    let promotion = promotion(&fjall, &native, throughput_ratio, p95_ratio);
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
        config,
        fjall_version: "3.1.8",
        native_to_fjall_read_throughput: throughput_ratio,
        native_to_fjall_p95_latency: p95_ratio,
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
    Ok((config, output, require_promotion))
}

fn launch_child(backend: &str, path: &Path, config: &Config) -> Result<Trial, String> {
    let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args(["--child", backend, "--path"])
        .arg(path)
        .args(["--trials", "1"])
        .args(["--workload", config.workload.cli_str()])
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
            transaction.insert(&keyspace, key(index), cold_value(index, config.value_bytes));
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
        transaction.insert(&keyspace, key(index), hot_value(index, config.value_bytes));
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let current = database.read_tx();
    let snapshot = if config.workload.uses_historical_snapshot() {
        &historical
    } else {
        &current
    };
    if config.workload == Workload::MetadataFanout {
        measure_fanout("fjall", path, config, |keys| {
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
        measure("fjall", path, config, |index| {
            Ok(snapshot
                .get(&keyspace, key(index))
                .map_err(|error| error.to_string())?
                .map(|value| value.to_vec()))
        })
    }
}

fn run_native(path: &Path, config: &Config) -> Result<Trial, String> {
    let mut database = Database::create(path).map_err(|error| error.to_string())?;
    for start in (0..config.cold_keys).step_by(config.batch_size) {
        let operations = (start..(start + config.batch_size).min(config.cold_keys))
            .map(|index| Mutation::Put {
                key: key(index),
                value: cold_value(index, config.value_bytes),
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
            value: hot_value(index, config.value_bytes),
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
    if config.workload == Workload::MetadataFanout {
        measure_fanout("native", path, config, |keys| {
            database
                .get_many(keys, snapshot)
                .map_err(|error| error.to_string())
        })
    } else {
        measure("native", path, config, |index| {
            database
                .get(&key(index), snapshot)
                .map_err(|error| error.to_string())
        })
    }
}

fn measure(
    backend: &str,
    path: &Path,
    config: &Config,
    mut get: impl FnMut(usize) -> Result<Option<Vec<u8>>, String>,
) -> Result<Trial, String> {
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
    Ok(Trial {
        backend: backend.into(),
        correctness_verified,
        reads_per_second: config.reads as f64 / elapsed.as_secs_f64(),
        items_per_sample: 1,
        latency: summarize(samples),
        disk_bytes: directory_bytes(path)?,
    })
}

fn measure_fanout(
    backend: &str,
    path: &Path,
    config: &Config,
    mut get_many: impl FnMut(&[Vec<u8>]) -> Result<Vec<Option<Vec<u8>>>, String>,
) -> Result<Trial, String> {
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
    Ok(Trial {
        backend: backend.into(),
        correctness_verified,
        reads_per_second: (config.reads * config.fanout_width) as f64 / elapsed.as_secs_f64(),
        items_per_sample: config.fanout_width,
        latency: summarize(samples),
        disk_bytes: directory_bytes(path)?,
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
        Workload::CurrentHotHit => Some(hot_value(index, config.value_bytes)),
        Workload::ColdHit | Workload::HistoricalHotHit => {
            Some(cold_value(index, config.value_bytes))
        }
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
        Some(hot_value(index, config.value_bytes))
    } else if index < config.cold_keys {
        Some(cold_value(index, config.value_bytes))
    } else {
        None
    }
}

fn key(index: usize) -> Vec<u8> {
    format!("runtime:control:{index:012}").into_bytes()
}

fn cold_value(index: usize, bytes: usize) -> Vec<u8> {
    vec![(index % 251) as u8; bytes]
}

fn hot_value(index: usize, bytes: usize) -> Vec<u8> {
    vec![((index + 97) % 251) as u8; bytes]
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
        disk_bytes: median_u64(trials.iter().map(|trial| trial.disk_bytes).collect()),
    }
}

fn promotion(
    fjall: &Trial,
    native: &Trial,
    throughput_ratio: f64,
    p95_ratio: f64,
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
    PromotionVerdict {
        passes: failures.is_empty(),
        failures,
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

fn directory_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let bytes = if metadata.is_dir() {
            directory_bytes(&entry.path())?
        } else {
            metadata.len()
        };
        total = total
            .checked_add(bytes)
            .ok_or_else(|| "directory byte count overflowed".to_owned())?;
    }
    Ok(total)
}
