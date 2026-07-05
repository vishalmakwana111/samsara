//! The supervisor daemon: subscribe to opencode's event stream, detect usage-limit
//! hits, and rotate the active Zen key.
//!
//! Event stream: `GET {server}/api/event` (SSE) — `event.subscribe`
//! (`packages/protocol/src/groups/event.ts:35`). The usage-limit signal is a session
//! status of `type: "retry"` carrying `action.reason ∈ {account_rate_limit, free_tier_limit}`
//! and an absolute `next` retry time (`packages/opencode/src/session/processor.ts:661-673`,
//! `session/retry.ts:76-121`). The exact event envelope is confirmed at runtime via
//! `--debug-events`; detection here searches for the marker recursively so it is robust to
//! the surrounding shape.

use crate::keystore::KeyStore;
use crate::local;
use crate::model::now_secs;
use crate::rotor::{self, LimitHit};
use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::header::AUTHORIZATION;
use serde_json::Value;
use std::time::Duration;

const RETRY_REASONS: [&str; 2] = ["account_rate_limit", "free_tier_limit"];

pub async fn run(
    default_cooldown: Duration,
    dir: Option<String>,
    debug_events: bool,
) -> Result<()> {
    if !debug_events {
        let store = KeyStore::load()?;
        if store.keys.is_empty() {
            anyhow::bail!("no keys in the pool — add some with `samsara add <key>` first");
        }
        println!(
            "{}",
            crate::ui::constellation(&store.keys, store.active.as_deref(), 1.0)
        );
        println!(
            "{}",
            crate::ui::mark(
                crate::ui::VIOLET,
                "✦",
                &format!(
                    "watching the sky — {} stars, default cooldown {}",
                    store.keys.len(),
                    humantime::format_duration(default_cooldown)
                )
            )
        );
        if let Some(warn) = local::credential_override_warning() {
            tracing::warn!("{warn}");
        }
    }

    let client = reqwest::Client::new();
    let mut backoff = Duration::from_secs(1);

    loop {
        match connect_and_watch(&client, dir.as_deref(), default_cooldown, debug_events).await {
            Ok(()) => {
                // Stream ended cleanly (e.g. server restarted after our own rotation).
                backoff = Duration::from_secs(1);
                tracing::info!("event stream ended; reconnecting…");
            }
            Err(e) => {
                tracing::warn!(
                    "event stream error: {e:#}; retrying in {}s",
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn connect_and_watch(
    client: &reqwest::Client,
    dir: Option<&str>,
    default_cooldown: Duration,
    debug_events: bool,
) -> Result<()> {
    let reg = local::registration()?
        .filter(|r| local::pid_alive(r.pid))
        .context("opencode daemon is not running (start opencode, then samsara reconnects)")?;

    let mut url = format!("{}/api/event", reg.url.trim_end_matches('/'));
    if let Some(d) = dir {
        url.push_str(&format!("?directory={}", urlencode(d)));
    }

    let mut req = client.get(&url);
    if let Some(auth) = local::basic_auth_header()? {
        req = req.header(AUTHORIZATION, auth);
    }
    let resp = req.send().await?.error_for_status()?;
    tracing::info!("subscribed to {url}");

    let mut stream = resp.bytes_stream().eventsource();
    while let Some(event) = stream.next().await {
        let event = event.context("reading SSE event")?;
        if event.data.is_empty() {
            continue;
        }
        if debug_events {
            println!("event[{}]: {}", event.event, event.data);
            continue;
        }
        let Ok(json) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        if let Some(hit) = detect_limit(&json) {
            handle_hit(hit, default_cooldown).await?;
            // After rotating we SIGTERM the server; the stream will drop and we reconnect.
        }
    }
    Ok(())
}

async fn handle_hit(hit: LimitHit, default_cooldown: Duration) -> Result<()> {
    let mut store = KeyStore::load()?;
    let active = store.active.clone().unwrap_or_else(|| "(none)".into());
    tracing::warn!(
        "usage limit hit on active key '{active}': {} (retry_after={:?}s)",
        hit.message,
        hit.retry_after_secs
    );
    match rotor::rotate(&mut store, &hit, default_cooldown)? {
        rotor::Outcome::Switched { from, to } => {
            println!("{}", crate::ui::comet(from.as_deref().unwrap_or("?"), &to));
        }
        rotor::Outcome::AllCooling { resume_at } => {
            let wait = resume_at.saturating_sub(now_secs());
            println!(
                "{}",
                crate::ui::mark(
                    crate::ui::CYAN,
                    "·",
                    &format!(
                        "every star is dark — soonest rebirth in {}",
                        crate::ui::fmt_dur(wait)
                    )
                )
            );
        }
        rotor::Outcome::Empty => tracing::error!("no keys in pool"),
    }
    Ok(())
}

/// Recursively search an event payload for the retry/limit marker and extract a `LimitHit`.
/// Matches an object carrying `action.reason ∈ RETRY_REASONS` (optionally under `type:"retry"`),
/// reading a human message and deriving `retry_after` from an absolute `next` (ms) when present.
fn detect_limit(value: &Value) -> Option<LimitHit> {
    fn action_reason_matches(obj: &serde_json::Map<String, Value>) -> bool {
        obj.get("action")
            .and_then(|a| a.get("reason"))
            .and_then(|r| r.as_str())
            .map(|r| RETRY_REASONS.contains(&r))
            .unwrap_or(false)
    }

    match value {
        Value::Object(obj) => {
            if action_reason_matches(obj) {
                let message = obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .or_else(|| {
                        obj.get("action")
                            .and_then(|a| a.get("message"))
                            .and_then(|m| m.as_str())
                    })
                    .unwrap_or("usage limit reached")
                    .to_string();
                let retry_after_secs = obj
                    .get("next")
                    .and_then(|n| n.as_f64())
                    .map(|next_ms| {
                        let next_secs = (next_ms / 1000.0).round() as i64;
                        (next_secs - now_secs() as i64).max(0) as u64
                    })
                    .filter(|&s| s > 0);
                return Some(LimitHit {
                    retry_after_secs,
                    message,
                });
            }
            obj.values().find_map(detect_limit)
        }
        Value::Array(arr) => arr.iter().find_map(detect_limit),
        _ => None,
    }
}

/// Minimal percent-encoding for a directory path used as a query value.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_nested_retry_action() {
        // Shape modelled on status.set({type:"retry", message, action:{reason}, next})
        let ev = json!({
            "type": "session.status",
            "properties": {
                "status": {
                    "type": "retry",
                    "attempt": 2,
                    "message": "5 hour usage limit reached. It will reset in 3 hours",
                    "action": { "reason": "account_rate_limit", "provider": "opencode" },
                    "next": (now_secs() as f64 + 3600.0) * 1000.0
                }
            }
        });
        let hit = detect_limit(&ev).expect("should detect");
        assert!(hit.message.contains("usage limit"));
        let ra = hit.retry_after_secs.expect("retry_after from next");
        assert!((3595..=3605).contains(&ra), "got {ra}");
    }

    #[test]
    fn ignores_unrelated_events() {
        let ev = json!({"type":"message.updated","properties":{"part":{"type":"text"}}});
        assert!(detect_limit(&ev).is_none());
    }

    #[test]
    fn ignores_other_retry_reasons() {
        let ev = json!({"status":{"type":"retry","action":{"reason":"provider_overloaded"}}});
        assert!(detect_limit(&ev).is_none());
    }
}
