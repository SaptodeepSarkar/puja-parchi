use serde::{Deserialize, Serialize};

/// Payment mode as written in the ledger.
/// Blank in the book means cash (user rule, 2026-10-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Cash,
    Online,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Cash => "CASH",
            Mode::Online => "ONLINE",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cash" | "nakad" | "नकद" => Some(Mode::Cash),
            "online" | "upi" | "gpay" | "phonepe" | "paytm" | "neft" => Some(Mode::Online),
            _ => None,
        }
    }
}

/// Where a field value came from. Shown in the UI as a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// User spoke it on the 6-second clip.
    Voice,
    /// Big vision model.
    Vision,
    /// Plain OCR text.
    Ocr,
    /// Name gazetteer / spelling correction.
    NameDb,
    /// User typed or corrected it on the review screen.
    #[default]
    Manual,
    /// Rule default (e.g. blank mode => cash).
    Default,
}

/// One editable value plus the evidence behind it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Field {
    pub value: Option<String>,
    pub source: Source,
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Other readings we rejected, so the reviewer can see the disagreement.
    #[serde(default)]
    pub alternates: Vec<String>,
}

impl Field {
    pub fn new(value: impl Into<String>, source: Source, confidence: f32) -> Self {
        Self { value: Some(value.into()), source, confidence, evidence: Vec::new(), alternates: Vec::new() }
    }

    pub fn empty(source: Source) -> Self {
        Self { source, ..Default::default() }
    }

    pub fn with_evidence(mut self, e: impl Into<String>) -> Self {
        self.evidence.push(e.into());
        self
    }

    pub fn text(&self) -> Option<&str> {
        self.value.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// Digits only: "1,100" / "₹1100" / "1100/-" -> 1100. Also accepts 0.
    pub fn number(&self) -> Option<i64> {
        let raw = self.text()?;
        let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        digits.parse().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Committed,
    Rejected,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Committed => "committed",
            Status::Rejected => "rejected",
        }
    }
}

/// A capture awaiting confirmation, or one already pushed to the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub book: i64,
    pub receipt: Field,
    pub name: Field,
    pub amount: Field,
    pub mode: Field,
    pub status: Status,
    /// Verbatim text the vision model read, kept for audit.
    pub raw_text: String,
    /// The vision model's own short analysis / doubts.
    pub vision_note: String,
    /// What the user actually said.
    pub transcript: String,
    pub photo_url: String,
    pub audio_url: Option<String>,
    pub warnings: Vec<String>,
    pub created_at: String,
    pub committed_row: Option<u32>,
}

/// A validated row ready for the spreadsheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    pub book: i64,
    pub receipt: i64,
    pub name: String,
    pub amount: i64,
    pub mode: Mode,
}

/// Excel cannot store control characters (they break `sharedStrings.xml`) and a
/// cell holds at most 32767 characters; a name is neither.
pub fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_string()
}

impl Draft {
    pub fn row(&self) -> Result<LedgerRow, Vec<String>> {
        let mut errs = Vec::new();
        if self.book <= 0 {
            errs.push("book number must be a positive number".into());
        }
        let receipt = match self.receipt.number() {
            Some(r) if (1..=9999).contains(&r) => r,
            _ => {
                errs.push("receipt number is missing or not 1-9999".into());
                0
            }
        };
        let name = clean_name(&self.name.text().unwrap_or_default());
        if name.len() < 2 {
            errs.push("name is empty".into());
        }
        let amount = match self.amount.number() {
            Some(a) if (1..=10_000_000).contains(&a) => a,
            _ => {
                errs.push("amount is missing or not 1 rupee or more".into());
                0
            }
        };
        let mode = self
            .mode
            .text()
            .and_then(Mode::parse)
            .unwrap_or(Mode::Cash);
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(LedgerRow { book: self.book, receipt, name, amount, mode })
    }
}
