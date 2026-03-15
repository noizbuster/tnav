pub mod browser;
pub mod callback_server;
pub mod oauth;
pub mod pkce;
pub mod provider;
pub mod tokens;

use std::time::Duration;

use crate::secrets::SecretStoreError;
use thiserror::Error;

pub use browser::{BrowserOpenOutcome, BrowserOpener, WebbrowserBrowser};
pub use callback_server::{
    CallbackPayload, CallbackServerHandle, CallbackWaitResult, OAuthCallbackError,
};
pub use oauth::{AuthorizationRequest, AwaitCallbackResult, OAuthService};
pub use pkce::PkceBundle;
pub use provider::OAuthProvider;
pub use tokens::{StoredTokenMetadata, TokenSet, persist_token_set};

pub const DEFAULT_CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

pub type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("OAuth provider configuration is invalid: {message}")]
    InvalidProviderConfig { message: String },
    #[error("OAuth URL is invalid: {message}")]
    InvalidUrl { message: String },
    #[error("failed to bind OAuth callback server on {host}: {message}")]
    CallbackBindFailed { host: String, message: String },
    #[error("OAuth callback server failed: {message}")]
    CallbackServerFailed { message: String },
    #[error("OAuth callback channel closed unexpectedly")]
    CallbackChannelClosed,
    #[error("timed out while waiting for OAuth callback")]
    OAuthCallbackTimeout,
    #[error("OAuth callback state did not match")]
    OAuthStateMismatch,
    #[error("OAuth callback returned provider error '{error}'")]
    OAuthProviderError {
        error: String,
        description: Option<String>,
    },
    #[error("failed to exchange OAuth authorization code: {message}")]
    OAuthExchangeFailed { message: String },
    #[error("failed to create HTTP client for OAuth exchange: {message}")]
    HttpClientBuildFailed { message: String },
    #[error("failed to serialize OAuth token metadata: {message}")]
    TokenMetadataSerializeFailed { message: String },
    #[error("secure secret storage is unavailable: {message}")]
    SecretStoreUnavailable { message: String },
    #[error("failed to persist OAuth token data: {message}")]
    SecretStoreWriteFailed { message: String },
}

impl From<SecretStoreError> for AuthError {
    fn from(error: SecretStoreError) -> Self {
        match error {
            SecretStoreError::Unavailable { message } => Self::SecretStoreUnavailable { message },
            other => Self::SecretStoreWriteFailed {
                message: other.to_string(),
            },
        }
    }
}
