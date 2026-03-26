pub mod ask;
pub mod auth;
pub mod doctor;
pub mod executor;
pub mod history;
pub mod init;
pub mod status;

use crate::auth::AuthError;
use crate::config::{Config, ConfigError};
use crate::errors::TnavError;
use crate::secrets::SecretStoreError;
use crate::ui::{PromptError, PromptOption, PromptService, select_from_list};

pub fn unsupported(command: &str) -> Result<(), TnavError> {
    Err(TnavError::UnsupportedMode {
        message: format!("'{command}' is not implemented in Task 7"),
    })
}

pub(crate) fn map_config_error(error: ConfigError) -> TnavError {
    match error {
        ConfigError::NotFound { path } => TnavError::ConfigNotFound {
            message: path.display().to_string(),
        },
        ConfigError::InvalidActiveProfile { profile } => TnavError::ConfigInvalid {
            message: format!("active profile '{profile}' does not exist"),
        },
        other => TnavError::ConfigInvalid {
            message: other.to_string(),
        },
    }
}

pub(crate) fn map_prompt_error(error: PromptError) -> TnavError {
    match error {
        PromptError::Cancelled => TnavError::UserCancelled,
        PromptError::PromptFailed { message, .. } => TnavError::InvalidInput { message },
    }
}

pub(crate) fn map_secret_store_error(error: SecretStoreError) -> TnavError {
    match error {
        SecretStoreError::Unavailable { message } => TnavError::SecretStoreUnavailable { message },
        other => TnavError::SecretStoreWriteFailed {
            message: other.to_string(),
        },
    }
}

pub(crate) fn map_auth_error(error: AuthError) -> TnavError {
    match error {
        AuthError::InvalidProviderConfig { message } | AuthError::InvalidUrl { message } => {
            TnavError::ConfigInvalid { message }
        }
        AuthError::CallbackBindFailed { message, .. }
        | AuthError::CallbackServerFailed { message }
        | AuthError::HttpClientBuildFailed { message }
        | AuthError::TokenMetadataSerializeFailed { message } => {
            TnavError::CommandFailed { message }
        }
        AuthError::CallbackChannelClosed => TnavError::CommandFailed {
            message: "OAuth callback channel closed unexpectedly".to_owned(),
        },
        AuthError::OAuthCallbackTimeout => TnavError::OAuthCallbackTimeout,
        AuthError::OAuthStateMismatch => TnavError::OAuthStateMismatch,
        AuthError::OAuthProviderError { error, description } => TnavError::CommandFailed {
            message: description
                .map(|description| format!("provider returned '{error}': {description}"))
                .unwrap_or_else(|| format!("provider returned '{error}'")),
        },
        AuthError::OAuthExchangeFailed { message } => TnavError::OAuthExchangeFailed { message },
        AuthError::SecretStoreUnavailable { message } => {
            TnavError::SecretStoreUnavailable { message }
        }
        AuthError::SecretStoreWriteFailed { message } => {
            TnavError::SecretStoreWriteFailed { message }
        }
    }
}

pub(crate) fn resolve_profile_name(
    config: &Config,
    requested_profile: Option<&str>,
    non_interactive: bool,
    prompts: &mut impl PromptService,
) -> Result<String, TnavError> {
    if config.profiles.is_empty() {
        return Err(TnavError::ConfigInvalid {
            message: "no profiles are configured yet; run 'tnav init' first".to_owned(),
        });
    }

    if let Some(profile_name) = requested_profile {
        return config
            .profile(profile_name)
            .map(|_| profile_name.to_owned())
            .ok_or_else(|| TnavError::InvalidInput {
                message: format!("profile '{profile_name}' does not exist"),
            });
    }

    if let Some(active_profile) = config.active_profile_name() {
        return Ok(active_profile.to_owned());
    }

    if config.profiles.len() == 1
        && let Some((profile_name, _)) = config.profiles.iter().next()
    {
        return Ok(profile_name.clone());
    }

    if non_interactive {
        return Err(TnavError::InvalidInput {
            message: "multiple profiles exist; pass --profile in non-interactive mode".to_owned(),
        });
    }

    let mut options = config
        .profiles
        .keys()
        .map(|name| PromptOption::simple(name.clone()))
        .collect::<Vec<_>>();
    options.sort_by(|left, right| left.label().cmp(right.label()));

    select_from_list(prompts, "Choose a profile:", &options).map_err(map_prompt_error)
}
