use std::process::ExitCode;
use thiserror::Error;

use crate::history::HistoryError;

#[derive(Debug, Error)]
pub enum TnavError {
    #[error("operation cancelled by user")]
    UserCancelled,
    #[error("configuration file was not found: {message}")]
    ConfigNotFound { message: String },
    #[error("configuration is invalid: {message}")]
    ConfigInvalid { message: String },
    #[error("invalid input: {message}")]
    InvalidInput { message: String },
    #[error("secure secret storage is unavailable: {message}")]
    SecretStoreUnavailable { message: String },
    #[error("failed to write to secure secret storage: {message}")]
    SecretStoreWriteFailed { message: String },
    #[error("failed to open a browser for authentication: {message}")]
    BrowserOpenFailed { message: String },
    #[error("timed out while waiting for OAuth callback")]
    OAuthCallbackTimeout,
    #[error("OAuth callback state did not match")]
    OAuthStateMismatch,
    #[error("failed to exchange OAuth authorization code: {message}")]
    OAuthExchangeFailed { message: String },
    #[error("network request failed: {message}")]
    NetworkError { message: String },
    #[error("requested mode is not supported yet: {message}")]
    UnsupportedMode { message: String },
    #[error("command failed: {message}")]
    CommandFailed { message: String },
    #[error("history operation failed: {message}")]
    HistoryError { message: String },
}

impl From<HistoryError> for TnavError {
    fn from(err: HistoryError) -> Self {
        TnavError::HistoryError {
            message: err.to_string(),
        }
    }
}

impl TnavError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::UserCancelled
            | Self::ConfigInvalid { .. }
            | Self::InvalidInput { .. }
            | Self::UnsupportedMode { .. } => ExitCode::from(2),
            Self::ConfigNotFound { .. }
            | Self::SecretStoreUnavailable { .. }
            | Self::SecretStoreWriteFailed { .. }
            | Self::BrowserOpenFailed { .. }
            | Self::OAuthCallbackTimeout
            | Self::OAuthStateMismatch
            | Self::OAuthExchangeFailed { .. }
            | Self::NetworkError { .. }
            | Self::CommandFailed { .. }
            | Self::HistoryError { .. } => ExitCode::from(1),
        }
    }
}
