//! samsara — auto-rotating opencode Zen API-key supervisor.

mod authfile;
mod cli;
mod fsx;
mod keystore;
mod local;
mod model;
mod paths;
mod rotor;
mod ui;
mod watcher;

use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SAMSARA_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = cli::Cli::parse();
    match cli::run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
