//! Append-only rotation history (`~/.local/state/samsara/history.jsonl`).

use crate::model::now_secs;
use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub ts: u64,
    /// e.g. "rotate", "switch", "exhausted", "add", "remove".
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Event {
    pub fn new(kind: &str) -> Event {
        Event {
            ts: now_secs(),
            kind: kind.to_string(),
            from: None,
            to: None,
            note: None,
        }
    }
    pub fn from(mut self, s: Option<String>) -> Event {
        self.from = s;
        self
    }
    pub fn to(mut self, s: impl Into<String>) -> Event {
        self.to = Some(s.into());
        self
    }
    pub fn note(mut self, s: impl Into<String>) -> Event {
        self.note = Some(s.into());
        self
    }
}

/// Append an event to the history log (best-effort; never fails the caller's flow).
pub fn append(ev: &Event) {
    if let Err(e) = try_append(ev) {
        tracing::debug!("history append failed: {e:#}");
    }
}

fn try_append(ev: &Event) -> Result<()> {
    let path = paths::samsara_history_jsonl()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(ev)?;
    line.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Read the most recent `n` events (oldest → newest).
pub fn recent(n: usize) -> Vec<Event> {
    let Ok(path) = paths::samsara_history_jsonl() else {
        return Vec::new();
    };
    let Ok(file) = std::fs::File::open(&path) else {
        return Vec::new();
    };
    let mut all: Vec<Event> = std::io::BufReader::new(file)
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    if all.len() > n {
        all.drain(0..all.len() - n);
    }
    all
}
