mod keyring_store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use keyring_store::KeyringSecretStore;
use thiserror::Error;

pub const KEYRING_SERVICE: &str = "tnav";

pub trait SecretStore {
    fn save_secret(&self, profile: &str, kind: SecretKind, value: &str) -> SecretStoreResult<()>;

    fn load_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<Option<String>>;

    fn delete_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<()>;
}

pub type SecretStoreResult<T> = Result<T, SecretStoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    ApiKey,
    OAuthAccessToken,
    OAuthRefreshToken,
    OAuthMetadata,
}

impl SecretKind {
    pub fn as_key_fragment(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::OAuthAccessToken => "oauth_access_token",
            Self::OAuthRefreshToken => "oauth_refresh_token",
            Self::OAuthMetadata => "oauth_metadata",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey {
    profile: String,
    kind: SecretKind,
}

impl SecretKey {
    pub fn new(profile: impl Into<String>, kind: SecretKind) -> Self {
        Self {
            profile: profile.into(),
            kind,
        }
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn kind(&self) -> SecretKind {
        self.kind
    }

    pub fn account_name(&self) -> String {
        format!(
            "profile/{}/{kind}",
            self.profile,
            kind = self.kind.as_key_fragment()
        )
    }
}

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret storage key is invalid: {message}")]
    InvalidKey { message: String },
    #[error("secure secret storage is unavailable: {message}")]
    Unavailable { message: String },
    #[error("failed to save secret '{kind}' for profile '{profile}': {message}")]
    SaveFailed {
        profile: String,
        kind: &'static str,
        message: String,
    },
    #[error("failed to load secret '{kind}' for profile '{profile}': {message}")]
    LoadFailed {
        profile: String,
        kind: &'static str,
        message: String,
    },
    #[error("failed to delete secret '{kind}' for profile '{profile}': {message}")]
    DeleteFailed {
        profile: String,
        kind: &'static str,
        message: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct MemorySecretStore {
    secrets: Arc<Mutex<HashMap<SecretKey, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn save_secret(&self, profile: &str, kind: SecretKind, value: &str) -> SecretStoreResult<()> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|error| SecretStoreError::Unavailable {
                message: error.to_string(),
            })?;
        secrets.insert(SecretKey::new(profile, kind), value.to_owned());
        Ok(())
    }

    fn load_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<Option<String>> {
        let secrets = self
            .secrets
            .lock()
            .map_err(|error| SecretStoreError::Unavailable {
                message: error.to_string(),
            })?;

        Ok(secrets.get(&SecretKey::new(profile, kind)).cloned())
    }

    fn delete_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<()> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|error| SecretStoreError::Unavailable {
                message: error.to_string(),
            })?;
        secrets.remove(&SecretKey::new(profile, kind));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySecretStore, SecretKey, SecretKind, SecretStore};

    #[test]
    fn secret_key_uses_plan_account_naming() {
        let key = SecretKey::new("default", SecretKind::OAuthRefreshToken);

        assert_eq!(key.account_name(), "profile/default/oauth_refresh_token");
    }

    #[test]
    fn memory_secret_store_round_trips_and_deletes_values() {
        let store = MemorySecretStore::new();

        store
            .save_secret("default", SecretKind::ApiKey, "secret-value")
            .expect("save succeeds");
        assert_eq!(
            store
                .load_secret("default", SecretKind::ApiKey)
                .expect("load succeeds"),
            Some("secret-value".to_owned())
        );

        store
            .delete_secret("default", SecretKind::ApiKey)
            .expect("delete succeeds");
        assert_eq!(
            store
                .load_secret("default", SecretKind::ApiKey)
                .expect("load succeeds after delete"),
            None
        );
    }
}
