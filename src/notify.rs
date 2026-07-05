//! Notifications on rotation / exhaustion: a native desktop banner and/or a webhook POST.
//! Both are opt-in via `samsara config` and best-effort (never fail the caller).

use crate::config::Settings;
use crate::model::now_secs;
use serde_json::json;

/// Notify that samsara rotated from `from` to `to`.
pub async fn rotation(from: Option<&str>, to: &str) {
    let s = Settings::load().unwrap_or_default();
    let title = "samsara";
    let body = match from {
        Some(f) => format!("rotated {f} → {to}"),
        None => format!("now serving {to}"),
    };
    if s.notify_banner {
        banner(title, &body);
    }
    if let Some(url) = s.notify_webhook.as_deref() {
        webhook(
            url,
            json!({"event":"rotate","from":from,"to":to,"ts":now_secs()}),
        )
        .await;
    }
}

/// Notify that every key is exhausted; `reset_in` is seconds to the soonest rebirth.
pub async fn exhausted(reset_in: u64) {
    let s = Settings::load().unwrap_or_default();
    let body = format!(
        "all keys exhausted — soonest rebirth in {}",
        crate::ui::fmt_dur(reset_in)
    );
    if s.notify_banner {
        banner("samsara", &body);
    }
    if let Some(url) = s.notify_webhook.as_deref() {
        webhook(
            url,
            json!({"event":"exhausted","reset_in_secs":reset_in,"ts":now_secs()}),
        )
        .await;
    }
}

#[cfg(target_os = "macos")]
fn banner(title: &str, body: &str) {
    // osascript display notification (escape double quotes)
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

#[cfg(target_os = "linux")]
fn banner(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .arg(title)
        .arg(body)
        .status();
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn banner(_title: &str, _body: &str) {}

async fn webhook(url: &str, payload: serde_json::Value) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("webhook client error: {e}");
            return;
        }
    };
    if let Err(e) = client.post(url).json(&payload).send().await {
        tracing::debug!("webhook POST failed: {e}");
    }
}
