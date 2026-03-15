use serde::Serialize;

use crate::auth::BrowserOpener;
use crate::auth::{AwaitCallbackResult, OAuthProvider, OAuthService};
use crate::cli::{AuthCommand, GlobalArgs};
use crate::config::{AuthMethod, load};
use crate::output::Output;
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};
use crate::ui::{InquirePromptService, confirm_overwrite, prompt_api_key};
use crate::util::redact::redact_secret;

use super::{
    map_auth_error, map_config_error, map_prompt_error, map_secret_store_error,
    resolve_profile_name, unsupported,
};
use crate::errors::TnavError;

#[derive(Debug, Serialize)]
struct ApiKeySuccess {
    command: &'static str,
    profile: String,
    provider: String,
    stored: bool,
}

#[derive(Debug, Serialize)]
struct LoginSuccess {
    command: &'static str,
    profile: String,
    provider: String,
    opened_browser: bool,
    authorization_url: String,
}

pub async fn run(command: AuthCommand, global: &GlobalArgs) -> Result<(), TnavError> {
    match command {
        AuthCommand::ApiKey => run_api_key(global).await,
        AuthCommand::Login => run_login(global).await,
        AuthCommand::Logout => unsupported("auth logout"),
        AuthCommand::Status => unsupported("auth status"),
        AuthCommand::Revoke => unsupported("auth revoke"),
    }
}

async fn run_api_key(global: &GlobalArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let config = load(global.config.as_deref()).map_err(map_config_error)?;
    let mut prompts = InquirePromptService::new();
    let profile_name = resolve_profile_name(
        &config,
        global.profile.as_deref(),
        global.non_interactive,
        &mut prompts,
    )?;
    let profile = config
        .profile(&profile_name)
        .ok_or_else(|| TnavError::ConfigInvalid {
            message: format!("profile '{profile_name}' no longer exists"),
        })?;

    let store = KeyringSecretStore::new();
    let existing = store
        .load_secret(&profile_name, SecretKind::ApiKey)
        .map_err(map_secret_store_error)?;
    if existing.is_some() && !global.yes {
        if global.non_interactive {
            return Err(TnavError::InvalidInput {
                message: format!(
                    "profile '{profile_name}' already has an API key; pass --yes to overwrite it"
                ),
            });
        }

        let overwrite = confirm_overwrite(
            &mut prompts,
            &format!("API key for profile '{profile_name}'"),
        )
        .map_err(map_prompt_error)?;
        if !overwrite {
            return Err(TnavError::UserCancelled);
        }
    }

    if global.non_interactive {
        return Err(TnavError::UnsupportedMode {
            message: "'tnav auth api-key' needs an interactive key prompt in Task 7".to_owned(),
        });
    }

    let api_key = prompt_api_key(&mut prompts).map_err(map_prompt_error)?;
    store
        .save_secret(&profile_name, SecretKind::ApiKey, &api_key)
        .map_err(map_secret_store_error)?;

    if output.is_json() {
        return output.print_json(&ApiKeySuccess {
            command: "auth api-key",
            profile: profile_name,
            provider: profile.provider.clone(),
            stored: true,
        });
    }

    output.line(format!(
        "Stored API key for profile '{}' ({}) as {}",
        profile_name,
        profile.provider,
        redact_secret(&api_key)
    ));
    Ok(())
}

async fn run_login(global: &GlobalArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let config = load(global.config.as_deref()).map_err(map_config_error)?;
    let mut prompts = InquirePromptService::new();
    let profile_name = resolve_profile_name(
        &config,
        global.profile.as_deref(),
        global.non_interactive,
        &mut prompts,
    )?;
    let profile = config
        .profile(&profile_name)
        .ok_or_else(|| TnavError::ConfigInvalid {
            message: format!("profile '{profile_name}' no longer exists"),
        })?;

    if !matches!(profile.auth_method, AuthMethod::OAuth) {
        return Err(TnavError::InvalidInput {
            message: format!("profile '{profile_name}' is not configured for OAuth login"),
        });
    }

    let provider = profile_to_oauth_provider(profile)?;
    let oauth_service = OAuthService::new().map_err(map_auth_error)?;
    let callback_server = oauth_service
        .start_callback_server(&provider)
        .await
        .map_err(map_auth_error)?;
    let redirect_uri = provider.redirect_uri_for_port(callback_server.local_addr().port());
    let request = oauth_service
        .build_authorization_url(&provider, &redirect_uri)
        .map_err(map_auth_error)?;

    let should_open_browser = !global.no_browser && profile.ui.open_browser_automatically;
    let mut opened_browser = false;
    if should_open_browser {
        let browser_outcome = oauth_service.browser().open(&request.authorization_url);
        opened_browser = browser_outcome.opened;
        if !browser_outcome.opened && !output.is_json() {
            output.line("Browser launch failed. Open this URL manually:");
            output.line(&request.authorization_url);
            if let Some(failure) = browser_outcome.failure {
                output.line(format!("Browser error: {failure}"));
            }
        }
    } else if !output.is_json() {
        output.line("Open this URL to continue OAuth login:");
        output.line(&request.authorization_url);
    }

    if !output.is_json() {
        output.line("Waiting for the localhost OAuth callback...");
    }

    let callback_result = oauth_service
        .await_callback(callback_server, &request.csrf_state)
        .await
        .map_err(map_auth_error)?;

    let code = match callback_result {
        AwaitCallbackResult::AuthorizationCode { code, .. } => code,
        AwaitCallbackResult::ProviderError { error, description } => {
            return Err(map_auth_error(crate::auth::AuthError::OAuthProviderError {
                error,
                description,
            }));
        }
        AwaitCallbackResult::TimedOut => return Err(TnavError::OAuthCallbackTimeout),
    };

    let token_set = oauth_service
        .exchange_code(&provider, &redirect_uri, code, request.pkce_verifier)
        .await
        .map_err(map_auth_error)?;
    oauth_service
        .save_token_set(&KeyringSecretStore::new(), &profile_name, &token_set)
        .map_err(map_auth_error)?;

    if output.is_json() {
        return output.print_json(&LoginSuccess {
            command: "auth login",
            profile: profile_name,
            provider: provider.name,
            opened_browser,
            authorization_url: request.authorization_url,
        });
    }

    output.line(format!(
        "OAuth login complete for profile '{}' ({})",
        profile_name, provider.name
    ));
    Ok(())
}

fn profile_to_oauth_provider(
    profile: &crate::config::ProfileConfig,
) -> Result<OAuthProvider, TnavError> {
    let client_id = profile
        .client_id
        .clone()
        .ok_or_else(|| TnavError::ConfigInvalid {
            message: format!("profile '{}' is missing client_id", profile.provider),
        })?;
    let authorization_url =
        profile
            .authorization_url
            .clone()
            .ok_or_else(|| TnavError::ConfigInvalid {
                message: format!(
                    "profile '{}' is missing authorization_url",
                    profile.provider
                ),
            })?;
    let token_url = profile
        .token_url
        .clone()
        .ok_or_else(|| TnavError::ConfigInvalid {
            message: format!("profile '{}' is missing token_url", profile.provider),
        })?;

    Ok(OAuthProvider::new(
        profile.provider.clone(),
        client_id,
        authorization_url,
        token_url,
        profile.revocation_url.clone(),
        profile.default_scopes.clone(),
        profile.redirect_host.clone(),
        profile.redirect_path.clone(),
    ))
}
