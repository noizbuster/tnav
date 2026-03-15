mod load;
mod model;
mod paths;
mod save;

use std::path::PathBuf;

pub use load::{load, load_from_path, load_optional, parse};
pub use model::{AuthMethod, Config, ProfileConfig, UiConfig};
pub use paths::{
    CONFIG_FILE_NAME, ConfigPaths, config_paths, default_cache_dir, default_config_dir,
    default_config_path, default_log_dir, resolve_config_path,
};
pub use save::{save, save_to_path, serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine a standard tnav configuration directory")]
    PathUnavailable,
    #[error("configuration file was not found at {path:?}")]
    NotFound { path: PathBuf },
    #[error("failed to read configuration file at {path:?}: {message}")]
    ReadFailed { path: PathBuf, message: String },
    #[error("failed to parse configuration file at {path:?}: {message}")]
    ParseFailed { path: PathBuf, message: String },
    #[error("configuration is invalid: active profile '{profile}' does not exist")]
    InvalidActiveProfile { profile: String },
    #[error("failed to serialize configuration: {message}")]
    SerializeFailed { message: String },
    #[error("failed to create configuration directory {path:?}: {message}")]
    CreateDirFailed { path: PathBuf, message: String },
    #[error("failed to write configuration file at {path:?}: {message}")]
    WriteFailed { path: PathBuf, message: String },
}
