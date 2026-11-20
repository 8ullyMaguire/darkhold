//! M64-14 — Ollama's OpenAI-compatible embeddings endpoint.
//!
//! The adapter owns no ranking policy and no retry: a configured-but-unreachable
//! endpoint is simply unavailable, and a failed or malformed response is an
//! `EmbeddingError` that the domain layer turns back into the caller's order.

use std::future::Future;
use std::pin::Pin;

use darkhold_domain::embeddings::{EmbeddingError, EmbeddingInput, EmbeddingProvider};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct OllamaEmbeddings {
    endpoint: String,
    model: String,
}

impl OllamaEmbeddings {
    #[must_use]
    pub fn new(endpoint: String, model: String) -> Self {
        Self { endpoint, model }
    }

    /// Optional configuration from the process environment. No credential is
    /// held here: a local Ollama endpoint is an address, not a secret.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let endpoint = std::env::var("DARKHOLD_OLLAMA_URL").ok()?;
        let model = std::env::var("DARKHOLD_OLLAMA_MODEL").ok()?;
        Some(Self::new(endpoint, model))
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

impl EmbeddingProvider for OllamaEmbeddings {
    fn available(&self) -> bool {
        !self.model.trim().is_empty()
            && (self.endpoint.starts_with("http://") || self.endpoint.starts_with("https://"))
    }

    fn embed<'a>(
        &'a self,
        input: &'a EmbeddingInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>, EmbeddingError>> + Send + 'a>> {
        Box::pin(async move {
            if !self.available() {
                return Err(EmbeddingError::Unavailable);
            }
            let response = reqwest::Client::new()
                .post(&self.endpoint)
                .json(&serde_json::json!({
                    "model": self.model,
                    "input": input.text,
                }))
                .send()
                .await
                .map_err(|_| EmbeddingError::Unavailable)?;
            if !response.status().is_success() {
                return Err(EmbeddingError::Unavailable);
            }
            let body: Value = response
                .json()
                .await
                .map_err(|_| EmbeddingError::InvalidVector)?;
            let raw = body
                .get("data")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("embedding"))
                .and_then(Value::as_array)
                .ok_or(EmbeddingError::InvalidVector)?;
            let vector: Vec<f32> = raw
                .iter()
                .filter_map(Value::as_f64)
                .map(|value| value as f32)
                .collect();
            if vector.is_empty()
                || vector.len() != raw.len()
                || vector.iter().any(|value| !value.is_finite())
            {
                return Err(EmbeddingError::InvalidVector);
            }
            Ok(vector)
        })
    }
}
