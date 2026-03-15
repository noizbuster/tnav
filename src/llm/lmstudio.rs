use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::llm::{
    ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, StreamSink, strip_markdown_fences,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http_client: Client,
    config: ConfiguredProvider,
}

impl OpenAiCompatibleClient {
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

    fn base_url(&self) -> &str {
        self.config.base_url_or_default().trim_end_matches('/')
    }

    fn chat_request(&self, prompt: &str, stream: bool) -> OpenAiCompatibleChatRequest {
        OpenAiCompatibleChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                OpenAiCompatibleMessage {
                    role: "system".to_owned(),
                    content: LLM_SYSTEM_PROMPT.to_owned(),
                },
                OpenAiCompatibleMessage {
                    role: "user".to_owned(),
                    content: prompt.to_owned(),
                },
            ],
            temperature: 0.2,
            stream,
        }
    }
}

impl LlmProvider for OpenAiCompatibleClient {
    fn generate_command<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.chat_request(prompt, false);

            let response = self
                .http_client
                .post(format!("{}/v1/chat/completions", self.base_url()))
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

            let payload: OpenAiCompatibleChatResponse =
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
                    message: "OpenAI-compatible provider returned no choices".to_owned(),
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
            let response = self
                .http_client
                .get(format!("{}/v1/models", self.base_url()))
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?
                .error_for_status()
                .map_err(|error| LlmError::InvalidResponse {
                    message: error.to_string(),
                })?;

            let payload: OpenAiCompatibleModelsResponse =
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
            let mut response = self
                .http_client
                .post(format!("{}/v1/chat/completions", self.base_url()))
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
                let payload: OpenAiCompatibleChatResponse = serde_json::from_str(&raw_body)
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;
                let content = payload
                    .choices
                    .into_iter()
                    .next()
                    .ok_or_else(|| LlmError::InvalidResponse {
                        message: "OpenAI-compatible provider returned no choices".to_owned(),
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
struct OpenAiCompatibleChatRequest {
    model: String,
    messages: Vec<OpenAiCompatibleMessage>,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiCompatibleMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleChatResponse {
    choices: Vec<OpenAiCompatibleChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleChoice {
    message: OpenAiCompatibleMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleStreamResponse {
    choices: Vec<OpenAiCompatibleStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleStreamChoice {
    delta: OpenAiCompatibleDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleModelsResponse {
    data: Vec<OpenAiCompatibleModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleModel {
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

    let response: OpenAiCompatibleStreamResponse =
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
    use super::parse_sse_data_line;

    #[test]
    fn parse_sse_data_line_extracts_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"echo hello"}}]}"#;

        let parsed = parse_sse_data_line(line).expect("parse sse data line");

        assert_eq!(parsed.as_deref(), Some("echo hello"));
    }

    #[test]
    fn parse_sse_data_line_ignores_done_marker() {
        let parsed = parse_sse_data_line("data: [DONE]").expect("parse done marker");

        assert!(parsed.is_none());
    }
}
