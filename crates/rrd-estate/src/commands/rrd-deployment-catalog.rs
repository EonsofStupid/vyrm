use rrd_contract::CanonicalId;
use rrd_estate::{LocalArgument, LocalDeployment, LocalDeploymentCatalog, LocalShutdown};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

struct Args {
    server: PathBuf,
    output: PathBuf,
    version: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rrd-deployment-catalog: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    ensure_output_absent(&args.output)?;
    let deployment = LocalDeployment::authenticate(
        CanonicalId::new("rrd-server")?,
        args.version,
        args.server,
        vec![
            LocalArgument::Literal("initialize".into()),
            LocalArgument::Literal("--root".into()),
            LocalArgument::InstanceRoot,
            LocalArgument::Literal("--instance".into()),
            LocalArgument::InstanceId,
        ],
        vec![
            LocalArgument::Literal("--root".into()),
            LocalArgument::InstanceRoot,
            LocalArgument::Literal("--bind".into()),
            LocalArgument::Literal("127.0.0.1:0".into()),
            LocalArgument::Literal("--shutdown-request-file".into()),
            LocalArgument::InstancePath(PathBuf::from("SHUTDOWN.REQUEST")),
            LocalArgument::Literal("--shutdown-complete-file".into()),
            LocalArgument::InstancePath(PathBuf::from("SHUTDOWN.COMPLETE")),
        ],
        BTreeMap::new(),
        LocalShutdown::RequestFile {
            request: PathBuf::from("SHUTDOWN.REQUEST"),
            complete: PathBuf::from("SHUTDOWN.COMPLETE"),
            timeout_ms: 5_000,
        },
    )?;
    let catalog = LocalDeploymentCatalog::single(deployment)?;
    let mut bytes = serde_json::to_vec_pretty(&catalog)?;
    bytes.push(b'\n');
    write_new(&args.output, &bytes)?;
    println!("{}", args.output.display());
    Ok(())
}

fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut server = None;
    let mut output = None;
    let mut version = env!("CARGO_PKG_VERSION").to_owned();
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--server" => server = Some(PathBuf::from(required(&mut arguments, "--server")?)),
            "--output" => output = Some(PathBuf::from(required(&mut arguments, "--output")?)),
            "--version" => version = required(&mut arguments, "--version")?,
            "--help" | "-h" => return Err(usage().into()),
            value => return Err(format!("unknown argument {value:?}\n{}", usage())),
        }
    }
    let server = match server {
        Some(server) => server,
        None => default_server_path()?,
    };
    Ok(Args {
        server,
        output: output.ok_or_else(|| format!("--output is required\n{}", usage()))?,
        version,
    })
}

fn default_server_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve catalogue executable: {error}"))?;
    let name = if cfg!(windows) {
        "rrd-server.exe"
    } else {
        "rrd-server"
    };
    Ok(executable
        .parent()
        .ok_or("catalogue executable has no parent directory")?
        .join(name))
}

fn required(arguments: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if parent != Path::new(".") {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "deployment catalogue already exists",
        ));
    }
    let temporary = path.with_extension("catalog.new");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut created_temporary = false;
    let result = (|| {
        let mut file = options.open(&temporary)?;
        created_temporary = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::hard_link(&temporary, path)?;
        std::fs::remove_file(&temporary)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created_temporary {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn ensure_output_absent(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "deployment catalogue already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn usage() -> &'static str {
    "usage: rrd-deployment-catalog --output PATH [--server PATH] [--version VERSION]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_an_output_and_rejects_unknown_arguments() {
        assert!(parse_args(["--wat".into()].into_iter()).is_err());
        assert!(parse_args(std::iter::empty()).is_err());
    }
}
