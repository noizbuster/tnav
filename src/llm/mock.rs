use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::llm::{LlmError, LlmProvider, StreamSink};

#[derive(Debug, Clone, Default)]
pub struct MockLlmClient {
    responses: Arc<Mutex<VecDeque<Result<String, LlmError>>>>,
    models: Arc<Mutex<Vec<String>>>,
}

impl MockLlmClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_response(&self, response: Result<String, LlmError>) -> Result<(), LlmError> {
        let mut responses = self
            .responses
            .lock()
            .map_err(|error| LlmError::InvalidResponse {
                message: error.to_string(),
            })?;
        responses.push_back(response);
        Ok(())
    }

    pub fn set_models(&self, models: Vec<String>) -> Result<(), LlmError> {
        let mut slot = self
            .models
            .lock()
            .map_err(|error| LlmError::InvalidResponse {
                message: error.to_string(),
            })?;
        *slot = models;
        Ok(())
    }
}

impl LlmProvider for MockLlmClient {
    fn generate_command<'a>(
        &'a self,
        _prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let mut responses =
                self.responses
                    .lock()
                    .map_err(|error| LlmError::InvalidResponse {
                        message: error.to_string(),
                    })?;

            responses.pop_front().unwrap_or_else(|| {
                Err(LlmError::InvalidResponse {
                    message: "no mock LLM response configured".to_owned(),
                })
            })
        })
    }

    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, LlmError>> + Send + 'a>> {
        Box::pin(async move {
            let models = self
                .models
                .lock()
                .map_err(|error| LlmError::InvalidResponse {
                    message: error.to_string(),
                })?;
            Ok(models.clone())
        })
    }

    fn stream_command<'a>(
        &'a self,
        _prompt: &'a str,
        _sink: &'a mut dyn StreamSink,
    ) -> Pin<Box<dyn Future<Output = Result<String, LlmError>> + Send + 'a>> {
        Box::pin(async move { self.generate_command("mock").await })
    }
}
