use thiserror::Error;

pub type LlmResult<T> = Result<T, LlmError>;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("failed to connect to the configured LLM provider: {message}")]
    ConnectionFailed { message: String },

    #[error("LLM request timed out")]
    Timeout,

    #[error("received an invalid LLM response: {message}")]
    InvalidResponse { message: String },

    #[error("LLM provider is currently rate limited")]
    RateLimited,

    #[error("LLM authentication failed: {message}")]
    AuthFailed { message: String },

    #[error("LLM model '{model}' was not found")]
    ModelNotFound { model: String },

    #[error("LLM configuration is missing: {message}")]
    ConfigMissing { message: String },
}
