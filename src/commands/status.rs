use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config::{llm_config_path, load_llm_config};
use crate::errors::TnavError;
use crate::llm::{ConfiguredProvider, LlmConfig};
use crate::output::Output;

use super::map_config_error;

#[derive(Debug, Serialize)]
struct StatusReport {
    command: &'static str,
    config_path: String,
    config_exists: bool,
    active_provider: Option<String>,
    selected_model: Option<String>,
    providers: Vec<ProviderStatus>,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    name: String,
    provider: &'static str,
    display_name: &'static str,
    active: bool,
    model: Option<String>,
    base_url: String,
}

pub async fn run(global: &GlobalArgs) -> Result<(), TnavError> {
    let output = Output::new(global);
    let config_path = llm_config_path().map_err(map_config_error)?;
    let config = load_llm_config().map_err(map_config_error)?;
    let report = build_status_report(config_path.display().to_string(), config);

    if output.is_json() {
        return output.print_json(&report);
    }

    output.line(format!("LLM config: {}", report.config_path));

    match report.active_provider.as_deref() {
        Some(active_provider) => output.line(format!("Active provider: {active_provider}")),
        None => output.line("Active provider: (none)"),
    }

    match report.selected_model.as_deref() {
        Some(selected_model) => output.line(format!("Selected model: {selected_model}")),
        None => output.line("Selected model: (none)"),
    }

    output.line("");
    output.green_heading("Configured providers");
    if report.providers.is_empty() {
        output.line("  - (none)");
        output.line("Next step: run 'tnav connect' to add a provider.");
        return Ok(());
    }

    for provider in &report.providers {
        output.line(format!("  - {}", provider_status_label(provider)));
    }

    Ok(())
}

fn build_status_report(config_path: String, config: Option<LlmConfig>) -> StatusReport {
    let config_exists = config.is_some();
    let config = config.unwrap_or_default().normalize();
    let selected_model = config
        .active_provider_config()
        .and_then(|provider| normalize_model(&provider.model).map(str::to_owned));
    let active_provider = config.active_provider.clone();
    let providers = config
        .providers
        .iter()
        .map(|provider| build_provider_status(provider, active_provider.as_deref()))
        .collect();

    StatusReport {
        command: "status",
        config_path,
        config_exists,
        active_provider,
        selected_model,
        providers,
    }
}

fn build_provider_status(
    provider: &ConfiguredProvider,
    active_provider_name: Option<&str>,
) -> ProviderStatus {
    ProviderStatus {
        name: provider.name.clone(),
        provider: provider.provider.value(),
        display_name: provider.provider.display_name(),
        active: active_provider_name == Some(provider.name.as_str()),
        model: normalize_model(&provider.model).map(str::to_owned),
        base_url: provider.base_url_or_default().to_owned(),
    }
}

fn provider_status_label(provider: &ProviderStatus) -> String {
    let mut label = format!("{} [{}]", provider.name, provider.provider);
    if provider.active {
        label.push_str(" (Connected)");
    }
    match provider.model.as_deref() {
        Some(model) => label.push_str(&format!(" [Model: {model}]")),
        None => label.push_str(" [No model]"),
    }
    label
}

fn normalize_model(model: &str) -> Option<&str> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use crate::llm::{ConfiguredProvider, DEFAULT_PROVIDER_TIMEOUT_SECS, LlmConfig, Provider};

    use super::{build_status_report, provider_status_label};

    fn provider_config(name: &str, provider: Provider, model: &str) -> ConfiguredProvider {
        ConfiguredProvider {
            name: name.to_owned(),
            provider,
            model: model.to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
        }
    }

    #[test]
    fn build_status_report_tracks_active_provider_and_selected_model() {
        let report = build_status_report(
            "/tmp/llm.toml".to_owned(),
            Some(LlmConfig {
                active_provider: Some("openai-1".to_owned()),
                providers: vec![
                    provider_config("ollama-1", Provider::Ollama, "qwen3.5:9b"),
                    provider_config("openai-1", Provider::OpenAI, "gpt-4.1-mini"),
                ],
            }),
        );

        assert!(report.config_exists);
        assert_eq!(report.active_provider.as_deref(), Some("openai-1"));
        assert_eq!(report.selected_model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(report.providers.len(), 2);
        assert!(!report.providers[0].active);
        assert!(report.providers[1].active);
    }

    #[test]
    fn provider_status_label_marks_connected_and_missing_model() {
        let report = build_status_report(
            "/tmp/llm.toml".to_owned(),
            Some(LlmConfig {
                active_provider: Some("ollama-1".to_owned()),
                providers: vec![provider_config("ollama-1", Provider::Ollama, "")],
            }),
        );

        assert_eq!(
            provider_status_label(&report.providers[0]),
            "ollama-1 [ollama] (Connected) [No model]"
        );
    }

    #[test]
    fn empty_status_report_has_no_active_provider_or_model() {
        let report = build_status_report("/tmp/llm.toml".to_owned(), None);

        assert!(!report.config_exists);
        assert!(report.active_provider.is_none());
        assert!(report.selected_model.is_none());
        assert!(report.providers.is_empty());
    }
}
