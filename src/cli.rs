//! Command-line interface: argument parsing and command handlers.

use crate::config::{Policy, Settings};
use crate::keystore::KeyStore;
use crate::model::{Provider, now_secs};
use crate::{authfile, history, local, secrets, ui, zen};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SecureAction {
    /// Move all key secrets into the OS keychain.
    Enable,
    /// Move all key secrets back to plaintext keys.json (0600).
    Disable,
    /// Show the current secret-storage backend.
    Status,
}

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
    /// Add an API key to the pool.
    Add {
        /// The API key (omit and use --stdin to avoid shell history).
        key: Option<String>,
        /// A label for this key (auto-generated if omitted).
        #[arg(short, long)]
        label: Option<String>,
        /// Provider this key is for: opencode (default), openrouter, anthropic.
        #[arg(short, long, default_value = "opencode")]
        provider: String,
        /// Read the key from stdin instead of the argument.
        #[arg(long)]
        stdin: bool,
        /// Skip validating the key against the provider.
        #[arg(long)]
        no_verify: bool,
    },
    /// Remove a key from the pool by label.
    Remove {
        /// The label of the key to remove.
        label: String,
    },
    /// List keys with active marker and cooldown status (the constellation).
    List,
    /// Show active key, live server state, cooldowns, and override warnings.
    Status,
    /// Make a key active now (swaps auth.json + reloads the daemon).
    Switch {
        /// The label of the key to activate.
        label: String,
    },
    /// Pin a key (preferred under the `priority` policy).
    Pin { label: String },
    /// Remove a pin.
    Unpin { label: String },
    /// Exclude a key from rotation.
    Disable { label: String },
    /// Re-include a disabled key in rotation.
    Enable { label: String },
    /// Set a key's rotation priority (higher is chosen first under `priority`).
    Priority { label: String, value: i32 },
    /// Show per-key usage stats.
    Stats,
    /// Show recent rotation history.
    History {
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },
    /// Run a preflight self-check.
    Doctor,
    /// View or change samsara settings.
    Config {
        /// Fallback cooldown, e.g. "12h".
        #[arg(long)]
        cooldown: Option<humantime::Duration>,
        /// Rotation policy: round-robin | priority.
        #[arg(long)]
        policy: Option<String>,
        /// Desktop banner notifications on/off.
        #[arg(long)]
        banner: Option<bool>,
        /// Webhook URL to POST on rotation (use --clear-webhook to remove).
        #[arg(long)]
        webhook: Option<String>,
        /// Remove the configured webhook.
        #[arg(long)]
        clear_webhook: bool,
    },
    /// Update samsara to the latest release (self-update).
    Update {
        /// Reinstall even if already on the latest version.
        #[arg(long)]
        force: bool,
    },
    /// Manage the background service (launchd/systemd).
    Service {
        #[command(subcommand)]
        action: crate::service::ServiceAction,
    },
    /// Move key secrets into / out of the OS keychain.
    Secure {
        #[command(subcommand)]
        action: SecureAction,
    },
    /// Live full-screen dashboard of the constellation.
    Watch,
    /// Run the supervisor: watch for limit hits and auto-rotate.
    Daemon {
        /// Fallback cooldown when the server sends no retry-after (e.g. "12h").
        #[arg(long)]
        default_cooldown: Option<humantime::Duration>,
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
        Command::Add {
            key,
            label,
            provider,
            stdin,
            no_verify,
        } => cmd_add(key, label, provider, stdin, no_verify).await,
        Command::Remove { label } => cmd_remove(label),
        Command::List => cmd_list(),
        Command::Status => cmd_status(),
        Command::Switch { label } => cmd_switch(label),
        Command::Pin { label } => cmd_flag(label, "pinned", true),
        Command::Unpin { label } => cmd_flag(label, "pinned", false),
        Command::Disable { label } => cmd_flag(label, "disabled", true),
        Command::Enable { label } => cmd_flag(label, "disabled", false),
        Command::Priority { label, value } => cmd_priority(label, value),
        Command::Stats => cmd_stats(),
        Command::History { limit } => cmd_history(limit),
        Command::Doctor => crate::doctor::run().await,
        Command::Config {
            cooldown,
            policy,
            banner,
            webhook,
            clear_webhook,
        } => cmd_config(cooldown, policy, banner, webhook, clear_webhook),
        Command::Update { force } => crate::update::run(force).await,
        Command::Service { action } => crate::service::run(action),
        Command::Secure { action } => cmd_secure(action),
        Command::Watch => crate::watcher::watch().await,
        Command::Daemon {
            default_cooldown,
            dir,
            debug_events,
        } => {
            let cd = default_cooldown.map(Into::into).unwrap_or_else(|| {
                std::time::Duration::from_secs(
                    Settings::load()
                        .map(|s| s.default_cooldown_secs)
                        .unwrap_or(43200),
                )
            });
            crate::watcher::run(cd, dir, debug_events).await
        }
    }
}

/// Write the store's active key into opencode's auth.json (best-effort sync).
fn sync_active_to_authfile(store: &KeyStore) -> Result<()> {
    if let Some(entry) = store.active_entry() {
        let secret = secrets::resolve(&entry.key)?;
        authfile::set_key(entry.provider.auth_id(), &secret)
            .context("writing active key to auth.json")?;
    }
    Ok(())
}

fn cmd_secure(action: SecureAction) -> Result<()> {
    anyhow::ensure!(
        secrets::available(),
        "keychain storage is only available on macOS"
    );
    let mut store = KeyStore::load()?;
    let mut settings = Settings::load()?;
    match action {
        SecureAction::Enable => {
            for k in &mut store.keys {
                if !secrets::is_reference(&k.key) {
                    secrets::set(&k.label, &k.key)?;
                    k.key = secrets::reference(&k.label);
                }
            }
            store.save()?;
            settings.keychain = true;
            settings.save()?;
            println!(
                "{}",
                ui::mark(ui::GREEN, "✦", "keys moved into the OS keychain")
            );
        }
        SecureAction::Disable => {
            for k in &mut store.keys {
                if secrets::is_reference(&k.key) {
                    let real = secrets::resolve(&k.key)?;
                    let label = k.label.clone();
                    k.key = real;
                    secrets::delete(&label)?;
                }
            }
            store.save()?;
            settings.keychain = false;
            settings.save()?;
            println!(
                "{}",
                ui::mark(
                    ui::GOLD,
                    "✧",
                    "keys moved back to plaintext keys.json (0600)"
                )
            );
        }
        SecureAction::Status => {
            let refs = store
                .keys
                .iter()
                .filter(|k| secrets::is_reference(&k.key))
                .count();
            let backend = if settings.keychain {
                "keychain"
            } else {
                "plaintext (0600)"
            };
            println!(
                "{}",
                ui::mark(ui::VIOLET, "✦", &format!("backend: {backend}"))
            );
            println!(
                "{}",
                ui::mark(
                    ui::ASH,
                    "·",
                    &format!("{refs}/{} keys stored in the keychain", store.keys.len())
                )
            );
        }
    }
    Ok(())
}

async fn cmd_add(
    key: Option<String>,
    label: Option<String>,
    provider: String,
    stdin: bool,
    no_verify: bool,
) -> Result<()> {
    let provider = Provider::parse(&provider).with_context(|| {
        format!("unknown provider '{provider}' (opencode|openrouter|anthropic)")
    })?;

    let key = match (key, stdin) {
        (Some(k), _) => k,
        (None, _) => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    let key = key.trim().to_string();
    anyhow::ensure!(!key.is_empty(), "key must not be empty");

    let mut store = KeyStore::load()?;
    anyhow::ensure!(
        !store.keys.iter().any(|k| k.key == key),
        "that exact key is already in the pool"
    );

    // validate against the provider (opencode/Zen only for now)
    if !no_verify && provider == Provider::Opencode {
        match zen::validate(&key).await {
            zen::Validity::Ok { models } => println!(
                "{}",
                ui::mark(
                    ui::GREEN,
                    "✦",
                    &format!("key verified ({models} models reachable)")
                )
            ),
            zen::Validity::Unauthorized => {
                anyhow::bail!(
                    "the provider rejected this key (401) — not added (use --no-verify to force)"
                )
            }
            zen::Validity::Other(code) => println!(
                "{}",
                ui::mark(
                    ui::GOLD,
                    "✧",
                    &format!("unexpected status {code} while verifying; adding anyway")
                )
            ),
            zen::Validity::Unreachable(e) => println!(
                "{}",
                ui::mark(
                    ui::GOLD,
                    "✧",
                    &format!("could not reach provider ({e}); adding unverified")
                )
            ),
        }
    }

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
    if let Some(e) = store.find_mut(&label) {
        e.provider = provider;
    }
    store.save()?;
    history::append(&history::Event::new("add").to(label.clone()));
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
            ui::mark(ui::GOLD, "✧", "it burns brightest — now the active key")
        );
    }
    Ok(())
}

fn cmd_remove(label: String) -> Result<()> {
    let mut store = KeyStore::load()?;
    let was_active = store.active.as_deref() == Some(label.as_str());
    store.remove(&label)?;
    store.save()?;
    history::append(&history::Event::new("remove").to(label.clone()));

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
    let active_secret = store
        .active_entry()
        .and_then(|a| secrets::resolve(&a.key).ok());
    dim(
        "auth.json",
        match (
            &live,
            active_secret.as_deref(),
            store.active_entry().is_some(),
        ) {
            (Some(lk), Some(ak), _) if lk == ak => {
                ui::paint(ui::GREEN, "in sync with the active star")
            }
            (Some(_), _, true) => ui::paint(
                ui::EMBER,
                &format!(
                    "out of sync — run `samsara switch {}`",
                    store.active.as_deref().unwrap_or("")
                ),
            ),
            (Some(_), _, false) => ui::paint(ui::ASH, "has a Zen key, but no active star"),
            (None, _, _) => ui::paint(ui::ASH, "no Zen key set"),
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

    dim(
        "daemon",
        match crate::service::daemon_pid() {
            Some(pid) => ui::paint(ui::GREEN, &format!("watching (pid {pid})")),
            None => ui::paint(ui::ASH, "not running — `samsara daemon`"),
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
    store.make_active(&label);
    store.save()?;
    sync_active_to_authfile(&store)?;
    history::append(&history::Event::new("switch").to(label.clone()));
    println!(
        "{}",
        ui::mark(
            ui::GOLD,
            "✧",
            &format!("'{label}' now burns brightest (written to auth.json)")
        )
    );
    reload_note();
    Ok(())
}

fn reload_note() {
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
}

fn cmd_flag(label: String, which: &str, on: bool) -> Result<()> {
    let mut store = KeyStore::load()?;
    match which {
        "pinned" => store.set_pinned(&label, on)?,
        "disabled" => store.set_disabled(&label, on)?,
        _ => unreachable!(),
    }
    store.save()?;
    let msg = match (which, on) {
        ("pinned", true) => format!("pinned '{label}' — preferred when rotating"),
        ("pinned", false) => format!("unpinned '{label}'"),
        ("disabled", true) => format!("disabled '{label}' — excluded from rotation"),
        ("disabled", false) => format!("enabled '{label}' — back in rotation"),
        _ => unreachable!(),
    };
    println!("{}", ui::mark(ui::GOLD, "✧", &msg));
    Ok(())
}

fn cmd_priority(label: String, value: i32) -> Result<()> {
    let mut store = KeyStore::load()?;
    store.set_priority(&label, value)?;
    store.save()?;
    println!(
        "{}",
        ui::mark(ui::GOLD, "✧", &format!("'{label}' priority set to {value}"))
    );
    Ok(())
}

fn cmd_stats() -> Result<()> {
    let store = KeyStore::load()?;
    if store.keys.is_empty() {
        println!("{}", ui::mark(ui::ASH, "·", "no keys yet"));
        return Ok(());
    }
    let now = now_secs();
    println!("\n  {}", ui::paint_bold(ui::GOLD, "✦ per-star usage"));
    println!(
        "  {}",
        ui::paint(
            ui::ASH,
            &format!(
                "{:<12} {:<6} {:<5} {:<9} {:<7} {:<8} {:<8} {}",
                "LABEL", "HITS", "ACT", "ACTIVE", "EVENTS", "BURN/h", "COOLDOWN", "FLAGS"
            )
        )
    );
    for k in &store.keys {
        let active_t = if k.active_secs == 0 {
            "—".into()
        } else {
            ui::fmt_dur(k.active_secs)
        };
        let burn = k
            .burn_rate_per_hour()
            .map(|r| format!("{r:.0}"))
            .unwrap_or_else(|| "—".into());
        let cooldown = k
            .learned_cooldown_secs
            .map(|s| format!("~{}", ui::fmt_dur(s)))
            .unwrap_or_else(|| "—".into());
        let flags = [
            if store.active.as_deref() == Some(&k.label) {
                "active"
            } else {
                ""
            },
            if k.pinned { "pinned" } else { "" },
            if k.disabled { "disabled" } else { "" },
            if k.is_cooling(now) { "cooling" } else { "" },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(",");
        println!(
            "  {:<12} {:<6} {:<5} {:<9} {:<7} {:<8} {:<8} {}",
            k.label,
            k.limit_hits,
            k.activations,
            active_t,
            k.events_seen,
            burn,
            cooldown,
            ui::paint(ui::CYAN, &flags)
        );
    }

    // learned estimates
    let estimates: Vec<String> = store
        .keys
        .iter()
        .filter_map(|k| {
            k.avg_active_per_limit().map(|s| {
                format!(
                    "{} lasts ~{} active between limits",
                    k.label,
                    ui::fmt_dur(s)
                )
            })
        })
        .collect();
    if !estimates.is_empty() {
        println!("\n  {}", ui::paint_bold(ui::GOLD, "✦ learned"));
        for e in estimates {
            println!("  {}", ui::paint(ui::ASH, &format!("· {e}")));
        }
    }
    println!(
        "\n  {}",
        ui::paint(
            ui::ASH,
            "ACT = activations · BURN/h = events per active hour · COOLDOWN = learned reset window"
        )
    );
    println!();
    Ok(())
}

fn cmd_history(limit: usize) -> Result<()> {
    let events = history::recent(limit);
    if events.is_empty() {
        println!("{}", ui::mark(ui::ASH, "·", "no history yet"));
        return Ok(());
    }
    println!("\n  {}", ui::paint_bold(ui::GOLD, "✦ recent history"));
    for e in events {
        let glyph = match e.kind.as_str() {
            "rotate" => ui::paint(ui::SAFFRON, "➤"),
            "exhausted" => ui::paint(ui::EMBER, "✶"),
            _ => ui::paint(ui::ASH, "·"),
        };
        let detail = match (e.from.as_deref(), e.to.as_deref()) {
            (Some(f), Some(t)) => format!("{f} → {t}"),
            (None, Some(t)) => t.to_string(),
            _ => e.note.clone().unwrap_or_default(),
        };
        println!(
            "  {glyph} {}  {}  {}",
            ui::paint(ui::ASH, &fmt_ts(e.ts)),
            e.kind,
            detail
        );
    }
    println!();
    Ok(())
}

fn cmd_config(
    cooldown: Option<humantime::Duration>,
    policy: Option<String>,
    banner: Option<bool>,
    webhook: Option<String>,
    clear_webhook: bool,
) -> Result<()> {
    let mut s = Settings::load()?;
    let mut changed = false;
    if let Some(cd) = cooldown {
        s.default_cooldown_secs = cd.as_secs();
        changed = true;
    }
    if let Some(p) = policy {
        s.policy = match p.to_lowercase().as_str() {
            "round-robin" | "roundrobin" | "rr" => Policy::RoundRobin,
            "priority" | "prio" => Policy::Priority,
            _ => anyhow::bail!("unknown policy '{p}' (round-robin|priority)"),
        };
        changed = true;
    }
    if let Some(b) = banner {
        s.notify_banner = b;
        changed = true;
    }
    if clear_webhook {
        s.notify_webhook = None;
        changed = true;
    } else if let Some(w) = webhook {
        s.notify_webhook = Some(w);
        changed = true;
    }
    if changed {
        s.save()?;
        println!("{}", ui::mark(ui::GOLD, "✧", "settings updated"));
    }

    println!("\n  {}", ui::paint_bold(ui::GOLD, "✦ settings"));
    let dim = |k: &str, v: String| println!("  {}  {}", ui::paint(ui::ASH, &format!("{k:<16}")), v);
    dim("cooldown", ui::fmt_dur(s.default_cooldown_secs));
    dim("policy", format!("{:?}", s.policy));
    dim("notify-banner", s.notify_banner.to_string());
    dim(
        "notify-webhook",
        s.notify_webhook.clone().unwrap_or_else(|| "(none)".into()),
    );
    println!();
    Ok(())
}

fn fmt_ts(_ts: u64) -> String {
    // Relative "Nm ago" without a date crate.
    let d = now_secs().saturating_sub(_ts);
    format!("{:>8} ago", ui::fmt_dur(d))
}
