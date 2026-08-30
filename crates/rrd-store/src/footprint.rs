//! Filesystem footprint evidence for storage engines and operator diagnostics.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FootprintBytes {
    pub apparent_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub files: u64,
}

impl FootprintBytes {
    fn add_file(&mut self, apparent: u64, allocated: Option<u64>) -> io::Result<()> {
        self.apparent_bytes = checked_add(self.apparent_bytes, apparent)?;
        self.allocated_bytes = match (self.allocated_bytes, allocated) {
            (None, None) => None,
            (Some(total), Some(bytes)) => Some(checked_add(total, bytes)?),
            (None, Some(bytes)) if self.files == 0 => Some(bytes),
            _ => None,
        };
        self.files = checked_add(self.files, 1)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageFootprint {
    pub apparent_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub allocated_bytes_source: Option<String>,
    pub files: u64,
    pub by_class: BTreeMap<String, FootprintBytes>,
}

/// Measures regular files beneath `root` without following symbolic links.
///
/// Apparent bytes are portable file lengths. On Unix, allocated bytes use
/// `st_blocks * 512`, which distinguishes sparse/preallocated address space
/// from filesystem blocks charged to the file. Other targets return `None`
/// instead of treating apparent length as physical allocation.
pub fn measure_storage_footprint(root: impl AsRef<Path>) -> io::Result<StorageFootprint> {
    let root = root.as_ref();
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();

    let mut total = FootprintBytes::default();
    let mut by_class = BTreeMap::<String, FootprintBytes>::new();
    for relative in files {
        let metadata = fs::symlink_metadata(root.join(&relative))?;
        let apparent = metadata.len();
        let allocated = allocated_bytes(&metadata)?;
        total.add_file(apparent, allocated)?;
        by_class
            .entry(classify(&relative).to_owned())
            .or_default()
            .add_file(apparent, allocated)?;
    }

    Ok(StorageFootprint {
        apparent_bytes: total.apparent_bytes,
        allocated_bytes: total.allocated_bytes,
        allocated_bytes_source: total
            .allocated_bytes
            .map(|_| "unix_st_blocks_times_512".to_owned()),
        files: total.files,
        by_class,
    })
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "storage footprint refuses symbolic link {}",
                    entry.path().display()
                ),
            ));
        }
        if file_type.is_dir() {
            collect_files(root, &entry.path(), output)?;
        } else if file_type.is_file() {
            output.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("entry was discovered below root")
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn classify(relative: &Path) -> &'static str {
    let file_name = relative.file_name().and_then(|name| name.to_str());
    let extension = relative
        .extension()
        .and_then(|extension| extension.to_str());
    if extension == Some("wal") {
        "wal"
    } else if extension == Some("jnl") {
        "journal"
    } else if extension == Some("seg") {
        "segment"
    } else if relative
        .components()
        .any(|component| component.as_os_str() == "tables")
    {
        "table"
    } else if relative
        .components()
        .any(|component| component.as_os_str() == "checkpoints")
    {
        "checkpoint"
    } else if relative
        .components()
        .any(|component| component.as_os_str() == "manifests")
        || matches!(file_name, Some("CURRENT" | "current" | "version"))
    {
        "manifest"
    } else {
        "metadata"
    }
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> io::Result<Option<u64>> {
    use std::os::unix::fs::MetadataExt;

    metadata
        .blocks()
        .checked_mul(512)
        .map(Some)
        .ok_or_else(|| io::Error::other("allocated byte count overflowed"))
}

#[cfg(not(unix))]
fn allocated_bytes(_metadata: &fs::Metadata) -> io::Result<Option<u64>> {
    Ok(None)
}

fn checked_add(left: u64, right: u64) -> io::Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| io::Error::other("storage footprint byte count overflowed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};

    #[test]
    fn footprint_separates_sparse_length_and_allocated_blocks() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("0.jnl");
        let mut file = fs::File::create(&path).unwrap();
        file.set_len(16 * 1_024 * 1_024).unwrap();
        file.rewind().unwrap();
        file.write_all(b"journal").unwrap();
        file.sync_all().unwrap();

        let footprint = measure_storage_footprint(root.path()).unwrap();
        assert_eq!(footprint.apparent_bytes, 16 * 1_024 * 1_024);
        assert_eq!(footprint.files, 1);
        assert_eq!(footprint.by_class["journal"].files, 1);
        #[cfg(unix)]
        assert!(footprint.allocated_bytes.unwrap() < footprint.apparent_bytes);
        #[cfg(not(unix))]
        assert!(footprint.allocated_bytes.is_none());
    }

    #[test]
    fn footprint_attributes_canonical_storage_classes() {
        let root = tempfile::tempdir().unwrap();
        for relative in [
            "wal/1.wal",
            "segments/a.seg",
            "keyspaces/1/tables/0",
            "manifests/a.json",
            "checkpoints/pin",
            "lock",
        ] {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }

        let footprint = measure_storage_footprint(root.path()).unwrap();
        assert_eq!(footprint.files, 6);
        for class in [
            "wal",
            "segment",
            "table",
            "manifest",
            "checkpoint",
            "metadata",
        ] {
            assert_eq!(footprint.by_class[class].files, 1, "{class}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn footprint_refuses_symbolic_links_instead_of_escaping_the_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join("linked.wal")).unwrap();

        let error = measure_storage_footprint(root.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("refuses symbolic link"));
    }
}
