use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LlmConfig {
    #[serde(default)]
    pub active_provider: Option<String>,
    #[serde(default)]
    pub providers: Vec<ConfiguredProvider>,
}

impl LlmConfig {
    pub fn normalize(mut self) -> Self {
        if self
            .active_provider
            .as_deref()
            .and_then(|provider| self.configured_provider(provider))
            .is_none()
        {
            self.active_provider = self.providers.first().map(|provider| provider.name.clone());
        }

        self
    }

    pub fn configured_provider(&self, name: &str) -> Option<&ConfiguredProvider> {
        self.providers.iter().find(|item| item.name == name)
    }

    pub fn configured_provider_mut(&mut self, name: &str) -> Option<&mut ConfiguredProvider> {
        self.providers.iter_mut().find(|item| item.name == name)
    }

    pub fn active_provider_config(&self) -> Option<&ConfiguredProvider> {
        self.active_provider
            .as_deref()
            .and_then(|provider| self.configured_provider(provider))
            .or_else(|| self.providers.first())
    }

    pub fn active_provider_config_mut(&mut self) -> Option<&mut ConfiguredProvider> {
        let provider_name = self
            .active_provider
            .clone()
            .or_else(|| self.providers.first().map(|item| item.name.clone()))?;
        self.configured_provider_mut(&provider_name)
    }

    pub fn upsert_provider(&mut self, provider_config: ConfiguredProvider) {
        let provider_name = provider_config.name.clone();

        if let Some(existing) = self.configured_provider_mut(&provider_name) {
            *existing = provider_config;
        } else {
            self.providers.push(provider_config);
        }

        if self.active_provider.is_none() {
            self.active_provider = Some(provider_name);
        }
    }

    pub fn set_active_provider(&mut self, name: &str) -> bool {
        if self.configured_provider(name).is_some() {
            self.active_provider = Some(name.to_owned());
            true
        } else {
            false
        }
    }

    pub fn remove_provider(&mut self, name: &str) -> bool {
        let before = self.providers.len();
        self.providers.retain(|item| item.name != name);
        let removed = self.providers.len() != before;

        if removed && self.active_provider.as_deref() == Some(name) {
            self.active_provider = self.providers.first().map(|item| item.name.clone());
        }

        removed
    }

    pub fn is_name_taken(&self, name: &str) -> bool {
        self.configured_provider(name).is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfiguredProvider {
    pub name: String,
    pub provider: Provider,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl ConfiguredProvider {
    pub fn base_url_or_default(&self) -> &str {
        self.base_url
            .as_deref()
            .unwrap_or_else(|| self.provider.default_base_url())
    }

    pub fn secret_profile_key(&self) -> String {
        format!("llm-provider/{}", self.name)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Provider {
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "openai-compatible", alias = "lmstudio")]
    OpenAiCompatible,
    #[serde(rename = "openai")]
    OpenAI,
}

impl Provider {
    pub const ALL: [Provider; 3] = [
        Provider::Ollama,
        Provider::OpenAI,
        Provider::OpenAiCompatible,
    ];

    pub fn default_base_url(&self) -> &'static str {
        match self {
            Self::Ollama => "http://localhost:11434",
            Self::OpenAiCompatible => "http://localhost:1234",
            Self::OpenAI => "https://api.openai.com/v1",
        }
    }

    pub fn value(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAiCompatible => "openai-compatible",
            Self::OpenAI => "openai",
        }
    }
}

fn default_timeout_secs() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::{ConfiguredProvider, LlmConfig, Provider};

    #[test]
    fn normalize_sets_first_provider_active_when_missing() {
        let config = LlmConfig {
            active_provider: None,
            providers: vec![ConfiguredProvider {
                name: "ollama-1".to_owned(),
                provider: Provider::Ollama,
                model: String::new(),
                base_url: None,
                api_key: None,
                timeout_secs: 30,
            }],
        }
        .normalize();

        assert_eq!(config.active_provider.as_deref(), Some("ollama-1"));
    }

    #[test]
    fn upsert_provider_replaces_existing_entry_by_name() {
        let mut config = LlmConfig::default();
        config.upsert_provider(ConfiguredProvider {
            name: "ollama-1".to_owned(),
            provider: Provider::Ollama,
            model: "old".to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: 30,
        });
        config.upsert_provider(ConfiguredProvider {
            name: "ollama-1".to_owned(),
            provider: Provider::Ollama,
            model: "new".to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: 30,
        });

        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "new");
    }

    #[test]
    fn removing_active_provider_promotes_first_remaining() {
        let mut config = LlmConfig {
            active_provider: Some("openai-1".to_owned()),
            providers: vec![
                ConfiguredProvider {
                    name: "openai-1".to_owned(),
                    provider: Provider::OpenAI,
                    model: "gpt".to_owned(),
                    base_url: None,
                    api_key: None,
                    timeout_secs: 30,
                },
                ConfiguredProvider {
                    name: "ollama-1".to_owned(),
                    provider: Provider::Ollama,
                    model: "llama".to_owned(),
                    base_url: None,
                    api_key: None,
                    timeout_secs: 30,
                },
            ],
        };

        config.remove_provider("openai-1");

        assert_eq!(config.active_provider.as_deref(), Some("ollama-1"));
    }
}
