use std::process::{Command, Stdio};

use crate::errors::TnavError;

pub fn execute_command(command: &str) -> Result<(), TnavError> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| TnavError::CommandFailed {
            message: format!("failed to execute shell command: {error}"),
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(TnavError::CommandFailed {
            message: format!("shell command exited with status {status}"),
        })
    }
}
