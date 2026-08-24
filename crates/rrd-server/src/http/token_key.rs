use super::*;

pub fn load_or_create_token_key(path: &Path) -> Result<[u8; TOKEN_KEY_BYTES]> {
    match read_token_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
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
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            read_token_key(path).map_err(Into::into)
        }
        Err(error) => Err(error.into()),
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
