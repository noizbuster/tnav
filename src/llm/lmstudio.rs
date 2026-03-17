use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::llm::{
    ConfiguredProvider, LLM_SYSTEM_PROMPT, LlmError, LlmProvider, Provider, StreamSink,
    strip_markdown_fences,
};
use crate::secrets::{KeyringSecretStore, SecretKind, SecretStore};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http_client: Client,
    config: ConfiguredProvider,
    secret_store: KeyringSecretStore,
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
            secret_store: KeyringSecretStore::new(),
        })
    }

    fn base_url(&self) -> &str {
        self.config.base_url_or_default().trim_end_matches('/')
    }

    fn api_base_url(&self) -> String {
        if self.config.provider == Provider::OpenAiCompatible && !self.base_url().ends_with("/v1") {
            format!("{}/v1", self.base_url())
        } else {
            self.base_url().to_owned()
        }
    }

    fn endpoint_url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base_url(), path.trim_start_matches('/'))
    }

    fn api_key(&self) -> Result<Option<String>, LlmError> {
        if !self.config.provider.uses_api_key_storage() {
            return Ok(None);
        }

        if let Some(api_key) = self
            .config
            .api_key
            .as_deref()
            .filter(|api_key| !api_key.trim().is_empty())
        {
            return Ok(Some(api_key.to_owned()));
        }

        let secret_key = self.config.secret_profile_key();
        tracing::debug!(
            secret_key = %secret_key,
            provider_name = %self.config.name,
            "Loading API key from keyring"
        );
        match self
            .secret_store
            .load_secret(&secret_key, SecretKind::ApiKey)
        {
            Ok(Some(key)) => {
                tracing::debug!(secret_key = %secret_key, "API key loaded successfully");
                Ok(Some(key))
            }
            Ok(None) => {
                tracing::error!(secret_key = %secret_key, "API key not found in keyring");
                Err(LlmError::ConfigMissing {
                    message: format!(
                        "{} API key is not configured in secure storage",
                        self.config.provider.display_name()
                    ),
                })
            }
            Err(error) => {
                tracing::error!(secret_key = %secret_key, error = %error, "Failed to load API key from keyring");
                Err(LlmError::AuthFailed {
                    message: error.to_string(),
                })
            }
        }
    }

    fn maybe_bearer_auth(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, LlmError> {
        Ok(match self.api_key()? {
            Some(api_key) => request.bearer_auth(api_key),
            None => request,
        })
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
                .maybe_bearer_auth(self.http_client.post(self.endpoint_url("chat/completions")))?
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: format!(
                        "{} rejected the configured API key",
                        self.config.provider.display_name()
                    ),
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
                .maybe_bearer_auth(self.http_client.get(self.endpoint_url("models")))?
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: format!(
                        "{} rejected the configured API key",
                        self.config.provider.display_name()
                    ),
                });
            }

            let response =
                response
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
                .maybe_bearer_auth(self.http_client.post(self.endpoint_url("chat/completions")))?
                .json(&request)
                .send()
                .await
                .map_err(|error| LlmError::ConnectionFailed {
                    message: error.to_string(),
                })?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(LlmError::AuthFailed {
                    message: format!(
                        "{} rejected the configured API key",
                        self.config.provider.display_name()
                    ),
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use crate::llm::{
        ConfiguredProvider, DEFAULT_PROVIDER_TIMEOUT_SECS, LlmProvider, Provider, StreamSink,
    };

    use super::{OpenAiCompatibleClient, parse_sse_data_line};

    type CapturedHeaders = Vec<(String, String)>;
    type CapturedRequest = (String, CapturedHeaders, String);
    type SingleRequestServerHandle = thread::JoinHandle<CapturedRequest>;

    fn provider_config(
        provider: Provider,
        base_url: String,
        api_key: Option<&str>,
    ) -> ConfiguredProvider {
        ConfiguredProvider {
            name: format!("{}-test", provider.value()),
            provider,
            model: "test-model".to_owned(),
            base_url: Some(base_url),
            api_key: api_key.map(str::to_owned),
            timeout_secs: DEFAULT_PROVIDER_TIMEOUT_SECS,
        }
    }

    fn spawn_single_request_server(response: &'static str) -> (String, SingleRequestServerHandle) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test server");
        let address = listener.local_addr().expect("read local addr");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 1024];

            loop {
                let read = stream.read(&mut buffer).expect("read request bytes");
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
                if bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                    break;
                }
            }

            let header_end = bytes
                .windows(4)
                .position(|chunk| chunk == b"\r\n\r\n")
                .map(|index| index + 4)
                .expect("request contains headers");
            let header_text = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        (name.trim().eq_ignore_ascii_case("content-length"))
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);

            while bytes.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read request body bytes");
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
            }

            let request = String::from_utf8_lossy(&bytes).into_owned();
            let (head, body) = request
                .split_once("\r\n\r\n")
                .expect("request contains headers");
            let mut lines = head.lines();
            let request_line = lines.next().expect("request line present").to_owned();
            let headers = lines
                .filter_map(|line| {
                    line.split_once(':').map(|(name, value)| {
                        (name.trim().to_ascii_lowercase(), value.trim().to_owned())
                    })
                })
                .collect::<Vec<_>>();

            stream
                .write_all(response.as_bytes())
                .expect("write response");

            (request_line, headers, body.to_owned())
        });

        (format!("http://{}", address), handle)
    }

    fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
    }

    struct CollectingSink {
        chunks: String,
    }

    impl CollectingSink {
        fn new() -> Self {
            Self {
                chunks: String::new(),
            }
        }
    }

    impl StreamSink for CollectingSink {
        fn on_chunk(&mut self, chunk: &str) {
            self.chunks.push_str(chunk);
        }
    }

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

    #[tokio::test]
    async fn local_openai_compatible_list_models_uses_v1_without_auth() {
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-type: application/json\r\n",
            "content-length: 31\r\n",
            "connection: close\r\n",
            "\r\n",
            "{\"data\":[{\"id\":\"local-model\"}]}"
        );
        let (base_url, handle) = spawn_single_request_server(response);
        let client = OpenAiCompatibleClient::new(provider_config(
            Provider::OpenAiCompatible,
            base_url,
            None,
        ))
        .expect("build client");

        let models = client.list_models().await.expect("list models succeeds");
        let (request_line, headers, _) = handle.join().expect("join server thread");

        assert_eq!(models, vec!["local-model".to_owned()]);
        assert_eq!(request_line, "GET /v1/models HTTP/1.1");
        assert_eq!(header_value(&headers, "authorization"), None);
    }

    #[tokio::test]
    async fn remote_openai_compatible_list_models_uses_base_root_and_bearer_auth() {
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-type: application/json\r\n",
            "content-length: 32\r\n",
            "connection: close\r\n",
            "\r\n",
            "{\"data\":[{\"id\":\"remote-model\"}]}"
        );
        let (server_url, handle) = spawn_single_request_server(response);
        let client = OpenAiCompatibleClient::new(provider_config(
            Provider::ZaiCodingPlanGlobal,
            format!("{server_url}/api/coding/paas/v4"),
            Some("secret-key"),
        ))
        .expect("build client");

        let models = client.list_models().await.expect("list models succeeds");
        let (request_line, headers, _) = handle.join().expect("join server thread");

        assert_eq!(models, vec!["remote-model".to_owned()]);
        assert_eq!(request_line, "GET /api/coding/paas/v4/models HTTP/1.1");
        assert_eq!(
            header_value(&headers, "authorization"),
            Some("Bearer secret-key")
        );
    }

    #[tokio::test]
    async fn remote_openai_compatible_chat_uses_base_root_and_bearer_auth() {
        let body = "{\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"echo hi\"}}]}";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked_response: &'static str = Box::leak(response.into_boxed_str());
        let (server_url, handle) = spawn_single_request_server(leaked_response);
        let client = OpenAiCompatibleClient::new(provider_config(
            Provider::ZaiCodingPlanGlobal,
            format!("{server_url}/api/coding/paas/v4"),
            Some("secret-key"),
        ))
        .expect("build client");

        let command = client
            .generate_command("say hi")
            .await
            .expect("generate command succeeds");
        let (request_line, headers, body) = handle.join().expect("join server thread");

        assert_eq!(command, "echo hi");
        assert_eq!(
            request_line,
            "POST /api/coding/paas/v4/chat/completions HTTP/1.1"
        );
        assert_eq!(
            header_value(&headers, "authorization"),
            Some("Bearer secret-key")
        );
        assert!(body.contains("\"model\":\"test-model\""));
        assert!(body.contains("\"stream\":false"));
    }

    #[tokio::test]
    async fn remote_openai_compatible_stream_uses_base_root_and_bearer_auth() {
        let body =
            "data: {\"choices\":[{\"delta\":{\"content\":\"echo streamed\"}}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked_response: &'static str = Box::leak(response.into_boxed_str());
        let (server_url, handle) = spawn_single_request_server(leaked_response);
        let client = OpenAiCompatibleClient::new(provider_config(
            Provider::ZaiCodingPlanGlobal,
            format!("{server_url}/api/coding/paas/v4"),
            Some("secret-key"),
        ))
        .expect("build client");
        let mut sink = CollectingSink::new();

        let command = client
            .stream_command("say hi", &mut sink)
            .await
            .expect("stream command succeeds");
        let (request_line, headers, body) = handle.join().expect("join server thread");

        assert_eq!(command, "echo streamed");
        assert_eq!(sink.chunks, "echo streamed");
        assert_eq!(
            request_line,
            "POST /api/coding/paas/v4/chat/completions HTTP/1.1"
        );
        assert_eq!(
            header_value(&headers, "authorization"),
            Some("Bearer secret-key")
        );
        assert!(body.contains("\"stream\":true"));
    }
}
