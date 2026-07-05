//! Small filesystem helpers: atomic, permission-locked writes.

use anyhow::{Context, Result};
use std::path::Path;

/// Write `bytes` to `path` atomically with mode 0600.
///
/// Writes to a sibling temp file (so the rename is atomic on the same filesystem),
/// sets the permissions before the rename, then renames into place. This never
/// leaves a half-written or world-readable secret file behind.
pub fn write_secure(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .context("target path has no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("target path has no file name")?;
    let tmp = dir.join(format!(".{file_name}.samsara.tmp"));

    write_bytes_0600(&tmp, bytes).with_context(|| format!("writing temp {}", tmp.display()))?;

    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn write_bytes_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    // Ensure mode is 0600 even if the file pre-existed with looser perms.
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_bytes_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    Ok(())
}
