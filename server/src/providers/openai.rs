//! One client for OpenAI-compatible chat (vision) and audio transcription.
//! Works with OpenAI, Gemini's `/v1beta/openai` compat endpoint, Groq, etc.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

use super::{ProviderError, Stt, Vision, VisionOut};
use crate::config::Config;
use crate::domain::Mode;

pub struct OpenAi {
    http: reqwest::Client,
    /// Chat endpoint - the vision model.
    base_url: String,
    /// Transcription endpoint. Same as `base_url` unless PUJA_STT_BASE_URL says
    /// otherwise (a local Whisper server beside a local vision model, say).
    stt_base_url: String,
    api_key: Option<String>,
    vision_model: String,
    stt_model: String,
    stt_language: Option<String>,
}

/// A model running on this machine needs no key. Requiring a dummy one made
/// "key missing" a lie on every local setup.
fn needs_no_key(base_url: &str) -> bool {
    let host = base_url
        .split("://")
        .nth(1)
        .unwrap_or(base_url) // no scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    host == "localhost"
        || host == "127.0.0.1"
        || host.starts_with("192.168.")
        || host.starts_with("10.")
        || host.starts_with("172.")
}

impl OpenAi {
    pub fn new(cfg: &Config) -> Self {
        let key = cfg.vision_api_key.clone().or_else(|| cfg.stt_api_key.clone());
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(k) = &key {
            if let Ok(mut v) = reqwest::header::HeaderValue::from_str(&format!("Bearer {k}")) {
                v.set_sensitive(true);
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: cfg.vision_base_url.trim_end_matches('/').to_string(),
            stt_base_url: cfg.stt_base_url.trim_end_matches('/').to_string(),
            api_key: key,
            vision_model: cfg.vision_model.clone(),
            stt_model: cfg.stt_model.clone(),
            stt_language: cfg.stt_language.clone(),
        }
    }

    fn ready(&self, base_url: &str) -> Result<(), ProviderError> {
        if self.api_key.is_none() && !needs_no_key(base_url) {
            return Err(ProviderError::NotConfigured);
        }
        Ok(())
    }
}

const PROMPT: &str = r#"You read written receipt stubs from an Indian Durga Puja donation book.
Return ONLY a JSON object with these keys:
  raw_text  - every line of text you can read, verbatim, newline separated
  name      - the donor's name exactly as written, UPPERCASE; null if unreadable
  receipt   - the receipt / stub number as digits only; null if unreadable
  amount    - rupees as an integer, no commas or currency sign; null if unreadable
  mode      - "online" if the stub says online / UPI / GPay / PhonePe / Paytm / transfer,
              "cash" if it says cash; null if nothing is written
  note      - one short sentence on what is unclear, or ""
  confidence- 0.0 to 1.0, how sure you are of name+receipt+amount together
Rules: never invent a value. Blank on the receipt means cash for this book,
but only report "cash" if the word cash is written; otherwise null.
The book number is chosen by the phone, do not guess it."#;

fn as_opt_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let s = s.trim();
            (!s.is_empty() && s != "null").then(|| s.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse the model's JSON answer defensively: models sometimes wrap it in prose
/// or a fenced block.
pub fn parse_vision_json(body: &str) -> Result<VisionOut, ProviderError> {
    let start = body.find('{').ok_or_else(|| ProviderError::BadResponse(body.chars().take(200).collect()))?;
    let end = body.rfind('}').ok_or_else(|| ProviderError::BadResponse(body.chars().take(200).collect()))?;
    if end < start {
        return Err(ProviderError::BadResponse(body.into()));
    }
    let v: Value = serde_json::from_str(&body[start..=end])
        .map_err(|e| ProviderError::BadResponse(format!("{e}: {}", &body[start..].chars().take(200).collect::<String>())))?;

    let mode = v.get("mode").and_then(as_opt_string).and_then(|m| Mode::parse(&m));
    Ok(VisionOut {
        raw_text: v.get("raw_text").and_then(as_opt_string).unwrap_or_default(),
        receipt: v.get("receipt").and_then(as_opt_string),
        name: v.get("name").and_then(as_opt_string),
        amount: v.get("amount").and_then(as_opt_string),
        mode,
        note: v.get("note").and_then(as_opt_string).unwrap_or_default(),
        confidence: v
            .get("confidence")
            .and_then(Value::as_f64)
            .map(|c| (c as f32).clamp(0.0, 1.0))
            .unwrap_or(0.3),
    })
}

#[async_trait]
impl Vision for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn analyze(&self, image: &[u8], mime: &str) -> Result<VisionOut, ProviderError> {
        self.ready(&self.base_url)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(image);
        let body = json!({
            "model": self.vision_model,
            "temperature": 0,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": PROMPT },
                { "role": "user", "content": [
                    { "type": "text", "text": "Read this receipt stub." },
                    { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{b64}") } }
                ]}
            ]
        });
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(ProviderError::Http(format!("{}: {}", status, text.chars().take(300).collect::<String>())));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ProviderError::BadResponse("no choices[0].message.content".into()))?;
        parse_vision_json(content)
    }
}

#[async_trait]
impl Stt for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn transcribe(&self, audio: &[u8], filename: &str) -> Result<String, ProviderError> {
        self.ready(&self.stt_base_url)?;
        // A WAV is what a native recorder produces; a webm is what the browser
        // produced. Model servers pick their decoder from the name, so say it.
        let wav = audio.len() > 12 && &audio[0..4] == b"RIFF";
        let (name, kind) = if wav { ("voice.wav", "audio/wav") } else { (filename, "audio/webm") };
        let part = reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name(name.to_string())
            .mime_str(kind)
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", self.stt_model.clone())
            .text("response_format", "json");
        // Only force a language when asked: Whisper auto-detects otherwise, and
        // forcing Hindi answers in Devanagari, which the number parser cannot read.
        if let Some(lang) = &self.stt_language {
            form = form.text("language", lang.clone());
        }
        let resp = self
            .http
            .post(format!("{}/audio/transcriptions", self.stt_base_url))
            .multipart(form)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(ProviderError::Http(format!("{}: {}", status, text.chars().take(300).collect::<String>())));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json() {
        let body = "```json\n{\"raw_text\":\"R/NO 12\",\"name\":\"RAM KUMAR\",\"receipt\":\"12\",\"amount\":501,\"mode\":\"cash\",\"note\":\"\",\"confidence\":0.9}\n```";
        let out = parse_vision_json(body).unwrap();
        assert_eq!(out.name.as_deref(), Some("RAM KUMAR"));
        assert_eq!(out.amount.as_deref(), Some("501"));
        assert_eq!(out.mode, Some(Mode::Cash));
    }

    #[test]
    fn nulls_become_none() {
        let body = r#"{"raw_text":"","name":null,"receipt":"null","amount":null,"mode":null,"note":"","confidence":0}"#;
        let out = parse_vision_json(body).unwrap();
        assert!(out.name.is_none());
        assert!(out.receipt.is_none());
        assert!(out.amount.is_none());
        assert!(out.mode.is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_vision_json("no json here").is_err());
    }

    #[test]
    fn local_endpoints_need_no_key() {
        assert!(needs_no_key("http://127.0.0.1:11434/v1"));
        assert!(needs_no_key("127.0.0.1:8081/v1"), "no scheme");
        assert!(needs_no_key("http://localhost:11434/v1"));
        assert!(needs_no_key("http://192.168.0.110:8081/v1"));
        assert!(!needs_no_key("https://api.openai.com/v1"));
        assert!(!needs_no_key("https://openrouter.ai/api/v1"));
    }
}
