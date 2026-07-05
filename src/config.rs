//! samsara settings (`~/.config/samsara/config.json`).

use crate::fsx;
use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_cooldown_secs() -> u64 {
    12 * 3600
}

/// How the next key is chosen when the active one burns out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    /// Cycle to the next key after the active one.
    #[default]
    RoundRobin,
    /// Prefer pinned, then highest priority, then least-recently-active.
    Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Fallback cooldown (seconds) when the server sends no retry-after.
    #[serde(default = "default_cooldown_secs")]
    pub default_cooldown_secs: u64,
    /// Rotation policy.
    #[serde(default)]
    pub policy: Policy,
    /// Show a native desktop banner on rotation / exhaustion (macOS).
    #[serde(default)]
    pub notify_banner: bool,
    /// POST a JSON payload to this URL on rotation / exhaustion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_webhook: Option<String>,
    /// Store key secrets in the OS keychain instead of plaintext keys.json.
    #[serde(default)]
    pub keychain: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_cooldown_secs: default_cooldown_secs(),
            policy: Policy::default(),
            notify_banner: false,
            notify_webhook: None,
            keychain: false,
        }
    }
}

impl Settings {
    /// Load settings, falling back to defaults if the file is absent.
    pub fn load() -> Result<Self> {
        let path = paths::samsara_config_json()?;
        if !path.exists() {
            return Ok(Settings::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        if text.trim().is_empty() {
            return Ok(Settings::default());
        }
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::samsara_config_json()?;
        fsx::write_secure(&path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
