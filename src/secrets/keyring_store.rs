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
        self.entry(profile, kind)?
            .set_password(value)
            .map_err(|error| map_keyring_error(profile, kind, error, SecretAction::Save))
    }

    fn load_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<Option<String>> {
        match self.entry(profile, kind)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(map_keyring_error(profile, kind, error, SecretAction::Load)),
        }
    }

    fn delete_secret(&self, profile: &str, kind: SecretKind) -> SecretStoreResult<()> {
        match self.entry(profile, kind)?.delete_credential() {
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
