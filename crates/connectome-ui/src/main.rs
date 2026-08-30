use rrd_contract::CanonicalId;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("connectome: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut rrd_address: SocketAddr = "127.0.0.1:9477".parse()?;
    let mut bind: SocketAddr = "127.0.0.1:4387".parse()?;
    let mut instance = None;
    let mut principal = None;
    let mut api_key_file = None;
    let mut scope = None;
    let mut shutdown_request_file = None;
    let mut shutdown_complete_file = None;
    let mut allow_remote = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rrd-address" => rrd_address = required(&mut args, "--rrd-address")?.parse()?,
            "--instance" => instance = Some(CanonicalId::new(required(&mut args, "--instance")?)?),
            "--principal" => {
                principal = Some(CanonicalId::new(required(&mut args, "--principal")?)?)
            }
            "--api-key-file" => {
                api_key_file = Some(PathBuf::from(required(&mut args, "--api-key-file")?))
            }
            "--scope" => scope = Some(required(&mut args, "--scope")?),
            "--bind" => bind = required(&mut args, "--bind")?.parse()?,
            "--shutdown-request-file" => {
                shutdown_request_file = Some(PathBuf::from(required(
                    &mut args,
                    "--shutdown-request-file",
                )?))
            }
            "--shutdown-complete-file" => {
                shutdown_complete_file = Some(PathBuf::from(required(
                    &mut args,
                    "--shutdown-complete-file",
                )?))
            }
            "--allow-remote" => allow_remote = true,
            "--help" | "-h" => {
                println!(
                    "connectome --instance ID --principal ID --api-key-file PATH [--rrd-address 127.0.0.1:9477] [--scope SCOPE] [--bind 127.0.0.1:4387] [--shutdown-request-file PATH --shutdown-complete-file PATH] [--allow-remote]\n\nClient-only Connectome gateway. RRD remains the sole database authority."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg:?}").into()),
        }
    }
    let instance = instance.ok_or("--instance is required")?;
    let principal = principal.ok_or("--principal is required")?;
    let api_key_file = api_key_file.ok_or("--api-key-file is required")?;
    if !bind.ip().is_loopback() && !allow_remote {
        return Err("non-loopback Connectome binding requires --allow-remote".into());
    }
    if shutdown_request_file.is_some() != shutdown_complete_file.is_some() {
        return Err(
            "--shutdown-request-file and --shutdown-complete-file must be provided together".into(),
        );
    }
    let shutdown = shutdown_request_file
        .zip(shutdown_complete_file)
        .map(|(request, complete)| connectome_ui::ShutdownFiles { request, complete });
    let api_key = read_secret(&api_key_file)?;
    let scope = scope.unwrap_or_else(|| format!("instance:{instance}"));
    connectome_ui::serve(connectome_ui::ConnectomeConfig {
        rrd_address,
        instance,
        principal,
        api_key,
        scope,
        bind,
        shutdown,
    })
}

fn required(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{option} needs a value"))
}

fn read_secret(path: &Path) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(format!(
                "API key file {} must not be accessible by group or others",
                path.display()
            )
            .into());
        }
    }
    let secret = std::fs::read_to_string(path)?;
    let secret = secret.trim_end_matches(['\r', '\n']).to_owned();
    if secret.is_empty() || secret.contains('\0') {
        return Err("API key file is empty or contains NUL".into());
    }
    Ok(secret)
}
