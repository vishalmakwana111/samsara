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
        // single-instance guard
        if let Some(pid) = crate::service::daemon_pid() {
            anyhow::bail!("a samsara daemon is already running (pid {pid})");
        }
        crate::service::write_pidfile()?;
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
        if let Some(mut hit) = detect_limit(&json).or_else(|| detect_hard_limit(&json)) {
            hit.session = find_session_id(&json);
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
    let session = hit.session.clone();
    match rotor::rotate(&mut store, &hit, default_cooldown)? {
        rotor::Outcome::Switched { from, to } => {
            println!("{}", crate::ui::comet(from.as_deref().unwrap_or("?"), &to));
            crate::notify::rotation(from.as_deref(), &to).await;
            if let Some(sid) = session {
                tracing::info!(
                    "session {sid} will continue on '{to}' at opencode's next interaction"
                );
            }
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
            crate::notify::exhausted(wait).await;
        }
        rotor::Outcome::Empty => tracing::error!("no keys in pool"),
    }
    Ok(())
}

/// Live full-screen dashboard: a twinkling constellation + recent history, refreshed
/// in place. Uses the alternate screen buffer; Ctrl-C restores the terminal.
pub async fn watch() -> Result<()> {
    use std::io::Write;
    print!("\x1b[?1049h\x1b[?25l"); // alt screen + hide cursor
    let _ = std::io::stdout().flush();

    let res = tokio::select! {
        _ = tokio::signal::ctrl_c() => Ok(()),
        r = watch_loop() => r,
    };

    print!("\x1b[?25h\x1b[?1049l"); // restore
    let _ = std::io::stdout().flush();
    res
}

async fn watch_loop() -> Result<()> {
    use std::io::Write;
    let mut frame: u64 = 0;
    loop {
        let store = KeyStore::load()?;
        let now = now_secs();
        let pulse = if (frame / 2).is_multiple_of(2) {
            1.0
        } else {
            0.45
        };

        let mut out = String::from("\x1b[2J\x1b[H");
        out.push_str(&crate::ui::constellation(
            &store.keys,
            store.active.as_deref(),
            pulse,
        ));
        out.push('\n');

        // footer: daemon + soonest reset
        let daemon = match crate::service::daemon_pid() {
            Some(pid) => {
                crate::ui::paint(crate::ui::GREEN, &format!("daemon watching (pid {pid})"))
            }
            None => crate::ui::paint(crate::ui::ASH, "daemon not running"),
        };
        out.push_str(&format!("  {daemon}\n"));
        if let Some(reset) = store.soonest_reset(now) {
            out.push_str(&format!(
                "  {}\n",
                crate::ui::paint(
                    crate::ui::CYAN,
                    &format!(
                        "next rebirth in {}",
                        crate::ui::fmt_dur(reset.saturating_sub(now))
                    )
                )
            ));
        }

        // recent history tail
        let events = crate::history::recent(5);
        if !events.is_empty() {
            out.push_str(&format!(
                "\n  {}\n",
                crate::ui::paint_bold(crate::ui::GOLD, "recent")
            ));
            for e in events {
                let detail = match (e.from.as_deref(), e.to.as_deref()) {
                    (Some(f), Some(t)) => format!("{f} → {t}"),
                    (_, Some(t)) => t.to_string(),
                    _ => e.note.clone().unwrap_or_default(),
                };
                out.push_str(&format!(
                    "  {} {} {}\n",
                    crate::ui::paint(
                        crate::ui::ASH,
                        &format!("{:>7}s ago", now.saturating_sub(e.ts))
                    ),
                    e.kind,
                    detail
                ));
            }
        }
        out.push_str(&crate::ui::paint(crate::ui::ASH, "\n  Ctrl-C to exit\n"));

        print!("{out}");
        let _ = std::io::stdout().flush();
        tokio::time::sleep(Duration::from_millis(500)).await;
        frame += 1;
    }
}

/// Hard-limit markers Zen uses for credit/monthly exhaustion (often surfaced as 401),
/// caught as a fallback when there's no retryable `account_rate_limit` action.
const HARD_MARKERS: [&str; 5] = [
    "GoUsageLimitError",
    "BlackUsageLimitError",
    "MonthlyLimitError",
    "insufficient_quota",
    "quota_exceeded",
];

/// Fallback detector: if the event mentions a hard usage/credit limit anywhere, treat it as
/// a limit hit (no retry-after → default cooldown applies).
fn detect_hard_limit(value: &Value) -> Option<LimitHit> {
    let blob = value.to_string();
    let marker = HARD_MARKERS.iter().find(|m| blob.contains(**m))?;
    Some(LimitHit {
        retry_after_secs: None,
        message: format!("{marker} (hard limit)"),
        session: None,
    })
}

/// Recursively find a session id under a `sessionID` / `session_id` / `session` string field.
fn find_session_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(obj) => {
            for key in ["sessionID", "session_id", "sessionId"] {
                if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
                    return Some(s.to_string());
                }
            }
            obj.values().find_map(find_session_id)
        }
        Value::Array(arr) => arr.iter().find_map(find_session_id),
        _ => None,
    }
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
                    session: None,
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
