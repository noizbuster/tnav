use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::llm::{
    ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, StreamSink, strip_markdown_fences,
};
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    http_client: Client,
    config: ConfiguredProvider,
    secret_store: KeyringSecretStore,
}

impl OpenAiClient {
    pub fn new(config: ConfiguredProvider) -> Result<Self, LlmError> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|error| LlmError::ConnectionFailed {
                message: error.to_string(),
            })?;

        Ok(Self {
            http_client,
            config,
            secret_store: KeyringSecretStore::new(),
        })
    }

    fn base_url(&self) -> &str {
        self.config.base_url_or_default().trim_end_matches('/')
    }

    fn api_key(&self) -> Result<String, LlmError> {
        if let Some(api_key) = self.config.inline_api_key() {
            return Ok(api_key.to_owned());
        }

        self.secret_store
            .load_secret(&self.config.secret_profile_key(), SecretKind::ApiKey)
            .map_err(|error| LlmError::AuthFailed {
                message: error.to_string(),
            })?
            .ok_or_else(|| LlmError::ConfigMissing {
                message: "OpenAI API key is not configured in secure storage".to_owned(),
            })
    }

    fn chat_request(&self, prompt: &str, stream: bool) -> OpenAiChatRequest {
        OpenAiChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_owned(),
                    content: LLM_SYSTEM_PROMPT.to_owned(),
                },
                OpenAiMessage {
                    role: "user".to_owned(),
                    content: prompt.to_owned(),
                },
            ],
            temperature: 0.2,
            stream,
        }
    }
}

impl LlmProvider for OpenAiClient {
    fn generate_command<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.chat_request(prompt, false);
            let api_key = self.api_key()?;

            let response = self
                .http_client
                .post(format!("{}/chat/completions", self.base_url()))
                .bearer_auth(api_key)
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: "OpenAI rejected the configured API key".to_owned(),
                });
            }

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(LlmError::ModelNotFound {
                    model: self.config.model.clone(),
                });
            }

            let response =
                response
                    .error_for_status()
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            let payload: OpenAiChatResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            let content = payload
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| LlmError::InvalidResponse {
                    message: "OpenAI returned no choices".to_owned(),
                })?
                .message
                .content;

            Ok(strip_markdown_fences(&content))
        })
    }

    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let api_key = self.api_key()?;
            let response = self
                .http_client
                .get(format!("{}/models", self.base_url()))
                .bearer_auth(api_key)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: "OpenAI rejected the configured API key".to_owned(),
                });
            }

            let response =
                response
                    .error_for_status()
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            let payload: OpenAiModelsResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            Ok(payload.data.into_iter().map(|model| model.id).collect())
        })
    }

    fn stream_command<'a>(
        &'a self,
        prompt: &'a str,
        sink: &'a mut dyn StreamSink,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.chat_request(prompt, true);
            let api_key = self.api_key()?;
            let mut response = self
                .http_client
                .post(format!("{}/chat/completions", self.base_url()))
                .bearer_auth(api_key)
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: "OpenAI rejected the configured API key".to_owned(),
                });
            }

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(LlmError::ModelNotFound {
                    model: self.config.model.clone(),
                });
            }

            response = response
                .error_for_status()
                .map_err(|error| LlmError::InvalidResponse {
                    message: error.to_string(),
                })?;

            let mut raw_body = String::new();
            let mut buffer = String::new();
            let mut collected = String::new();

            while let Some(chunk) =
                response
                    .chunk()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?
            {
                let chunk_text = String::from_utf8_lossy(&chunk);
                raw_body.push_str(&chunk_text);
                buffer.push_str(&chunk_text);

                while let Some(newline_index) = buffer.find('\n') {
                    let line = buffer[..newline_index].trim().to_owned();
                    buffer.drain(..=newline_index);

                    if let Some(piece) = parse_sse_data_line(&line)? {
                        sink.on_chunk(&piece);
                        collected.push_str(&piece);
                    }
                }
            }

            let trailing = buffer.trim();
            if !trailing.is_empty()
                && let Some(piece) = parse_sse_data_line(trailing)?
            {
                sink.on_chunk(&piece);
                collected.push_str(&piece);
            }

            if collected.is_empty() {
                let payload: OpenAiChatResponse =
                    serde_json::from_str(&raw_body).map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;
                let content = payload
                    .choices
                    .into_iter()
                    .next()
                    .ok_or_else(|| LlmError::InvalidResponse {
                        message: "OpenAI returned no choices".to_owned(),
                    })?
                    .message
                    .content;
                return Ok(strip_markdown_fences(&content));
            }

            Ok(strip_markdown_fences(&collected))
        })
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamResponse {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

fn parse_sse_data_line(line: &str) -> Result<Option<String>, LlmError> {
    if line.is_empty() || !line.starts_with("data:") {
        return Ok(None);
    }

    let payload = line.trim_start_matches("data:").trim();
    if payload == "[DONE]" {
        return Ok(None);
    }

    let response: OpenAiStreamResponse =
        serde_json::from_str(payload).map_err(|error| LlmError::InvalidResponse {
            message: error.to_string(),
        })?;

    Ok(response
        .choices
        .into_iter()
        .filter_map(|choice| choice.delta.content)
        .next())
}

#[cfg(test)]
mod tests {
    use crate::llm::{ConfiguredProvider, DEFAULT_PROVIDER_TIMEOUT_SECS, OpenAiClient, Provider};

    use super::parse_sse_data_line;

    #[test]
    fn api_key_prefers_inline_config_value() {
        let client = OpenAiClient::new(ConfiguredProvider {
            name: "openai".to_owned(),
            provider: Provider::OpenAI,
            model: "gpt-4.1-mini".to_owned(),
            base_url: None,
            api_key: Some("inline-openai-key".to_owned()),
            timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
        })
        .expect("client builds");

        assert_eq!(
            client.api_key().expect("inline api key"),
            "inline-openai-key"
        );
    }

    #[test]
    fn parse_sse_data_line_extracts_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"pwd"}}]}"#;

        let parsed = parse_sse_data_line(line).expect("parse sse data line");

        assert_eq!(parsed.as_deref(), Some("pwd"));
    }

    #[test]
    fn parse_sse_data_line_ignores_done_marker() {
        let parsed = parse_sse_data_line("data: [DONE]").expect("parse done marker");

        assert!(parsed.is_none());
    }
}
