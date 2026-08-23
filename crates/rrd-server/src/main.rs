use rrd_contract::CanonicalId;
use rrd_server::{load_or_create_token_key, RrdHttpServer};
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use vyrm_store::PersistentEngine;

struct Args {
    db: PathBuf,
    bind: SocketAddr,
    instance: CanonicalId,
    token_key_file: Option<PathBuf>,
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
    server
        .serve_until(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut db = None;
    let mut bind = "127.0.0.1:9477".parse().expect("static bind address");
    let mut instance = None;
    let mut token_key_file = None;
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
            "--help" | "-h" => return Err(usage().into()),
            value => return Err(format!("unknown argument {value:?}\n{}", usage())),
        }
    }
    Ok(Args {
        db: db.ok_or_else(|| format!("--db is required\n{}", usage()))?,
        bind,
        instance: instance.ok_or_else(|| format!("--instance is required\n{}", usage()))?,
        token_key_file,
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
    "usage: rrd-server --db PATH --instance ID [--bind 127.0.0.1:9477] [--token-key-file PATH]"
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
}
