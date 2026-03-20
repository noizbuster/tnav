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
    pub command: Option<Command>,
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
    Connect,
    Model(ModelArgs),
    History(HistoryArgs),
    Status,
    #[command(subcommand)]
    Auth(AuthCommand),
    #[command(subcommand)]
    Config(ConfigCommand),
    #[command(subcommand)]
    Profile(ProfileCommand),
    Doctor,
    Version,
    #[command(external_subcommand)]
    Prompt(Vec<String>),
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
pub struct ModelArgs {
    #[arg(value_name = "MODEL")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct HistoryArgs {
    /// Limit number of entries to show
    #[arg(short = 'n', long, default_value = "20")]
    pub limit: usize,

    /// Clear history for current profile
    #[arg(long)]
    pub clear: bool,

    /// Show raw JSON output
    #[arg(long)]
    pub json: bool,
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

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn external_words_become_prompt_command() {
        let cli = Cli::parse_from(["tnav", "show", "current", "directory"]);

        match cli.command {
            Some(Command::Prompt(parts)) => {
                assert_eq!(parts, vec!["show", "current", "directory"]);
            }
            other => panic!("expected prompt fallback, got {other:?}"),
        }
    }

    #[test]
    fn known_subcommand_still_parses_as_subcommand() {
        let cli = Cli::parse_from(["tnav", "doctor"]);

        assert!(matches!(cli.command, Some(Command::Doctor)));
    }

    #[test]
    fn connect_parses_as_real_subcommand() {
        let cli = Cli::parse_from(["tnav", "connect"]);

        assert!(matches!(cli.command, Some(Command::Connect)));
    }

    #[test]
    fn model_parses_as_real_subcommand() {
        let cli = Cli::parse_from(["tnav", "model", "llama3.2"]);

        match cli.command {
            Some(Command::Model(args)) => assert_eq!(args.model.as_deref(), Some("llama3.2")),
            other => panic!("expected model subcommand, got {other:?}"),
        }
    }

    #[test]
    fn status_parses_as_real_subcommand() {
        let cli = Cli::parse_from(["tnav", "status"]);

        assert!(matches!(cli.command, Some(Command::Status)));
    }

    #[test]
    fn plain_tnav_has_no_command() {
        let cli = Cli::parse_from(["tnav"]);

        assert!(cli.command.is_none());
    }
}
