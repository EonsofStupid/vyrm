use rrd_contract::CanonicalId;
use rrd_server::{load_or_create_token_key, RrdHttpServer};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use vyrm_store::PersistentEngine;

struct Args {
    db: PathBuf,
    bind: SocketAddr,
    instance: CanonicalId,
    token_key_file: Option<PathBuf>,
    shutdown_request_file: Option<PathBuf>,
    shutdown_complete_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("rrd-server: {error}");
        std::process::exit(2);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .try_init()?;
    }
    let args = parse_args(std::env::args().skip(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let engine = PersistentEngine::open(&args.db)?;
    let key_path = args
        .token_key_file
        .unwrap_or_else(|| args.db.join("RRD.SERVER.SECRET"));
    let token_key = load_or_create_token_key(&key_path)?;
    let server = RrdHttpServer::bind(engine, args.instance, token_key, args.bind)?;
    eprintln!("rrd-server: http://{}", server.local_addr());
    let shutdown_request_file = args.shutdown_request_file;
    let shutdown_complete_file = args.shutdown_complete_file;
    server
        .serve_until(shutdown_signal(shutdown_request_file))
        .await?;
    if let Some(path) = shutdown_complete_file {
        write_shutdown_completion(&path)?;
    }
    Ok(())
}

async fn shutdown_signal(request_file: Option<PathBuf>) {
    let Some(path) = request_file else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = wait_for_shutdown_request(path) => {}
    }
}

async fn wait_for_shutdown_request(path: PathBuf) {
    loop {
        if path.is_file() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn write_shutdown_completion(path: &Path) -> io::Result<()> {
    use std::io::Write;

    let temporary = path.with_extension("complete.new");
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(b"rrd-server-graceful-shutdown-v1\n")?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    #[cfg(unix)]
    std::fs::File::open(path.parent().expect("shutdown marker has a parent"))?.sync_all()?;
    Ok(())
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut db = None;
    let mut bind = "127.0.0.1:9477".parse().expect("static bind address");
    let mut instance = None;
    let mut token_key_file = None;
    let mut shutdown_request_file = None;
    let mut shutdown_complete_file = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--db" => db = Some(PathBuf::from(required_value(&mut arguments, "--db")?)),
            "--bind" => {
                let value = required_value(&mut arguments, "--bind")?;
                bind = value
                    .parse()
                    .map_err(|error| format!("invalid --bind {value:?}: {error}"))?;
            }
            "--instance" => {
                instance = Some(
                    CanonicalId::new(required_value(&mut arguments, "--instance")?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--token-key-file" => {
                token_key_file = Some(PathBuf::from(required_value(
                    &mut arguments,
                    "--token-key-file",
                )?));
            }
            "--shutdown-request-file" => {
                shutdown_request_file = Some(PathBuf::from(required_value(
                    &mut arguments,
                    "--shutdown-request-file",
                )?));
            }
            "--shutdown-complete-file" => {
                shutdown_complete_file = Some(PathBuf::from(required_value(
                    &mut arguments,
                    "--shutdown-complete-file",
                )?));
            }
            "--help" | "-h" => return Err(usage().into()),
            value => return Err(format!("unknown argument {value:?}\n{}", usage())),
        }
    }
    if shutdown_request_file.is_some() != shutdown_complete_file.is_some() {
        return Err(format!(
            "--shutdown-request-file and --shutdown-complete-file must be provided together\n{}",
            usage()
        ));
    }
    if let (Some(request), Some(complete)) = (&shutdown_request_file, &shutdown_complete_file) {
        if !request.is_absolute() || !complete.is_absolute() || request == complete {
            return Err("shutdown control files must be distinct absolute paths".into());
        }
    }
    Ok(Args {
        db: db.ok_or_else(|| format!("--db is required\n{}", usage()))?,
        bind,
        instance: instance.ok_or_else(|| format!("--instance is required\n{}", usage()))?,
        token_key_file,
        shutdown_request_file,
        shutdown_complete_file,
    })
}

fn required_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn usage() -> &'static str {
    "usage: rrd-server --db PATH --instance ID [--bind 127.0.0.1:9477] [--token-key-file PATH] [--shutdown-request-file PATH --shutdown-complete-file PATH]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_require_explicit_database_and_instance() {
        assert!(parse_args(["--db".into(), "state".into()].into_iter()).is_err());
        let args = parse_args(
            [
                "--db".into(),
                "state".into(),
                "--instance".into(),
                "alpha".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(args.bind, "127.0.0.1:9477".parse().unwrap());
        assert_eq!(args.instance.as_str(), "alpha");
    }

    #[test]
    fn shutdown_control_files_are_a_paired_absolute_contract() {
        let root = std::env::current_dir().unwrap();
        let request = root.join("shutdown.request");
        let complete = root.join("shutdown.complete");
        let incomplete = vec![
            "--db".into(),
            "state".into(),
            "--instance".into(),
            "alpha".into(),
            "--shutdown-request-file".into(),
            request.to_string_lossy().into_owned(),
        ];
        assert!(parse_args(incomplete.into_iter()).is_err());

        let arguments = vec![
            "--db".into(),
            "state".into(),
            "--instance".into(),
            "alpha".into(),
            "--shutdown-request-file".into(),
            request.to_string_lossy().into_owned(),
            "--shutdown-complete-file".into(),
            complete.to_string_lossy().into_owned(),
        ];
        let args = parse_args(arguments.into_iter()).unwrap();
        assert_eq!(args.shutdown_complete_file, Some(complete));
    }
}
