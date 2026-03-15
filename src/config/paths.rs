use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::ConfigError;

pub const CONFIG_FILE_NAME: &str = "config.toml";

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "noizbuster";
const APPLICATION: &str = "tnav";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
}

pub fn config_paths(explicit_path: Option<&Path>) -> Result<ConfigPaths, ConfigError> {
    let config_file = resolve_config_path(explicit_path)?;
    let config_dir = config_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    Ok(ConfigPaths {
        config_dir,
        config_file,
    })
}

pub fn default_config_dir() -> Result<PathBuf, ConfigError> {
    let project_dirs = project_dirs()?;
    Ok(project_dirs.config_dir().to_path_buf())
}

pub fn default_cache_dir() -> Result<PathBuf, ConfigError> {
    let project_dirs = project_dirs()?;
    Ok(project_dirs.cache_dir().to_path_buf())
}

pub fn default_log_dir() -> Result<PathBuf, ConfigError> {
    Ok(default_cache_dir()?.join("logs"))
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    Ok(default_config_dir()?.join(CONFIG_FILE_NAME))
}

pub fn resolve_config_path(explicit_path: Option<&Path>) -> Result<PathBuf, ConfigError> {
    explicit_path
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_config_path)
}

fn project_dirs() -> Result<ProjectDirs, ConfigError> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).ok_or(ConfigError::PathUnavailable)
}
