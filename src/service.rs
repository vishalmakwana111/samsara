//! Background-service management (launchd on macOS, systemd --user on Linux) and
//! the daemon PID file used to detect/stop a running supervisor.

use crate::paths;
use crate::ui;
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum ServiceAction {
    /// Install & start the service (auto-starts on login, restarts on failure).
    Install,
    /// Stop & remove the service.
    Uninstall,
    /// Show whether the service is installed/running.
    Status,
}

pub fn run(action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Install => install(),
        ServiceAction::Uninstall => uninstall(),
        ServiceAction::Status => status(),
    }
}

/// The PID of a running samsara daemon, if the PID file is present and alive.
pub fn daemon_pid() -> Option<i32> {
    let path = paths::samsara_pidfile().ok()?;
    let pid: i32 = std::fs::read_to_string(&path).ok()?.trim().parse().ok()?;
    if crate::local::pid_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Write the current process PID to the daemon PID file.
pub fn write_pidfile() -> Result<()> {
    let path = paths::samsara_pidfile()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(())
}

fn exe() -> Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

// ---------------- macOS (launchd) ----------------

#[cfg(target_os = "macos")]
const LABEL: &str = "ai.samsara.daemon";

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn install() -> Result<()> {
    let exe = exe()?;
    let log = paths::samsara_state_dir()?.join("daemon.log");
    std::fs::create_dir_all(paths::samsara_state_dir()?)?;
    let path = plist_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key><array><string>{exe}</string><string>daemon</string></array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>StandardOutPath</key><string>{log}</string>
    <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        log = log.display()
    );
    std::fs::write(&path, plist).with_context(|| format!("writing {}", path.display()))?;
    // reload if already loaded, then load
    let _ = run_cmd("launchctl", &["unload", &path.to_string_lossy()]);
    run_cmd("launchctl", &["load", "-w", &path.to_string_lossy()])?;
    println!(
        "{}",
        ui::mark(ui::GREEN, "✦", "service installed & started (launchd)")
    );
    println!(
        "{}",
        ui::mark(ui::ASH, "·", &format!("logs: {}", log.display()))
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall() -> Result<()> {
    let path = plist_path()?;
    if path.exists() {
        let _ = run_cmd("launchctl", &["unload", "-w", &path.to_string_lossy()]);
        std::fs::remove_file(&path)?;
    }
    println!("{}", ui::mark(ui::ASH, "·", "service uninstalled"));
    Ok(())
}

#[cfg(target_os = "macos")]
fn status() -> Result<()> {
    let installed = plist_path()?.exists();
    print_status(installed);
    Ok(())
}

// ---------------- Linux (systemd --user) ----------------

#[cfg(target_os = "linux")]
fn unit_path() -> Result<PathBuf> {
    Ok(paths::samsara_config_dir()?
        .parent()
        .context("config dir has no parent")?
        .join("systemd/user/samsara.service"))
}

#[cfg(target_os = "linux")]
fn install() -> Result<()> {
    let exe = exe()?;
    let path = unit_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let unit = format!(
        "[Unit]\nDescription=samsara Zen key rotator\n\n[Service]\nExecStart={exe} daemon\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n"
    );
    std::fs::write(&path, unit).with_context(|| format!("writing {}", path.display()))?;
    run_cmd("systemctl", &["--user", "daemon-reload"])?;
    run_cmd(
        "systemctl",
        &["--user", "enable", "--now", "samsara.service"],
    )?;
    println!(
        "{}",
        ui::mark(
            ui::GREEN,
            "✦",
            "service installed & started (systemd --user)"
        )
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall() -> Result<()> {
    let _ = run_cmd(
        "systemctl",
        &["--user", "disable", "--now", "samsara.service"],
    );
    if let Ok(p) = unit_path() {
        let _ = std::fs::remove_file(p);
    }
    let _ = run_cmd("systemctl", &["--user", "daemon-reload"]);
    println!("{}", ui::mark(ui::ASH, "·", "service uninstalled"));
    Ok(())
}

#[cfg(target_os = "linux")]
fn status() -> Result<()> {
    let installed = unit_path()?.exists();
    print_status(installed);
    Ok(())
}

// ---------------- other platforms ----------------

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install() -> Result<()> {
    bail!("service install is only supported on macOS and Linux")
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn uninstall() -> Result<()> {
    bail!("service install is only supported on macOS and Linux")
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn status() -> Result<()> {
    print_status(false);
    Ok(())
}

fn print_status(installed: bool) {
    if installed {
        println!("{}", ui::mark(ui::GREEN, "✦", "service: installed"));
    } else {
        println!(
            "{}",
            ui::mark(
                ui::ASH,
                "·",
                "service: not installed — `samsara service install`"
            )
        );
    }
    match daemon_pid() {
        Some(pid) => println!(
            "{}",
            ui::mark(ui::GREEN, "✦", &format!("daemon: running (pid {pid})"))
        ),
        None => println!("{}", ui::mark(ui::ASH, "·", "daemon: not running")),
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("running {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {} failed", args.join(" "));
    }
    Ok(())
}
