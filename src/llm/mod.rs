mod config;
mod error;
mod lmstudio;
mod mock;
mod ollama;
mod openai;
mod provider;

pub use config::{ConfiguredProvider, LlmConfig, Provider};
pub use error::{LlmError, LlmResult};
pub use lmstudio::OpenAiCompatibleClient;
pub use mock::MockLlmClient;
pub use ollama::{OllamaClient, strip_markdown_fences};
pub use openai::OpenAiClient;
pub use provider::{LLM_SYSTEM_PROMPT, LlmProvider, StreamSink};
