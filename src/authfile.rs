//! Read-modify-write of opencode's `auth.json`.
//!
//! auth.json is a JSON object keyed by provider id. The Zen credential lives under
//! `"opencode": { "type": "api", "key": "..." }`
//! (`packages/opencode/src/auth/index.ts:10,14-36`). We only ever touch the `opencode`
//! entry and preserve everything else (e.g. an `openrouter` credential). The file is
//! mode 0600 and we keep it that way.

use crate::fsx;
use crate::model::ZEN_PROVIDER_ID;
use crate::paths;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::path::Path;

/// Read the current Zen (`opencode`) API key from auth.json, if present.
pub fn read_zen_key() -> Result<Option<String>> {
    read_key(ZEN_PROVIDER_ID)
}

/// Read the current API key for `provider_id` from auth.json, if present.
pub fn read_key(provider_id: &str) -> Result<Option<String>> {
    let path = paths::opencode_auth_json()?;
    read_key_at(&path, provider_id)
}

/// Overwrite the API key for `provider_id` in auth.json, preserving all other entries.
pub fn set_key(provider_id: &str, key: &str) -> Result<()> {
    let path = paths::opencode_auth_json()?;
    set_key_at(&path, provider_id, key)
}

fn load(path: &Path) -> Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("{} is not a JSON object", path.display()),
    }
}

fn read_key_at(path: &Path, provider_id: &str) -> Result<Option<String>> {
    let map = load(path)?;
    Ok(map
        .get(provider_id)
        .and_then(|entry| entry.get("key"))
        .and_then(|k| k.as_str())
        .map(|s| s.to_string()))
}

fn set_key_at(path: &Path, provider_id: &str, key: &str) -> Result<()> {
    let mut map = load(path)?;

    // Preserve any existing metadata on the provider entry; only force type + key.
    let entry = map
        .entry(provider_id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let obj = match entry {
        Value::Object(o) => o,
        other => {
            *other = Value::Object(Map::new());
            other.as_object_mut().expect("just set to object")
        }
    };
    obj.insert("type".to_string(), Value::String("api".to_string()));
    obj.insert("key".to_string(), Value::String(key.to_string()));

    let bytes = serde_json::to_vec_pretty(&Value::Object(map))?;
    fsx::write_secure(path, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("samsara-test-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        dir.push("auth.json");
        dir
    }

    #[test]
    fn set_key_preserves_other_providers() {
        let path = tmp_path("preserve");
        // seed with an openrouter credential + an existing opencode key with metadata
        let seed = serde_json::json!({
            "openrouter": { "type": "api", "key": "or-secret-123" },
            "opencode": { "type": "api", "key": "old-zen", "metadata": { "note": "keep" } }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&seed).unwrap()).unwrap();

        set_key_at(&path, ZEN_PROVIDER_ID, "new-zen-key").unwrap();

        let map = load(&path).unwrap();
        // openrouter untouched
        assert_eq!(map["openrouter"]["key"], "or-secret-123");
        // opencode key swapped, type forced to api, metadata preserved
        assert_eq!(map["opencode"]["key"], "new-zen-key");
        assert_eq!(map["opencode"]["type"], "api");
        assert_eq!(map["opencode"]["metadata"]["note"], "keep");

        assert_eq!(
            read_key_at(&path, ZEN_PROVIDER_ID).unwrap().as_deref(),
            Some("new-zen-key")
        );
    }

    #[test]
    fn set_key_creates_file_when_missing() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        set_key_at(&path, ZEN_PROVIDER_ID, "fresh").unwrap();
        assert_eq!(
            read_key_at(&path, ZEN_PROVIDER_ID).unwrap().as_deref(),
            Some("fresh")
        );
    }

    #[cfg(unix)]
    #[test]
    fn written_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp_path("perms");
        set_key_at(&path, ZEN_PROVIDER_ID, "secret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "auth.json must be owner-only");
    }

    #[test]
    fn read_missing_key_is_none() {
        let path = tmp_path("none");
        std::fs::write(&path, br#"{"openrouter":{"type":"api","key":"x"}}"#).unwrap();
        assert_eq!(read_key_at(&path, ZEN_PROVIDER_ID).unwrap(), None);
    }
}
