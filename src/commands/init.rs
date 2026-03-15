use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config::{load_optional, save};
use crate::output::Output;
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};
use crate::ui::{InitWizard, InquirePromptService, confirm_overwrite};
use crate::util::redact::redact_secret;

use super::{map_config_error, map_prompt_error, map_secret_store_error};
use crate::errors::TnavError;

#[derive(Debug, Serialize)]
struct InitSuccess {
    command: &'static str,
    profile: String,
    config_path: String,
    auth_method: &'static str,
    api_key_stored: bool,
}

pub async fn run(global: &GlobalArgs) -> Result<(), TnavError> {
    if global.non_interactive {
        return Err(TnavError::UnsupportedMode {
            message: "'tnav init' needs interactive answers in Task 7".to_owned(),
        });
    }

    let output = Output::new(global);
    let mut config = load_optional(global.config.as_deref())
        .map_err(map_config_error)?
        .unwrap_or_default();
    let default_profile_name = global
        .profile
        .as_deref()
        .or(config.active_profile_name())
        .or(Some("default"));

    if !output.is_json() {
        output.line("tnav init");
        output.line("Guided setup for your first working profile.");
    }

    let mut prompts = InquirePromptService::new();
    let mut wizard = InitWizard::new(&mut prompts, default_profile_name);
    let result = wizard.run().map_err(map_prompt_error)?;

    if config.profiles.contains_key(&result.profile_name) && !global.yes {
        let overwrite =
            confirm_overwrite(&mut prompts, &format!("profile '{}'", result.profile_name))
                .map_err(map_prompt_error)?;
        if !overwrite {
            return Err(TnavError::UserCancelled);
        }
    }

    let auth_method = match &result.profile.auth_method {
        crate::config::AuthMethod::ApiKey => "api_key",
        crate::config::AuthMethod::OAuth => "oauth",
    };
    let api_key_preview = result.api_key.as_deref().map(redact_secret);

    config
        .profiles
        .insert(result.profile_name.clone(), result.profile);
    config.set_active_profile(result.profile_name.clone());

    let config_path = save(&config, global.config.as_deref()).map_err(map_config_error)?;

    if let Some(api_key) = result.api_key.as_deref() {
        KeyringSecretStore::new()
            .save_secret(&result.profile_name, SecretKind::ApiKey, api_key)
            .map_err(map_secret_store_error)?;
    }

    if output.is_json() {
        return output.print_json(&InitSuccess {
            command: "init",
            profile: result.profile_name,
            config_path: config_path.display().to_string(),
            auth_method,
            api_key_stored: api_key_preview.is_some(),
        });
    }

    output.line(format!("Saved config to {}", config_path.display()));
    for line in result.summary_lines {
        output.line(format!("- {line}"));
    }
    if let Some(api_key_preview) = api_key_preview {
        output.line(format!("Stored API key securely as {api_key_preview}"));
    }
    if auth_method == "oauth" {
        output.line(format!(
            "Next step: run 'tnav --profile {} auth login' to finish OAuth sign-in.",
            result.profile_name
        ));
    }

    Ok(())
}
