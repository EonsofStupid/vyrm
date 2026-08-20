//! Isolated Fjall/vyrmKV point-read profile for an AI runtime's hot control set.
//!
//! Setup is deliberately excluded from measurement: a cold immutable corpus is
//! published first, then a small set of routing, lease, cursor, and outcome-like
//! keys is overwritten in the active memtable. The measured phase repeatedly
//! reads only that current hot set. This is one physical profile, not a general
//! database benchmark.

use fjall::{KeyspaceCreateOptions, PersistMode, Readable, SingleWriterTxDatabase};
use serde::{Deserialize, Serialize};
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vyrm_kv::{Database, Durability, Mutation, WriteBatch};

const FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    trials: usize,
    cold_keys: usize,
    hot_keys: usize,
    reads: usize,
    batch_size: usize,
    value_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            trials: 5,
            cold_keys: 8_192,
            hot_keys: 128,
            reads: 65_536,
            batch_size: 128,
            value_bytes: 128,
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
    latency: Latency,
    disk_bytes: u64,
}

#[derive(Debug, Serialize)]
struct Evidence {
    format_version: u16,
    measured_at_unix_ms: u128,
    architecture: String,
    operating_system: String,
    workload: &'static str,
    config: Config,
    fjall_version: &'static str,
    fjall_trials: Vec<Trial>,
    native_trials: Vec<Trial>,
    fjall: Trial,
    native: Trial,
    native_to_fjall_read_throughput: f64,
    native_to_fjall_p95_latency: f64,
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
    let (config, output) = parse(&arguments)?;
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
    let evidence = Evidence {
        format_version: FORMAT_VERSION,
        measured_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis(),
        architecture: std::env::consts::ARCH.into(),
        operating_system: std::env::consts::OS.into(),
        workload: "current point reads over a hot memtable overlay above one immutable corpus",
        config,
        fjall_version: "3.1.8",
        native_to_fjall_read_throughput: native.reads_per_second / fjall.reads_per_second,
        native_to_fjall_p95_latency: native.latency.p95_ns as f64
            / fjall.latency.p95_ns.max(1) as f64,
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
    Ok(())
}

fn parse(arguments: &[String]) -> Result<(Config, Option<PathBuf>), String> {
    let mut config = Config::default();
    let mut output = None;
    let mut index = 0;
    while index < arguments.len() {
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("missing value after {}", arguments[index]))?;
        let parsed = || {
            value
                .parse::<usize>()
                .map_err(|error| format!("invalid value for {}: {error}", arguments[index]))
        };
        match arguments[index].as_str() {
            "--trials" => config.trials = parsed()?,
            "--cold-keys" => config.cold_keys = parsed()?,
            "--hot-keys" => config.hot_keys = parsed()?,
            "--reads" => config.reads = parsed()?,
            "--batch-size" => config.batch_size = parsed()?,
            "--value-bytes" => config.value_bytes = parsed()?,
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
    ]
    .contains(&0)
    {
        return Err("all numeric values must be greater than zero".into());
    }
    if config.hot_keys > config.cold_keys || config.batch_size > config.cold_keys {
        return Err("hot keys and batch size cannot exceed the cold corpus".into());
    }
    Ok((config, output))
}

fn launch_child(backend: &str, path: &Path, config: &Config) -> Result<Trial, String> {
    let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args(["--child", backend, "--path"])
        .arg(path)
        .args(["--trials", "1"])
        .args(["--cold-keys", &config.cold_keys.to_string()])
        .args(["--hot-keys", &config.hot_keys.to_string()])
        .args(["--reads", &config.reads.to_string()])
        .args(["--batch-size", &config.batch_size.to_string()])
        .args(["--value-bytes", &config.value_bytes.to_string()])
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
    let (config, output) = parse(&arguments[4..])?;
    if output.is_some() {
        return Err("child output path is not supported".into());
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
    let mut transaction = database.write_tx().durability(Some(PersistMode::SyncAll));
    for index in 0..config.hot_keys {
        transaction.insert(&keyspace, key(index), hot_value(index, config.value_bytes));
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let snapshot = database.read_tx();
    measure("fjall", path, config, |index| {
        snapshot
            .get(&keyspace, key(index))
            .map_err(|error| error.to_string())?
            .map(|value| value.to_vec())
            .ok_or_else(|| format!("fjall hot key {index} is absent"))
    })
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
    let snapshot = database.snapshot();
    measure("native", path, config, |index| {
        database
            .get(&key(index), snapshot)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("native hot key {index} is absent"))
    })
}

fn measure(
    backend: &str,
    path: &Path,
    config: &Config,
    mut get: impl FnMut(usize) -> Result<Vec<u8>, String>,
) -> Result<Trial, String> {
    for iteration in 0..config.hot_keys * 4 {
        black_box(get(iteration % config.hot_keys)?);
    }
    let started = Instant::now();
    let mut samples = Vec::with_capacity(config.reads);
    let mut correctness_verified = true;
    for iteration in 0..config.reads {
        let index = iteration.wrapping_mul(7_919) % config.hot_keys;
        let read_started = Instant::now();
        let value = get(index)?;
        samples.push(read_started.elapsed());
        correctness_verified &= value == hot_value(index, config.value_bytes);
        black_box(value);
    }
    let elapsed = started.elapsed();
    Ok(Trial {
        backend: backend.into(),
        correctness_verified,
        reads_per_second: config.reads as f64 / elapsed.as_secs_f64(),
        latency: summarize(samples),
        disk_bytes: directory_bytes(path)?,
    })
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
