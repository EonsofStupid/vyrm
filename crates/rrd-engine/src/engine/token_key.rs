use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

pub const TOKEN_KEY_BYTES: usize = 32;
pub const API_KEY_HEX_BYTES: usize = TOKEN_KEY_BYTES * 2;

/// Loads the RRD token-signing key or creates it exactly once.
///
/// Key lifecycle belongs to the engine security authority. Transports and
/// embedded adapters consume this function; they do not implement competing
/// key formats or permission rules.
pub fn load_or_create_token_key(path: &Path) -> io::Result<[u8; TOKEN_KEY_BYTES]> {
    match read_token_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }

    let mut key = [0_u8; TOKEN_KEY_BYTES];
    getrandom::fill(&mut key)
        .map_err(|error| io::Error::other(format!("token-key entropy failed: {error}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key)?;
            file.sync_all()?;
            sync_parent(path)?;
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_token_key(path),
        Err(error) => Err(error),
    }
}

/// Loads or creates a printable 256-bit API credential in a private file.
///
/// This is deliberately separate from the binary token-signing key format:
/// clients can transmit it without lossy text conversion while retaining the
/// same entropy and permission guarantees.
pub fn load_or_create_api_key(path: &Path) -> io::Result<String> {
    match read_api_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }

    let mut entropy = [0_u8; TOKEN_KEY_BYTES];
    getrandom::fill(&mut entropy)
        .map_err(|error| io::Error::other(format!("API-key entropy failed: {error}")))?;
    let mut key = String::with_capacity(API_KEY_HEX_BYTES);
    for byte in entropy {
        use std::fmt::Write as _;
        write!(&mut key, "{byte:02x}").expect("writing to String cannot fail");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(key.as_bytes())?;
            file.sync_all()?;
            sync_parent(path)?;
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_api_key(path),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(path.parent().expect("token key has a parent"))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn read_token_key(path: &Path) -> io::Result<[u8; TOKEN_KEY_BYTES]> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "token key is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "token key must not be accessible to group or other users",
            ));
        }
    }

    let mut bytes = Vec::new();
    File::open(path)?
        .take((TOKEN_KEY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "token key must contain exactly 32 bytes",
        )
    })
}

fn read_api_key(path: &Path) -> io::Result<String> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "API key is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "API key must not be accessible to group or other users",
            ));
        }
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((API_KEY_HEX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() != API_KEY_HEX_BYTES
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("API key must contain exactly {API_KEY_HEX_BYTES} lowercase hexadecimal bytes"),
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_reopen_preserve_one_key() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("RRD.SECRET");
        let created = load_or_create_token_key(&path).unwrap();
        let reopened = load_or_create_token_key(&path).unwrap();
        assert_eq!(created, reopened);
        assert_ne!(created, [0_u8; TOKEN_KEY_BYTES]);
    }

    #[test]
    fn create_and_reopen_preserve_one_printable_api_key() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("CLIENT.SECRET");
        let created = load_or_create_api_key(&path).unwrap();
        let reopened = load_or_create_api_key(&path).unwrap();
        assert_eq!(created, reopened);
        assert_eq!(created.len(), API_KEY_HEX_BYTES);
    }
}
