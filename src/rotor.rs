//! Rotation engine: decide + apply a key switch when the active key hits its limit.

use crate::authfile;
use crate::keystore::KeyStore;
use crate::local;
use crate::model::now_secs;
use anyhow::Result;
use std::time::Duration;

/// A detected usage-limit event for the currently-active account.
#[derive(Debug, Clone, Default)]
pub struct LimitHit {
    /// Seconds until the account's limit resets, if the server told us.
    pub retry_after_secs: Option<u64>,
    /// Human-readable reason/message (for logging + last_error).
    pub message: String,
    /// The opencode session that hit the limit, if we could identify it.
    pub session: Option<String>,
}

/// What happened when we tried to rotate.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Switched from `from` to `to`.
    Switched { from: Option<String>, to: String },
    /// Every key is cooling; wait until `resume_at` (unix secs).
    AllCooling { resume_at: u64 },
    /// No keys in the pool.
    Empty,
}

/// Compute the cooldown-until timestamp for a limit hit.
pub fn cooldown_until(now: u64, hit: &LimitHit, default_cooldown: Duration) -> u64 {
    let secs = hit.retry_after_secs.unwrap_or(default_cooldown.as_secs());
    now.saturating_add(secs)
}

/// Apply the rotation decision to the store (pure: no I/O). Marks the active key
/// cooling and moves `active` to the next available key. Returns the outcome.
pub fn decide(
    store: &mut KeyStore,
    hit: &LimitHit,
    default_cooldown: Duration,
    now: u64,
    policy: crate::config::Policy,
) -> Outcome {
    if store.keys.is_empty() {
        return Outcome::Empty;
    }
    let from = store.active.clone();
    if let Some(active) = from.clone() {
        let until = cooldown_until(now, hit, default_cooldown);
        let _ = store.set_cooldown(&active, until, Some(hit.message.clone()));
    }

    match store.select_next(policy, now).map(|k| k.label.clone()) {
        Some(next) => {
            store.make_active(&next);
            Outcome::Switched { from, to: next }
        }
        None => {
            let resume_at = store.soonest_reset(now).unwrap_or(now);
            Outcome::AllCooling { resume_at }
        }
    }
}

/// Full side-effecting rotation: decide, persist, mirror to auth.json, reload the daemon,
/// and record the event in history.
pub fn rotate(store: &mut KeyStore, hit: &LimitHit, default_cooldown: Duration) -> Result<Outcome> {
    let policy = crate::config::Settings::load()
        .map(|s| s.policy)
        .unwrap_or_default();
    let outcome = decide(store, hit, default_cooldown, now_secs(), policy);
    store.save()?;

    match &outcome {
        Outcome::Switched { from, to } => {
            if let Some(entry) = store.find(to) {
                authfile::set_key(entry.provider.auth_id(), &entry.key)?;
            }
            crate::history::append(
                &crate::history::Event::new("rotate")
                    .from(from.clone())
                    .to(to.clone())
                    .note(hit.message.clone()),
            );
            match local::reload() {
                Ok(true) => tracing::info!("restarted opencode daemon to load key '{to}'"),
                Ok(false) => {
                    tracing::info!("opencode not running; key '{to}' applies on next start")
                }
                Err(e) => tracing::warn!("could not reload daemon: {e:#}"),
            }
        }
        Outcome::AllCooling { .. } => crate::history::append(
            &crate::history::Event::new("exhausted").note(hit.message.clone()),
        ),
        Outcome::Empty => {}
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(labels: &[&str]) -> KeyStore {
        let mut s = KeyStore::default();
        for l in labels {
            s.add((*l).to_string(), format!("key-{l}")).unwrap();
        }
        s
    }

    #[test]
    fn switches_to_next_and_cools_active() {
        let mut s = store(&["a", "b"]);
        let hit = LimitHit {
            retry_after_secs: Some(3600),
            message: "12h limit".into(),
            session: None,
        };
        let outcome = decide(
            &mut s,
            &hit,
            Duration::from_secs(43200),
            1000,
            crate::config::Policy::RoundRobin,
        );
        assert_eq!(
            outcome,
            Outcome::Switched {
                from: Some("a".into()),
                to: "b".into()
            }
        );
        assert_eq!(s.active.as_deref(), Some("b"));
        assert_eq!(s.find("a").unwrap().cooling_until, Some(1000 + 3600));
    }

    #[test]
    fn uses_default_cooldown_when_no_retry_after() {
        let mut s = store(&["a", "b"]);
        let hit = LimitHit {
            retry_after_secs: None,
            message: "limit".into(),
            session: None,
        };
        decide(
            &mut s,
            &hit,
            Duration::from_secs(43200),
            1000,
            crate::config::Policy::RoundRobin,
        );
        assert_eq!(s.find("a").unwrap().cooling_until, Some(1000 + 43200));
    }

    #[test]
    fn all_cooling_reports_soonest_reset() {
        let mut s = store(&["a", "b"]);
        s.set_cooldown("b", 5000, None).unwrap();
        let hit = LimitHit {
            retry_after_secs: Some(1000),
            message: "limit".into(),
            session: None,
        };
        // active a gets cooled to 2000; b cools to 5000 → all cooling, soonest = 2000
        let outcome = decide(
            &mut s,
            &hit,
            Duration::from_secs(43200),
            1000,
            crate::config::Policy::RoundRobin,
        );
        assert_eq!(outcome, Outcome::AllCooling { resume_at: 2000 });
    }

    #[test]
    fn empty_pool() {
        let mut s = KeyStore::default();
        let hit = LimitHit {
            retry_after_secs: None,
            message: "x".into(),
            session: None,
        };
        assert_eq!(
            decide(
                &mut s,
                &hit,
                Duration::from_secs(1),
                0,
                crate::config::Policy::RoundRobin
            ),
            Outcome::Empty
        );
    }
}
