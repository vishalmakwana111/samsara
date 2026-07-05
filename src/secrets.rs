//! Optional secure key storage in the OS keychain.
//!
//! When enabled, `keys.json` stores a reference `keychain:<label>` instead of the raw key,
//! and the secret lives in the macOS Keychain (via the `security` CLI). `resolve()` turns a
//! stored value back into the real secret. Plaintext (0600) remains the default.

#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::{Result, bail};

const SENTINEL: &str = "keychain:";
#[cfg(target_os = "macos")]
const SERVICE: &str = "ai.samsara.key";

/// True if the stored value is a keychain reference rather than a raw key.
pub fn is_reference(stored: &str) -> bool {
    stored.starts_with(SENTINEL)
}

/// The reference string stored in keys.json for a given label.
pub fn reference(label: &str) -> String {
    format!("{SENTINEL}{label}")
}

/// Turn a stored value into the real secret (fetches from the keychain if it's a reference).
pub fn resolve(stored: &str) -> Result<String> {
    match stored.strip_prefix(SENTINEL) {
        Some(label) => get(label),
        None => Ok(stored.to_string()),
    }
}

pub fn available() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
pub fn set(label: &str, secret: &str) -> Result<()> {
    // -U updates if present; -w passes the secret.
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            SERVICE,
            "-a",
            label,
            "-w",
            secret,
        ])
        .status()
        .context("running `security`")?;
    if !status.success() {
        bail!("failed to store key '{label}' in the keychain");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn get(label: &str) -> Result<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE, "-a", label, "-w"])
        .output()
        .context("running `security`")?;
    if !out.status.success() {
        bail!("key '{label}' not found in the keychain");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub fn delete(label: &str) -> Result<()> {
    let _ = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", label])
        .status();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn set(_label: &str, _secret: &str) -> Result<()> {
    bail!("keychain storage is only available on macOS")
}
#[cfg(not(target_os = "macos"))]
pub fn get(_label: &str) -> Result<String> {
    bail!("keychain storage is only available on macOS")
}
#[cfg(not(target_os = "macos"))]
pub fn delete(_label: &str) -> Result<()> {
    Ok(())
}
