pub mod app;
pub mod auth;
pub mod cli;
pub mod commands;
pub mod config;
pub mod errors;
pub mod output;
pub mod profiles;
pub mod secrets;
pub mod ui;
pub mod util;

use crate::cli::Cli;
use crate::errors::TnavError;
use tracing_subscriber::EnvFilter;

pub async fn run(cli: Cli) -> Result<(), TnavError> {
    app::run(cli).await
}

pub fn init_tracing(cli: &Cli) {
    let directive = if cli.verbose() > 0 {
        "tnav=debug,warn"
    } else if cli.quiet() {
        "error"
    } else {
        "info"
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directive));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
