//! Arbitrate between what the volunteer said, what the vision model read, and
//! what the name gazetteer knows. One value per field, plus the evidence that
//! produced it, so the review screen can show its work.

use super::names::{self, Normalized};
use super::receipt::{Field, Mode, Source};
use super::speech;
use crate::providers::VisionOut;

#[derive(Debug, Clone, Default)]
pub struct Merged {
    pub receipt: Field,
    pub name: Field,
    pub amount: Field,
    pub mode: Field,
    pub warnings: Vec<String>,
    pub raw_text: String,
    pub vision_note: String,
}

fn name_field(raw: &str, source: Source, base_conf: f32) -> (Field, Normalized) {
    let n = names::normalize(raw);
    let mut f = Field::new(n.name.clone(), source, (base_conf * n.confidence).clamp(0.0, 1.0));
    for fix in &n.fixes {
        f.evidence.push(fix.clone());
    }
    if n.confidence < 0.6 {
        f.evidence.push("no close gazetteer match, kept as heard".into());
    }
    (f, n)
}

fn same_name(a: &str, b: &str) -> bool {
    names::normalize(a).name == names::normalize(b).name
}

/// Voice beats vision: the volunteer is reading the actual stub. Vision is the
/// second opinion, OCR text is kept only as audit evidence.
pub fn build(book: i64, vision: Option<&VisionOut>, transcript: &str) -> Merged {
    let _ = book;
    let mut spoken = speech::parse(transcript);
    let mut m = Merged {
        raw_text: vision.map(|v| v.raw_text.clone()).unwrap_or_default(),
        vision_note: vision.map(|v| v.note.clone()).unwrap_or_default(),
        ..Default::default()
    };

    // One bare number with no "receipt" cue: speech calls it the amount. If the
    // photo shows that same number as the receipt, the volunteer is confirming
    // the receipt - keeping it as the amount would put the receipt number in the
    // money column.
    if spoken.receipt.is_none() {
        let read_receipt = vision.and_then(|v| v.receipt.as_deref());
        let read_amount = vision.and_then(|v| v.amount.as_deref());
        if let (Some(vr), Some(va)) = (read_receipt, read_amount) {
            if spoken.amount.as_deref() == Some(vr.trim()) && va.trim() != vr.trim() {
                let said = spoken.amount.take().unwrap_or_default();
                m.warnings.push(format!(
                    "you said {said}, which the photo shows as the receipt - the money column uses the photo's Rs {}",
                    va.trim()
                ));
                spoken.receipt = Some(said);
            }
        }
    }

    match vision {
        None => m.warnings.push("no vision model configured: fields come from your voice".into()),
        Some(v) if v.confidence < 0.5 => m.warnings.push(format!(
            "vision model was unsure ({:.0}%){}",
            v.confidence * 100.0,
            if v.note.is_empty() { String::new() } else { format!(": {}", v.note) }
        )),
        Some(_) => {}
    }

    // Receipt number
    m.receipt = if let Some(r) = &spoken.receipt {
        let mut f = Field::new(r, Source::Voice, 0.95).with_evidence("heard in your voice note");
        if let Some(v) = vision.and_then(|v| v.receipt.as_ref()) {
            if v != r {
                f.alternates.push(format!("vision: {v}"));
                m.warnings.push(format!("vision read receipt {v}, you said {r} - using yours"));
            }
        }
        f
    } else if let Some(v) = vision.and_then(|v| v.receipt.clone()) {
        Field::new(v, Source::Vision, vision.map(|v| v.confidence).unwrap_or(0.3))
            .with_evidence("read by the vision model")
    } else {
        Field::empty(Source::Manual)
    };

    // Name
    m.name = if let Some(n) = &spoken.name {
        let (mut f, norm) = name_field(n, Source::Voice, 0.95);
        f.evidence.insert(0, "heard in your voice note".into());
        if norm.confidence < 0.99 {
            f.source = Source::NameDb;
        }
        if let Some(v) = vision.and_then(|v| v.name.as_ref()) {
            if !same_name(v, &norm.name) {
                f.alternates.push(format!("vision: {v}"));
                m.warnings.push(format!("vision read the name as {v}"));
            }
        }
        f
    } else if let Some(v) = vision.and_then(|v| v.name.clone()) {
        let (f, _) = name_field(&v, Source::Vision, vision.map(|v| v.confidence).unwrap_or(0.3));
        f
    } else {
        Field::empty(Source::Manual)
    };

    // Amount
    m.amount = if let Some(a) = &spoken.amount {
        let mut f = Field::new(a, Source::Voice, 0.95).with_evidence("heard in your voice note");
        if let Some(v) = vision.and_then(|v| v.amount.as_ref()) {
            if v.trim() != a.trim() {
                f.alternates.push(format!("vision: {v}"));
                m.warnings.push(format!("vision read Rs {v}, you said Rs {a} - using yours"));
            }
        }
        f
    } else if let Some(v) = vision.and_then(|v| v.amount.clone()) {
        Field::new(v, Source::Vision, vision.map(|v| v.confidence).unwrap_or(0.3))
            .with_evidence("read by the vision model")
    } else {
        Field::empty(Source::Manual)
    };

    // Mode: blank on the stub means cash.
    m.mode = match (spoken.mode, vision.and_then(|v| v.mode)) {
        (Some(Mode::Online), _) => Field::new("ONLINE", Source::Voice, 0.95).with_evidence("you said online"),
        (Some(Mode::Cash), _) => Field::new("CASH", Source::Voice, 0.95).with_evidence("you said cash"),
        (None, Some(Mode::Online)) => {
            Field::new("ONLINE", Source::Vision, 0.8).with_evidence("vision saw an online / UPI mark")
        }
        (None, Some(Mode::Cash)) => Field::new("CASH", Source::Vision, 0.8).with_evidence("vision saw cash written"),
        (None, None) => Field::new("CASH", Source::Default, 0.6)
            .with_evidence("nothing written on the stub, so cash"),
    };
    if let Some(v) = vision {
        if let (Some(Mode::Online), true) = (spoken.mode, v.mode == Some(Mode::Cash)) {
            m.warnings.push("you said online, vision saw cash - check the mode".into());
        }
    }

    if m.name.text().is_none() {
        m.warnings.push("name is missing - type it before pushing".into());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vision() -> VisionOut {
        VisionOut {
            raw_text: "R/NO 851 SUDDHA DAIRY RS 1100 CASH".into(),
            receipt: Some("851".into()),
            name: Some("SUDDHA DAIRY".into()),
            amount: Some("1100".into()),
            mode: Some(Mode::Cash),
            note: String::new(),
            confidence: 0.8,
        }
    }

    #[test]
    fn voice_wins_and_conflict_is_reported() {
        let spoken = "name suddha dairy receipt number eight five one eleven hundred cash";
        let m = build(18, Some(&vision()), spoken);
        assert_eq!(m.receipt.number(), Some(851));
        assert_eq!(m.amount.number(), Some(1100));
        assert_eq!(m.mode.text(), Some("CASH"));
        assert_eq!(m.name.text(), Some("SUDHA DAIRY"), "gazetteer repairs SUDDHA");
        assert!(m.warnings.iter().all(|w| !w.contains("receipt 851")), "no false conflict");
    }

    #[test]
    fn vision_only_still_produces_a_draft() {
        let m = build(18, Some(&vision()), "");
        assert_eq!(m.receipt.number(), Some(851));
        assert_eq!(m.amount.number(), Some(1100));
    }

    #[test]
    fn no_vision_and_no_voice_means_manual() {
        let m = build(18, None, "");
        assert!(m.receipt.text().is_none());
        assert_eq!(m.mode.text(), Some("CASH"), "default mode is cash");
        assert!(m.warnings.iter().any(|w| w.contains("no vision model")));
    }

    #[test]
    fn voice_conflict_names_the_other_reading() {
        let m = build(18, Some(&vision()), "receipt 852 1100");
        assert_eq!(m.receipt.number(), Some(852));
        assert!(m.receipt.alternates.iter().any(|a| a.contains("851")));
        assert!(m.warnings.iter().any(|w| w.contains("vision read receipt 851")));
    }

    #[test]
    fn lone_number_matching_the_read_receipt_is_not_the_amount() {
        // Heard "Atharvasingh 866." on a stub whose receipt is 866 and money Rs 201.
        let mut v = vision();
        v.receipt = Some("866".into());
        v.name = Some("ATHARV SINGH".into());
        v.amount = Some("201".into());
        let m = build(18, Some(&v), "Atharvasingh 866.");
        assert_eq!(m.receipt.number(), Some(866));
        assert_eq!(m.amount.number(), Some(201), "the receipt number must not land in the money column");
        assert_eq!(m.amount.source, Source::Vision);
        assert!(m.warnings.iter().any(|w| w.contains("shows as the receipt")));
    }
}
