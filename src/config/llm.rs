use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::llm::{ConfiguredProvider, DEFAULT_PROVIDER_TIMEOUT_SECS, LlmConfig, Provider};

use super::{ConfigError, default_config_dir};

pub const LLM_CONFIG_FILE_NAME: &str = "llm.toml";

pub fn llm_config_path() -> Result<PathBuf, ConfigError> {
    Ok(default_config_dir()?.join(LLM_CONFIG_FILE_NAME))
}

pub fn load_llm_config() -> Result<Option<LlmConfig>, ConfigError> {
    let path = llm_config_path()?;

    match fs::read_to_string(&path) {
        Ok(contents) => parse_llm_config(&contents, &path).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::ReadFailed {
            path,
            message: error.to_string(),
        }),
    }
}

pub fn save_llm_config(config: &LlmConfig) -> Result<PathBuf, ConfigError> {
    let path = llm_config_path()?;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_config_dirs(parent)?;
    }

    let serialized =
        toml::to_string_pretty(config).map_err(|error| ConfigError::SerializeFailed {
            message: error.to_string(),
        })?;

    write_llm_config_file(&path, &serialized)?;
    Ok(path)
}

fn parse_llm_config(contents: &str, path: &Path) -> Result<LlmConfig, ConfigError> {
    let stored: StoredLlmConfig =
        toml::from_str(contents).map_err(|error| ConfigError::ParseFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    Ok(match stored {
        StoredLlmConfig::Current(config) => config.normalize(),
        StoredLlmConfig::LegacySingle(provider) => migrate_legacy_single(provider),
        StoredLlmConfig::LegacyMulti(config) => migrate_legacy_multi(config),
    })
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StoredLlmConfig {
    LegacySingle(LegacyConfiguredProvider),
    LegacyMulti(LegacyLlmConfig),
    Current(LlmConfig),
}

#[derive(serde::Deserialize)]
struct LegacyLlmConfig {
    active_provider: Option<Provider>,
    providers: Vec<LegacyConfiguredProvider>,
}

#[derive(serde::Deserialize)]
struct LegacyConfiguredProvider {
    provider: Provider,
    model: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default = "legacy_default_timeout_secs")]
    timeout_secs: u64,
}

fn migrate_legacy_multi(config: LegacyLlmConfig) -> LlmConfig {
    let providers = config
        .providers
        .into_iter()
        .enumerate()
        .map(|(index, provider)| migrate_legacy_provider(provider, index + 1))
        .collect::<Vec<_>>();

    let active_provider = config.active_provider.and_then(|provider| {
        providers
            .iter()
            .find(|item| item.provider == provider)
            .map(|item| item.name.clone())
    });

    LlmConfig {
        active_provider,
        providers,
    }
    .normalize()
}

fn migrate_legacy_single(provider: LegacyConfiguredProvider) -> LlmConfig {
    let configured = migrate_legacy_provider(provider, 1);

    LlmConfig {
        active_provider: Some(configured.name.clone()),
        providers: vec![configured],
    }
    .normalize()
}

fn migrate_legacy_provider(
    provider: LegacyConfiguredProvider,
    sequence: usize,
) -> ConfiguredProvider {
    ConfiguredProvider {
        name: format!("{}-{sequence}", provider.provider.value()),
        provider: provider.provider,
        model: provider.model,
        base_url: provider.base_url,
        api_key: provider.api_key,
        timeout_secs: provider.timeout_secs,
    }
}

fn legacy_default_timeout_secs() -> u64 {
    DEFAULT_PROVIDER_TIMEOUT_SECS
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

fn write_llm_config_file(path: &Path, contents: &str) -> Result<(), ConfigError> {
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

    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

#[cfg(not(unix))]
fn open_config_file(path: &Path) -> Result<std::fs::File, ConfigError> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
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

#[cfg(test)]
mod tests {
    use crate::llm::DEFAULT_PROVIDER_TIMEOUT_SECS;

    use super::parse_llm_config;

    #[test]
    fn legacy_provider_deserialize_uses_one_minute_timeout_by_default() {
        let config = parse_llm_config(
            r#"
provider = "openai"
model = "gpt-4.1-mini"
"#,
            std::path::Path::new("/tmp/llm.toml"),
        )
        .expect("legacy config should parse");

        assert_eq!(config.providers.len(), 1);
        assert_eq!(
            config.providers[0].timeout_secs,
            DEFAULT_PROVIDER_TIMEOUT_SECS
        );
    }
}
