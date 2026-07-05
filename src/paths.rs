//! Filesystem path resolution.
//!
//! opencode uses XDG base directories (via the `xdg` npm package) even on macOS —
//! confirmed empirically: data lives at `~/.local/share/opencode`, state at
//! `~/.local/state/opencode`. We replicate that resolution here rather than using
//! platform-native dirs so we always agree with opencode.
//! See `packages/core/src/global.ts:10-27` in the opencode source.

use anyhow::{Context, Result};
use std::path::PathBuf;

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable is not set")
}

fn xdg_dir(env: &str, default_suffix: &str) -> Result<PathBuf> {
    if let Some(val) = std::env::var_os(env)
        && !val.is_empty()
    {
        return Ok(PathBuf::from(val));
    }
    Ok(home()?.join(default_suffix))
}

/// opencode's data dir: `$XDG_DATA_HOME/opencode` or `~/.local/share/opencode`.
pub fn opencode_data_dir() -> Result<PathBuf> {
    Ok(xdg_dir("XDG_DATA_HOME", ".local/share")?.join("opencode"))
}

/// opencode's state dir: `$XDG_STATE_HOME/opencode` or `~/.local/state/opencode`.
pub fn opencode_state_dir() -> Result<PathBuf> {
    Ok(xdg_dir("XDG_STATE_HOME", ".local/state")?.join("opencode"))
}

/// opencode's credential file: `<data>/auth.json`.
pub fn opencode_auth_json() -> Result<PathBuf> {
    Ok(opencode_data_dir()?.join("auth.json"))
}

/// opencode's daemon registration file: `<state>/server.json`.
pub fn opencode_server_json() -> Result<PathBuf> {
    Ok(opencode_state_dir()?.join("server.json"))
}

/// opencode's daemon password file: `<state>/password`.
pub fn opencode_password_file() -> Result<PathBuf> {
    Ok(opencode_state_dir()?.join("password"))
}

/// samsara's own config dir: `$XDG_CONFIG_HOME/samsara` or `~/.config/samsara`.
pub fn samsara_config_dir() -> Result<PathBuf> {
    Ok(xdg_dir("XDG_CONFIG_HOME", ".config")?.join("samsara"))
}

/// samsara's key pool file: `<config>/keys.json`.
pub fn samsara_keys_json() -> Result<PathBuf> {
    Ok(samsara_config_dir()?.join("keys.json"))
}

/// samsara's settings file: `<config>/config.json`.
pub fn samsara_config_json() -> Result<PathBuf> {
    Ok(samsara_config_dir()?.join("config.json"))
}

/// samsara's state dir: `$XDG_STATE_HOME/samsara` or `~/.local/state/samsara`.
pub fn samsara_state_dir() -> Result<PathBuf> {
    Ok(xdg_dir("XDG_STATE_HOME", ".local/state")?.join("samsara"))
}

/// samsara's rotation history log: `<state>/history.jsonl`.
pub fn samsara_history_jsonl() -> Result<PathBuf> {
    Ok(samsara_state_dir()?.join("history.jsonl"))
}

/// samsara daemon PID file: `<state>/daemon.pid`.
pub fn samsara_pidfile() -> Result<PathBuf> {
    Ok(samsara_state_dir()?.join("daemon.pid"))
}
