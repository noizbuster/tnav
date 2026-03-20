use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

use super::{HistoryEntry, HistoryStore};

const HISTORY_DIR_NAME: &str = "history";
const HISTORY_FILE_EXTENSION: &str = "json";
const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "noizbuster";
const APPLICATION: &str = "tnav";

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("history I/O error: {message}")]
    IoError { message: String },
    #[error("history JSON error: {message}")]
    JsonError { message: String },
    #[error("invalid profile name: {message}")]
    InvalidProfile { message: String },
}

pub fn history_file_path(profile: &str) -> Result<PathBuf, HistoryError> {
    let sanitized_profile = sanitize_profile(profile)?;
    let history_dir = history_dir_path()?;

    create_history_dirs(&history_dir)?;

    let file_path = history_dir.join(format!("{sanitized_profile}.{HISTORY_FILE_EXTENSION}"));
    tracing::debug!(
        profile = %profile,
        sanitized_profile = %sanitized_profile,
        path = %file_path.display(),
        "Resolved history file path"
    );

    Ok(file_path)
}

pub fn load_history(profile: &str) -> Result<HistoryStore, HistoryError> {
    let path = history_file_path(profile)?;

    match fs::read_to_string(&path) {
        Ok(contents) => {
            let mut store = serde_json::from_str::<HistoryStore>(&contents).map_err(|error| {
                HistoryError::JsonError {
                    message: format!(
                        "failed to parse history file at {}: {error}",
                        path.display()
                    ),
                }
            })?;

            store.profile = profile.trim().to_owned();

            tracing::debug!(
                profile = %store.profile,
                path = %path.display(),
                entries = store.entries.len(),
                "Loaded history store"
            );

            Ok(store)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let store = HistoryStore {
                profile: profile.trim().to_owned(),
                ..HistoryStore::default()
            };

            tracing::debug!(
                profile = %store.profile,
                path = %path.display(),
                "History file not found; returning empty store"
            );

            Ok(store)
        }
        Err(error) => Err(HistoryError::IoError {
            message: format!("failed to read history file at {}: {error}", path.display()),
        }),
    }
}

pub fn save_history(store: &HistoryStore) -> Result<PathBuf, HistoryError> {
    let path = history_file_path(&store.profile)?;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_history_dirs(parent)?;
    }

    let serialized =
        serde_json::to_string_pretty(store).map_err(|error| HistoryError::JsonError {
            message: format!("failed to serialize history store: {error}"),
        })?;

    let tmp_path = temporary_file_path(&path);

    write_history_file(&tmp_path, serialized.as_bytes())?;

    fs::rename(&tmp_path, &path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        HistoryError::IoError {
            message: format!(
                "failed to atomically replace history file from {} to {}: {error}",
                tmp_path.display(),
                path.display()
            ),
        }
    })?;

    tracing::debug!(
        profile = %store.profile,
        path = %path.display(),
        entries = store.entries.len(),
        "Saved history store"
    );

    Ok(path)
}

pub fn append_entry(profile: &str, entry: HistoryEntry) -> Result<(), HistoryError> {
    let mut store = load_history(profile)?;
    store.push(entry);
    save_history(&store)?;

    tracing::debug!(
        profile = %profile,
        entries = store.entries.len(),
        "Appended history entry"
    );

    Ok(())
}

fn history_dir_path() -> Result<PathBuf, HistoryError> {
    let project_dirs =
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).ok_or_else(|| {
            HistoryError::IoError {
                message: "could not determine standard tnav configuration directory".to_owned(),
            }
        })?;

    Ok(project_dirs.config_dir().join(HISTORY_DIR_NAME))
}

fn sanitize_profile(profile: &str) -> Result<String, HistoryError> {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return Err(HistoryError::InvalidProfile {
            message: "profile name cannot be empty".to_owned(),
        });
    }

    let sanitized = trimmed
        .chars()
        .map(|value| match value {
            '/' | '\\' | ':' => '_',
            _ => value,
        })
        .collect::<String>();

    if sanitized.is_empty() {
        return Err(HistoryError::InvalidProfile {
            message: "profile name is invalid after sanitization".to_owned(),
        });
    }

    Ok(sanitized)
}

fn temporary_file_path(path: &Path) -> PathBuf {
    let mut temp_path = path.as_os_str().to_os_string();
    temp_path.push(".tmp");
    PathBuf::from(temp_path)
}

fn create_history_dirs(path: &Path) -> Result<(), HistoryError> {
    fs::create_dir_all(path).map_err(|error| HistoryError::IoError {
        message: format!(
            "failed to create history directory {}: {error}",
            path.display()
        ),
    })?;

    set_dir_permissions(path)
}

fn write_history_file(path: &Path, contents: &[u8]) -> Result<(), HistoryError> {
    let mut file = open_history_file(path)?;

    file.write_all(contents)
        .map_err(|error| HistoryError::IoError {
            message: format!(
                "failed to write temporary history file at {}: {error}",
                path.display()
            ),
        })?;

    file.sync_all().map_err(|error| HistoryError::IoError {
        message: format!(
            "failed to flush temporary history file at {}: {error}",
            path.display()
        ),
    })?;

    set_file_permissions(path)
}

#[cfg(unix)]
fn open_history_file(path: &Path) -> Result<std::fs::File, HistoryError> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| HistoryError::IoError {
            message: format!(
                "failed to open temporary history file at {}: {error}",
                path.display()
            ),
        })
}

#[cfg(not(unix))]
fn open_history_file(path: &Path) -> Result<std::fs::File, HistoryError> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| HistoryError::IoError {
            message: format!(
                "failed to open temporary history file at {}: {error}",
                path.display()
            ),
        })
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<(), HistoryError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        HistoryError::IoError {
            message: format!(
                "failed to set history directory permissions at {}: {error}",
                path.display()
            ),
        }
    })
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<(), HistoryError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), HistoryError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        HistoryError::IoError {
            message: format!(
                "failed to set history file permissions at {}: {error}",
                path.display()
            ),
        }
    })
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), HistoryError> {
    Ok(())
}
