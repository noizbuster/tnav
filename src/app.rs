use crate::cli::{Cli, Command};
use crate::commands;
use crate::errors::TnavError;
use tracing::debug;

pub async fn run(cli: Cli) -> Result<(), TnavError> {
    debug!(?cli, "dispatching CLI command");

    let Cli { global, command } = cli;

    match command {
        Some(Command::Init) => commands::init::run(&global).await,
        Some(Command::Connect) => commands::ask::run_connect(&global).await,
        Some(Command::Model(args)) => {
            commands::ask::run_model(&global, args.model.as_deref()).await
        }
        Some(Command::History(args)) => commands::history::run(&global, &args).await,
        Some(Command::Status) => commands::status::run(&global).await,
        Some(Command::Auth(auth_command)) => commands::auth::run(auth_command, &global).await,
        Some(Command::Doctor) => commands::doctor::run(&global).await,
        Some(Command::Config(command)) => commands::unsupported(&format!("config {command:?}")),
        Some(Command::Profile(command)) => commands::unsupported(&format!("profile {command:?}")),
        Some(Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(Command::Prompt(parts)) => commands::ask::run(&global, Some(&parts.join(" "))).await,
        None => commands::ask::run(&global, None).await,
    }
}
