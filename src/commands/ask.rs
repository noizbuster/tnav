use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

use inquire::{Confirm, Password, Select, Text};

use crate::cli::GlobalArgs;
use crate::commands::executor::execute_command;
use crate::config::{llm_config_path, load_llm_config, save_llm_config};
use crate::errors::TnavError;
use crate::llm::{
    AnthropicClient, ConfiguredProvider, GoogleClient, LlmConfig, LlmError, LlmProvider,
    OllamaClient, OpenAiClient, OpenAiCompatibleClient, Provider, StreamSink,
};
use crate::output::Output;
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};
use crate::ui::{
    ConfirmResult, InquirePromptService, PromptOption, confirm_execute_command, edit_command,
    prompt_command_request, select_provider,
};

const PREVIEW_CLEAR_HEADROOM_LINES: usize = 1;
const CONNECT_ROOT_HELP: &str =
    "Choose a configured provider to connect/edit/delete, or pick an available provider to add.";
const CONNECT_ACTION_HELP: &str = "Choose how to manage the selected provider.";

pub async fn run(global: &GlobalArgs, question: Option<&str>) -> Result<(), TnavError> {
    let output = Output::new(global);
    let provider_config = if question.is_none() && !global.non_interactive {
        ensure_ready_for_interactive_prompt(global, &output).await?
    } else {
        let config = load_llm_config()
            .map_err(map_llm_config_error)?
            .ok_or_else(|| TnavError::ConfigNotFound {
                message: "run 'tnav connect' first".to_owned(),
            })?
            .normalize();
        require_selected_model(&config)?
    };

    let mut prompts = InquirePromptService::new();
    let question = match question {
        Some(question) => question.to_owned(),
        None if global.non_interactive => {
            return Err(TnavError::InvalidInput {
                message:
                    "provide a prompt in non-interactive mode, or use 'tnav connect' / 'tnav model'"
                        .to_owned(),
            });
        }
        None => prompt_command_request(&mut prompts).map_err(map_prompt_error)?,
    };
    let provider = build_provider(&provider_config)?;
    let llm_prompt = build_llm_request_prompt(&question, &gather_runtime_context());
    let mut command = if output.is_json() || global.quiet {
        provider
            .generate_command(&llm_prompt)
            .await
            .map_err(map_llm_error)?
    } else {
        let mut progress = GenerationProgress::new(&output, "Generating shell code");
        let command = provider
            .stream_command(&llm_prompt, &mut progress)
            .await
            .map_err(map_llm_error)?;
        let streamed_lines = progress.finish();
        output.clear_rendered_lines(streamed_lines);
        command
    };
    let mut rendered_preview_lines = 0usize;

    loop {
        if !output.is_json() {
            rendered_preview_lines = output.command_preview(&command);
        }

        match confirm_execute_command(&mut prompts, &command).map_err(map_prompt_error)? {
            ConfirmResult::Execute => return execute_command(&command),
            ConfirmResult::Edit => {
                if !output.is_json() {
                    output.clear_rendered_lines(
                        rendered_preview_lines.saturating_add(PREVIEW_CLEAR_HEADROOM_LINES),
                    );
                    rendered_preview_lines = 0;
                }
                command = edit_command(&mut prompts, &command).map_err(map_prompt_error)?;
            }
            ConfirmResult::Cancel => return Err(TnavError::UserCancelled),
        }
    }
}

struct GenerationProgress<'a> {
    output: &'a Output,
    progress: Option<crate::output::ProgressHandle>,
    saw_chunk: bool,
    ended_with_newline: bool,
    streamed_text: String,
}

impl<'a> GenerationProgress<'a> {
    fn new(output: &'a Output, message: &str) -> Self {
        Self {
            output,
            progress: output.start_progress(message),
            saw_chunk: false,
            ended_with_newline: false,
            streamed_text: String::new(),
        }
    }

    fn finish(&mut self) -> usize {
        if let Some(progress) = self.progress.as_mut() {
            progress.stop();
        }

        if self.saw_chunk && !self.ended_with_newline {
            println!();
        }

        if self.saw_chunk {
            1 + rendered_text_line_count(&self.streamed_text)
        } else {
            0
        }
    }
}

impl StreamSink for GenerationProgress<'_> {
    fn on_chunk(&mut self, chunk: &str) {
        if let Some(progress) = self.progress.as_mut() {
            progress.stop();
        }

        if !self.saw_chunk {
            self.output.line("Streaming response:");
            self.saw_chunk = true;
        }

        print!("{chunk}");
        let _ = io::stdout().flush();
        self.streamed_text.push_str(chunk);
        self.ended_with_newline = chunk.ends_with('\n');
    }
}

fn rendered_text_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.matches('\n').count() + usize::from(!text.ends_with('\n'))
    }
}

pub async fn run_connect(global: &GlobalArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let mut config = load_llm_config()
        .map_err(map_llm_config_error)?
        .unwrap_or_default()
        .normalize();

    render_connect_sections(&output, &config);
    let selection = prompt_connect_selection(&config)?;

    match selection {
        ConnectSelection::Manage(provider) => {
            manage_provider(global, &output, &mut config, provider).await?
        }
        ConnectSelection::Add(provider) => {
            add_provider(global, &output, &mut config, provider).await?
        }
    }

    let path = save_llm_config(&config).map_err(map_llm_config_error)?;
    output.line(format!("Saved LLM provider config to {}", path.display()));
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectSelection {
    Manage(String),
    Add(Provider),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManageProviderAction {
    Connect,
    Edit,
    Delete,
}

struct PreparedProviderConfig {
    config: ConfiguredProvider,
    replacement_api_key: Option<String>,
}

fn render_connect_sections(output: &Output, config: &LlmConfig) {
    output.green_heading("Configured providers");
    if config.providers.is_empty() {
        output.line("  - (none)");
    } else {
        for provider in &config.providers {
            output.line(format!(
                "  - {}",
                configured_provider_label(
                    provider,
                    config.active_provider.as_deref() == Some(provider.name.as_str())
                )
            ));
        }
    }

    output.line("");
    output.yellow_heading("Available providers");
    for provider in available_providers() {
        output.line(format!("  - {}", provider.display_name()));
    }
    output.line("");
}

fn prompt_connect_selection(config: &LlmConfig) -> Result<ConnectSelection, TnavError> {
    let options = connect_menu_options(config);
    let selected = select_option("Connect:", CONNECT_ROOT_HELP, options)?;

    parse_connect_selection(&selected)
}

fn connect_menu_options(config: &LlmConfig) -> Vec<PromptOption> {
    let mut options = Vec::new();

    for provider in &config.providers {
        let label = configured_provider_label(
            provider,
            config.active_provider.as_deref() == Some(provider.name.as_str()),
        );
        options.push(PromptOption::new(
            format!("manage:{}", provider.name),
            format!("Manage {label}"),
        ));
    }

    for provider in available_providers() {
        options.push(PromptOption::new(
            format!("add:{}", provider.value()),
            format!("Add {}", provider.display_name()),
        ));
    }

    options
}

fn parse_connect_selection(value: &str) -> Result<ConnectSelection, TnavError> {
    let (action, provider) = value
        .split_once(':')
        .ok_or_else(|| TnavError::InvalidInput {
            message: format!("unsupported connect action '{value}'"),
        })?;

    match action {
        "manage" => Ok(ConnectSelection::Manage(provider.to_owned())),
        "add" => Ok(ConnectSelection::Add(parse_provider_value(provider)?)),
        _ => Err(TnavError::InvalidInput {
            message: format!("unsupported connect action '{value}'"),
        }),
    }
}

fn parse_provider_value(value: &str) -> Result<Provider, TnavError> {
    match value {
        "ollama" => Ok(Provider::Ollama),
        "openai" => Ok(Provider::OpenAI),
        "openai-compatible" | "lmstudio" => Ok(Provider::OpenAiCompatible),
        "anthropic" => Ok(Provider::Anthropic),
        "google" => Ok(Provider::Google),
        "mistral" => Ok(Provider::Mistral),
        "groq" => Ok(Provider::Groq),
        "deepseek" => Ok(Provider::DeepSeek),
        "xai" => Ok(Provider::XAI),
        "zai" => Ok(Provider::Zai),
        "zai-coding-plan-global" => Ok(Provider::ZaiCodingPlanGlobal),
        "zai-coding-plan-china" => Ok(Provider::ZaiCodingPlanChina),
        _ => Err(TnavError::InvalidInput {
            message: format!("unsupported provider '{value}'"),
        }),
    }
}

fn configured_provider_label(config: &ConfiguredProvider, active: bool) -> String {
    let mut label = format!("{} [{}]", config.name, config.provider.value());
    if active {
        label.push_str(" (Connected)");
    }
    if config.model.trim().is_empty() {
        label.push_str(" [No model]");
    }
    label
}

fn available_providers() -> Vec<Provider> {
    Provider::ALL.into_iter().collect()
}

async fn manage_provider(
    _global: &GlobalArgs,
    output: &Output,
    config: &mut LlmConfig,
    provider_name: String,
) -> Result<(), TnavError> {
    let provider_config = config
        .configured_provider(&provider_name)
        .cloned()
        .ok_or_else(|| TnavError::InvalidInput {
            message: format!("provider '{provider_name}' is not configured"),
        })?;

    let action = prompt_manage_provider_action(
        provider_config.provider,
        config.active_provider.as_deref() == Some(provider_config.name.as_str()),
    )?;

    match action {
        ManageProviderAction::Connect => {
            config.set_active_provider(&provider_config.name);
            output.line(format!(
                "Connected provider set to {}",
                provider_config.name
            ));
        }
        ManageProviderAction::Edit => {
            let was_active =
                config.active_provider.as_deref() == Some(provider_config.name.as_str());
            let existing_names = config
                .providers
                .iter()
                .filter(|item| item.name != provider_config.name)
                .map(|item| item.name.clone())
                .collect::<Vec<_>>();
            let prepared = prompt_provider_configuration(
                provider_config.provider,
                Some(&provider_config),
                &existing_names,
            )?;
            let updated = prepared.config;
            let updated_name = updated.name.clone();
            let renamed = updated.name != provider_config.name;
            if renamed {
                config.remove_provider(&provider_config.name);
            }
            config.upsert_provider(updated);
            if was_active {
                config.set_active_provider(&updated_name);
            }
            if provider_config.provider.uses_api_key_storage()
                && let Some(updated_provider) = config.configured_provider(&updated_name)
            {
                persist_openai_secret_update(
                    &provider_config,
                    updated_provider,
                    prepared.replacement_api_key.as_deref(),
                )?;
            }
            output.line(format!("Updated {} provider settings", updated_name));
        }
        ManageProviderAction::Delete => {
            let confirmed = confirm_action(
                &format!("Delete provider '{}' ?", provider_config.name),
                Some("This removes the saved provider configuration."),
            )?;
            if confirmed {
                config.remove_provider(&provider_config.name);
                if provider_config.provider.uses_api_key_storage() {
                    KeyringSecretStore::new()
                        .delete_secret(&provider_config.secret_profile_key(), SecretKind::ApiKey)
                        .map_err(map_secret_store_error)?;
                }
                output.line(format!("Deleted {} provider", provider_config.name));
            }
        }
    }

    Ok(())
}

fn prompt_manage_provider_action(
    _provider: Provider,
    connected: bool,
) -> Result<ManageProviderAction, TnavError> {
    let mut options = Vec::new();
    if !connected {
        options.push(PromptOption::new("connect", "Connect this provider"));
    }
    options.push(PromptOption::new("edit", "Edit provider settings"));
    options.push(PromptOption::new("delete", "Delete provider"));

    let selected = select_option("Provider action:", CONNECT_ACTION_HELP, options)?;

    match selected.as_str() {
        "connect" => Ok(ManageProviderAction::Connect),
        "edit" => Ok(ManageProviderAction::Edit),
        "delete" => Ok(ManageProviderAction::Delete),
        _ => Err(TnavError::InvalidInput {
            message: format!("unsupported provider action '{selected}'"),
        }),
    }
}

async fn add_provider(
    _global: &GlobalArgs,
    output: &Output,
    config: &mut LlmConfig,
    provider: Provider,
) -> Result<(), TnavError> {
    let existing_names = config
        .providers
        .iter()
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    let prepared = prompt_provider_configuration(provider, None, &existing_names)?;
    let provider_name = prepared.config.name.clone();
    config.upsert_provider(prepared.config.clone());
    config.set_active_provider(&provider_name);
    if provider.uses_api_key_storage() {
        persist_openai_secret_update(
            &prepared.config,
            &prepared.config,
            prepared.replacement_api_key.as_deref(),
        )?;
    }
    output.line(format!("Added and connected {}", provider_name));
    Ok(())
}

fn prompt_provider_configuration(
    provider: Provider,
    existing: Option<&ConfiguredProvider>,
    existing_names: &[String],
) -> Result<PreparedProviderConfig, TnavError> {
    let name = existing
        .map(|config| config.name.clone())
        .unwrap_or_else(|| suggested_provider_name(provider, existing_names));

    let name = prompt_provider_name(provider, &name, existing_names)?;

    let default_base_url = existing
        .and_then(|config| config.base_url.as_deref())
        .unwrap_or_else(|| provider.default_base_url());
    let customized_base_url = prompt_text_value(
        "Base URL:",
        Some("Edit the provider base URL. Leave the default value as-is to use the built-in URL."),
        Some(default_base_url),
    )?;

    let base_url = if customized_base_url.trim() == provider.default_base_url() {
        None
    } else {
        Some(customized_base_url.trim().to_owned())
    };

    let replacement_api_key = match provider {
        Provider::OpenAI => {
            if existing.is_some() {
                prompt_optional_secret_value(
                    "API key (leave blank to keep current):",
                    Some("Enter a new OpenAI API key only if you want to replace the stored one."),
                )?
            } else {
                Some(prompt_required_secret_value(
                    "API key:",
                    Some("Paste the OpenAI API key for this provider."),
                )?)
            }
        }
        Provider::Ollama | Provider::OpenAiCompatible => None,
        Provider::Anthropic
        | Provider::Google
        | Provider::Mistral
        | Provider::Groq
        | Provider::DeepSeek
        | Provider::XAI
        | Provider::Zai
        | Provider::ZaiCodingPlanGlobal
        | Provider::ZaiCodingPlanChina => {
            if existing.is_some() {
                prompt_optional_secret_value(
                    "API key (leave blank to keep current):",
                    Some("Enter a new API key only if you want to replace the stored one."),
                )?
            } else {
                Some(prompt_required_secret_value(
                    "API key:",
                    Some("Paste the API key for this provider."),
                )?)
            }
        }
    };

    Ok(PreparedProviderConfig {
        config: ConfiguredProvider {
            name,
            provider,
            model: existing
                .map(|config| config.model.clone())
                .unwrap_or_default(),
            base_url,
            api_key: existing.and_then(|config| config.api_key.clone()),
            timeout_secs: existing.map(|config| config.timeout_secs).unwrap_or(30),
        },
        replacement_api_key,
    })
}

fn persist_openai_secret_update(
    old_provider: &ConfiguredProvider,
    new_provider: &ConfiguredProvider,
    replacement_api_key: Option<&str>,
) -> Result<(), TnavError> {
    let store = KeyringSecretStore::new();

    if let Some(api_key) = replacement_api_key {
        store
            .save_secret(
                &new_provider.secret_profile_key(),
                SecretKind::ApiKey,
                api_key,
            )
            .map_err(map_secret_store_error)?;
        if old_provider.name != new_provider.name {
            store
                .delete_secret(&old_provider.secret_profile_key(), SecretKind::ApiKey)
                .map_err(map_secret_store_error)?;
        }
        return Ok(());
    }

    if let Some(secret) = store
        .load_secret(&old_provider.secret_profile_key(), SecretKind::ApiKey)
        .map_err(map_secret_store_error)?
    {
        store
            .save_secret(
                &new_provider.secret_profile_key(),
                SecretKind::ApiKey,
                &secret,
            )
            .map_err(map_secret_store_error)?;
        store
            .delete_secret(&old_provider.secret_profile_key(), SecretKind::ApiKey)
            .map_err(map_secret_store_error)?;
    }

    Ok(())
}

fn suggested_provider_name(provider: Provider, existing_names: &[String]) -> String {
    let base = provider.value();
    if !existing_names.iter().any(|name| name == base) {
        return base.to_owned();
    }

    let mut index = 2usize;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing_names.iter().any(|name| name == &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn prompt_provider_name(
    provider: Provider,
    default_name: &str,
    existing_names: &[String],
) -> Result<String, TnavError> {
    let value = prompt_text_value(
        "Connection name:",
        Some("Use a unique name to distinguish this provider instance."),
        Some(default_name),
    )?;

    validate_provider_name(provider, &value, existing_names)
}

fn validate_provider_name(
    _provider: Provider,
    value: &str,
    existing_names: &[String],
) -> Result<String, TnavError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(TnavError::InvalidInput {
            message: "provider name cannot be empty".to_owned(),
        });
    }
    if normalized.contains('/') || normalized.contains(':') {
        return Err(TnavError::InvalidInput {
            message: "provider name cannot contain '/' or ':'".to_owned(),
        });
    }
    if existing_names.iter().any(|name| name == normalized) {
        return Err(TnavError::InvalidInput {
            message: format!("provider name '{normalized}' already exists"),
        });
    }

    Ok(normalized.to_owned())
}

fn select_option(
    message: &'static str,
    help: &'static str,
    options: Vec<PromptOption>,
) -> Result<String, TnavError> {
    if options.is_empty() {
        return Err(TnavError::InvalidInput {
            message: "at least one option is required".to_owned(),
        });
    }

    let prompt = Select::new(message, options)
        .with_help_message(help)
        .without_filtering()
        .with_page_size(10);

    prompt
        .prompt()
        .map(|choice| choice.value().to_owned())
        .map_err(|error| map_inquire_error(message, error))
}

fn prompt_text_value(
    message: &'static str,
    help: Option<&'static str>,
    default: Option<&str>,
) -> Result<String, TnavError> {
    let mut prompt = Text::new(message);
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }
    if let Some(default) = default.filter(|value| !value.trim().is_empty()) {
        prompt = prompt.with_default(default);
    }

    prompt
        .prompt()
        .map(|value| value.trim().to_owned())
        .map_err(|error| map_inquire_error(message, error))
}

fn prompt_required_secret_value(
    message: &'static str,
    help: Option<&'static str>,
) -> Result<String, TnavError> {
    let mut prompt = Password::new(message).without_confirmation();
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }

    let value = prompt
        .prompt()
        .map(|value| value.trim().to_owned())
        .map_err(|error| map_inquire_error(message, error))?;

    if value.is_empty() {
        Err(TnavError::InvalidInput {
            message: "API key cannot be empty".to_owned(),
        })
    } else {
        Ok(value)
    }
}

fn prompt_optional_secret_value(
    message: &'static str,
    help: Option<&'static str>,
) -> Result<Option<String>, TnavError> {
    let mut prompt = Password::new(message).without_confirmation();
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }

    let value = prompt
        .prompt()
        .map(|value| value.trim().to_owned())
        .map_err(|error| map_inquire_error(message, error))?;

    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn confirm_action(message: &str, help: Option<&'static str>) -> Result<bool, TnavError> {
    let mut prompt = Confirm::new(message).with_default(false);
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }

    prompt
        .prompt()
        .map_err(|error| map_inquire_error("confirmation", error))
}

fn map_inquire_error(prompt: &'static str, error: inquire::error::InquireError) -> TnavError {
    match error {
        inquire::error::InquireError::OperationCanceled
        | inquire::error::InquireError::OperationInterrupted => TnavError::UserCancelled,
        other => TnavError::InvalidInput {
            message: format!("prompt '{prompt}' failed: {other}"),
        },
    }
}

pub async fn run_model(
    global: &GlobalArgs,
    requested_model: Option<&str>,
) -> Result<(), TnavError> {
    run_model_selection(global, requested_model).await
}

async fn run_model_selection(
    global: &GlobalArgs,
    requested_model: Option<&str>,
) -> Result<(), TnavError> {
    let output = Output::new(global);
    let mut prompts = InquirePromptService::new();
    let mut config = load_llm_config()
        .map_err(map_llm_config_error)?
        .ok_or_else(|| TnavError::ConfigNotFound {
            message: "run 'tnav connect' first".to_owned(),
        })?
        .normalize();

    let active_provider =
        config
            .active_provider_config()
            .cloned()
            .ok_or_else(|| TnavError::ConfigNotFound {
                message: "run 'tnav connect' first".to_owned(),
            })?;

    let model = match requested_model {
        None | Some("") => {
            let provider = build_provider(&active_provider)?;
            let models = provider.list_models().await.map_err(map_llm_error)?;
            if models.is_empty() {
                return Err(TnavError::CommandFailed {
                    message: "the configured provider did not return any models".to_owned(),
                });
            }
            let options = models
                .into_iter()
                .map(PromptOption::simple)
                .collect::<Vec<_>>();
            select_provider(&mut prompts, &options).map_err(map_prompt_error)?
        }
        Some(model) => model.to_owned(),
    };

    if let Some(provider) = config.active_provider_config_mut() {
        provider.model = model;
    }
    let path = save_llm_config(&config).map_err(map_llm_config_error)?;
    output.line(format!("Saved selected model to {}", path.display()));
    Ok(())
}

async fn ensure_ready_for_interactive_prompt(
    global: &GlobalArgs,
    output: &Output,
) -> Result<ConfiguredProvider, TnavError> {
    let mut config = load_llm_config()
        .map_err(map_llm_config_error)?
        .map(LlmConfig::normalize);

    let mut actions = interactive_setup_actions(config.as_ref());

    if actions.needs_connect {
        output.red_box("No LLM provider is configured yet.");
        output.line("Starting `tnav connect` setup...");
        run_connect(global).await?;
        config = load_llm_config()
            .map_err(map_llm_config_error)?
            .map(LlmConfig::normalize);
        actions = interactive_setup_actions(config.as_ref());
    }

    let config = config.ok_or_else(|| TnavError::ConfigNotFound {
        message: "run 'tnav connect' first".to_owned(),
    })?;

    if actions.needs_model {
        output.red_box("No LLM model is selected yet.");
        output.line("Starting `tnav model` setup...");
        run_model_selection(global, Some("")).await?;
        let config = load_llm_config()
            .map_err(map_llm_config_error)?
            .ok_or_else(|| TnavError::ConfigNotFound {
                message: "run 'tnav connect' first".to_owned(),
            })?
            .normalize();
        return require_selected_model(&config);
    }

    require_selected_model(&config)
}

fn interactive_setup_actions(config: Option<&LlmConfig>) -> InteractiveSetupActions {
    match config {
        None => InteractiveSetupActions {
            needs_connect: true,
            needs_model: true,
        },
        Some(config) => InteractiveSetupActions {
            needs_connect: config.providers.is_empty(),
            needs_model: config
                .active_provider_config()
                .map(|provider| provider.model.trim().is_empty())
                .unwrap_or(true),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InteractiveSetupActions {
    needs_connect: bool,
    needs_model: bool,
}

fn require_selected_model(config: &LlmConfig) -> Result<ConfiguredProvider, TnavError> {
    let provider =
        config
            .active_provider_config()
            .cloned()
            .ok_or_else(|| TnavError::ConfigInvalid {
                message: "run 'tnav connect' first".to_owned(),
            })?;

    if provider.model.trim().is_empty() {
        return Err(TnavError::ConfigInvalid {
            message: "run 'tnav model' first".to_owned(),
        });
    }

    Ok(provider)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeContext {
    shell_name: String,
    shell_path: Option<String>,
    shell_version: Option<String>,
    os_kind: String,
    os_version: Option<String>,
    architecture: String,
}

fn build_llm_request_prompt(question: &str, context: &RuntimeContext) -> String {
    let mut details = vec![
        format!("- Shell: {}", context.shell_name),
        format!("- Architecture: {}", context.architecture),
        format!("- OS: {}", context.os_kind),
    ];

    if let Some(shell_path) = context.shell_path.as_deref() {
        details.push(format!("- Shell path: {shell_path}"));
    }

    if let Some(shell_version) = context.shell_version.as_deref() {
        details.push(format!("- Shell version: {shell_version}"));
    }

    if let Some(os_version) = context.os_version.as_deref() {
        details.push(format!("- OS version: {os_version}"));
    }

    format!(
        "User request:\n{question}\n\nCurrent environment:\n{}\n\nGenerate shell code that fits this shell and OS.",
        details.join("\n")
    )
}

fn gather_runtime_context() -> RuntimeContext {
    let shell_path = env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("COMSPEC")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });

    let shell_name = shell_path
        .as_deref()
        .and_then(shell_name_from_path)
        .unwrap_or_else(|| "unknown".to_owned());

    let shell_version = shell_path
        .as_deref()
        .and_then(read_shell_version)
        .or_else(|| shell_version_from_name(&shell_name));

    let (os_kind, os_version) = current_os_details();

    RuntimeContext {
        shell_name,
        shell_path,
        shell_version,
        os_kind,
        os_version,
        architecture: env::consts::ARCH.to_owned(),
    }
}

fn shell_name_from_path(shell_path: &str) -> Option<String> {
    Path::new(shell_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

fn read_shell_version(shell_path: &str) -> Option<String> {
    for args in [["--version"], ["-version"], ["-v"]] {
        if let Some(version) = command_first_line(shell_path, &args) {
            return Some(version);
        }
    }

    None
}

fn shell_version_from_name(shell_name: &str) -> Option<String> {
    if shell_name == "unknown" {
        return None;
    }

    read_shell_version(shell_name)
}

fn command_first_line(executable: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(executable).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn current_os_details() -> (String, Option<String>) {
    match env::consts::OS {
        "linux" => ("linux".to_owned(), linux_os_version()),
        "macos" => (
            "macOS".to_owned(),
            command_first_line("sw_vers", &["-productVersion"]),
        ),
        "windows" => (
            "windows".to_owned(),
            command_first_line("cmd", &["/C", "ver"]),
        ),
        other => (other.to_owned(), None),
    }
}

fn linux_os_version() -> Option<String> {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| parse_linux_os_release(&contents))
        .or_else(|| command_first_line("uname", &["-sr"]))
}

fn parse_linux_os_release(contents: &str) -> Option<String> {
    let mut pretty_name = None;
    let mut name = None;
    let mut version = None;

    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            pretty_name = Some(unquote_os_release_value(value));
        } else if let Some(value) = line.strip_prefix("NAME=") {
            name = Some(unquote_os_release_value(value));
        } else if let Some(value) = line.strip_prefix("VERSION=") {
            version = Some(unquote_os_release_value(value));
        }
    }

    pretty_name.or_else(|| match (name, version) {
        (Some(name), Some(version)) if !version.is_empty() => Some(format!("{name} {version}")),
        (Some(name), _) => Some(name),
        _ => None,
    })
}

fn unquote_os_release_value(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}

fn build_provider(config: &ConfiguredProvider) -> Result<Box<dyn LlmProvider>, TnavError> {
    match config.provider {
        Provider::Ollama => Ok(Box::new(
            OllamaClient::new(config.clone()).map_err(map_llm_error)?,
        )),
        Provider::OpenAiCompatible => Ok(Box::new(
            OpenAiCompatibleClient::new(config.clone()).map_err(map_llm_error)?,
        )),
        Provider::OpenAI => Ok(Box::new(
            OpenAiClient::new(config.clone()).map_err(map_llm_error)?,
        )),
        Provider::Anthropic => Ok(Box::new(
            AnthropicClient::new(config.clone()).map_err(map_llm_error)?,
        )),
        Provider::Google => Ok(Box::new(
            GoogleClient::new(config.clone()).map_err(map_llm_error)?,
        )),
        Provider::Mistral
        | Provider::Groq
        | Provider::DeepSeek
        | Provider::XAI
        | Provider::Zai
        | Provider::ZaiCodingPlanGlobal
        | Provider::ZaiCodingPlanChina => Ok(Box::new(
            OpenAiCompatibleClient::new(config.clone()).map_err(map_llm_error)?,
        )),
    }
}

fn map_llm_error(error: LlmError) -> TnavError {
    match error {
        LlmError::ConfigMissing { message } => TnavError::ConfigInvalid { message },
        LlmError::AuthFailed { message } => TnavError::SecretStoreUnavailable { message },
        LlmError::ConnectionFailed { message } => TnavError::NetworkError { message },
        LlmError::Timeout => TnavError::CommandFailed {
            message: "LLM request timed out".to_owned(),
        },
        LlmError::InvalidResponse { message } => TnavError::CommandFailed { message },
        LlmError::RateLimited => TnavError::CommandFailed {
            message: "LLM provider is currently rate limited".to_owned(),
        },
        LlmError::ModelNotFound { model } => TnavError::CommandFailed {
            message: format!("LLM model '{model}' was not found"),
        },
    }
}

fn map_llm_config_error(error: crate::config::ConfigError) -> TnavError {
    match error {
        crate::config::ConfigError::NotFound { .. } => TnavError::ConfigNotFound {
            message: llm_config_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "llm.toml".to_owned()),
        },
        other => TnavError::ConfigInvalid {
            message: other.to_string(),
        },
    }
}

fn map_prompt_error(error: crate::ui::PromptError) -> TnavError {
    match error {
        crate::ui::PromptError::Cancelled => TnavError::UserCancelled,
        crate::ui::PromptError::PromptFailed { message, .. } => TnavError::InvalidInput { message },
    }
}

fn map_secret_store_error(error: crate::secrets::SecretStoreError) -> TnavError {
    match error {
        crate::secrets::SecretStoreError::Unavailable { message } => {
            TnavError::SecretStoreUnavailable { message }
        }
        other => TnavError::SecretStoreWriteFailed {
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::llm::{ConfiguredProvider, LlmConfig, Provider};

    use super::{
        InteractiveSetupActions, RuntimeContext, build_llm_request_prompt, connect_menu_options,
        interactive_setup_actions, parse_linux_os_release, require_selected_model,
        suggested_provider_name, validate_provider_name,
    };

    fn provider_config(name: &str, provider: Provider, model: &str) -> ConfiguredProvider {
        ConfiguredProvider {
            name: name.to_owned(),
            provider,
            model: model.to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: 30,
        }
    }

    fn llm_config(active_provider: Option<&str>, providers: Vec<ConfiguredProvider>) -> LlmConfig {
        LlmConfig {
            active_provider: active_provider.map(str::to_owned),
            providers,
        }
    }

    #[test]
    fn missing_config_requires_connect_and_model() {
        let actions = interactive_setup_actions(None);

        assert_eq!(
            actions,
            InteractiveSetupActions {
                needs_connect: true,
                needs_model: true,
            }
        );
    }

    #[test]
    fn empty_model_requires_only_model_selection() {
        let config = llm_config(
            Some("ollama"),
            vec![provider_config("ollama", Provider::Ollama, "")],
        );

        let actions = interactive_setup_actions(Some(&config));

        assert_eq!(
            actions,
            InteractiveSetupActions {
                needs_connect: false,
                needs_model: true,
            }
        );
    }

    #[test]
    fn complete_config_skips_guided_setup() {
        let config = llm_config(
            Some("ollama"),
            vec![provider_config("ollama", Provider::Ollama, "llama3.2")],
        );

        let actions = interactive_setup_actions(Some(&config));

        assert_eq!(
            actions,
            InteractiveSetupActions {
                needs_connect: false,
                needs_model: false,
            }
        );
    }

    #[test]
    fn provider_change_requires_model_selection_again() {
        let config = llm_config(
            Some("openai-compatible"),
            vec![provider_config(
                "openai-compatible",
                Provider::OpenAiCompatible,
                "",
            )],
        );

        let actions = interactive_setup_actions(Some(&config));

        assert_eq!(
            actions,
            InteractiveSetupActions {
                needs_connect: false,
                needs_model: true,
            }
        );
    }

    #[test]
    fn empty_model_requires_model_selection() {
        let config = llm_config(
            Some("ollama"),
            vec![provider_config("ollama", Provider::Ollama, "")],
        );

        let error = require_selected_model(&config).expect_err("model should be required");
        assert_eq!(
            error.to_string(),
            "configuration is invalid: run 'tnav model' first"
        );
    }

    #[test]
    fn non_empty_model_passes_readiness_check() {
        let config = llm_config(
            Some("ollama"),
            vec![provider_config("ollama", Provider::Ollama, "llama3.2")],
        );

        let ready = require_selected_model(&config).expect("model should be accepted");
        assert_eq!(
            ready,
            provider_config("ollama", Provider::Ollama, "llama3.2")
        );
    }

    #[test]
    fn build_llm_request_prompt_includes_runtime_details() {
        let context = RuntimeContext {
            shell_name: "bash".to_owned(),
            shell_path: Some("/bin/bash".to_owned()),
            shell_version: Some("GNU bash, version 5.2.21".to_owned()),
            os_kind: "linux".to_owned(),
            os_version: Some("Ubuntu 24.04.1 LTS".to_owned()),
            architecture: "x86_64".to_owned(),
        };

        let prompt = build_llm_request_prompt("show current directory", &context);

        assert!(prompt.contains("User request:\nshow current directory"));
        assert!(prompt.contains("- Shell: bash"));
        assert!(prompt.contains("- Shell path: /bin/bash"));
        assert!(prompt.contains("- Shell version: GNU bash, version 5.2.21"));
        assert!(prompt.contains("- OS: linux"));
        assert!(prompt.contains("- OS version: Ubuntu 24.04.1 LTS"));
        assert!(prompt.contains("Generate shell code that fits this shell and OS."));
    }

    #[test]
    fn parse_linux_os_release_prefers_pretty_name() {
        let version = parse_linux_os_release(
            "NAME=Ubuntu\nVERSION=24.04.1 LTS\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n",
        );

        assert_eq!(version.as_deref(), Some("Ubuntu 24.04.1 LTS"));
    }

    #[test]
    fn parse_linux_os_release_falls_back_to_name_and_version() {
        let version =
            parse_linux_os_release("NAME=Fedora Linux\nVERSION=40 (Workstation Edition)\n");

        assert_eq!(
            version.as_deref(),
            Some("Fedora Linux 40 (Workstation Edition)")
        );
    }

    #[test]
    fn current_provider_is_labeled_connected_in_connect_menu() {
        let options = connect_menu_options(&llm_config(
            Some("compat-a"),
            vec![provider_config("compat-a", Provider::OpenAiCompatible, "")],
        ));

        assert_eq!(options[0].value(), "manage:compat-a");
        assert_eq!(
            options[0].label(),
            "Manage compat-a [openai-compatible] (Connected) [No model]"
        );
        assert_eq!(options[1].value(), "add:ollama");
        assert_eq!(options[2].value(), "add:openai");
    }

    #[test]
    fn connect_menu_without_config_lists_all_available_providers() {
        let options = connect_menu_options(&LlmConfig::default());

        assert_eq!(options[0].value(), "add:ollama");
        assert_eq!(options[1].value(), "add:openai");
        assert_eq!(options[2].value(), "add:openai-compatible");
    }

    #[test]
    fn connect_menu_keeps_multiple_instances_of_same_provider_type() {
        let options = connect_menu_options(&llm_config(
            Some("compat-a"),
            vec![
                provider_config("compat-a", Provider::OpenAiCompatible, "model-a"),
                provider_config("compat-b", Provider::OpenAiCompatible, "model-b"),
            ],
        ));

        assert_eq!(options[0].value(), "manage:compat-a");
        assert_eq!(options[1].value(), "manage:compat-b");
        assert_eq!(options[2].value(), "add:ollama");
        assert_eq!(options[3].value(), "add:openai");
        assert_eq!(options[4].value(), "add:openai-compatible");
    }

    #[test]
    fn suggested_provider_name_increments_same_provider_type_instances() {
        let suggested = suggested_provider_name(
            Provider::OpenAiCompatible,
            &[
                "openai-compatible".to_owned(),
                "openai-compatible-2".to_owned(),
            ],
        );

        assert_eq!(suggested, "openai-compatible-3");
    }

    #[test]
    fn validate_provider_name_rejects_duplicate_instance_name() {
        let error = validate_provider_name(
            Provider::OpenAiCompatible,
            "compat-a",
            &["compat-a".to_owned()],
        )
        .expect_err("duplicate name should fail");

        assert!(error.to_string().contains("already exists"));
    }
}
