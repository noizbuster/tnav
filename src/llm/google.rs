use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::llm::{
    ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, StreamSink, strip_markdown_fences,
};
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};

#[derive(Debug, Clone)]
pub struct GoogleClient {
    http_client: Client,
    config: ConfiguredProvider,
    secret_store: KeyringSecretStore,
}

impl GoogleClient {
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
                message: "Google API key is not configured in secure storage".to_owned(),
            })
    }

    fn model_resource_name(&self) -> String {
        normalize_model_resource_name(&self.config.model)
    }

    fn generate_request(&self, prompt: &str) -> GoogleGenerateContentRequest {
        GoogleGenerateContentRequest {
            contents: vec![GoogleContent::user_text(prompt)],
            system_instruction: Some(GoogleContent::system_text(LLM_SYSTEM_PROMPT)),
            generation_config: Some(GoogleGenerationConfig { temperature: 0.2 }),
        }
    }
}

impl LlmProvider for GoogleClient {
    fn generate_command<'a>(
        &'a self,
        prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.generate_request(prompt);
            let api_key = self.api_key()?;
            let response = self
                .http_client
                .post(format!(
                    "{}/{}:generateContent",
                    self.base_url(),
                    self.model_resource_name()
                ))
                .query(&[("key", api_key.as_str())])
                .json(&request)
                .send()
                .await
                .map_err(map_request_error)?;

            let response = handle_response_status(response, Some(&self.config.model)).await?;
            let payload: GoogleGenerateContentResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            extract_candidate_text(payload)
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
                .query(&[("key", api_key.as_str())])
                .send()
                .await
                .map_err(map_request_error)?;

            let response = handle_response_status(response, None).await?;
            let payload: GoogleModelsResponse =
                response
                    .json()
                    .await
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            parse_model_names(payload)
        })
    }

    fn stream_command<'a>(
        &'a self,
        prompt: &'a str,
        sink: &'a mut dyn StreamSink,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.generate_request(prompt);
            let api_key = self.api_key()?;
            let mut response = self
                .http_client
                .post(format!(
                    "{}/{}:streamGenerateContent",
                    self.base_url(),
                    self.model_resource_name()
                ))
                .query(&[("alt", "sse"), ("key", api_key.as_str())])
                .json(&request)
                .send()
                .await
                .map_err(map_request_error)?;

            response = handle_response_status(response, Some(&self.config.model)).await?;

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
                let payload: GoogleGenerateContentResponse = serde_json::from_str(&raw_body)
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;
                return extract_candidate_text(payload);
            }

            Ok(strip_markdown_fences(&collected))
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGenerateContentRequest {
    contents: Vec<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GoogleGenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GoogleGenerationConfig {
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GooglePart>,
}

impl GoogleContent {
    fn system_text(text: &str) -> Self {
        Self {
            role: None,
            parts: vec![GooglePart {
                text: text.to_owned(),
            }],
        }
    }

    fn user_text(text: &str) -> Self {
        Self {
            role: Some("user".to_owned()),
            parts: vec![GooglePart {
                text: text.to_owned(),
            }],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GooglePart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GoogleGenerateContentResponse {
    #[serde(default)]
    candidates: Vec<GoogleCandidate>,
}

#[derive(Debug, Deserialize)]
struct GoogleCandidate {
    #[serde(default)]
    content: Option<GoogleContentResponse>,
}

#[derive(Debug, Deserialize)]
struct GoogleContentResponse {
    #[serde(default)]
    parts: Vec<GoogleResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GoogleResponsePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleModelsResponse {
    #[serde(default)]
    models: Vec<GoogleModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleModel {
    name: String,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleErrorEnvelope {
    error: GoogleErrorPayload,
}

#[derive(Debug, Deserialize)]
struct GoogleErrorPayload {
    message: String,
    #[serde(default)]
    status: Option<String>,
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

async fn handle_response_status(
    response: Response,
    model: Option<&str>,
) -> Result<Response, LlmError> {
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

    let google_error = serde_json::from_str::<GoogleErrorEnvelope>(&body)
        .ok()
        .map(|payload| payload.error);
    let error_message = google_error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or(body.as_str());

    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(LlmError::AuthFailed {
            message: google_error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "Google rejected the configured API key".to_owned()),
        });
    }

    if status == StatusCode::TOO_MANY_REQUESTS
        || google_error
            .as_ref()
            .and_then(|error| error.status.as_deref())
            .map(|value| value == "RESOURCE_EXHAUSTED")
            .unwrap_or(false)
    {
        return Err(LlmError::RateLimited);
    }

    if let Some(model) = model
        && (status == StatusCode::NOT_FOUND || is_missing_model_message(error_message))
    {
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
            || normalized.contains("unsupported")
            || normalized.contains("not supported")
            || normalized.contains("unknown"))
}

fn normalize_model_resource_name(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.starts_with("models/") {
        trimmed.to_owned()
    } else {
        format!("models/{trimmed}")
    }
}

fn display_model_name(name: &str) -> String {
    name.strip_prefix("models/").unwrap_or(name).to_owned()
}

fn extract_candidate_text(payload: GoogleGenerateContentResponse) -> Result<String, LlmError> {
    let content = payload
        .candidates
        .into_iter()
        .next()
        .and_then(|candidate| candidate.content)
        .map(|content| {
            content
                .parts
                .into_iter()
                .filter_map(|part| part.text)
                .collect::<String>()
        })
        .unwrap_or_default();

    if content.is_empty() {
        return Err(LlmError::InvalidResponse {
            message: "Google returned no text content".to_owned(),
        });
    }

    Ok(strip_markdown_fences(&content))
}

fn parse_model_names(payload: GoogleModelsResponse) -> Result<Vec<String>, LlmError> {
    let models = payload
        .models
        .into_iter()
        .filter(|model| {
            model.supported_generation_methods.is_empty()
                || model
                    .supported_generation_methods
                    .iter()
                    .any(|method| method == "generateContent" || method == "streamGenerateContent")
        })
        .map(|model| display_model_name(&model.name))
        .collect::<Vec<_>>();

    if models.is_empty() {
        return Err(LlmError::InvalidResponse {
            message: "Google returned no generateContent-capable models".to_owned(),
        });
    }

    Ok(models)
}

fn parse_sse_data_line(line: &str) -> Result<Option<String>, LlmError> {
    if line.is_empty() || !line.starts_with("data:") {
        return Ok(None);
    }

    let payload = line.trim_start_matches("data:").trim();
    if payload == "[DONE]" || payload.is_empty() {
        return Ok(None);
    }

    let response: GoogleGenerateContentResponse =
        serde_json::from_str(payload).map_err(|error| LlmError::InvalidResponse {
            message: error.to_string(),
        })?;

    Ok(Some(extract_candidate_text(response)?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        GoogleContent, GoogleGenerateContentRequest, GoogleGenerateContentResponse,
        GoogleGenerationConfig, GoogleModelsResponse, extract_candidate_text,
        is_missing_model_message, normalize_model_resource_name, parse_model_names,
        parse_sse_data_line,
    };

    #[test]
    fn google_request_serializes_generate_content_shape() {
        let request = GoogleGenerateContentRequest {
            contents: vec![GoogleContent::user_text("pwd")],
            system_instruction: Some(GoogleContent::system_text("system prompt")),
            generation_config: Some(GoogleGenerationConfig { temperature: 0.2 }),
        };

        let value = serde_json::to_value(&request).expect("serialize google request");

        assert_eq!(value["contents"][0]["role"], json!("user"));
        assert_eq!(value["contents"][0]["parts"][0]["text"], json!("pwd"));
        assert_eq!(
            value["systemInstruction"]["parts"][0]["text"],
            json!("system prompt")
        );
        let temperature = value["generationConfig"]["temperature"]
            .as_f64()
            .expect("temperature should serialize as a number");
        assert!((temperature - 0.2).abs() < 1e-6);
    }

    #[test]
    fn extract_candidate_text_collects_parts_and_strips_fences() {
        let payload: GoogleGenerateContentResponse = serde_json::from_value(json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "```bash\n" },
                            { "text": "pwd\n" },
                            { "text": "```" }
                        ]
                    }
                }
            ]
        }))
        .expect("deserialize google response");

        let text = extract_candidate_text(payload).expect("extract google text");

        assert_eq!(text, "pwd");
    }

    #[test]
    fn parse_model_names_keeps_generate_capable_models() {
        let payload: GoogleModelsResponse = serde_json::from_value(json!({
            "models": [
                {
                    "name": "models/gemini-2.5-flash",
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/text-embedding-004",
                    "supportedGenerationMethods": ["embedContent"]
                },
                {
                    "name": "models/gemini-2.5-pro",
                    "supportedGenerationMethods": ["streamGenerateContent"]
                }
            ]
        }))
        .expect("deserialize models response");

        let models = parse_model_names(payload).expect("parse models");

        assert_eq!(models, vec!["gemini-2.5-flash", "gemini-2.5-pro"]);
    }

    #[test]
    fn parse_sse_data_line_extracts_text_from_stream_chunk() {
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"echo hello"}]}}]}"#;

        let parsed = parse_sse_data_line(line).expect("parse google sse line");

        assert_eq!(parsed.as_deref(), Some("echo hello"));
    }

    #[test]
    fn parse_sse_data_line_ignores_done_marker() {
        let parsed = parse_sse_data_line("data: [DONE]").expect("parse done marker");

        assert!(parsed.is_none());
    }

    #[test]
    fn normalize_model_resource_name_accepts_bare_and_prefixed_models() {
        assert_eq!(
            normalize_model_resource_name("gemini-2.5-flash"),
            "models/gemini-2.5-flash"
        );
        assert_eq!(
            normalize_model_resource_name("models/gemini-2.5-pro"),
            "models/gemini-2.5-pro"
        );
    }

    #[test]
    fn missing_model_detection_matches_google_error_text() {
        assert!(is_missing_model_message("Model 'gemini-x' not found"));
        assert!(is_missing_model_message("Unknown model requested"));
        assert!(!is_missing_model_message("API key not valid"));
    }
}
