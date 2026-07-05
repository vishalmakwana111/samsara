//! samsara — auto-rotating opencode Zen API-key supervisor.

mod authfile;
mod cli;
mod config;
mod doctor;
mod fsx;
mod history;
mod keystore;
mod local;
mod model;
mod notify;
mod paths;
mod rotor;
mod service;
mod ui;
mod update;
mod watcher;
mod zen;

use clap::{CommandFactory, FromArgMatches};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SAMSARA_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Build the command with a starfield banner, themed colors, and an example
    // footer, then parse — so `--help` and bare `samsara` feel like the constellation.
    let command = cli::Cli::command()
        .styles(ui::clap_styles())
        .before_help(ui::banner())
        .after_help(ui::help_footer());
    let cli = match cli::Cli::from_arg_matches_mut(&mut command.get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };

    match cli::run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
