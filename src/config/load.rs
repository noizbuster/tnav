use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::{Config, ConfigError, resolve_config_path};

pub fn load(explicit_path: Option<&Path>) -> Result<Config, ConfigError> {
    let path = resolve_config_path(explicit_path)?;
    load_from_path(&path)
}

pub fn load_optional(explicit_path: Option<&Path>) -> Result<Option<Config>, ConfigError> {
    let path = resolve_config_path(explicit_path)?;

    match load_from_path(&path) {
        Ok(config) => Ok(Some(config)),
        Err(ConfigError::NotFound { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_from_path(path: &Path) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => ConfigError::NotFound {
            path: path.to_path_buf(),
        },
        _ => ConfigError::ReadFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        },
    })?;

    parse_with_path(&contents, path)
}

pub fn parse(contents: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(contents).map_err(|error| ConfigError::ParseFailed {
        path: Path::new("<inline>").to_path_buf(),
        message: error.to_string(),
    })?;

    config.validate()?;
    Ok(config)
}

fn parse_with_path(contents: &str, path: &Path) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(contents).map_err(|error| ConfigError::ParseFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    config.validate()?;
    Ok(config)
}
