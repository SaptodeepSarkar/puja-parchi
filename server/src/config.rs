use std::path::PathBuf;

/// All runtime configuration comes from env vars (see `.env.example`).
/// Defaults are chosen so `cargo run` works from the repo with zero setup.
#[derive(Debug, Clone)]
pub struct Config {
    pub addr: String,
    pub tls_addr: String,
    pub tls: bool,
    pub data_dir: PathBuf,
    pub ledger_path: PathBuf,
    pub ledger_sheet: String,
    pub app_dir: PathBuf,
    pub pin: Option<String>,
    pub vision_provider: String,
    pub vision_base_url: String,
    pub vision_api_key: Option<String>,
    pub vision_model: String,
    /// Speech can live somewhere else than vision: a local Whisper server while
    /// the reading model runs on a different port, or a different vendor.
    pub stt_base_url: String,
    pub stt_api_key: Option<String>,
    pub stt_model: String,
    /// Empty means let the model detect the language. Forcing "hi" makes
    /// Whisper answer in Devanagari, which the number parser cannot read.
    pub stt_language: Option<String>,
}

fn var(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = PathBuf::from(var("PUJA_DATA_DIR").unwrap_or_else(|| "data".into()));
        let ledger_path = var("PUJA_LEDGER_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("ledger/Durga Puja 2026.xlsx"));
        let vision_base_url = var("PUJA_VISION_BASE_URL")
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        let vision_api_key = var("PUJA_VISION_API_KEY");
        Self {
            addr: var("PUJA_ADDR").unwrap_or_else(|| "0.0.0.0:8080".into()),
            tls_addr: var("PUJA_TLS_ADDR").unwrap_or_else(|| "0.0.0.0:8443".into()),
            tls: var("PUJA_TLS").map(|v| v != "off").unwrap_or(true),
            app_dir: PathBuf::from(var("PUJA_APP_DIR").unwrap_or_else(|| "app/dist".into())),
            ledger_sheet: var("PUJA_LEDGER_SHEET").unwrap_or_else(|| "CASH ".into()),
            pin: var("PUJA_PIN"),
            vision_provider: var("PUJA_VISION_PROVIDER")
                .unwrap_or_else(|| "none".into())
                .to_lowercase(),
            stt_base_url: var("PUJA_STT_BASE_URL").unwrap_or_else(|| vision_base_url.clone()),
            stt_api_key: var("PUJA_STT_API_KEY").or_else(|| vision_api_key.clone()),
            vision_base_url,
            vision_api_key,
            vision_model: var("PUJA_VISION_MODEL").unwrap_or_else(|| "gpt-4o-mini".into()),
            stt_model: var("PUJA_STT_MODEL").unwrap_or_else(|| "whisper-1".into()),
            stt_language: var("PUJA_STT_LANGUAGE"),
            data_dir,
            ledger_path,
        }
    }
}
