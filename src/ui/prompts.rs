use std::collections::{HashSet, VecDeque};
use std::env;
use std::fmt::{self, Display};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command as ProcessCommand;

use inquire::error::InquireError;
use inquire::validator::Validation;
use inquire::{Confirm, MultiSelect, Password, PasswordDisplayMode, Select, Text};
use tempfile::Builder;
use thiserror::Error;

pub type PromptResult<T> = Result<T, PromptError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmResult {
    Execute,
    Edit,
    Cancel,
}

impl Display for ConfirmResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execute => f.write_str("Execute"),
            Self::Edit => f.write_str("Edit"),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PromptError {
    #[error("prompt cancelled by user")]
    Cancelled,
    #[error("prompt '{prompt}' failed: {message}")]
    PromptFailed {
        prompt: &'static str,
        message: String,
    },
}

impl PromptError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptOption {
    value: String,
    label: String,
}

impl PromptOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    pub fn simple(value: impl Into<String>) -> Self {
        let value = value.into();
        Self::new(value.clone(), value)
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Display for PromptOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub trait PromptService {
    fn prompt_profile_name(&mut self, default: Option<&str>) -> PromptResult<String>;
    fn prompt_api_key(&mut self) -> PromptResult<String>;
    fn prompt_command_request(&mut self, message: &str) -> PromptResult<String>;
    fn select_provider(&mut self, providers: &[PromptOption]) -> PromptResult<String>;
    fn multiselect_scopes(
        &mut self,
        scopes: &[PromptOption],
        defaults: &[String],
    ) -> PromptResult<Vec<String>>;
    fn confirm_overwrite(&mut self, target: &str) -> PromptResult<bool>;
    fn confirm_execute_command(&mut self, command: &str) -> PromptResult<ConfirmResult>;
    fn edit_command(&mut self, command: &str) -> PromptResult<String>;
}

pub fn prompt_profile_name(
    prompts: &mut impl PromptService,
    default: Option<&str>,
) -> PromptResult<String> {
    prompts.prompt_profile_name(default)
}

pub fn prompt_api_key(prompts: &mut impl PromptService) -> PromptResult<String> {
    prompts.prompt_api_key()
}

pub fn prompt_command_request(
    prompts: &mut impl PromptService,
    message: &str,
) -> PromptResult<String> {
    prompts.prompt_command_request(message)
}

pub fn select_provider(
    prompts: &mut impl PromptService,
    providers: &[PromptOption],
) -> PromptResult<String> {
    prompts.select_provider(providers)
}

pub fn multiselect_scopes(
    prompts: &mut impl PromptService,
    scopes: &[PromptOption],
    defaults: &[String],
) -> PromptResult<Vec<String>> {
    prompts.multiselect_scopes(scopes, defaults)
}

pub fn confirm_overwrite(prompts: &mut impl PromptService, target: &str) -> PromptResult<bool> {
    prompts.confirm_overwrite(target)
}

pub fn confirm_execute_command(
    prompts: &mut impl PromptService,
    command: &str,
) -> PromptResult<ConfirmResult> {
    prompts.confirm_execute_command(command)
}

pub fn edit_command(prompts: &mut impl PromptService, command: &str) -> PromptResult<String> {
    prompts.edit_command(command)
}

#[derive(Debug, Default, Clone)]
pub struct InquirePromptService;

impl InquirePromptService {
    pub fn new() -> Self {
        Self
    }
}

impl PromptService for InquirePromptService {
    fn prompt_profile_name(&mut self, default: Option<&str>) -> PromptResult<String> {
        let mut prompt = Text::new("Profile name:")
            .with_help_message("Enter a short name to identify this saved profile.")
            .with_validator(profile_name_validator);

        if let Some(default) = default.filter(|value| !value.trim().is_empty()) {
            prompt = prompt.with_default(default.trim());
        }

        map_prompt_result(
            "profile name",
            prompt.prompt().map(|value| normalize_profile_name(&value)),
        )
    }

    fn prompt_api_key(&mut self) -> PromptResult<String> {
        let prompt = build_api_key_prompt(
            "API key:",
            "Paste the API key exactly as issued. Input is masked; press Ctrl+R to reveal temporarily.",
        )
        .with_validator(api_key_validator);

        map_prompt_result(
            "api key",
            prompt.prompt().map(|value| normalize_secret_value(&value)),
        )
    }

    fn prompt_command_request(&mut self, message: &str) -> PromptResult<String> {
        let prompt = Text::new(message)
            .with_help_message("Describe what shell command you want tnav to generate.")
            .with_validator(command_request_validator);

        map_prompt_result(
            "prompt request",
            prompt
                .prompt()
                .map(|value| normalize_command_request(&value)),
        )
    }

    fn select_provider(&mut self, providers: &[PromptOption]) -> PromptResult<String> {
        ensure_non_empty("provider selection", providers)?;

        let prompt = Select::new("Provider:", providers.to_vec())
            .with_help_message("Choose the provider to use for this profile.")
            .without_filtering()
            .with_page_size(page_size(providers.len()));

        map_prompt_result(
            "provider selection",
            prompt.prompt().map(|choice| choice.value),
        )
    }

    fn multiselect_scopes(
        &mut self,
        scopes: &[PromptOption],
        defaults: &[String],
    ) -> PromptResult<Vec<String>> {
        ensure_non_empty("scope selection", scopes)?;

        let default_set: HashSet<&str> = defaults.iter().map(String::as_str).collect();
        let default_indexes = scopes
            .iter()
            .enumerate()
            .filter_map(|(index, scope)| default_set.contains(scope.value()).then_some(index))
            .collect::<Vec<_>>();

        let prompt = MultiSelect::new("Scopes:", scopes.to_vec())
            .with_help_message("Use space to toggle scopes, then press enter to continue.")
            .with_page_size(page_size(scopes.len()));

        let prompt = if default_indexes.is_empty() {
            prompt
        } else {
            prompt.with_default(&default_indexes)
        };

        map_prompt_result(
            "scope selection",
            prompt
                .prompt()
                .map(|selected| selected.into_iter().map(|scope| scope.value).collect()),
        )
    }

    fn confirm_overwrite(&mut self, target: &str) -> PromptResult<bool> {
        let message = format!("Overwrite existing {target}?");

        let prompt = Confirm::new(&message)
            .with_default(false)
            .with_help_message("This replaces the current saved value.");

        map_prompt_result("overwrite confirmation", prompt.prompt())
    }

    fn confirm_execute_command(&mut self, _command: &str) -> PromptResult<ConfirmResult> {
        let options = vec![
            ConfirmResult::Execute,
            ConfirmResult::Edit,
            ConfirmResult::Cancel,
        ];
        let prompt = Select::new("Action:", options)
            .with_help_message("Use the generated shell code shown above.")
            .without_filtering();

        map_prompt_result("command confirmation", prompt.prompt())
    }

    fn edit_command(&mut self, command: &str) -> PromptResult<String> {
        edit_command_in_editor(command)
    }
}

fn build_api_key_prompt<'a>(message: &'a str, help_message: &'a str) -> Password<'a> {
    Password::new(message)
        .without_confirmation()
        .with_display_mode(PasswordDisplayMode::Masked)
        .with_display_toggle_enabled()
        .with_help_message(help_message)
}

#[derive(Debug, Default, Clone)]
pub struct ScriptedPromptService {
    profile_names: VecDeque<PromptResult<String>>,
    api_keys: VecDeque<PromptResult<String>>,
    command_requests: VecDeque<PromptResult<String>>,
    providers: VecDeque<PromptResult<String>>,
    scope_sets: VecDeque<PromptResult<Vec<String>>>,
    overwrite_decisions: VecDeque<PromptResult<bool>>,
    command_confirmations: VecDeque<PromptResult<ConfirmResult>>,
    edited_commands: VecDeque<PromptResult<String>>,
}

impl ScriptedPromptService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_profile_name(&mut self, value: PromptResult<String>) -> &mut Self {
        self.profile_names.push_back(value);
        self
    }

    pub fn push_api_key(&mut self, value: PromptResult<String>) -> &mut Self {
        self.api_keys.push_back(value);
        self
    }

    pub fn push_command_request(&mut self, value: PromptResult<String>) -> &mut Self {
        self.command_requests.push_back(value);
        self
    }

    pub fn push_provider(&mut self, value: PromptResult<String>) -> &mut Self {
        self.providers.push_back(value);
        self
    }

    pub fn push_scopes(&mut self, value: PromptResult<Vec<String>>) -> &mut Self {
        self.scope_sets.push_back(value);
        self
    }

    pub fn push_confirm_overwrite(&mut self, value: PromptResult<bool>) -> &mut Self {
        self.overwrite_decisions.push_back(value);
        self
    }

    pub fn push_command_confirmation(&mut self, value: PromptResult<ConfirmResult>) -> &mut Self {
        self.command_confirmations.push_back(value);
        self
    }

    pub fn push_edited_command(&mut self, value: PromptResult<String>) -> &mut Self {
        self.edited_commands.push_back(value);
        self
    }

    fn next<T>(queue: &mut VecDeque<PromptResult<T>>, prompt: &'static str) -> PromptResult<T> {
        queue.pop_front().unwrap_or_else(|| {
            Err(PromptError::PromptFailed {
                prompt,
                message: "no scripted response configured".to_owned(),
            })
        })
    }
}

impl PromptService for ScriptedPromptService {
    fn prompt_profile_name(&mut self, _default: Option<&str>) -> PromptResult<String> {
        let value = Self::next(&mut self.profile_names, "profile name")?;
        validate_profile_name(&value)
    }

    fn prompt_api_key(&mut self) -> PromptResult<String> {
        let value = Self::next(&mut self.api_keys, "api key")?;
        validate_api_key(&value)
    }

    fn prompt_command_request(&mut self, _message: &str) -> PromptResult<String> {
        let value = Self::next(&mut self.command_requests, "prompt request")?;
        validate_command_request(&value)
    }

    fn select_provider(&mut self, providers: &[PromptOption]) -> PromptResult<String> {
        ensure_non_empty("provider selection", providers)?;

        let value = Self::next(&mut self.providers, "provider selection")?;
        if providers.iter().any(|provider| provider.value() == value) {
            Ok(value)
        } else {
            Err(PromptError::PromptFailed {
                prompt: "provider selection",
                message: format!("unknown scripted provider '{value}'"),
            })
        }
    }

    fn multiselect_scopes(
        &mut self,
        scopes: &[PromptOption],
        _defaults: &[String],
    ) -> PromptResult<Vec<String>> {
        ensure_non_empty("scope selection", scopes)?;

        let selected = Self::next(&mut self.scope_sets, "scope selection")?;
        let allowed = scopes
            .iter()
            .map(|scope| scope.value())
            .collect::<HashSet<_>>();
        let mut normalized = Vec::with_capacity(selected.len());

        for scope in selected {
            let scope = normalize_profile_name(&scope);
            if !allowed.contains(scope.as_str()) {
                return Err(PromptError::PromptFailed {
                    prompt: "scope selection",
                    message: format!("unknown scripted scope '{scope}'"),
                });
            }

            normalized.push(scope);
        }

        Ok(normalized)
    }

    fn confirm_overwrite(&mut self, _target: &str) -> PromptResult<bool> {
        Self::next(&mut self.overwrite_decisions, "overwrite confirmation")
    }

    fn confirm_execute_command(&mut self, _command: &str) -> PromptResult<ConfirmResult> {
        Self::next(&mut self.command_confirmations, "command confirmation")
    }

    fn edit_command(&mut self, _command: &str) -> PromptResult<String> {
        Self::next(&mut self.edited_commands, "edit command")
    }
}

fn ensure_non_empty<T>(prompt: &'static str, options: &[T]) -> PromptResult<()> {
    if options.is_empty() {
        return Err(PromptError::PromptFailed {
            prompt,
            message: "at least one option is required".to_owned(),
        });
    }

    Ok(())
}

fn page_size(len: usize) -> usize {
    len.clamp(1, 10)
}

fn map_prompt_result<T>(prompt: &'static str, result: Result<T, InquireError>) -> PromptResult<T> {
    result.map_err(|error| match error {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => {
            PromptError::Cancelled
        }
        other => PromptError::PromptFailed {
            prompt,
            message: other.to_string(),
        },
    })
}

fn profile_name_validator(input: &str) -> Result<Validation, inquire::CustomUserError> {
    if normalize_profile_name(input).is_empty() {
        return Ok(Validation::Invalid("Profile name cannot be empty.".into()));
    }

    Ok(Validation::Valid)
}

fn api_key_validator(input: &str) -> Result<Validation, inquire::CustomUserError> {
    if normalize_secret_value(input).is_empty() {
        return Ok(Validation::Invalid("API key cannot be empty.".into()));
    }

    Ok(Validation::Valid)
}

fn command_request_validator(input: &str) -> Result<Validation, inquire::CustomUserError> {
    if normalize_command_request(input).is_empty() {
        return Ok(Validation::Invalid("Prompt cannot be empty.".into()));
    }

    Ok(Validation::Valid)
}

fn validate_profile_name(input: &str) -> PromptResult<String> {
    let normalized = normalize_profile_name(input);
    if normalized.is_empty() {
        return Err(PromptError::PromptFailed {
            prompt: "profile name",
            message: "profile name cannot be empty".to_owned(),
        });
    }

    Ok(normalized)
}

fn validate_api_key(input: &str) -> PromptResult<String> {
    let normalized = normalize_secret_value(input);
    if normalized.is_empty() {
        return Err(PromptError::PromptFailed {
            prompt: "api key",
            message: "api key cannot be empty".to_owned(),
        });
    }

    Ok(normalized)
}

fn validate_command_request(input: &str) -> PromptResult<String> {
    let normalized = normalize_command_request(input);
    if normalized.is_empty() {
        return Err(PromptError::PromptFailed {
            prompt: "prompt request",
            message: "prompt cannot be empty".to_owned(),
        });
    }

    Ok(normalized)
}

fn edit_command_in_editor(command: &str) -> PromptResult<String> {
    let mut file = Builder::new()
        .prefix("tnav-edit-")
        .suffix(".sh")
        .tempfile()
        .map_err(|error| PromptError::PromptFailed {
            prompt: "edit command",
            message: format!("failed to create temporary file: {error}"),
        })?;

    file.write_all(command.as_bytes())
        .and_then(|_| file.flush())
        .map_err(|error| PromptError::PromptFailed {
            prompt: "edit command",
            message: format!("failed to seed temporary file: {error}"),
        })?;

    let path = file.path().to_path_buf();
    open_editor(&path)?;

    let edited = fs::read_to_string(&path).map_err(|error| PromptError::PromptFailed {
        prompt: "edit command",
        message: format!("failed to read edited command: {error}"),
    })?;

    validate_edited_command(&edited)
}

fn open_editor(path: &Path) -> PromptResult<()> {
    let editor = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(default_editor_command);

    let status = if cfg!(windows) {
        ProcessCommand::new("cmd")
            .arg("/C")
            .arg(format!("{editor} \"{}\"", path.display()))
            .status()
    } else {
        ProcessCommand::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$1\""))
            .arg("tnav-editor")
            .arg(path)
            .status()
    }
    .map_err(|error| PromptError::PromptFailed {
        prompt: "edit command",
        message: format!("failed to start editor: {error}"),
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(PromptError::PromptFailed {
            prompt: "edit command",
            message: format!("editor exited with status {status}"),
        })
    }
}

fn default_editor_command() -> String {
    if cfg!(windows) {
        "notepad".to_owned()
    } else {
        "nano".to_owned()
    }
}

fn validate_edited_command(input: &str) -> PromptResult<String> {
    if input.trim().is_empty() {
        return Err(PromptError::PromptFailed {
            prompt: "edit command",
            message: "edited command cannot be empty".to_owned(),
        });
    }

    Ok(input.trim_end_matches(['\n', '\r']).to_owned())
}

fn normalize_profile_name(input: &str) -> String {
    input.trim().to_owned()
}

fn normalize_secret_value(input: &str) -> String {
    input.trim().to_owned()
}

fn normalize_command_request(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_profile_name_is_trimmed() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_profile_name(Ok("  default  ".to_owned()));

        let value = prompts.prompt_profile_name(None).unwrap();

        assert_eq!(value, "default");
    }

    #[test]
    fn scripted_api_key_trims_edges_only() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_api_key(Ok("  sk-test value  \n".to_owned()));

        let value = prompts.prompt_api_key().unwrap();

        assert_eq!(value, "sk-test value");
    }

    #[test]
    fn scripted_command_request_ignores_custom_message_but_validates_input() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_command_request(Ok("  show current directory  ".to_owned()));

        let value = prompts
            .prompt_command_request("Prompt (glm-4.5-air):")
            .unwrap();

        assert_eq!(value, "show current directory");
    }

    #[test]
    fn api_key_prompt_masks_input_and_allows_reveal_toggle() {
        let prompt = build_api_key_prompt(
            "API key:",
            "Paste the API key exactly as issued. Input is masked; press Ctrl+R to reveal temporarily.",
        );

        assert_eq!(prompt.display_mode, PasswordDisplayMode::Masked);
        assert!(prompt.enable_display_toggle);
        assert!(!prompt.enable_confirmation);
    }

    #[test]
    fn scripted_provider_must_exist() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_provider(Ok("missing".to_owned()));

        let error = prompts
            .select_provider(&[PromptOption::simple("github")])
            .unwrap_err();

        assert!(matches!(error, PromptError::PromptFailed { .. }));
    }

    #[test]
    fn scripted_scopes_must_exist() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_scopes(Ok(vec!["read".to_owned(), "admin".to_owned()]));

        let error = prompts
            .multiselect_scopes(
                &[PromptOption::simple("read"), PromptOption::simple("write")],
                &[],
            )
            .unwrap_err();

        assert!(matches!(error, PromptError::PromptFailed { .. }));
    }

    #[test]
    fn scripted_cancelled_response_stays_typed() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_confirm_overwrite(Err(PromptError::Cancelled));

        let error = prompts.confirm_overwrite("profile").unwrap_err();

        assert!(error.is_cancelled());
    }

    #[test]
    fn scripted_edit_command_preserves_multiline_text() {
        let mut prompts = ScriptedPromptService::new();
        prompts.push_edited_command(Ok("set -euo pipefail\npwd\nls".to_owned()));

        let edited = prompts.edit_command("pwd").unwrap();

        assert_eq!(edited, "set -euo pipefail\npwd\nls");
    }

    #[test]
    fn validate_edited_command_rejects_empty_text() {
        let error = validate_edited_command("\n\n").unwrap_err();

        assert!(matches!(error, PromptError::PromptFailed { .. }));
    }
}
