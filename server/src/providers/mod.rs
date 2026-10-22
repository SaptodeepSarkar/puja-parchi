//! Vision + speech-to-text providers. Everything speaks the OpenAI-compatible
//! protocol, so OpenAI, Gemini's compat endpoint, Groq, Together and a local
//! llama.cpp / vLLM all work by changing PUJA_VISION_BASE_URL.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::Config;
use crate::domain::Mode;

pub mod openai;

#[derive(Debug, Clone, Default)]
pub struct VisionOut {
    /// Verbatim text the model could read off the receipt.
    pub raw_text: String,
    pub receipt: Option<String>,
    pub name: Option<String>,
    pub amount: Option<String>,
    pub mode: Option<Mode>,
    /// The model's own comments: doubts, damaged print, handwriting.
    pub note: String,
    pub confidence: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("no vision provider configured (set PUJA_VISION_PROVIDER=openai and a key)")]
    NotConfigured,
    #[error("provider request failed: {0}")]
    Http(String),
    #[error("provider returned unreadable JSON: {0}")]
    BadResponse(String),
}

#[async_trait]
pub trait Vision: Send + Sync {
    fn name(&self) -> &'static str;
    async fn analyze(&self, image: &[u8], mime: &str) -> Result<VisionOut, ProviderError>;
}

#[async_trait]
pub trait Stt: Send + Sync {
    fn name(&self) -> &'static str;
    async fn transcribe(&self, audio: &[u8], filename: &str) -> Result<String, ProviderError>;
}

/// Used when no model is configured: the pipeline simply has no vision evidence
/// and the spoken fields carry the capture. This is a supported mode, not a bug.
pub struct NoVision;
pub struct NoStt;

#[async_trait]
impl Vision for NoVision {
    fn name(&self) -> &'static str {
        "none"
    }
    async fn analyze(&self, _: &[u8], _: &str) -> Result<VisionOut, ProviderError> {
        Err(ProviderError::NotConfigured)
    }
}

#[async_trait]
impl Stt for NoStt {
    fn name(&self) -> &'static str {
        "none"
    }
    async fn transcribe(&self, _: &[u8], _: &str) -> Result<String, ProviderError> {
        Err(ProviderError::NotConfigured)
    }
}

/// Deterministic stand-in for tests and demos: `PUJA_VISION_PROVIDER=fake`.
pub struct FakeVision;
pub struct FakeStt;

#[async_trait]
impl Vision for FakeVision {
    fn name(&self) -> &'static str {
        "fake"
    }
    async fn analyze(&self, _: &[u8], _: &str) -> Result<VisionOut, ProviderError> {
        Ok(VisionOut {
            raw_text: "BOOK 18  R/NO 9501  SUDDHA DAIRY  Rs 1100  CASH".into(),
            receipt: Some("9501".into()),
            name: Some("SUDDHA DAIRY".into()),
            amount: Some("1100".into()),
            mode: Some(Mode::Cash),
            note: "demo provider, fixed output".into(),
            confidence: 0.8,
        })
    }
}

#[async_trait]
impl Stt for FakeStt {
    fn name(&self) -> &'static str {
        "fake"
    }
    async fn transcribe(&self, _: &[u8], _: &str) -> Result<String, ProviderError> {
        Ok("name suddha dairy receipt number nine five zero one eleven hundred cash".into())
    }
}

pub fn vision_from_config(cfg: &Config) -> Arc<dyn Vision> {
    match cfg.vision_provider.as_str() {
        "openai" => Arc::new(openai::OpenAi::new(cfg)),
        "fake" => Arc::new(FakeVision),
        "none" | "" => Arc::new(NoVision),
        other => {
            tracing::warn!("unknown PUJA_VISION_PROVIDER={other}, falling back to none");
            Arc::new(NoVision)
        }
    }
}

pub fn stt_from_config(cfg: &Config) -> Arc<dyn Stt> {
    match cfg.vision_provider.as_str() {
        "openai" => Arc::new(openai::OpenAi::new(cfg)),
        "fake" => Arc::new(FakeStt),
        _ => Arc::new(NoStt),
    }
}

/// Is anything answering at this OpenAI-compatible base URL?
///
/// Any HTTP status counts: ollama answers 200 for `/models`, whisper.cpp answers
/// 404 because it has no such route. Both mean the model server is up, which is
/// the only thing this asks.
pub async fn probe(base_url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(format!("{}/models", base_url.trim_end_matches('/')))
        .send()
        .await
        .is_ok()
}
