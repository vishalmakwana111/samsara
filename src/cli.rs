//! Command-line interface: argument parsing and one-shot command handlers.

use crate::authfile;
use crate::keystore::KeyStore;
use crate::local;
use crate::model::now_secs;
use crate::ui;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "samsara",
    about = "Auto-rotating opencode Zen API-key supervisor",
    version,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a Zen API key to the pool (paste the key).
    Add {
        /// The Zen API key.
        key: String,
        /// A label for this key (auto-generated if omitted).
        #[arg(short, long)]
        label: Option<String>,
    },
    /// Remove a key from the pool by label.
    Remove {
        /// The label of the key to remove.
        label: String,
    },
    /// List keys (masked) with active marker and cooldown status.
    List,
    /// Show active key, live server state, cooldowns, and override warnings.
    Status,
    /// Make a key active now (swaps auth.json + reloads the daemon).
    Switch {
        /// The label of the key to activate.
        label: String,
    },
    /// Run the supervisor: watch for limit hits and auto-rotate.
    Daemon {
        /// Fallback cooldown when the server sends no retry-after (e.g. "12h").
        #[arg(long, default_value = "12h")]
        default_cooldown: humantime::Duration,
        /// Project directory to scope the event stream to (adds ?directory=).
        #[arg(long)]
        dir: Option<String>,
        /// Print every raw SSE event and do NOT rotate (used to inspect event shapes).
        #[arg(long)]
        debug_events: bool,
    },
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Add { key, label } => cmd_add(key, label),
        Command::Remove { label } => cmd_remove(label),
        Command::List => cmd_list(),
        Command::Status => cmd_status(),
        Command::Switch { label } => cmd_switch(label),
        Command::Daemon {
            default_cooldown,
            dir,
            debug_events,
        } => crate::watcher::run(default_cooldown.into(), dir, debug_events).await,
    }
}

/// Write the store's active key into opencode's auth.json (best-effort sync).
fn sync_active_to_authfile(store: &KeyStore) -> Result<()> {
    if let Some(entry) = store.active_entry() {
        authfile::set_zen_key(&entry.key).context("writing active key to auth.json")?;
    }
    Ok(())
}

fn cmd_add(key: String, label: Option<String>) -> Result<()> {
    let key = key.trim().to_string();
    anyhow::ensure!(!key.is_empty(), "key must not be empty");
    let mut store = KeyStore::load()?;

    let label = label.unwrap_or_else(|| {
        let mut n = store.keys.len() + 1;
        loop {
            let candidate = format!("key{n}");
            if store.find(&candidate).is_none() {
                break candidate;
            }
            n += 1;
        }
    });

    let became_active = store.active.is_none();
    store.add(label.clone(), key)?;
    store.save()?;
    if became_active {
        sync_active_to_authfile(&store)?;
    }

    println!(
        "{}",
        ui::mark(
            ui::GREEN,
            "✦",
            &format!("a new star rises: {}", ui::paint_bold(ui::WHITE, &label))
        )
    );
    if became_active {
        println!(
            "{}",
            ui::mark(ui::GOLD, "✧", "it burns brightest — now the active Zen key")
        );
    }
    Ok(())
}

fn cmd_remove(label: String) -> Result<()> {
    let mut store = KeyStore::load()?;
    let was_active = store.active.as_deref() == Some(label.as_str());
    store.remove(&label)?;
    store.save()?;

    println!(
        "{}",
        ui::mark(
            ui::ASH,
            "·",
            &format!("the star '{label}' fades from the sky")
        )
    );
    if was_active {
        match store.active_entry() {
            Some(next) => {
                sync_active_to_authfile(&store)?;
                println!(
                    "{}",
                    ui::mark(
                        ui::GOLD,
                        "✧",
                        &format!("'{}' now burns brightest", next.label)
                    )
                );
            }
            None => println!(
                "{}",
                ui::mark(ui::ASH, "·", "the sky is dark; auth.json left unchanged")
            ),
        }
    }
    Ok(())
}

fn cmd_list() -> Result<()> {
    let store = KeyStore::load()?;
    if store.keys.is_empty() {
        println!(
            "{}",
            ui::empty_sky(&[
                "no stars yet.",
                "kindle your first:",
                "samsara add <key> --label <name>",
            ])
        );
        return Ok(());
    }
    println!(
        "{}",
        ui::constellation(&store.keys, store.active.as_deref(), 1.0)
    );
    Ok(())
}

fn cmd_status() -> Result<()> {
    let store = KeyStore::load()?;
    let now = now_secs();

    if store.keys.is_empty() {
        println!(
            "{}",
            ui::empty_sky(&["no stars yet.", "kindle your first:", "samsara add <key>"])
        );
    } else {
        println!(
            "{}",
            ui::constellation(&store.keys, store.active.as_deref(), 1.0)
        );
    }

    let dim = |label: &str, val: String| {
        println!("  {}  {}", ui::paint(ui::ASH, &format!("{label:<10}")), val);
    };

    let live = authfile::read_zen_key().unwrap_or(None);
    dim(
        "auth.json",
        match (&live, store.active_entry()) {
            (Some(lk), Some(a)) if *lk == a.key => {
                ui::paint(ui::GREEN, "in sync with the active star")
            }
            (Some(_), Some(_)) => ui::paint(
                ui::EMBER,
                &format!(
                    "out of sync — run `samsara switch {}`",
                    store.active.as_deref().unwrap_or("")
                ),
            ),
            (Some(_), None) => ui::paint(ui::ASH, "has a Zen key, but no active star"),
            (None, _) => ui::paint(ui::ASH, "no Zen key set"),
        },
    );

    dim(
        "opencode",
        match local::registration()? {
            Some(reg) if local::pid_alive(reg.pid) => {
                ui::paint(ui::GREEN, &format!("running (pid {})", reg.pid))
            }
            _ => ui::paint(ui::ASH, "not running"),
        },
    );

    let ready = store.keys.iter().filter(|k| !k.is_cooling(now)).count();
    let cooling = store.keys.len() - ready;
    let mut pool = format!("{} stars · {ready} lit · {cooling} dark", store.keys.len());
    if cooling > 0
        && let Some(reset) = store.soonest_reset(now)
    {
        pool.push_str(&format!(
            " · next rebirth in {}",
            ui::fmt_dur(reset.saturating_sub(now))
        ));
    }
    dim("sky", ui::paint(ui::CYAN, &pool));

    if let Some(warn) = local::credential_override_warning() {
        println!("\n{}", ui::mark(ui::EMBER, "✶", &warn));
    }
    Ok(())
}

fn cmd_switch(label: String) -> Result<()> {
    let mut store = KeyStore::load()?;
    store
        .find(&label)
        .with_context(|| format!("no key labelled '{label}'"))?;
    store.active = Some(label.clone());
    store.save()?;
    sync_active_to_authfile(&store)?;
    println!(
        "{}",
        ui::mark(
            ui::GOLD,
            "✧",
            &format!("'{label}' now burns brightest (written to auth.json)")
        )
    );

    match local::reload() {
        Ok(true) => println!(
            "{}",
            ui::mark(ui::VIOLET, "✦", "the sky realigns — opencode reloaded")
        ),
        Ok(false) => println!(
            "{}",
            ui::mark(
                ui::ASH,
                "·",
                "opencode is dark; the new star rises on next start"
            )
        ),
        Err(e) => println!(
            "{}",
            ui::mark(ui::EMBER, "✶", &format!("could not realign: {e:#}"))
        ),
    }
    Ok(())
}
