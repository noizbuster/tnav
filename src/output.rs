use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::errors::TnavError;

#[derive(Debug, Clone, Copy)]
pub struct Output {
    json: bool,
    quiet: bool,
}

impl Output {
    pub fn new(global: &GlobalArgs) -> Self {
        Self {
            json: global.json,
            quiet: global.quiet,
        }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    pub fn print_json<T>(&self, value: &T) -> Result<(), TnavError>
    where
        T: Serialize,
    {
        let json =
            serde_json::to_string_pretty(value).map_err(|error| TnavError::CommandFailed {
                message: format!("failed to serialize JSON output: {error}"),
            })?;
        println!("{json}");
        Ok(())
    }

    pub fn line(&self, message: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", message.as_ref());
        }
    }
}
