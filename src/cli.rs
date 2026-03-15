use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "tnav",
    version,
    about = "Interactive terminal navigation scaffold"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn verbose(&self) -> u8 {
        self.global.verbose
    }

    pub fn quiet(&self) -> bool {
        self.global.quiet
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Init,
    #[command(subcommand)]
    Auth(AuthCommand),
    #[command(subcommand)]
    Config(ConfigCommand),
    #[command(subcommand)]
    Profile(ProfileCommand),
    Doctor,
    Version,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AuthCommand {
    Login,
    Logout,
    #[command(name = "api-key")]
    ApiKey,
    Status,
    Revoke,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    Show,
    Set,
    Path,
    Reset,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProfileCommand {
    List,
    Add,
    Remove,
    Use,
}

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[arg(long)]
    pub quiet: bool,
    #[arg(long)]
    pub no_browser: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub non_interactive: bool,
}
