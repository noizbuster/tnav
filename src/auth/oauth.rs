use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};

use crate::auth::browser::{BrowserOpener, WebbrowserBrowser};
use crate::auth::callback_server::{CallbackPayload, CallbackServerHandle, CallbackWaitResult};
use crate::auth::pkce::PkceBundle;
use crate::auth::provider::OAuthProvider;
use crate::auth::tokens::{TokenSet, persist_token_set};
use crate::auth::{AuthError, AuthResult, DEFAULT_CALLBACK_TIMEOUT};
use crate::secrets::SecretStore;

type ConfiguredBasicClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Debug)]
pub struct AuthorizationRequest {
    pub authorization_url: String,
    pub csrf_state: String,
    pub pkce_verifier: PkceCodeVerifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwaitCallbackResult {
    AuthorizationCode {
        code: String,
        state: String,
    },
    ProviderError {
        error: String,
        description: Option<String>,
    },
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct OAuthService<B = WebbrowserBrowser> {
    browser: B,
    http_client: reqwest::Client,
}

impl Default for OAuthService<WebbrowserBrowser> {
    fn default() -> Self {
        Self::new().expect("default OAuthService HTTP client should build")
    }
}

impl OAuthService<WebbrowserBrowser> {
    pub fn new() -> AuthResult<Self> {
        Self::with_browser(WebbrowserBrowser::new())
    }
}

impl<B> OAuthService<B>
where
    B: BrowserOpener,
{
    pub fn with_browser(browser: B) -> AuthResult<Self> {
        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AuthError::HttpClientBuildFailed {
                message: error.to_string(),
            })?;

        Ok(Self {
            browser,
            http_client,
        })
    }

    pub fn build_authorization_url(
        &self,
        provider: &OAuthProvider,
        redirect_uri: &str,
    ) -> AuthResult<AuthorizationRequest> {
        provider.validate()?;

        let pkce = PkceBundle::generate();
        let client = build_client(provider, redirect_uri)?;
        let scopes = provider.scopes.iter().cloned().map(Scope::new);

        let mut authorize_request = client.authorize_url(|| CsrfToken::new(pkce.state.clone()));

        for scope in scopes {
            authorize_request = authorize_request.add_scope(scope);
        }

        let (authorization_url, _) = authorize_request
            .set_pkce_challenge(pkce.code_challenge)
            .url();

        Ok(AuthorizationRequest {
            authorization_url: authorization_url.to_string(),
            csrf_state: pkce.state,
            pkce_verifier: pkce.code_verifier,
        })
    }

    pub async fn start_callback_server(
        &self,
        provider: &OAuthProvider,
    ) -> AuthResult<CallbackServerHandle> {
        provider.validate()?;
        CallbackServerHandle::bind(provider.callback_bind_host(), &provider.redirect_path).await
    }

    pub async fn await_callback(
        &self,
        callback_server: CallbackServerHandle,
        expected_state: &str,
    ) -> AuthResult<AwaitCallbackResult> {
        self.await_callback_with_timeout(callback_server, expected_state, DEFAULT_CALLBACK_TIMEOUT)
            .await
    }

    pub async fn await_callback_with_timeout(
        &self,
        callback_server: CallbackServerHandle,
        expected_state: &str,
        timeout: Duration,
    ) -> AuthResult<AwaitCallbackResult> {
        match callback_server.wait_for_callback(timeout).await? {
            CallbackWaitResult::TimedOut => Ok(AwaitCallbackResult::TimedOut),
            CallbackWaitResult::Received(CallbackPayload::AuthorizationCode { code, state }) => {
                verify_callback_state(Some(state.as_str()), expected_state)?;

                Ok(AwaitCallbackResult::AuthorizationCode { code, state })
            }
            CallbackWaitResult::Received(CallbackPayload::ProviderError {
                error,
                description,
                state,
            }) => {
                verify_callback_state(state.as_deref(), expected_state)?;

                Ok(AwaitCallbackResult::ProviderError { error, description })
            }
            CallbackWaitResult::Received(CallbackPayload::InvalidRequest { message }) => {
                Err(AuthError::CallbackServerFailed { message })
            }
        }
    }

    pub async fn exchange_code(
        &self,
        provider: &OAuthProvider,
        redirect_uri: &str,
        code: impl Into<String>,
        pkce_verifier: PkceCodeVerifier,
    ) -> AuthResult<TokenSet> {
        provider.validate()?;

        let token_response = build_client(provider, redirect_uri)?
            .exchange_code(AuthorizationCode::new(code.into()))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&self.http_client)
            .await
            .map_err(|error| AuthError::OAuthExchangeFailed {
                message: error.to_string(),
            })?;

        let issued_at_unix_seconds = current_unix_timestamp();
        let expires_at_unix_seconds = token_response
            .expires_in()
            .map(|duration| issued_at_unix_seconds.saturating_add(duration.as_secs()));

        Ok(TokenSet {
            access_token: token_response.access_token().secret().to_owned(),
            refresh_token: token_response
                .refresh_token()
                .map(|token| token.secret().to_owned()),
            expires_at_unix_seconds,
            issued_at_unix_seconds,
            token_type: Some(format!("{:?}", token_response.token_type())),
            scopes: token_response
                .scopes()
                .map(|scopes| {
                    scopes
                        .iter()
                        .map(|scope| scope.as_ref().to_owned())
                        .collect()
                })
                .unwrap_or_else(|| provider.scopes.clone()),
        })
    }

    pub fn save_token_set(
        &self,
        secret_store: &dyn SecretStore,
        profile: &str,
        token_set: &TokenSet,
    ) -> AuthResult<()> {
        persist_token_set(secret_store, profile, token_set)
    }

    pub fn browser(&self) -> &B {
        &self.browser
    }
}

fn build_client(provider: &OAuthProvider, redirect_uri: &str) -> AuthResult<ConfiguredBasicClient> {
    let auth_url = AuthUrl::new(provider.authorization_url.clone()).map_err(|error| {
        AuthError::InvalidUrl {
            message: error.to_string(),
        }
    })?;
    let token_url =
        TokenUrl::new(provider.token_url.clone()).map_err(|error| AuthError::InvalidUrl {
            message: error.to_string(),
        })?;
    let redirect_url =
        RedirectUrl::new(redirect_uri.to_owned()).map_err(|error| AuthError::InvalidUrl {
            message: error.to_string(),
        })?;

    Ok(BasicClient::new(ClientId::new(provider.client_id.clone()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url))
}

fn current_unix_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn verify_callback_state(received_state: Option<&str>, expected_state: &str) -> AuthResult<()> {
    if received_state == Some(expected_state) {
        Ok(())
    } else {
        Err(AuthError::OAuthStateMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::OAuthService;
    use crate::auth::provider::OAuthProvider;

    #[test]
    fn authorization_url_contains_pkce_and_state() {
        let service = OAuthService::new().expect("service builds");
        let provider = OAuthProvider::new(
            "example",
            "client-id",
            "https://example.com/oauth/authorize",
            "https://example.com/oauth/token",
            None,
            vec!["read".to_owned(), "write".to_owned()],
            "127.0.0.1",
            "/oauth/callback",
        );

        let request = service
            .build_authorization_url(&provider, "http://127.0.0.1:8080/oauth/callback")
            .expect("authorization url builds");

        assert!(request.authorization_url.contains("response_type=code"));
        assert!(request.authorization_url.contains("code_challenge="));
        assert!(request.authorization_url.contains("state="));
        assert!(!request.csrf_state.is_empty());
    }
}
