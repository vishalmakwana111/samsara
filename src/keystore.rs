//! samsara's own key pool + rotation state, persisted at `~/.config/samsara/keys.json` (0600).
//!
//! This is samsara's source of truth for which keys exist, which is active, and per-key
//! cooldowns. The *active* key is mirrored into opencode's auth.json via `authfile`.

use crate::fsx;
use crate::model::{KeyEntry, now_secs};
use crate::paths;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct KeyStore {
    #[serde(default)]
    pub keys: Vec<KeyEntry>,
    /// Label of the key samsara considers active (mirrored into auth.json).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(skip)]
    path: PathBuf,
}

impl KeyStore {
    /// Load the store from the default location (empty if the file doesn't exist yet).
    pub fn load() -> Result<Self> {
        let path = paths::samsara_keys_json()?;
        Self::load_at(&path)
    }

    pub fn load_at(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(KeyStore {
                path: path.to_path_buf(),
                ..Default::default()
            });
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut store: KeyStore = if text.trim().is_empty() {
            KeyStore::default()
        } else {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        };
        store.path = path.to_path_buf();
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self)?;
        fsx::write_secure(&self.path, &bytes)?;
        Ok(())
    }

    pub fn find(&self, label: &str) -> Option<&KeyEntry> {
        self.keys.iter().find(|k| k.label == label)
    }

    pub fn find_mut(&mut self, label: &str) -> Option<&mut KeyEntry> {
        self.keys.iter_mut().find(|k| k.label == label)
    }

    /// Add a new key. Fails if the label already exists.
    pub fn add(&mut self, label: String, key: String) -> Result<()> {
        if self.find(&label).is_some() {
            bail!("a key labelled '{label}' already exists (remove it first)");
        }
        self.keys.push(KeyEntry {
            label: label.clone(),
            key,
            cooling_until: None,
            cooling_since: None,
            last_error: None,
            added_at: Some(now_secs()),
        });
        // First key added becomes active by default.
        if self.active.is_none() {
            self.active = Some(label);
        }
        Ok(())
    }

    /// Remove a key by label. Returns the removed entry.
    pub fn remove(&mut self, label: &str) -> Result<KeyEntry> {
        let idx = self
            .keys
            .iter()
            .position(|k| k.label == label)
            .with_context(|| format!("no key labelled '{label}'"))?;
        let removed = self.keys.remove(idx);
        if self.active.as_deref() == Some(label) {
            // Fall back to the first remaining key (if any).
            self.active = self.keys.first().map(|k| k.label.clone());
        }
        Ok(removed)
    }

    /// Mark a key as cooling down until `until` (unix secs), with an error note.
    pub fn set_cooldown(&mut self, label: &str, until: u64, error: Option<String>) -> Result<()> {
        let entry = self
            .find_mut(label)
            .with_context(|| format!("no key labelled '{label}'"))?;
        entry.cooling_since = Some(now_secs());
        entry.cooling_until = Some(until);
        entry.last_error = error;
        Ok(())
    }

    /// The active key entry, if set.
    pub fn active_entry(&self) -> Option<&KeyEntry> {
        self.active.as_deref().and_then(|l| self.find(l))
    }

    /// Pick the next available (non-cooling) key after the active one, round-robin.
    /// Returns `None` if every key is cooling.
    pub fn next_available(&self, now: u64) -> Option<&KeyEntry> {
        if self.keys.is_empty() {
            return None;
        }
        let start = self
            .active
            .as_deref()
            .and_then(|a| self.keys.iter().position(|k| k.label == a))
            .unwrap_or(0);
        let n = self.keys.len();
        // Scan the ring starting just after the active key; allow reselecting active last.
        for offset in 1..=n {
            let idx = (start + offset) % n;
            let cand = &self.keys[idx];
            if !cand.is_cooling(now) {
                return Some(cand);
            }
        }
        None
    }

    /// The soonest time (unix secs) any cooling key becomes available again.
    pub fn soonest_reset(&self, now: u64) -> Option<u64> {
        self.keys
            .iter()
            .filter_map(|k| k.cooling_until.filter(|&u| u > now))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(labels: &[&str]) -> KeyStore {
        let mut s = KeyStore::default();
        for l in labels {
            s.add((*l).to_string(), format!("key-{l}")).unwrap();
        }
        s
    }

    #[test]
    fn add_sets_first_active_and_rejects_dupes() {
        let mut s = store_with(&["a", "b"]);
        assert_eq!(s.active.as_deref(), Some("a"));
        assert!(s.add("a".into(), "x".into()).is_err());
    }

    #[test]
    fn remove_reassigns_active() {
        let mut s = store_with(&["a", "b"]);
        s.remove("a").unwrap();
        assert_eq!(s.active.as_deref(), Some("b"));
        s.remove("b").unwrap();
        assert_eq!(s.active, None);
    }

    #[test]
    fn next_available_skips_cooling_and_wraps() {
        let mut s = store_with(&["a", "b", "c"]);
        // active = a. b cooling → should pick c.
        s.set_cooldown("b", 1000, None).unwrap();
        assert_eq!(s.next_available(500).unwrap().label, "c");
        // c also cooling → wraps, a not cooling → picks a (reselect active last)
        s.set_cooldown("c", 1000, None).unwrap();
        assert_eq!(s.next_available(500).unwrap().label, "a");
        // everything cooling → None
        s.set_cooldown("a", 1000, None).unwrap();
        assert!(s.next_available(500).is_none());
        assert_eq!(s.soonest_reset(500), Some(1000));
    }

    #[test]
    fn round_trip_persists() {
        let mut path = std::env::temp_dir();
        path.push(format!("samsara-ks-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path.push("keys.json");

        let mut s = KeyStore::load_at(&path).unwrap();
        s.add("a".into(), "secret-a".into()).unwrap();
        s.set_cooldown("a", 9999, Some("limit".into())).unwrap();
        s.save().unwrap();

        let loaded = KeyStore::load_at(&path).unwrap();
        assert_eq!(loaded.keys.len(), 1);
        assert_eq!(loaded.active.as_deref(), Some("a"));
        assert_eq!(loaded.keys[0].cooling_until, Some(9999));
        assert_eq!(loaded.keys[0].last_error.as_deref(), Some("limit"));
    }
}
