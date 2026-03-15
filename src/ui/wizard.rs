use inquire::{Confirm, Select, Text};

use crate::config::{AuthMethod, ProfileConfig, UiConfig};
use crate::ui::{
    PromptError, PromptOption, PromptService, multiselect_scopes, prompt_api_key,
    prompt_profile_name, select_provider,
};

#[derive(Debug, Clone)]
pub struct InitWizardResult {
    pub profile_name: String,
    pub profile: ProfileConfig,
    pub api_key: Option<String>,
    pub summary_lines: Vec<String>,
}

pub struct InitWizard<'a, P> {
    prompts: &'a mut P,
    default_profile_name: Option<&'a str>,
}

impl<'a, P> InitWizard<'a, P>
where
    P: PromptService,
{
    pub fn new(prompts: &'a mut P, default_profile_name: Option<&'a str>) -> Self {
        Self {
            prompts,
            default_profile_name,
        }
    }

    pub fn run(&mut self) -> Result<InitWizardResult, PromptError> {
        let profile_name = prompt_profile_name(self.prompts, self.default_profile_name)?;
        let auth_choice = select_auth_choice()?;

        let (profile, api_key, mut summary_lines) = match auth_choice {
            InitAuthChoice::ApiKey => {
                let (profile, api_key, provider_label) = self.configure_api_key_profile()?;
                (
                    profile,
                    Some(api_key),
                    vec![format!("configured API key profile for {provider_label}")],
                )
            }
            InitAuthChoice::OAuth => {
                let (profile, provider_label) = self.configure_oauth_profile()?;
                (
                    profile,
                    None,
                    vec![format!("configured OAuth profile for {provider_label}")],
                )
            }
            InitAuthChoice::Both => {
                let (profile, provider_label) = self.configure_oauth_profile()?;
                let api_key = prompt_api_key(self.prompts)?;
                (
                    profile,
                    Some(api_key),
                    vec![
                        format!("configured OAuth profile for {provider_label}"),
                        "captured an API key to store separately in the secret store".to_owned(),
                    ],
                )
            }
        };

        summary_lines.push(format!("saved profile name: {profile_name}"));

        Ok(InitWizardResult {
            profile_name,
            profile,
            api_key,
            summary_lines,
        })
    }

    fn configure_api_key_profile(
        &mut self,
    ) -> Result<(ProfileConfig, String, String), PromptError> {
        let provider = select_provider(
            self.prompts,
            &[
                PromptOption::new("openai", "OpenAI"),
                PromptOption::new("anthropic", "Anthropic"),
                PromptOption::new("custom", "Custom API key provider"),
            ],
        )?;

        let (provider_name, base_url, label) = match provider.as_str() {
            "openai" => (
                "openai".to_owned(),
                Some("https://api.openai.com/v1".to_owned()),
                "OpenAI".to_owned(),
            ),
            "anthropic" => (
                "anthropic".to_owned(),
                Some("https://api.anthropic.com".to_owned()),
                "Anthropic".to_owned(),
            ),
            _ => {
                let provider_name = prompt_required_text(
                    "Provider name:",
                    Some("Enter a short provider identifier for this API key profile."),
                    None,
                )?;
                let base_url = prompt_optional_text(
                    "Base URL (optional):",
                    Some("Leave blank if this provider does not need a saved base URL."),
                    None,
                )?;

                (provider_name.clone(), base_url, provider_name)
            }
        };

        let api_key = prompt_api_key(self.prompts)?;
        let profile = ProfileConfig {
            provider: provider_name,
            auth_method: AuthMethod::ApiKey,
            base_url,
            default_scopes: Vec::new(),
            client_id: None,
            authorization_url: None,
            token_url: None,
            revocation_url: None,
            redirect_host: "127.0.0.1".to_owned(),
            redirect_path: "/oauth/callback".to_owned(),
            ui: UiConfig::default(),
        };

        Ok((profile, api_key, label))
    }

    fn configure_oauth_profile(&mut self) -> Result<(ProfileConfig, String), PromptError> {
        let provider = select_provider(
            self.prompts,
            &[
                PromptOption::new("github", "GitHub"),
                PromptOption::new("custom", "Custom OAuth provider"),
            ],
        )?;

        match provider.as_str() {
            "github" => self.configure_github_oauth_profile(),
            _ => self.configure_custom_oauth_profile(),
        }
    }

    fn configure_github_oauth_profile(&mut self) -> Result<(ProfileConfig, String), PromptError> {
        let client_id = prompt_required_text(
            "OAuth client ID:",
            Some("Create a GitHub OAuth app and paste the public client ID here."),
            None,
        )?;
        let default_scopes = multiselect_scopes(
            self.prompts,
            &[
                PromptOption::simple("repo"),
                PromptOption::simple("read:user"),
                PromptOption::simple("gist"),
                PromptOption::simple("workflow"),
            ],
            &["repo".to_owned(), "read:user".to_owned()],
        )?;
        let open_browser_automatically = prompt_confirm(
            "Open the browser automatically during login?",
            true,
            Some("If disabled, tnav prints the authorization URL and waits for the callback."),
        )?;

        Ok((
            ProfileConfig {
                provider: "github".to_owned(),
                auth_method: AuthMethod::OAuth,
                base_url: Some("https://api.github.com".to_owned()),
                default_scopes,
                client_id: Some(client_id),
                authorization_url: Some("https://github.com/login/oauth/authorize".to_owned()),
                token_url: Some("https://github.com/login/oauth/access_token".to_owned()),
                revocation_url: None,
                redirect_host: "127.0.0.1".to_owned(),
                redirect_path: "/oauth/callback".to_owned(),
                ui: UiConfig {
                    open_browser_automatically,
                },
            },
            "GitHub".to_owned(),
        ))
    }

    fn configure_custom_oauth_profile(&mut self) -> Result<(ProfileConfig, String), PromptError> {
        let provider_name = prompt_required_text(
            "Provider name:",
            Some("Use a stable name that you want to keep in config."),
            None,
        )?;
        let base_url = prompt_optional_text(
            "Base URL (optional):",
            Some("Leave blank if the provider only needs OAuth endpoints."),
            None,
        )?;
        let client_id = prompt_required_text(
            "OAuth client ID:",
            Some("Paste the public client ID used for the OAuth authorization code flow."),
            None,
        )?;
        let authorization_url = prompt_required_text(
            "Authorization URL:",
            Some("Example: https://provider.example.com/oauth/authorize"),
            None,
        )?;
        let token_url = prompt_required_text(
            "Token URL:",
            Some("Example: https://provider.example.com/oauth/token"),
            None,
        )?;
        let revocation_url = prompt_optional_text(
            "Revocation URL (optional):",
            Some("Leave blank if the provider does not expose a token revocation endpoint."),
            None,
        )?;
        let scope_text = prompt_optional_text(
            "Default scopes (comma-separated, optional):",
            Some("Example: read,write,offline_access"),
            None,
        )?;
        let redirect_host = prompt_required_text(
            "Redirect host:",
            Some("Loopback hosts only, such as 127.0.0.1 or ::1."),
            Some("127.0.0.1"),
        )?;
        let redirect_path = prompt_required_text(
            "Redirect path:",
            Some("This should match the callback path registered with the provider."),
            Some("/oauth/callback"),
        )?;
        let open_browser_automatically = prompt_confirm(
            "Open the browser automatically during login?",
            true,
            Some("If disabled, tnav prints the authorization URL and waits for the callback."),
        )?;

        Ok((
            ProfileConfig {
                provider: provider_name.clone(),
                auth_method: AuthMethod::OAuth,
                base_url,
                default_scopes: parse_csv_list(scope_text.as_deref()),
                client_id: Some(client_id),
                authorization_url: Some(authorization_url),
                token_url: Some(token_url),
                revocation_url,
                redirect_host,
                redirect_path,
                ui: UiConfig {
                    open_browser_automatically,
                },
            },
            provider_name,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitAuthChoice {
    ApiKey,
    OAuth,
    Both,
}

impl std::fmt::Display for InitAuthChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => f.write_str("API key"),
            Self::OAuth => f.write_str("OAuth login"),
            Self::Both => f.write_str("Both (OAuth + API key)"),
        }
    }
}

fn select_auth_choice() -> Result<InitAuthChoice, PromptError> {
    map_prompt_result(
        "auth choice",
        Select::new(
            "Authentication setup:",
            vec![
                InitAuthChoice::ApiKey,
                InitAuthChoice::OAuth,
                InitAuthChoice::Both,
            ],
        )
        .with_help_message("Choose the authentication path to configure during init.")
        .without_filtering()
        .prompt(),
    )
}

fn prompt_required_text(
    message: &'static str,
    help: Option<&'static str>,
    default: Option<&str>,
) -> Result<String, PromptError> {
    let prompt = text_prompt(message, help, default);

    map_prompt_result(message, prompt.prompt()).and_then(|value: String| {
        let normalized = value.trim().to_owned();
        if normalized.is_empty() {
            Err(PromptError::PromptFailed {
                prompt: message,
                message: "value cannot be empty".to_owned(),
            })
        } else {
            Ok(normalized)
        }
    })
}

fn prompt_optional_text(
    message: &'static str,
    help: Option<&'static str>,
    default: Option<&str>,
) -> Result<Option<String>, PromptError> {
    let prompt = text_prompt(message, help, default);
    map_prompt_result(message, prompt.prompt()).map(|value: String| {
        let normalized = value.trim().to_owned();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn prompt_confirm(
    message: &'static str,
    default: bool,
    help: Option<&'static str>,
) -> Result<bool, PromptError> {
    let mut prompt = Confirm::new(message).with_default(default);
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }
    map_prompt_result(message, prompt.prompt())
}

fn text_prompt<'a>(
    message: &'static str,
    help: Option<&'static str>,
    default: Option<&'a str>,
) -> Text<'a, 'a> {
    let mut prompt = Text::new(message);
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }
    if let Some(default) = default.filter(|value| !value.trim().is_empty()) {
        prompt = prompt.with_default(default);
    }
    prompt
}

fn parse_csv_list(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn map_prompt_result<T>(
    prompt: &'static str,
    result: Result<T, inquire::error::InquireError>,
) -> Result<T, PromptError> {
    result.map_err(|error| match error {
        inquire::error::InquireError::OperationCanceled
        | inquire::error::InquireError::OperationInterrupted => PromptError::Cancelled,
        other => PromptError::PromptFailed {
            prompt,
            message: other.to_string(),
        },
    })
}
