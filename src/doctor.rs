//! `samsara doctor` — preflight self-check for the silent-failure modes.

use crate::keystore::KeyStore;
use crate::model::Provider;
use crate::{authfile, local, ui, zen};
use anyhow::Result;

#[derive(Clone, Copy)]
enum Level {
    Ok,
    Warn,
    Fail,
}

fn line(level: Level, msg: &str) {
    let (c, g) = match level {
        Level::Ok => (ui::GREEN, "✦"),
        Level::Warn => (ui::GOLD, "✧"),
        Level::Fail => (ui::EMBER, "✶"),
    };
    println!("{}", ui::mark(c, g, msg));
}

pub async fn run() -> Result<()> {
    println!(
        "{}",
        ui::mark(ui::VIOLET, "✦", "samsara doctor · reading the sky")
    );
    println!();

    let store = KeyStore::load()?;
    let mut fails = 0u32;
    let mut warns = 0u32;
    let mut bump = |lvl: Level| match lvl {
        Level::Fail => fails += 1,
        Level::Warn => warns += 1,
        Level::Ok => {}
    };

    // 1. keys present
    match store.keys.len() {
        0 => {
            line(Level::Fail, "no keys in the pool — run `samsara add <key>`");
            bump(Level::Fail);
        }
        1 => {
            line(
                Level::Warn,
                "only one key — nothing to rotate to; add another",
            );
            bump(Level::Warn);
        }
        n => line(Level::Ok, &format!("{n} keys in the pool")),
    }

    // 2. duplicate key material
    let dupes = duplicate_keys(&store);
    if dupes.is_empty() {
        if !store.keys.is_empty() {
            line(Level::Ok, "no duplicate keys");
        }
    } else {
        line(
            Level::Warn,
            &format!(
                "duplicate key material: {} (they share one limit)",
                dupes.join(", ")
            ),
        );
        bump(Level::Warn);
    }

    // 3. OPENCODE_API_KEY override
    if let Some(w) = local::credential_override_warning() {
        line(Level::Fail, &w);
        bump(Level::Fail);
    } else {
        line(Level::Ok, "no OPENCODE_API_KEY override in the environment");
    }

    // 4. auth.json in sync with the active key
    match (
        authfile::read_zen_key().unwrap_or(None),
        store.active_entry(),
    ) {
        (Some(lk), Some(a)) if lk == a.key => {
            line(Level::Ok, "auth.json in sync with the active key")
        }
        (_, Some(a)) => {
            line(
                Level::Warn,
                &format!("auth.json out of sync — run `samsara switch {}`", a.label),
            );
            bump(Level::Warn);
        }
        (_, None) => {}
    }

    // 5. opencode reachable
    match local::registration()? {
        Some(reg) if local::pid_alive(reg.pid) => {
            line(Level::Ok, &format!("opencode running (pid {})", reg.pid))
        }
        _ => line(
            Level::Warn,
            "opencode not running (the daemon reconnects when it starts)",
        ),
    }

    // 6. live-validate each Zen key against the provider
    println!();
    line(Level::Ok, "validating keys with the provider…");
    for k in &store.keys {
        if k.provider != Provider::Opencode {
            line(
                Level::Ok,
                &format!("{}: {} key (not validated)", k.label, k.provider.auth_id()),
            );
            continue;
        }
        match zen::validate(&k.key).await {
            zen::Validity::Ok { models } => {
                line(Level::Ok, &format!("{}: valid ({models} models)", k.label))
            }
            zen::Validity::Unauthorized => {
                line(
                    Level::Fail,
                    &format!("{}: rejected (401) — dead or revoked key", k.label),
                );
                bump(Level::Fail);
            }
            zen::Validity::Other(code) => {
                line(
                    Level::Warn,
                    &format!("{}: unexpected status {code}", k.label),
                );
                bump(Level::Warn);
            }
            zen::Validity::Unreachable(e) => {
                line(
                    Level::Warn,
                    &format!("{}: could not reach Zen ({e})", k.label),
                );
                bump(Level::Warn);
            }
        }
    }

    // summary
    println!();
    if fails == 0 && warns == 0 {
        println!(
            "{}",
            ui::mark(ui::GREEN, "✦", "all clear — the sky is bright")
        );
    } else {
        println!(
            "{}",
            ui::mark(
                if fails > 0 { ui::EMBER } else { ui::GOLD },
                if fails > 0 { "✶" } else { "✧" },
                &format!("{fails} problem(s), {warns} warning(s)"),
            )
        );
    }
    Ok(())
}

fn duplicate_keys(store: &KeyStore) -> Vec<String> {
    let mut seen = std::collections::HashMap::<&str, usize>::new();
    for k in &store.keys {
        *seen.entry(k.key.as_str()).or_insert(0) += 1;
    }
    store
        .keys
        .iter()
        .filter(|k| seen.get(k.key.as_str()).copied().unwrap_or(0) > 1)
        .map(|k| k.label.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
