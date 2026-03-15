use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = tnav::cli::Cli::parse();
    tnav::init_tracing(&cli);

    match tnav::run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            error.exit_code()
        }
    }
}
