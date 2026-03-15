use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Config, ConfigError, resolve_config_path};

pub fn save(config: &Config, explicit_path: Option<&Path>) -> Result<PathBuf, ConfigError> {
    let path = resolve_config_path(explicit_path)?;
    save_to_path(config, &path)?;
    Ok(path)
}

pub fn save_to_path(config: &Config, path: &Path) -> Result<(), ConfigError> {
    config.validate()?;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_config_dirs(parent)?;
    }

    let serialized = serialize(config)?;
    write_config_file(path, &serialized)
}

fn create_config_dirs(path: &Path) -> Result<(), ConfigError> {
    let mut missing_dirs = Vec::new();
    let mut current = Some(path);

    while let Some(dir) = current {
        if dir.as_os_str().is_empty() || dir.exists() {
            break;
        }

        missing_dirs.push(dir.to_path_buf());
        current = dir.parent();
    }

    for dir in missing_dirs.iter().rev() {
        fs::create_dir(dir).map_err(|error| ConfigError::CreateDirFailed {
            path: dir.clone(),
            message: error.to_string(),
        })?;

        set_dir_permissions(dir)?;
    }

    Ok(())
}

pub fn serialize(config: &Config) -> Result<String, ConfigError> {
    config.validate()?;

    toml::to_string_pretty(config).map_err(|error| ConfigError::SerializeFailed {
        message: error.to_string(),
    })
}

fn write_config_file(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let mut file = open_config_file(path)?;
    file.write_all(contents.as_bytes())
        .map_err(|error| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    file.sync_all().map_err(|error| ConfigError::WriteFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    set_file_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn open_config_file(path: &Path) -> Result<std::fs::File, ConfigError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    Ok(file)
}

#[cfg(not(unix))]
fn open_config_file(path: &Path) -> Result<std::fs::File, ConfigError> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    Ok(file)
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        ConfigError::CreateDirFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), ConfigError> {
    Ok(())
}
