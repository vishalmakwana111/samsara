//! Shared data types for samsara.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// The opencode provider id under which the Zen credential is stored in auth.json.
pub const ZEN_PROVIDER_ID: &str = "opencode";

/// Current unix time in whole seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Which upstream provider a key authenticates to (maps to an auth.json entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Opencode,
    Openrouter,
    Anthropic,
}

impl Provider {
    /// The auth.json provider id this maps to.
    pub fn auth_id(&self) -> &'static str {
        match self {
            Provider::Opencode => "opencode",
            Provider::Openrouter => "openrouter",
            Provider::Anthropic => "anthropic",
        }
    }
    pub fn parse(s: &str) -> Option<Provider> {
        match s.to_lowercase().as_str() {
            "opencode" | "zen" => Some(Provider::Opencode),
            "openrouter" => Some(Provider::Openrouter),
            "anthropic" => Some(Provider::Anthropic),
            _ => None,
        }
    }
}

/// One API key in samsara's pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KeyEntry {
    /// Human label the user assigns (e.g. "work", "personal").
    pub label: String,
    /// The raw API key.
    pub key: String,
    /// Which provider this key is for (defaults to opencode Zen).
    #[serde(default)]
    pub provider: Provider,
    /// Unix seconds until which this key is cooling down after hitting its limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooling_until: Option<u64>,
    /// Unix seconds when the current cooldown began (for progress gauges).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooling_since: Option<u64>,
    /// Last limit/error message observed for this key (for `status`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Unix seconds when the key was added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<u64>,
    /// Workspace id (if discovered), used to warn about same-workspace duplicates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Excluded from rotation while true.
    #[serde(default)]
    pub disabled: bool,
    /// Preferred over unpinned keys when selecting the next key.
    #[serde(default)]
    pub pinned: bool,
    /// Higher priority is chosen first (ties broken by least-recently-active).
    #[serde(default)]
    pub priority: i32,
    /// Unix seconds this key was last made active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active: Option<u64>,
    /// How many times this key has hit its limit.
    #[serde(default)]
    pub limit_hits: u32,
}

impl KeyEntry {
    /// True if the key is currently cooling down (limit not yet reset).
    pub fn is_cooling(&self, now: u64) -> bool {
        matches!(self.cooling_until, Some(until) if until > now)
    }

    /// Seconds remaining on the cooldown, or 0 if available.
    pub fn cooldown_remaining(&self, now: u64) -> u64 {
        match self.cooling_until {
            Some(until) if until > now => until - now,
            _ => 0,
        }
    }

    /// Fraction of the cooldown already elapsed, in [0.0, 1.0]. 1.0 when not cooling.
    pub fn cooldown_progress(&self, now: u64) -> f32 {
        match (self.cooling_since, self.cooling_until) {
            (Some(since), Some(until)) if until > since && until > now => {
                let total = (until - since) as f32;
                let done = (now.saturating_sub(since)) as f32;
                (done / total).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// A masked form of the key safe to print (e.g. "sk-abc…wxyz").
    #[allow(dead_code)]
    pub fn masked(&self) -> String {
        mask_secret(&self.key)
    }
}

/// Mask a secret for display: keep a short head and tail, hide the middle.
#[allow(dead_code)]
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let n = chars.len();
    if n <= 10 {
        return format!("{}…({n} chars)", chars.first().copied().unwrap_or('*'));
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[n - 4..].iter().collect();
    format!("{head}…{tail} ({n} chars)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_logic() {
        let mut k = KeyEntry {
            label: "a".into(),
            key: "sk-1234567890abcdef".into(),
            ..Default::default()
        };
        assert!(!k.is_cooling(100));
        assert_eq!(k.cooldown_remaining(100), 0);

        k.cooling_until = Some(200);
        assert!(k.is_cooling(100));
        assert_eq!(k.cooldown_remaining(100), 100);
        assert!(!k.is_cooling(300));
        assert_eq!(k.cooldown_remaining(300), 0);
    }

    #[test]
    fn masking_hides_middle() {
        let secret = "sk-abcdef1234567890wxyz";
        let m = mask_secret(secret);
        assert!(m.starts_with("sk-abc"));
        assert!(m.ends_with(&format!("wxyz ({} chars)", secret.len())));
        assert!(!m.contains("def1234567890"));
        // short secrets do not leak
        let s = mask_secret("short");
        assert!(s.contains("chars"));
        assert!(!s.contains("hort"));
    }
}
