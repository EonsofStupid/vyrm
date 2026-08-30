use rrd_contract::CanonicalId;
use rrd_engine::{RrdEngine, SecurityBootstrapOutcome};
use std::io;
use std::path::PathBuf;

struct Args {
    database: PathBuf,
    instance: CanonicalId,
    manifest: PathBuf,
    at_unix_ms: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rrd-security-bootstrap: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    match RrdEngine::bootstrap_security_store(
        &args.database,
        args.instance,
        &args.manifest,
        args.at_unix_ms,
    )? {
        SecurityBootstrapOutcome::Initialized => {
            println!("rrd-security-bootstrap: initialized")
        }
        SecurityBootstrapOutcome::Unchanged => println!("rrd-security-bootstrap: unchanged"),
    }
    Ok(())
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut database = None;
    let mut instance = None;
    let mut manifest = None;
    let mut at_unix_ms = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--db" => database = Some(PathBuf::from(required(&mut arguments, "--db")?)),
            "--instance" => {
                instance = Some(
                    CanonicalId::new(required(&mut arguments, "--instance")?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--manifest" => {
                manifest = Some(PathBuf::from(required(&mut arguments, "--manifest")?));
            }
            "--at-unix-ms" => {
                let value = required(&mut arguments, "--at-unix-ms")?;
                at_unix_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --at-unix-ms: {error}"))?,
                );
            }
            "--help" | "-h" => return Err(usage().into()),
            value => return Err(format!("unknown argument {value:?}\n{}", usage())),
        }
    }
    let at_unix_ms = at_unix_ms.ok_or_else(|| format!("--at-unix-ms is required\n{}", usage()))?;
    if at_unix_ms == 0 {
        return Err("--at-unix-ms must be greater than zero".into());
    }
    Ok(Args {
        database: database.ok_or_else(|| format!("--db is required\n{}", usage()))?,
        instance: instance.ok_or_else(|| format!("--instance is required\n{}", usage()))?,
        manifest: manifest.ok_or_else(|| format!("--manifest is required\n{}", usage()))?,
        at_unix_ms,
    })
}

fn required(arguments: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value\n{}", usage()))
}

fn usage() -> &'static str {
    "usage: rrd-security-bootstrap --db PATH --instance ID --manifest PATH --at-unix-ms MILLIS"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_explicit_and_bounded() {
        assert!(parse_args(std::iter::empty()).is_err());
        assert!(parse_args(
            [
                "--db",
                "/tmp/db",
                "--instance",
                "instance-a",
                "--manifest",
                "/tmp/bootstrap.json",
                "--at-unix-ms",
                "0",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .is_err());
        let parsed = parse_args(
            [
                "--db",
                "/tmp/db",
                "--instance",
                "instance-a",
                "--manifest",
                "/tmp/bootstrap.json",
                "--at-unix-ms",
                "42",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(parsed.database, PathBuf::from("/tmp/db"));
        assert_eq!(parsed.instance.as_str(), "instance-a");
        assert_eq!(parsed.at_unix_ms, 42);
    }
}
