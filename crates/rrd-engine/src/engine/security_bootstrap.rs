use super::*;
use rrd_security::{
    Principal, PrincipalKind, ResourceGrant, SecurityRepository, SecurityState, SECURITY_FORMAT,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};

const BOOTSTRAP_FORMAT: u16 = 1;
const MAX_BOOTSTRAP_BYTES: u64 = 1024 * 1024;
const MAX_CREDENTIAL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityBootstrapOutcome {
    Initialized,
    Unchanged,
}

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

impl RrdEngine {
    /// Initializes one security authority through the RRD composition root.
    /// The adapter supplies bounded manifest coordinates; it cannot construct a
    /// security repository or physical store.
    pub fn bootstrap_security_store(
        database: &Path,
        instance: CanonicalId,
        manifest: &Path,
        at_unix_ms: u64,
    ) -> Result<SecurityBootstrapOutcome> {
        if at_unix_ms == 0 {
            return Err(ServiceError::Contract(
                "security bootstrap time must be non-zero".into(),
            ));
        }
        let engine = Self::open_local_authority(database, instance)?;
        engine
            .bootstrap_security(manifest, at_unix_ms)
            .map_err(|error| ServiceError::Contract(error.to_string()))
    }

    fn bootstrap_security(
        &self,
        manifest: &Path,
        at_unix_ms: u64,
    ) -> std::result::Result<SecurityBootstrapOutcome, Box<dyn std::error::Error>> {
        let manifest_bytes = read_bounded(manifest, MAX_BOOTSTRAP_BYTES, false)?;
        let manifest: BootstrapManifest = serde_json::from_slice(&manifest_bytes)?;
        let state = materialize(manifest)?;
        let operation_sha256 = digest::sha256_hex(&manifest_bytes);
        let repository = SecurityRepository::new(&self.storage, self.instance.clone());
        match repository.load()? {
            Some(existing) if existing == state => Ok(SecurityBootstrapOutcome::Unchanged),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "security authority is already initialized with different policy",
            )
            .into()),
            None => {
                repository.initialize(
                    state,
                    at_unix_ms,
                    "rrd-security-bootstrap",
                    &format!("bootstrap-request-{}", &operation_sha256[..16]),
                    &format!("bootstrap-operation-{}", &operation_sha256[..16]),
                )?;
                Ok(SecurityBootstrapOutcome::Initialized)
            }
        }
    }
}

fn materialize(
    manifest: BootstrapManifest,
) -> std::result::Result<SecurityState, Box<dyn std::error::Error>> {
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
