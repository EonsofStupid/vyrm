//! Versioned deployment boundary for a vyrm/connectome instance.
//!
//! A major platform owns a dedicated instance. Related small projects may be
//! admitted to an umbrella, but only by an explicit relative member path. The
//! manifest deliberately contains no canonical absolute paths so an instance
//! can be relocated without becoming a different instance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub const INSTANCE_FORMAT: u32 = 1;
pub const INSTANCE_FILE: &str = ".vyrm/instance.toml";

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
        let path = root.join(INSTANCE_FILE);
        if path.exists() {
            return Ok((Self::load(root)?, false));
        }

        let id = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("vyrm-instance");
        let manifest = Self::dedicated(id)?;
        std::fs::create_dir_all(path.parent().expect("instance manifest has a parent"))?;
        let temporary = path.with_extension("toml.new");
        std::fs::write(&temporary, toml::to_string_pretty(&manifest)?)?;
        std::fs::rename(&temporary, &path)?;
        Ok((manifest, true))
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
                    "no vyrm instance contains {}; run `vyrm init --root {}` first",
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
        self.instance_root.join(crate::STORE_DIR)
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
    if member.starts_with(".vyrm") {
        return Err("the runtime state directory cannot be an instance member".into());
    }
    Ok(())
}
