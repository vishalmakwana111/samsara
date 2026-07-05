//! Talking to (and controlling) the local opencode daemon.
//!
//! Discovery mirrors `packages/cli/src/services/daemon.ts`: a `server.json`
//! `{ url, pid, version }` plus a `password` file, both in the state dir. The server
//! authenticates with HTTP Basic `opencode:<password>` (`packages/server/src/auth.ts:52-63`).
//!
//! Because opencode caches provider config with an infinite TTL
//! (`packages/opencode/src/config/config.ts:281-289`), a running daemon will not notice a
//! swapped auth.json key until it restarts. `reload()` stops the daemon (SIGTERM); the next
//! opencode client request respawns it (`opencode serve --register`) and re-reads auth.json.

use crate::paths;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Registration {
    pub url: String,
    pub pid: i32,
    #[allow(dead_code)]
    #[serde(default)]
    pub version: Option<String>,
}

/// Read the daemon registration, if a server has registered itself.
pub fn registration() -> Result<Option<Registration>> {
    let path = paths::opencode_server_json()?;
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let reg: Registration =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(reg))
}

/// Read the daemon password, if present.
pub fn password() -> Result<Option<String>> {
    let path = paths::opencode_password_file()?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(
        std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .trim()
            .to_string(),
    ))
}

/// The HTTP Basic auth header value for the local server, if a password exists.
pub fn basic_auth_header() -> Result<Option<String>> {
    Ok(password()?.map(|pw| {
        use base64_lite::encode;
        format!("Basic {}", encode(format!("opencode:{pw}").as_bytes()))
    }))
}

/// Is a pid currently alive? (signal 0 probe)
#[cfg(unix)]
pub fn pid_alive(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), None).is_ok()
}

#[cfg(not(unix))]
pub fn pid_alive(_pid: i32) -> bool {
    false
}

/// True if a registered daemon appears to be running.
#[allow(dead_code)]
pub fn is_running() -> Result<bool> {
    Ok(match registration()? {
        Some(reg) => pid_alive(reg.pid),
        None => false,
    })
}

/// Stop the running daemon so it reloads config (auth.json) on its next respawn.
/// Returns true if a running server was signalled.
#[cfg(unix)]
pub fn reload() -> Result<bool> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let Some(reg) = registration()? else {
        return Ok(false);
    };
    if !pid_alive(reg.pid) {
        return Ok(false);
    }
    kill(Pid::from_raw(reg.pid), Signal::SIGTERM)
        .with_context(|| format!("sending SIGTERM to opencode server pid {}", reg.pid))?;
    Ok(true)
}

#[cfg(not(unix))]
pub fn reload() -> Result<bool> {
    anyhow::bail!("daemon reload is only supported on unix")
}

/// Detect credential overrides that would make an auth.json swap ineffective.
/// Returns a human-readable warning if `OPENCODE_API_KEY` is set (config-file
/// `provider.opencode.options.apiKey` is a separate, file-based override we note in docs).
pub fn credential_override_warning() -> Option<String> {
    match std::env::var("OPENCODE_API_KEY") {
        Ok(v) if !v.is_empty() => Some(
            "OPENCODE_API_KEY is set in the environment; it OVERRIDES the auth.json Zen key, \
             so samsara's key swaps will be ignored. Unset it for rotation to take effect."
                .to_string(),
        ),
        _ => None,
    }
}

/// Minimal base64 (standard alphabet, with padding) to avoid an extra crate for one header.
mod base64_lite {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[((n >> 6) & 63) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::encode;
        #[test]
        fn known_vectors() {
            assert_eq!(encode(b"opencode:pw"), "b3BlbmNvZGU6cHc=");
            assert_eq!(encode(b""), "");
            assert_eq!(encode(b"f"), "Zg==");
            assert_eq!(encode(b"fo"), "Zm8=");
            assert_eq!(encode(b"foo"), "Zm9v");
        }
    }
}
