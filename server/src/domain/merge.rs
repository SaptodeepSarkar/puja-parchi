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

/// The amount the photo shows, as a plain rupee figure.
///
/// Models answer with a currency sign, a decimal, or words - the real stub
/// carries both "Two hundred one" and "Rs 201" - so the voice parser reads this
/// text too. Anything that is not a plausible rupee amount is dropped here
/// rather than trusted with the money column.
fn vision_amount(vision: Option<&VisionOut>) -> Option<String> {
    let raw = vision.and_then(|v| v.amount.as_deref())?.trim();
    if raw.is_empty() {
        return None;
    }
    let digits = speech::parse(raw).amount.unwrap_or_else(|| raw.to_string());
    let n = digits.parse::<i64>().ok().or_else(|| {
        digits
            .parse::<f64>()
            .ok()
            .filter(|f| f.fract() == 0.0)
            .map(|f| f as i64)
    })?;
    (1..=99_999).contains(&n).then(|| n.to_string())
}

/// The same money however it was written: "201" and "201.00" are one amount.
fn same_number(a: &str, b: &str) -> bool {
    match (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
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

    // Amount. The written stub leads here, unlike every other field: the donor
    // wrote that number, it is what the book records, and the model is reading
    // the same paper - so a number said out of order (a receipt number, say)
    // cannot become the money. The one exception is a photo the model itself
    // was unsure of, where the volunteer's own reading is the better bet.
    // Whichever loses is kept as an alternate and the disagreement is reported.
    let read = vision_amount(vision);
    let sure = vision.is_some_and(|v| v.confidence >= 0.5);
    let voice = spoken.amount.clone();
    // The photo leads unless the model was unsure of its own reading and the
    // volunteer said something.
    let photo_leads = match (&read, &voice) {
        (Some(_), None) => true,
        (Some(_), Some(_)) => sure,
        (None, _) => false,
    };
    m.amount = if photo_leads {
        // photo_leads is only true with a reading in hand; an empty string here
        // would fall back to typing rather than panic mid-request.
        let v = read.clone().unwrap_or_default();
        let mut f = Field::new(v.clone(), Source::Vision, vision.map(|v| v.confidence).unwrap_or(0.3))
            .with_evidence("read by the vision model");
        match &voice {
            Some(a) if same_number(&v, a) => {
                f.evidence.push("your voice note agrees".into());
            }
            Some(a) => {
                f.alternates.push(format!("voice: {a}"));
                m.warnings.push(format!(
                    "the photo reads Rs {v}, you said Rs {a} - using the photo{}",
                    if sure { "" } else { ", which the model was unsure about" }
                ));
            }
            None => {}
        }
        f
    } else if let Some(a) = voice {
        let mut f = Field::new(a.clone(), Source::Voice, 0.95).with_evidence("heard in your voice note");
        if let Some(v) = read {
            f.alternates.push(format!("vision: {v}"));
            m.warnings.push(format!(
                "the photo's Rs {v} was an unsure read, you said Rs {a} - using yours"
            ));
        }
        f
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
    fn agreeing_readings_raise_no_warning() {
        let spoken = "name suddha dairy receipt number eight five one eleven hundred cash";
        let m = build(18, Some(&vision()), spoken);
        assert_eq!(m.receipt.number(), Some(851));
        assert_eq!(m.amount.number(), Some(1100));
        assert_eq!(m.mode.text(), Some("CASH"));
        assert_eq!(m.name.text(), Some("SUDHA DAIRY"), "gazetteer repairs SUDDHA");
        assert!(m.warnings.is_empty(), "both readings agree: {:?}", m.warnings);
        assert!(m.amount.evidence.iter().any(|e| e.contains("voice note agrees")));
    }

    #[test]
    fn the_photo_decides_the_amount_when_they_disagree() {
        let mut v = vision();
        v.amount = Some("700".into());
        let m = build(18, Some(&v), "receipt number eight five one eleven hundred");
        assert_eq!(m.amount.number(), Some(700), "the written stub wins");
        assert_eq!(m.amount.source, Source::Vision);
        assert!(m.amount.alternates.iter().any(|a| a.contains("1100")), "the voice is kept");
        assert!(m.warnings.iter().any(|w| w.contains("using the photo")));
    }

    #[test]
    fn an_unsure_photo_loses_the_amount_to_the_voice() {
        let mut v = vision();
        v.amount = Some("700".into());
        v.confidence = 0.4;
        let m = build(18, Some(&v), "receipt number eight five one eleven hundred");
        assert_eq!(m.amount.number(), Some(1100));
        assert_eq!(m.amount.source, Source::Voice);
        assert!(m.amount.alternates.iter().any(|a| a.contains("700")));
        assert!(m.warnings.iter().any(|w| w.contains("unsure read")));
    }

    #[test]
    fn a_photo_amount_in_words_is_read_too() {
        // The real stub says "the sum of rupees Two hundred one" as well as Rs 201.
        let mut v = vision();
        v.amount = Some("Two hundred one".into());
        let m = build(18, Some(&v), "");
        assert_eq!(m.amount.number(), Some(201));
        assert_eq!(m.amount.source, Source::Vision);
    }

    #[test]
    fn a_worthless_photo_amount_never_reaches_the_money_column() {
        for junk in ["", "0", "100000", "not a number", "Rs"] {
            let mut v = vision();
            v.amount = Some(junk.into());
            let m = build(18, Some(&v), "receipt number eight five one eleven hundred");
            assert_eq!(m.amount.number(), Some(1100), "junk {junk:?} must not win");
            assert_eq!(m.amount.source, Source::Voice);
        }
    }

    #[test]
    fn a_decimal_photo_amount_is_still_a_rupee_figure() {
        let mut v = vision();
        v.amount = Some("201.00".into());
        let m = build(18, Some(&v), "");
        assert_eq!(m.amount.number(), Some(201));
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
