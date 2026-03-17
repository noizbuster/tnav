use serde::{Deserialize, Serialize};

pub const DEFAULT_PROVIDER_TIMEOUT_SECS: u64 = 60;

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

    pub fn inline_api_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
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
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "mistral")]
    Mistral,
    #[serde(rename = "groq")]
    Groq,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "xai")]
    XAI,
    #[serde(rename = "zai")]
    Zai,
    #[serde(rename = "zai-coding-plan-global")]
    ZaiCodingPlanGlobal,
    #[serde(rename = "zai-coding-plan-china")]
    ZaiCodingPlanChina,
}

impl Provider {
    pub const ALL: [Provider; 12] = [
        Provider::Ollama,
        Provider::OpenAI,
        Provider::OpenAiCompatible,
        Provider::Anthropic,
        Provider::Google,
        Provider::Mistral,
        Provider::Groq,
        Provider::DeepSeek,
        Provider::XAI,
        Provider::Zai,
        Provider::ZaiCodingPlanGlobal,
        Provider::ZaiCodingPlanChina,
    ];

    pub fn default_base_url(&self) -> &'static str {
        match self {
            Self::Ollama => "http://localhost:11434",
            Self::OpenAiCompatible => "http://localhost:1234",
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta",
            Self::Mistral => "https://api.mistral.ai/v1",
            Self::Groq => "https://api.groq.com/openai/v1",
            Self::DeepSeek => "https://api.deepseek.com/v1",
            Self::XAI => "https://api.x.ai/v1",
            Self::Zai => "https://api.z.ai/api/paas/v4",
            Self::ZaiCodingPlanGlobal => "https://api.z.ai/api/coding/paas/v4",
            Self::ZaiCodingPlanChina => "https://open.bigmodel.cn/api/coding/paas/v4",
        }
    }

    pub fn value(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAiCompatible => "openai-compatible",
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::Mistral => "mistral",
            Self::Groq => "groq",
            Self::DeepSeek => "deepseek",
            Self::XAI => "xai",
            Self::Zai => "zai",
            Self::ZaiCodingPlanGlobal => "zai-coding-plan-global",
            Self::ZaiCodingPlanChina => "zai-coding-plan-china",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::OpenAiCompatible => "OpenAI-compatible",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic (Claude)",
            Self::Google => "Google (Gemini)",
            Self::Mistral => "Mistral AI",
            Self::Groq => "Groq",
            Self::DeepSeek => "DeepSeek",
            Self::XAI => "xAI (Grok)",
            Self::Zai => "z.ai",
            Self::ZaiCodingPlanGlobal => "z.ai Coding Plan (Global)",
            Self::ZaiCodingPlanChina => "z.ai Coding Plan (China)",
        }
    }

    pub fn uses_api_key_storage(&self) -> bool {
        match self {
            Self::Ollama | Self::OpenAiCompatible => false,
            Self::OpenAI
            | Self::Anthropic
            | Self::Google
            | Self::Mistral
            | Self::Groq
            | Self::DeepSeek
            | Self::XAI
            | Self::Zai
            | Self::ZaiCodingPlanGlobal
            | Self::ZaiCodingPlanChina => true,
        }
    }
}

fn default_timeout_secs() -> u64 {
    DEFAULT_PROVIDER_TIMEOUT_SECS
}

#[cfg(test)]
mod tests {
    use super::{ConfiguredProvider, DEFAULT_PROVIDER_TIMEOUT_SECS, LlmConfig, Provider};

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
                timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
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
            timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
        });
        config.upsert_provider(ConfiguredProvider {
            name: "ollama-1".to_owned(),
            provider: Provider::Ollama,
            model: "new".to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
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
                    timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
                },
                ConfiguredProvider {
                    name: "ollama-1".to_owned(),
                    provider: Provider::Ollama,
                    model: "llama".to_owned(),
                    base_url: None,
                    api_key: None,
                    timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
                },
            ],
        };

        config.remove_provider("openai-1");

        assert_eq!(config.active_provider.as_deref(), Some("ollama-1"));
    }

    #[test]
    fn configured_provider_deserialize_uses_one_minute_timeout_by_default() {
        let provider: ConfiguredProvider = toml::from_str(
            r#"
name = "openai-1"
provider = "openai"
model = "gpt-4.1-mini"
"#,
        )
        .expect("provider should deserialize");

        assert_eq!(provider.timeout_secs, DEFAULT_PROVIDER_TIMEOUT_SECS);
    }

    #[test]
    fn provider_all_contains_all_variants() {
        assert_eq!(Provider::ALL.len(), 12);
        assert!(Provider::ALL.contains(&Provider::Ollama));
        assert!(Provider::ALL.contains(&Provider::OpenAI));
        assert!(Provider::ALL.contains(&Provider::OpenAiCompatible));
        assert!(Provider::ALL.contains(&Provider::Anthropic));
        assert!(Provider::ALL.contains(&Provider::Google));
        assert!(Provider::ALL.contains(&Provider::Mistral));
        assert!(Provider::ALL.contains(&Provider::Groq));
        assert!(Provider::ALL.contains(&Provider::DeepSeek));
        assert!(Provider::ALL.contains(&Provider::XAI));
        assert!(Provider::ALL.contains(&Provider::Zai));
        assert!(Provider::ALL.contains(&Provider::ZaiCodingPlanGlobal));
        assert!(Provider::ALL.contains(&Provider::ZaiCodingPlanChina));
    }

    #[test]
    fn provider_default_base_urls() {
        assert_eq!(
            Provider::Ollama.default_base_url(),
            "http://localhost:11434"
        );
        assert_eq!(
            Provider::OpenAiCompatible.default_base_url(),
            "http://localhost:1234"
        );
        assert_eq!(
            Provider::OpenAI.default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            Provider::Anthropic.default_base_url(),
            "https://api.anthropic.com"
        );
        assert_eq!(
            Provider::Google.default_base_url(),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(
            Provider::Mistral.default_base_url(),
            "https://api.mistral.ai/v1"
        );
        assert_eq!(
            Provider::Groq.default_base_url(),
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(
            Provider::DeepSeek.default_base_url(),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(Provider::XAI.default_base_url(), "https://api.x.ai/v1");
        assert_eq!(
            Provider::Zai.default_base_url(),
            "https://api.z.ai/api/paas/v4"
        );
        assert_eq!(
            Provider::ZaiCodingPlanGlobal.default_base_url(),
            "https://api.z.ai/api/coding/paas/v4"
        );
        assert_eq!(
            Provider::ZaiCodingPlanChina.default_base_url(),
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
    }

    #[test]
    fn provider_value_strings() {
        assert_eq!(Provider::Ollama.value(), "ollama");
        assert_eq!(Provider::OpenAiCompatible.value(), "openai-compatible");
        assert_eq!(Provider::OpenAI.value(), "openai");
        assert_eq!(Provider::Anthropic.value(), "anthropic");
        assert_eq!(Provider::Google.value(), "google");
        assert_eq!(Provider::Mistral.value(), "mistral");
        assert_eq!(Provider::Groq.value(), "groq");
        assert_eq!(Provider::DeepSeek.value(), "deepseek");
        assert_eq!(Provider::XAI.value(), "xai");
        assert_eq!(Provider::Zai.value(), "zai");
        assert_eq!(
            Provider::ZaiCodingPlanGlobal.value(),
            "zai-coding-plan-global"
        );
        assert_eq!(
            Provider::ZaiCodingPlanChina.value(),
            "zai-coding-plan-china"
        );
    }

    #[test]
    fn provider_display_names() {
        assert_eq!(Provider::Ollama.display_name(), "Ollama");
        assert_eq!(Provider::OpenAI.display_name(), "OpenAI");
        assert_eq!(
            Provider::OpenAiCompatible.display_name(),
            "OpenAI-compatible"
        );
        assert_eq!(Provider::Anthropic.display_name(), "Anthropic (Claude)");
        assert_eq!(Provider::Google.display_name(), "Google (Gemini)");
        assert_eq!(Provider::Mistral.display_name(), "Mistral AI");
        assert_eq!(Provider::Groq.display_name(), "Groq");
        assert_eq!(Provider::DeepSeek.display_name(), "DeepSeek");
        assert_eq!(Provider::XAI.display_name(), "xAI (Grok)");
        assert_eq!(Provider::Zai.display_name(), "z.ai");
        assert_eq!(
            Provider::ZaiCodingPlanGlobal.display_name(),
            "z.ai Coding Plan (Global)"
        );
        assert_eq!(
            Provider::ZaiCodingPlanChina.display_name(),
            "z.ai Coding Plan (China)"
        );
    }

    #[test]
    fn provider_api_key_storage_flags() {
        assert!(!Provider::Ollama.uses_api_key_storage());
        assert!(!Provider::OpenAiCompatible.uses_api_key_storage());
        assert!(Provider::OpenAI.uses_api_key_storage());
        assert!(Provider::Anthropic.uses_api_key_storage());
        assert!(Provider::Google.uses_api_key_storage());
        assert!(Provider::Mistral.uses_api_key_storage());
        assert!(Provider::Groq.uses_api_key_storage());
        assert!(Provider::DeepSeek.uses_api_key_storage());
        assert!(Provider::XAI.uses_api_key_storage());
        assert!(Provider::Zai.uses_api_key_storage());
        assert!(Provider::ZaiCodingPlanGlobal.uses_api_key_storage());
        assert!(Provider::ZaiCodingPlanChina.uses_api_key_storage());
    }

    #[test]
    fn provider_serde_roundtrip() {
        let providers = [
            Provider::Ollama,
            Provider::OpenAiCompatible,
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Google,
            Provider::Mistral,
            Provider::Groq,
            Provider::DeepSeek,
            Provider::XAI,
            Provider::Zai,
            Provider::ZaiCodingPlanGlobal,
            Provider::ZaiCodingPlanChina,
        ];

        for provider in providers {
            let json = serde_json::to_string(&provider).unwrap();
            let parsed: Provider = serde_json::from_str(&json).unwrap();
            assert_eq!(provider, parsed);
        }
    }

    #[test]
    fn provider_serde_lmstudio_alias() {
        let json = r#""lmstudio""#;
        let parsed: Provider = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, Provider::OpenAiCompatible);
    }
}
