use rrd_contract::CanonicalId;
use rrd_security::{
    Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState, SECURITY_FORMAT,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use vyrm_core::digest;
use rrd_store::PersistentEngine;

const BOOTSTRAP_FORMAT: u16 = 1;
const MAX_BOOTSTRAP_BYTES: u64 = 1024 * 1024;
const MAX_CREDENTIAL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapManifest {
    format_version: u16,
    revision: u64,
    principals: Vec<BootstrapPrincipal>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapPrincipal {
    id: CanonicalId,
    kind: PrincipalKind,
    credential_file: PathBuf,
    not_before_unix_ms: u64,
    expires_at_unix_ms: u64,
    grants: Vec<ResourceGrant>,
}

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
    let manifest_bytes = read_bounded(&args.manifest, MAX_BOOTSTRAP_BYTES, false)?;
    let manifest: BootstrapManifest = serde_json::from_slice(&manifest_bytes)?;
    let state = materialize(manifest)?;
    let operation_sha256 = digest::sha256_hex(&manifest_bytes);
    let engine = PersistentEngine::open(&args.database)?;
    let repository = SecurityRepository::new(&engine, args.instance);
    match repository.load()? {
        Some(existing) if existing == state => {
            println!("rrd-security-bootstrap: unchanged");
            Ok(())
        }
        Some(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "security authority is already initialized with different policy",
        )
        .into()),
        None => {
            repository.initialize(
                state,
                args.at_unix_ms,
                "rrd-security-bootstrap",
                &format!("bootstrap-request-{}", &operation_sha256[..16]),
                &format!("bootstrap-operation-{}", &operation_sha256[..16]),
            )?;
            println!("rrd-security-bootstrap: initialized");
            Ok(())
        }
    }
}

fn materialize(manifest: BootstrapManifest) -> Result<SecurityState, Box<dyn std::error::Error>> {
    if manifest.format_version != BOOTSTRAP_FORMAT || manifest.revision == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported bootstrap format or zero policy revision",
        )
        .into());
    }
    let mut principals = BTreeMap::new();
    for provisioned in manifest.principals {
        if !provisioned.credential_file.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "credential_file must be an absolute mounted-secret path",
            )
            .into());
        }
        let credential = read_bounded(&provisioned.credential_file, MAX_CREDENTIAL_BYTES, true)?;
        if credential.is_empty() {
            return Err(
                io::Error::new(io::ErrorKind::InvalidData, "credential file is empty").into(),
            );
        }
        let principal = Principal {
            id: provisioned.id.clone(),
            kind: provisioned.kind,
            credential_sha256: digest::sha256_hex(&credential),
            not_before_unix_ms: provisioned.not_before_unix_ms,
            expires_at_unix_ms: provisioned.expires_at_unix_ms,
            disabled: false,
            grants: provisioned.grants,
        };
        principal.validate()?;
        if principals.insert(provisioned.id, principal).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bootstrap principals must have unique identities",
            )
            .into());
        }
    }
    let state = SecurityState {
        format_version: SECURITY_FORMAT,
        revision: manifest.revision,
        principals,
    };
    state.validate()?;
    Ok(state)
}

fn read_bounded(path: &Path, limit: u64, private: bool) -> io::Result<Vec<u8>> {
    let parent = std::fs::canonicalize(path.parent().unwrap_or(Path::new("/")))?;
    let resolved = std::fs::canonicalize(path)?;
    if !resolved.starts_with(&parent) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "bootstrap input symlink escapes its mounted directory",
        ));
    }
    let metadata = std::fs::metadata(&resolved)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bootstrap input is not a bounded regular file",
        ));
    }
    if private {
        ensure_private(&resolved, &metadata)?;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(resolved)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bootstrap input exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn ensure_private(path: &Path, metadata: &std::fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    if mode & 0o007 != 0
        || mode & 0o030 != 0
        || metadata.uid() != std::fs::metadata(path.parent().unwrap_or(Path::new("/")))?.uid()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "credential file must deny other access and group write/execute",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private(_path: &Path, _metadata: &std::fs::Metadata) -> io::Result<()> {
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
        .ok_or_else(|| format!("{option} requires a value"))
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
                "--db".into(),
                "state".into(),
                "--instance".into(),
                "alpha".into(),
                "--manifest".into(),
                "bootstrap.json".into(),
                "--at-unix-ms".into(),
                "0".into(),
            ]
            .into_iter(),
        )
        .is_err());
    }
}
