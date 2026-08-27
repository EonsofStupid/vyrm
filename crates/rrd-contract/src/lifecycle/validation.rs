use crate::{invalid, Result};

pub(super) fn optional_identity(name: &str, value: &Option<String>) -> Result<()> {
    value
        .as_deref()
        .map(|value| identity(name, value))
        .unwrap_or(Ok(()))
}

pub(super) fn identity(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return invalid(format!("invalid lifecycle {name}"));
    }
    Ok(())
}

pub(super) fn text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains('\0') {
        return invalid(format!("invalid lifecycle {name}"));
    }
    Ok(())
}

pub(super) fn optional_sha256(name: &str, value: &Option<String>) -> Result<()> {
    value
        .as_deref()
        .map(|value| sha256(name, value))
        .unwrap_or(Ok(()))
}

pub(super) fn sha256(name: &str, value: &str) -> Result<()> {
    hex(name, value, 64)
}

pub(super) fn hex(name: &str, value: &str, len: usize) -> Result<()> {
    if value.len() != len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return invalid(format!(
            "lifecycle {name} must be {len} lowercase hex characters"
        ));
    }
    Ok(())
}
