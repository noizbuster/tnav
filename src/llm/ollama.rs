use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::llm::{ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, StreamSink};

#[derive(Debug, Clone)]
pub struct OllamaClient {
    http_client: Client,
    config: ConfiguredProvider,
}

impl OllamaClient {
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
        })
    }

    fn chat_url(&self) -> String {
        format!(
            "{}/api/chat",
            self.config.base_url_or_default().trim_end_matches('/')
        )
    }

    fn tags_url(&self) -> String {
        format!(
            "{}/api/tags",
            self.config.base_url_or_default().trim_end_matches('/')
        )
    }

    fn chat_request<'a>(&self, prompt: &'a str, stream: bool) -> OllamaChatRequest<'a> {
        OllamaChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system",
                    content: LLM_SYSTEM_PROMPT,
                },
                OllamaMessage {
                    role: "user",
                    content: prompt,
                },
            ],
            stream,
            options: OllamaOptions { temperature: 0.2 },
        }
    }
}

impl LlmProvider for OllamaClient {
    fn generate_command<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.chat_request(prompt, false);

            let response = self
                .http_client
                .post(self.chat_url())
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

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

            let payload: OllamaChatResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            Ok(strip_markdown_fences(&payload.message.content))
        })
    }

    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .http_client
                .get(self.tags_url())
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?
                .error_for_status()
                .map_err(|error| LlmError::InvalidResponse {
                    message: error.to_string(),
                })?;

            let payload: OllamaTagsResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            Ok(payload.models.into_iter().map(|model| model.name).collect())
        })
    }

    fn stream_command<'a>(
        &'a self,
        prompt: &'a str,
        sink: &'a mut dyn StreamSink,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.chat_request(prompt, true);
            let mut response = self
                .http_client
                .post(self.chat_url())
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

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

                    if let Some(piece) = parse_ollama_stream_line(&line)? {
                        sink.on_chunk(&piece);
                        collected.push_str(&piece);
                    }
                }
            }

            let trailing = buffer.trim();
            if !trailing.is_empty()
                && let Some(piece) = parse_ollama_stream_line(trailing)?
            {
                sink.on_chunk(&piece);
                collected.push_str(&piece);
            }

            if collected.is_empty() {
                let payload: OllamaChatResponse =
                    serde_json::from_str(&raw_body).map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;
                return Ok(strip_markdown_fences(&payload.message.content));
            }

            Ok(strip_markdown_fences(&collected))
        })
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest<'a> {
    model: String,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamResponse {
    #[serde(default)]
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelSummary>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelSummary {
    name: String,
}

pub fn strip_markdown_fences(value: &str) -> String {
    let trimmed = value.trim();

    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.strip_prefix("bash").unwrap_or(rest);
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.strip_suffix("```").unwrap_or(rest);
        return rest.trim().to_owned();
    }

    trimmed.to_owned()
}

fn parse_ollama_stream_line(line: &str) -> Result<Option<String>, LlmError> {
    if line.is_empty() {
        return Ok(None);
    }

    let payload: OllamaStreamResponse =
        serde_json::from_str(line).map_err(|error| LlmError::InvalidResponse {
            message: error.to_string(),
        })?;

    let content = payload
        .message
        .map(|message| message.content)
        .unwrap_or_default();
    if payload.done && content.is_empty() {
        return Ok(None);
    }

    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_ollama_stream_line;

    #[test]
    fn parse_ollama_stream_line_extracts_content() {
        let line = r#"{"message":{"content":"echo hello"},"done":false}"#;

        let parsed = parse_ollama_stream_line(line).expect("parse ollama stream line");

        assert_eq!(parsed.as_deref(), Some("echo hello"));
    }

    #[test]
    fn parse_ollama_stream_line_ignores_done_without_content() {
        let line = r#"{"done":true}"#;

        let parsed = parse_ollama_stream_line(line).expect("parse ollama done line");

        assert!(parsed.is_none());
    }
}
