//! `samsara update` — self-update the binary from the latest GitHub release.
//!
//! Checks the latest release tag, downloads the archive for the current platform,
//! verifies its SHA-256, and atomically replaces the running executable.

use crate::ui;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::Path;

const REPO: &str = "vishalmakwana111/samsara";

pub async fn run(force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let target = current_target()?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("samsara/", env!("CARGO_PKG_VERSION")))
        .build()?;

    println!(
        "{}",
        ui::mark(ui::VIOLET, "✦", "consulting the sky for a newer star…")
    );
    let tag = latest_tag(&client)
        .await
        .context("checking latest release")?;
    let latest = tag.trim_start_matches('v');

    if !force && latest == current {
        println!(
            "{}",
            ui::mark(
                ui::GREEN,
                "✦",
                &format!("already the brightest star (v{current})")
            )
        );
        return Ok(());
    }

    println!(
        "{}",
        ui::mark(
            ui::SAFFRON,
            "➤",
            &format!("v{current} → {tag} · pulling {target}")
        )
    );

    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let asset = format!("samsara-{target}.tar.gz");
    let checksum = format!("samsara-{target}.sha256");
    let archive = fetch(&client, &format!("{base}/{asset}"))
        .await
        .context("downloading archive")?;

    // verify sha256 (best-effort — warn if the checksum file is missing)
    match fetch(&client, &format!("{base}/{checksum}")).await {
        Ok(sumfile) => verify_sha256(&archive, &sumfile)?,
        Err(_) => println!(
            "{}",
            ui::mark(ui::ASH, "·", "no checksum published; trusting HTTPS")
        ),
    }

    let tmp = std::env::temp_dir().join(format!("samsara-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let _guard = TmpGuard(tmp.clone());

    let archive_path = tmp.join(&asset);
    std::fs::write(&archive_path, &archive)?;
    extract(&archive_path, &tmp)?;

    let new_bin = tmp.join("samsara");
    if !new_bin.exists() {
        bail!("downloaded archive did not contain a 'samsara' binary");
    }

    replace_running_exe(&new_bin)?;
    println!(
        "{}",
        ui::mark(
            ui::GOLD,
            "✧",
            &format!("reborn as {tag} — run `samsara --version` to confirm")
        )
    );
    Ok(())
}

/// The release target triple for the platform this binary was built for.
fn current_target() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        (os, arch) => bail!("no prebuilt binary for {os}/{arch} — build from source instead"),
    })
}

async fn latest_tag(client: &reqwest::Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let value: serde_json::Value = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    value
        .get("tag_name")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .context("release response had no tag_name")
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?
        .bytes()
        .await?
        .to_vec())
}

fn verify_sha256(archive: &[u8], sumfile: &[u8]) -> Result<()> {
    let expected = String::from_utf8_lossy(sumfile)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(archive);
    let actual = hex(&hasher.finalize());
    if expected.is_empty() {
        bail!("checksum file was empty");
    }
    if expected != actual {
        bail!("checksum mismatch (expected {expected}, got {actual}) — aborting");
    }
    println!("{}", ui::mark(ui::GREEN, "✦", "checksum verified"));
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .context("running tar (is it installed?)")?;
    if !status.success() {
        bail!("tar failed to extract the archive");
    }
    Ok(())
}

/// Atomically swap the new binary in for the currently running executable.
fn replace_running_exe(new_bin: &Path) -> Result<()> {
    let current = std::env::current_exe().context("locating the running executable")?;
    let dir = current.parent().context("executable has no parent dir")?;
    // Stage beside the target so the final rename is atomic (same filesystem).
    let staged = dir.join(".samsara.update.tmp");
    std::fs::copy(new_bin, &staged).with_context(|| {
        format!(
            "copying into {} (need write permission — re-run the installer with sudo if installed system-wide)",
            dir.display()
        )
    })?;
    set_exec(&staged)?;
    std::fs::rename(&staged, &current)
        .with_context(|| format!("replacing {}", current.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_exec(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_exec(_p: &Path) -> Result<()> {
    Ok(())
}

struct TmpGuard(std::path::PathBuf);
impl Drop for TmpGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
