use keyring::Entry;
use keyring::Error as KeyringError;

use super::{
    KEYRING_SERVICE, SecretKey, SecretKind, SecretStore, SecretStoreError, SecretStoreResult,
};

#[derive(Debug, Clone, Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<Entry> {
        let secret_key = SecretKey::new(profile, kind);
        Entry::new(KEYRING_SERVICE, &secret_key.account_name()).map_err(|error| match error {
            KeyringError::NoStorageAccess(message) => SecretStoreError::Unavailable {
                message: message.to_string(),
            },
            KeyringError::PlatformFailure(message) => SecretStoreError::Unavailable {
                message: message.to_string(),
            },
            other => SecretStoreError::InvalidKey {
                message: other.to_string(),
            },
        })
    }
}

impl SecretStore for KeyringSecretStore {
    fn save_secret(&self, profile: &str, kind: SecretKind, value: &str) -> SecretStoreResult<()> {
        let entry = self.entry(profile, kind)?;
        tracing::debug!(
            profile = %profile,
            kind = ?kind,
            "Saving secret to keyring"
        );
        entry.set_password(value).map_err(|error| {
            tracing::error!(profile = %profile, kind = ?kind, error = %error, "Failed to save secret");
            map_keyring_error(profile, kind, error, SecretAction::Save)
        })?;
        tracing::debug!(profile = %profile, kind = ?kind, "Secret saved successfully");
        Ok(())
    }

    fn load_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<Option<String>> {
        let entry = self.entry(profile, kind)?;
        tracing::debug!(
            profile = %profile,
            kind = ?kind,
            "Loading secret from keyring"
        );
        match entry.get_password() {
            Ok(value) => {
                tracing::debug!(profile = %profile, kind = ?kind, "Secret loaded successfully");
                Ok(Some(value))
            }
            Err(KeyringError::NoEntry) => {
                tracing::warn!(profile = %profile, kind = ?kind, "No secret found in keyring");
                Ok(None)
            }
            Err(error) => {
                tracing::error!(profile = %profile, kind = ?kind, error = %error, "Failed to load secret");
                Err(map_keyring_error(profile, kind, error, SecretAction::Load))
            }
        }
    }

    fn delete_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<()> {
        let entry = self.entry(profile, kind)?;
        tracing::debug!(
            profile = %profile,
            kind = ?kind,
            "Deleting secret from keyring"
        );
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(
                profile,
                kind,
                error,
                SecretAction::Delete,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SecretAction {
    Save,
    Load,
    Delete,
}

fn map_keyring_error(
    profile: &str,
    kind: SecretKind,
    error: KeyringError,
    action: SecretAction,
) -> SecretStoreError {
    match error {
        KeyringError::NoStorageAccess(message) => {
            return SecretStoreError::Unavailable {
                message: message.to_string(),
            };
        }
        KeyringError::PlatformFailure(message) => {
            return SecretStoreError::Unavailable {
                message: message.to_string(),
            };
        }
        _ => {}
    }

    let message = error.to_string();
    let kind = kind.as_key_fragment();
    let profile = profile.to_owned();

    match action {
        SecretAction::Save => SecretStoreError::SaveFailed {
            profile,
            kind,
            message,
        },
        SecretAction::Load => SecretStoreError::LoadFailed {
            profile,
            kind,
            message,
        },
        SecretAction::Delete => SecretStoreError::DeleteFailed {
            profile,
            kind,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyringSecretStore, SecretKind, SecretStore};

    #[test]
    fn keyring_round_trip_with_llm_provider_key_format() {
        let store = KeyringSecretStore::new();
        let profile = "llm-provider/test-provider-instance";
        let test_value = "test-api-key-12345";

        let save_result = store.save_secret(profile, SecretKind::ApiKey, test_value);
        if save_result.is_err() {
            eprintln!("Skipping test: keyring save failed (secret service may not be available)");
            return;
        }

        let loaded = store
            .load_secret(profile, SecretKind::ApiKey)
            .expect("load should succeed if save succeeded");

        let _ = store.delete_secret(profile, SecretKind::ApiKey);

        if loaded.is_none() {
            eprintln!(
                "Skipping test: keyring load returned None after save (secret service may not be persisting)"
            );
            return;
        }

        assert_eq!(
            loaded,
            Some(test_value.to_owned()),
            "loaded value should match saved value"
        );
    }
}
