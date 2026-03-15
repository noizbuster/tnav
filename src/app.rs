use crate::cli::{Cli, Command};
use crate::commands;
use crate::errors::TnavError;
use tracing::debug;

pub async fn run(cli: Cli) -> Result<(), TnavError> {
    debug!(?cli, "dispatching CLI command");

    let Cli { global, command } = cli;

    match command {
        Command::Init => commands::init::run(&global).await,
        Command::Auth(auth_command) => commands::auth::run(auth_command, &global).await,
        Command::Doctor => commands::doctor::run(&global).await,
        Command::Config(command) => commands::unsupported(&format!("config {command:?}")),
        Command::Profile(command) => commands::unsupported(&format!("profile {command:?}")),
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
