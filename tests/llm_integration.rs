use std::sync::{Mutex, OnceLock};

use tempfile::tempdir;
use tnav::config::{load_llm_config, save_llm_config};
use tnav::llm::{
    ConfiguredProvider, LlmConfig, LlmError, LlmProvider, MockLlmClient, Provider,
    strip_markdown_fences,
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn llm_config_round_trips_with_toml() {
    let config = LlmConfig {
        active_provider: Some("ollama".to_owned()),
        providers: vec![ConfiguredProvider {
            name: "ollama".to_owned(),
            provider: Provider::Ollama,
            model: "llama3.2".to_owned(),
            base_url: Some("http://localhost:11434".to_owned()),
            api_key: None,
            timeout_secs: 30,
        }],
    };

    let serialized = toml::to_string(&config).expect("serialize llm config");
    let parsed: LlmConfig = toml::from_str(&serialized).expect("parse llm config");

    assert_eq!(parsed, config);
}

#[tokio::test]
async fn mock_client_returns_queued_response() {
    let client = MockLlmClient::new();
    client
        .push_response(Ok("echo hello".to_owned()))
        .expect("queue response");

    let response = client
        .generate_command("say hello")
        .await
        .expect("mock response");

    assert_eq!(response, "echo hello");
}

#[tokio::test]
async fn mock_client_returns_models() {
    let client = MockLlmClient::new();
    client
        .set_models(vec!["llama3.2".to_owned(), "mistral".to_owned()])
        .expect("set models");

    let models = client.list_models().await.expect("list models");

    assert_eq!(models, vec!["llama3.2", "mistral"]);
}

#[test]
fn strip_markdown_fences_removes_code_block_wrapper() {
    let command = strip_markdown_fences("```bash\nls -la\n```");

    assert_eq!(command, "ls -la");
}

#[test]
fn llm_error_display_is_user_facing() {
    let error = LlmError::ModelNotFound {
        model: "gpt-test".to_owned(),
    };

    assert_eq!(error.to_string(), "LLM model 'gpt-test' was not found");
}

#[test]
fn llm_config_save_and_load_uses_standard_config_dir() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempdir().expect("tempdir");
    let original = std::env::var_os("XDG_CONFIG_HOME");
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.path());
    }

    let config = LlmConfig {
        active_provider: Some("local-compat".to_owned()),
        providers: vec![ConfiguredProvider {
            name: "local-compat".to_owned(),
            provider: Provider::OpenAiCompatible,
            model: "local-model".to_owned(),
            base_url: None,
            api_key: None,
            timeout_secs: 45,
        }],
    };

    let saved_path = save_llm_config(&config).expect("save llm config");
    let loaded = load_llm_config()
        .expect("load llm config")
        .expect("llm config exists");

    assert!(saved_path.ends_with("llm.toml"));
    assert_eq!(loaded, config);

    match original {
        Some(value) => unsafe {
            std::env::set_var("XDG_CONFIG_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        },
    }
}

#[test]
fn llm_config_load_accepts_legacy_lmstudio_provider_name() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempdir().expect("tempdir");
    let original = std::env::var_os("XDG_CONFIG_HOME");
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.path());
    }

    let config_dir = temp.path().join("tnav");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(
        config_dir.join("llm.toml"),
        "provider = \"lmstudio\"\nmodel = \"legacy-model\"\ntimeout_secs = 30\n",
    )
    .expect("write legacy llm.toml");

    let parsed = load_llm_config()
        .expect("load llm config")
        .expect("legacy llm config exists");

    assert_eq!(
        parsed.active_provider.as_deref(),
        Some("openai-compatible-1")
    );
    assert_eq!(parsed.providers.len(), 1);
    assert_eq!(parsed.providers[0].name, "openai-compatible-1");
    assert_eq!(parsed.providers[0].provider, Provider::OpenAiCompatible);
    assert_eq!(parsed.providers[0].model, "legacy-model");

    match original {
        Some(value) => unsafe {
            std::env::set_var("XDG_CONFIG_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        },
    }
}
