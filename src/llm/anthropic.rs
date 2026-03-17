use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::llm::{
    ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, StreamSink, strip_markdown_fences,
};
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 1024;

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    http_client: Client,
    config: ConfiguredProvider,
    secret_store: KeyringSecretStore,
}

impl AnthropicClient {
    pub fn new(config: ConfiguredProvider) -> Result<Self, LlmError> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(map_http_client_error)?;

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
        self.secret_store
            .load_secret(&self.config.secret_profile_key(), SecretKind::ApiKey)
            .map_err(|error| LlmError::AuthFailed {
                message: error.to_string(),
            })?
            .ok_or_else(|| LlmError::ConfigMissing {
                message: "Anthropic API key is not configured in secure storage".to_owned(),
            })
    }

    fn messages_request(&self, prompt: &str, stream: bool) -> AnthropicMessagesRequest {
        AnthropicMessagesRequest {
            model: self.config.model.clone(),
            system: LLM_SYSTEM_PROMPT.to_owned(),
            messages: vec![AnthropicInputMessage {
                role: "user".to_owned(),
                content: prompt.to_owned(),
            }],
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: 0.2,
            stream,
        }
    }

    async fn send_messages_request(
        &self,
        request: &AnthropicMessagesRequest,
    ) -> Result<Response, LlmError> {
        let api_key = self.api_key()?;
        let response = self
            .http_client
            .post(format!("{}/v1/messages", self.base_url()))
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(request)
            .send()
            .await
            .map_err(map_request_error)?;

        handle_response_status(response, &self.config.model).await
    }
}

impl LlmProvider for AnthropicClient {
    fn generate_command<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.messages_request(prompt, false);
            let response = self.send_messages_request(&request).await?;
            let payload: AnthropicMessageResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            extract_message_text(payload)
        })
    }

    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            Err(LlmError::InvalidResponse {
                message: "Anthropic model listing is not supported by this client; set a model explicitly with 'tnav model <name>'".to_owned(),
            })
        })
    }

    fn stream_command<'a>(
        &'a self,
        prompt: &'a str,
        sink: &'a mut dyn StreamSink,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.messages_request(prompt, true);
            let mut response = self.send_messages_request(&request).await?;
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
                return Err(LlmError::InvalidResponse {
                    message: "Anthropic returned no text content in the streaming response"
                        .to_owned(),
                });
            }

            Ok(strip_markdown_fences(&collected))
        })
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    system: String,
    messages: Vec<AnthropicInputMessage>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct AnthropicInputMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorEnvelope {
    error: AnthropicErrorPayload,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorPayload {
    #[serde(rename = "type")]
    kind: String,
    message: String,
}

fn map_http_client_error(error: reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout
    } else {
        LlmError::ConnectionFailed {
            message: error.to_string(),
        }
    }
}

fn map_request_error(error: reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout
    } else {
        LlmError::ConnectionFailed {
            message: error.to_string(),
        }
    }
}

async fn handle_response_status(response: Response, model: &str) -> Result<Response, LlmError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .map_err(|error| LlmError::InvalidResponse {
            message: error.to_string(),
        })?;

    let anthropic_error = serde_json::from_str::<AnthropicErrorEnvelope>(&body)
        .ok()
        .map(|payload| payload.error);

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(LlmError::AuthFailed {
            message: anthropic_error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "Anthropic rejected the configured API key".to_owned()),
        });
    }

    if status == StatusCode::TOO_MANY_REQUESTS
        || status.as_u16() == 529
        || anthropic_error
            .as_ref()
            .map(|error| error.kind == "rate_limit_error" || error.kind == "overloaded_error")
            .unwrap_or(false)
    {
        return Err(LlmError::RateLimited);
    }

    let error_message = anthropic_error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or(body.as_str());
    if status == StatusCode::NOT_FOUND || is_missing_model_message(error_message) {
        return Err(LlmError::ModelNotFound {
            model: model.to_owned(),
        });
    }

    Err(LlmError::InvalidResponse {
        message: error_message.to_owned(),
    })
}

fn is_missing_model_message(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("model")
        && (normalized.contains("not found")
            || normalized.contains("does not exist")
            || normalized.contains("unknown"))
}

fn extract_message_text(payload: AnthropicMessageResponse) -> Result<String, LlmError> {
    let content = payload
        .content
        .into_iter()
        .filter(|block| block.kind == "text")
        .filter_map(|block| block.text)
        .collect::<String>();

    if content.is_empty() {
        return Err(LlmError::InvalidResponse {
            message: "Anthropic returned no text content".to_owned(),
        });
    }

    Ok(strip_markdown_fences(&content))
}

fn parse_sse_data_line(line: &str) -> Result<Option<String>, LlmError> {
    if line.is_empty() || !line.starts_with("data:") {
        return Ok(None);
    }

    let payload = line.trim_start_matches("data:").trim();
    if payload.is_empty() {
        return Ok(None);
    }

    let event: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| LlmError::InvalidResponse {
            message: error.to_string(),
        })?;

    match event.get("type").and_then(serde_json::Value::as_str) {
        Some("content_block_delta") => Ok(event.get("delta").and_then(|delta| {
            match delta.get("type").and_then(serde_json::Value::as_str) {
                Some("text_delta") => delta
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                _ => None,
            }
        })),
        Some("error") => {
            let error =
                serde_json::from_value::<AnthropicErrorEnvelope>(event).map_err(|error| {
                    LlmError::InvalidResponse {
                        message: error.to_string(),
                    }
                })?;

            if error.error.kind == "rate_limit_error" || error.error.kind == "overloaded_error" {
                Err(LlmError::RateLimited)
            } else {
                Err(LlmError::InvalidResponse {
                    message: error.error.message,
                })
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AnthropicMessageResponse, AnthropicMessagesRequest, extract_message_text,
        is_missing_model_message, parse_sse_data_line,
    };

    #[test]
    fn anthropic_request_serializes_messages_api_shape() {
        let request = AnthropicMessagesRequest {
            model: "claude-3-5-sonnet-latest".to_owned(),
            system: "system prompt".to_owned(),
            messages: vec![super::AnthropicInputMessage {
                role: "user".to_owned(),
                content: "pwd".to_owned(),
            }],
            max_tokens: 1024,
            temperature: 0.2,
            stream: true,
        };

        let value = serde_json::to_value(&request).expect("serialize anthropic request");

        assert_eq!(value["model"], json!("claude-3-5-sonnet-latest"));
        assert_eq!(value["system"], json!("system prompt"));
        assert_eq!(value["messages"][0]["role"], json!("user"));
        assert_eq!(value["messages"][0]["content"], json!("pwd"));
        assert_eq!(value["max_tokens"], json!(1024));
        assert_eq!(value["stream"], json!(true));
    }

    #[test]
    fn extract_message_text_collects_text_blocks_and_strips_fences() {
        let payload: AnthropicMessageResponse = serde_json::from_value(json!({
            "content": [
                {"type": "text", "text": "```bash\npwd\n```"},
                {"type": "tool_use", "id": "tool_1", "name": "noop", "input": {}}
            ]
        }))
        .expect("deserialize anthropic message response");

        let text = extract_message_text(payload).expect("extract text content");

        assert_eq!(text, "pwd");
    }

    #[test]
    fn parse_sse_data_line_extracts_text_delta() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"echo hello"}}"#;

        let parsed = parse_sse_data_line(line).expect("parse text delta");

        assert_eq!(parsed.as_deref(), Some("echo hello"));
    }

    #[test]
    fn parse_sse_data_line_ignores_non_text_events() {
        let line = r#"data: {"type":"message_stop"}"#;

        let parsed = parse_sse_data_line(line).expect("parse message stop");

        assert!(parsed.is_none());
    }

    #[test]
    fn parse_sse_data_line_maps_stream_rate_limit_errors() {
        let line =
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;

        let error = parse_sse_data_line(line).expect_err("rate limit error expected");

        assert!(matches!(error, crate::llm::LlmError::RateLimited));
    }

    #[test]
    fn missing_model_detection_matches_anthropic_error_text() {
        assert!(is_missing_model_message("Model 'claude-x' not found"));
        assert!(is_missing_model_message("The model does not exist"));
        assert!(!is_missing_model_message("invalid x-api-key"));
    }
}
