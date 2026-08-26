//! Versioned deployment boundary for an RRFlow/Connectome instance.
//!
//! A major platform owns a dedicated instance. Related small projects may be
//! admitted to an umbrella, but only by an explicit relative member path. The
//! manifest deliberately contains no canonical absolute paths so an instance
//! can be relocated without becoming a different instance.

use rrd_contract::CanonicalId;
use rrd_core::digest;
use rrd_store::{ControlTransition, Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{RrdEngine, ServiceError};

pub const INSTANCE_FORMAT: u32 = 1;
pub const INSTANCE_FILE: &str = ".rrflow/instance.toml";
pub const PROJECT_AUTHORITY_FORMAT: u16 = 1;
const PROJECT_AUTHORITY_KEY_PREFIX: &str = "server/state/project-authority";
static MANIFEST_STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceMode {
    Dedicated,
    Umbrella,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceManifest {
    pub format: u32,
    pub id: String,
    pub mode: InstanceMode,
    pub members: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceBinding {
    pub manifest: InstanceManifest,
    pub instance_root: PathBuf,
    pub project_root: PathBuf,
    pub member: PathBuf,
}

/// Immutable persisted identity joining one RRD database to one canonical
/// project root. Moving or copying the database requires an explicit future
/// rebind operation; an ordinary server start can never rewrite this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAuthorityBinding {
    pub format: u16,
    pub instance_id: CanonicalId,
    pub instance_root: String,
    pub project_root: String,
    pub member: String,
    pub store_path: String,
    pub manifest_sha256: String,
    pub binding_sha256: String,
}

impl ProjectAuthorityBinding {
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.format != PROJECT_AUTHORITY_FORMAT {
            return Err(format!(
                "unsupported project authority format {} (expected {})",
                self.format, PROJECT_AUTHORITY_FORMAT
            )
            .into());
        }
        for (name, value) in [
            ("instance_root", self.instance_root.as_str()),
            ("project_root", self.project_root.as_str()),
            ("member", self.member.as_str()),
            ("store_path", self.store_path.as_str()),
        ] {
            if value.is_empty() || value.as_bytes().contains(&0) {
                return Err(format!("project authority {name} is empty or contains NUL").into());
            }
        }
        validate_sha256(&self.manifest_sha256, "manifest_sha256")?;
        validate_sha256(&self.binding_sha256, "binding_sha256")?;
        if self.binding_sha256 != self.computed_binding_sha256() {
            return Err("project authority binding digest does not match its fields".into());
        }
        Ok(())
    }

    fn computed_binding_sha256(&self) -> String {
        digest::sha256_hex(
            &serde_json::to_vec(&(
                self.format,
                &self.instance_id,
                &self.instance_root,
                &self.project_root,
                &self.member,
                &self.store_path,
                &self.manifest_sha256,
            ))
            .expect("project authority fields serialize"),
        )
    }
}

impl InstanceManifest {
    pub fn dedicated(id: impl Into<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let manifest = Self {
            format: INSTANCE_FORMAT,
            id: id.into(),
            mode: InstanceMode::Dedicated,
            members: vec![PathBuf::from(".")],
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn umbrella(
        id: impl Into<String>,
        members: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manifest = Self {
            format: INSTANCE_FORMAT,
            id: id.into(),
            mode: InstanceMode::Umbrella,
            members: members.into_iter().collect(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = root.join(INSTANCE_FILE);
        let raw = std::fs::read_to_string(&path).map_err(|error| {
            format!("cannot read instance manifest {}: {error}", path.display())
        })?;
        let manifest: Self = toml::from_str(&raw).map_err(|error| {
            format!("cannot parse instance manifest {}: {error}", path.display())
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Initializes a dedicated instance unless a valid manifest already
    /// exists. Existing topology is never rewritten as a side effect of
    /// harness setup.
    pub fn ensure_dedicated(root: &Path) -> Result<(Self, bool), Box<dyn std::error::Error>> {
        let id = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .map(default_instance_id)
            .unwrap_or_else(|| "rrflow-instance".into());
        Self::ensure_dedicated_as(root, id)
    }

    /// Initializes or verifies a dedicated instance with an operator-selected
    /// identity. This is the provisioning boundary used by estate and cluster
    /// controllers before they start `rrd-server`.
    pub fn ensure_dedicated_as(
        root: &Path,
        id: impl Into<String>,
    ) -> Result<(Self, bool), Box<dyn std::error::Error>> {
        let expected = Self::dedicated(id)?;
        let path = root.join(INSTANCE_FILE);
        if path.exists() {
            let existing = Self::load(root)?;
            if existing != expected {
                return Err(format!(
                    "instance manifest {} does not match requested dedicated instance {}",
                    path.display(),
                    expected.id
                )
                .into());
            }
            return Ok((existing, false));
        }

        let parent = path.parent().expect("instance manifest has a parent");
        std::fs::create_dir_all(parent)?;
        let sequence = MANIFEST_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            "instance.toml.{}.{}.new",
            std::process::id(),
            sequence
        ));
        let encoded = toml::to_string_pretty(&expected)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options.open(&temporary)?;
            file.write_all(encoded.as_bytes())?;
            file.sync_all()?;
            drop(file);
            match std::fs::hard_link(&temporary, &path) {
                Ok(()) => {
                    std::fs::remove_file(&temporary)?;
                    sync_parent(&path)?;
                    Ok((expected.clone(), true))
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    std::fs::remove_file(&temporary)?;
                    let existing = Self::load(root)?;
                    if existing == expected {
                        Ok((existing, false))
                    } else {
                        Err(format!(
                            "instance manifest {} was concurrently created with a different identity",
                            path.display()
                        )
                        .into())
                    }
                }
                Err(error) => Err(error.into()),
            }
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.format != INSTANCE_FORMAT {
            return Err(format!(
                "unsupported instance manifest format {} (expected {})",
                self.format, INSTANCE_FORMAT
            )
            .into());
        }
        if self.id.trim().is_empty() || self.id.as_bytes().contains(&0) {
            return Err("instance id must be non-empty and contain no NUL".into());
        }
        CanonicalId::new(self.id.clone())
            .map_err(|error| format!("instance id is not canonical: {error}"))?;
        if self.members.is_empty() {
            return Err("instance must declare at least one member".into());
        }

        let mut unique = BTreeSet::new();
        for member in &self.members {
            validate_member(member)?;
            if !unique.insert(member.clone()) {
                return Err(format!("duplicate instance member {}", member.display()).into());
            }
        }

        if self.mode == InstanceMode::Dedicated && self.members.as_slice() != [PathBuf::from(".")] {
            return Err("dedicated instances must contain exactly the `.` member".into());
        }
        if self.mode == InstanceMode::Umbrella
            && self.members.iter().any(|member| member == Path::new("."))
        {
            return Err("umbrella instances must name each member; `.` would include the whole root implicitly".into());
        }
        Ok(())
    }

    /// Tests declared membership only. Filesystem existence and canonical
    /// containment are checked when the runtime binds a manifest to a root.
    pub fn admits(&self, member: &Path) -> bool {
        self.members.iter().any(|candidate| candidate == member)
    }
}

fn default_instance_id(name: &str) -> String {
    let mut canonical = String::with_capacity(name.len());
    let mut separator = false;
    for byte in name.bytes().map(|byte| byte.to_ascii_lowercase()) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.') {
            canonical.push(char::from(byte));
            separator = false;
        } else if !canonical.is_empty() && !separator {
            canonical.push('-');
            separator = true;
        }
    }
    while canonical.ends_with(['-', '.', '_']) {
        canonical.pop();
    }
    while canonical.starts_with(['-', '.', '_']) {
        canonical.remove(0);
    }
    if canonical.is_empty() {
        "rrflow-instance".into()
    } else {
        canonical
    }
}

impl InstanceBinding {
    /// Finds the nearest enclosing manifest and proves that `project_root` is
    /// an admitted root. Nearest wins so a dedicated major instance nested
    /// beneath an umbrella cannot accidentally bind to the umbrella.
    pub fn discover(project_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let project_root = std::fs::canonicalize(project_root).map_err(|error| {
            format!(
                "cannot establish project root {}: {error}",
                project_root.display()
            )
        })?;
        if !project_root.is_dir() {
            return Err(
                format!("project root {} is not a directory", project_root.display()).into(),
            );
        }

        let instance_root = project_root
            .ancestors()
            .find(|candidate| candidate.join(INSTANCE_FILE).is_file())
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                format!(
                    "no RRFlow instance contains {}; run `rrflow init --root {}` first",
                    project_root.display(),
                    project_root.display()
                )
            })?;
        let manifest = InstanceManifest::load(&instance_root)?;

        let member = match manifest.mode {
            InstanceMode::Dedicated => {
                if project_root != instance_root {
                    return Err(format!(
                        "dedicated instance {} admits only its root {}, not {}",
                        manifest.id,
                        instance_root.display(),
                        project_root.display()
                    )
                    .into());
                }
                PathBuf::from(".")
            }
            InstanceMode::Umbrella => {
                let mut matched = None;
                for member in &manifest.members {
                    let path = instance_root.join(member);
                    let canonical = std::fs::canonicalize(&path).map_err(|error| {
                        format!(
                            "umbrella member {} cannot be resolved beneath {}: {error}",
                            member.display(),
                            instance_root.display()
                        )
                    })?;
                    if !canonical.starts_with(&instance_root) {
                        return Err(format!(
                            "umbrella member {} escapes instance root {}",
                            member.display(),
                            instance_root.display()
                        )
                        .into());
                    }
                    if canonical == project_root {
                        matched = Some(member.clone());
                    }
                }
                matched.ok_or_else(|| {
                    format!(
                        "project {} is not an explicit member of umbrella instance {}",
                        project_root.display(),
                        manifest.id
                    )
                })?
            }
        };

        Ok(Self {
            manifest,
            instance_root,
            project_root,
            member,
        })
    }

    /// Umbrella declarations are valid configuration, but execution remains
    /// closed until every mutable projection and ledger is member-scoped.
    pub fn require_runtime_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.manifest.mode == InstanceMode::Umbrella {
            return Err(format!(
                "umbrella instance {} is declared but runtime execution is postponed until member-scoped routing, reasoning, and policy state land",
                self.manifest.id
            )
            .into());
        }
        Ok(())
    }

    pub fn expected_store(&self) -> PathBuf {
        self.instance_root.join(super::STORE_DIR)
    }

    /// Compares destinations after resolving their existing parent
    /// directories. The store itself may not exist yet, but symlinked parents
    /// and relative spellings cannot make a foreign store look local.
    pub fn verify_store_path(&self, store: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let requested = intended_path(store)?;
        let expected = intended_path(&self.expected_store())?;
        if requested != expected {
            return Err(format!(
                "database {} does not belong to instance {} (expected {})",
                requested.display(),
                self.manifest.id,
                expected.display()
            )
            .into());
        }
        Ok(requested)
    }

    pub fn authority_binding(&self) -> Result<ProjectAuthorityBinding, Box<dyn std::error::Error>> {
        self.require_runtime_ready()?;
        let store_path = self.verify_store_path(&self.expected_store())?;
        let instance_root = utf8_path(&self.instance_root, "instance root")?;
        let project_root = utf8_path(&self.project_root, "project root")?;
        let member = utf8_path(&self.member, "instance member")?;
        let store_path = utf8_path(&store_path, "store path")?;
        let manifest_sha256 = digest::sha256_hex(&serde_json::to_vec(&(
            self.manifest.format,
            &self.manifest.id,
            self.manifest.mode,
            &self.manifest.members,
        ))?);
        let mut authority = ProjectAuthorityBinding {
            format: PROJECT_AUTHORITY_FORMAT,
            instance_id: CanonicalId::new(self.manifest.id.clone())?,
            instance_root,
            project_root,
            member,
            store_path,
            manifest_sha256,
            binding_sha256: String::new(),
        };
        authority.binding_sha256 = authority.computed_binding_sha256();
        authority.validate()?;
        Ok(authority)
    }
}

impl RrdEngine {
    /// Persists or verifies the immutable project authority for this database.
    /// A mismatched existing record is never rewritten by startup.
    pub fn bind_project_authority(
        &self,
        binding: &InstanceBinding,
        at: u64,
    ) -> crate::Result<ProjectAuthorityBinding> {
        if at == 0 {
            return Err(ServiceError::Contract(
                "project authority binding time must be non-zero".into(),
            ));
        }
        let authority = binding
            .authority_binding()
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        if authority.instance_id != *self.instance_id() {
            return Err(ServiceError::ProjectBindingMismatch);
        }
        let engine_store = intended_path(self.storage.path())
            .map_err(|error| ServiceError::Contract(error.to_string()))?;
        if authority.store_path
            != utf8_path(&engine_store, "engine store path")
                .map_err(|error| ServiceError::Contract(error.to_string()))?
        {
            return Err(ServiceError::ProjectBindingMismatch);
        }
        let key = project_authority_key(self.instance_id());
        if let Some(encoded) = self.storage.control_record(&key)? {
            let existing: ProjectAuthorityBinding = serde_json::from_slice(&encoded)
                .map_err(|error| ServiceError::Storage(error.to_string()))?;
            existing
                .validate()
                .map_err(|error| ServiceError::Storage(error.to_string()))?;
            return if existing == authority {
                Ok(existing)
            } else {
                Err(ServiceError::ProjectBindingMismatch)
            };
        }
        self.storage.commit_control_transition(&ControlTransition {
            key,
            expected: None,
            replacement: Some(
                serde_json::to_vec(&authority)
                    .map_err(|error| ServiceError::Storage(error.to_string()))?,
            ),
            at,
            actor: "rrd-engine".into(),
            action: "project.authority.bound".into(),
            request_id: format!("project-bind-{}", &authority.binding_sha256[..24]),
            operation_id: format!("project-bind-{}", &authority.binding_sha256[..32]),
        })?;
        Ok(authority)
    }

    pub fn project_authority_binding(&self) -> crate::Result<Option<ProjectAuthorityBinding>> {
        let Some(encoded) = self
            .storage
            .control_record(&project_authority_key(self.instance_id()))?
        else {
            return Ok(None);
        };
        let binding: ProjectAuthorityBinding = serde_json::from_slice(&encoded)
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        binding
            .validate()
            .map_err(|error| ServiceError::Storage(error.to_string()))?;
        Ok(Some(binding))
    }
}

fn project_authority_key(instance: &CanonicalId) -> String {
    format!("{PROJECT_AUTHORITY_KEY_PREFIX}/{}", instance.as_str())
}

fn utf8_path(path: &Path, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} must be valid UTF-8").into())
}

fn validate_sha256(value: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(format!("project authority {name} must be lowercase SHA-256 hex").into())
    }
}

fn intended_path(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let parent = absolute
        .parent()
        .ok_or_else(|| format!("path {} has no parent", absolute.display()))?;
    let name = absolute
        .file_name()
        .ok_or_else(|| format!("path {} has no final component", absolute.display()))?;
    Ok(std::fs::canonicalize(parent)?.join(name))
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(path.parent().expect("persisted path has a parent"))?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn validate_member(member: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if member.as_os_str().is_empty() || member.is_absolute() {
        return Err(format!(
            "instance member {} must be a non-empty relative path",
            member.display()
        )
        .into());
    }
    if member == Path::new(".") {
        return Ok(());
    }
    for component in member.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(format!(
                "instance member {} must not contain parent, root, or current-directory components",
                member.display()
            )
            .into());
        }
    }
    if member.starts_with(".rrflow") {
        return Err("the runtime state directory cannot be an instance member".into());
    }
    Ok(())
}
