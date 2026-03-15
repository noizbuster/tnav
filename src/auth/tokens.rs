use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::auth::{AuthError, AuthResult};
use crate::secrets::{SecretKind, SecretStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_seconds: Option<u64>,
    pub issued_at_unix_seconds: u64,
    pub token_type: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredTokenMetadata {
    pub expires_at_unix_seconds: Option<u64>,
    pub issued_at_unix_seconds: u64,
    pub token_type: Option<String>,
    pub scopes: Vec<String>,
}

impl TokenSet {
    pub fn new(
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        expires_at_unix_seconds: Option<u64>,
        token_type: Option<String>,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            access_token: access_token.into(),
            refresh_token,
            expires_at_unix_seconds,
            issued_at_unix_seconds: current_unix_timestamp(),
            token_type,
            scopes,
        }
    }

    pub fn metadata(&self) -> StoredTokenMetadata {
        StoredTokenMetadata {
            expires_at_unix_seconds: self.expires_at_unix_seconds,
            issued_at_unix_seconds: self.issued_at_unix_seconds,
            token_type: self.token_type.clone(),
            scopes: self.scopes.clone(),
        }
    }
}

pub fn persist_token_set(
    secret_store: &dyn SecretStore,
    profile: &str,
    token_set: &TokenSet,
) -> AuthResult<()> {
    secret_store.save_secret(
        profile,
        SecretKind::OAuthAccessToken,
        &token_set.access_token,
    )?;

    match &token_set.refresh_token {
        Some(refresh_token) => {
            secret_store.save_secret(profile, SecretKind::OAuthRefreshToken, refresh_token)?;
        }
        None => {
            secret_store.delete_secret(profile, SecretKind::OAuthRefreshToken)?;
        }
    }

    let metadata = serde_json::to_string(&token_set.metadata()).map_err(|error| {
        AuthError::TokenMetadataSerializeFailed {
            message: error.to_string(),
        }
    })?;

    secret_store.save_secret(profile, SecretKind::OAuthMetadata, &metadata)?;

    Ok(())
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::secrets::{MemorySecretStore, SecretKind, SecretStore};

    use super::{StoredTokenMetadata, TokenSet, persist_token_set};

    #[test]
    fn persist_token_set_writes_tokens_and_metadata() {
        let store = MemorySecretStore::new();
        let token_set = TokenSet::new(
            "access-token",
            Some("refresh-token".to_owned()),
            Some(1234),
            Some("Bearer".to_owned()),
            vec!["read".to_owned(), "write".to_owned()],
        );

        persist_token_set(&store, "default", &token_set).expect("token persistence succeeds");

        assert_eq!(
            store
                .load_secret("default", SecretKind::OAuthAccessToken)
                .expect("access token loads"),
            Some("access-token".to_owned())
        );
        assert_eq!(
            store
                .load_secret("default", SecretKind::OAuthRefreshToken)
                .expect("refresh token loads"),
            Some("refresh-token".to_owned())
        );

        let metadata = store
            .load_secret("default", SecretKind::OAuthMetadata)
            .expect("metadata loads")
            .expect("metadata exists");
        let metadata: StoredTokenMetadata =
            serde_json::from_str(&metadata).expect("metadata is valid json");

        assert_eq!(metadata.expires_at_unix_seconds, Some(1234));
        assert_eq!(metadata.token_type.as_deref(), Some("Bearer"));
        assert_eq!(metadata.scopes, vec!["read", "write"]);
    }
}
