//! Repair a spoken / OCR'd Indian name against the gazetteer.
//!
//! Contract (do not change the signature):
//! - `normalize` is pure, no I/O, no allocation-heavy work.
//! - Output name is UPPERCASE, single-spaced, no leading/trailing space.
//! - Confidence in 0.0..=1.0. 1.0 = every token was already canonical.
//! - `fixes` holds one human sentence per changed token, e.g.
//!   "SUDDHA -> SUDHA (gazetteer, 0.91)". Never change a token silently.
//! - Single letters ("V", "K") are initials: keep, never "correct".
//! - Tokens with no close gazetteer match are kept as-is (low confidence),
//!   because a real donor may simply be new.
//! - If the whole name is empty after cleaning, return an empty name and 0.0.
//!
//! Matching: Jaro-Winkler (or Levenshtein ratio) >= 0.88 against gazetteer
//! tokens; prefer the highest score, tie-break on shorter canonical length.

use super::gazetteer;

const MIN_SCORE: f32 = 0.88;
const UNKNOWN_SCORE: f32 = 0.45;
const MAX_PHRASE: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Normalized {
    pub name: String,
    pub confidence: f32,
    pub fixes: Vec<String>,
}

pub fn normalize(raw: &str) -> Normalized {
    // Letters/digits stay, separators and whitespace become one space.
    let mut cleaned = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_alphanumeric() {
            cleaned.extend(c.to_uppercase());
        } else if c.is_whitespace() || matches!(c, '.' | ',' | '-' | '/') {
            cleaned.push(' ');
        }
    }

    let tokens: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|t| !gazetteer::STOPWORDS.contains(t) && !t.chars().all(char::is_numeric))
        .collect();

    if tokens.is_empty() {
        return Normalized { name: String::new(), confidence: 0.0, fixes: Vec::new() };
    }

    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut fixes = Vec::new();
    let mut total = 0.0f32;
    let mut i = 0;

    while i < tokens.len() {
        // A known multi-word business/person name beats correcting its words one by one.
        let mut phrase: Option<(usize, String)> = None;
        for len in (2..=MAX_PHRASE).rev() {
            if i + len <= tokens.len() {
                let joined = tokens[i..i + len].join(" ");
                if gazetteer::NAMES.iter().any(|n| *n == joined) {
                    phrase = Some((len, joined));
                    break;
                }
            }
        }
        if let Some((len, joined)) = phrase {
            out.push(joined);
            total += len as f32;
            i += len;
            continue;
        }

        let tok = tokens[i];
        if tok.chars().count() == 1 || is_known(tok) {
            out.push(tok.to_string());
            total += 1.0;
            i += 1;
            continue;
        }

        match best_match(tok) {
            Some((candidate, score)) => {
                out.push(candidate.to_string());
                total += score;
                fixes.push(format!("{tok} -> {candidate} (gazetteer, {score:.2})"));
            }
            None => {
                out.push(tok.to_string());
                total += UNKNOWN_SCORE;
            }
        }
        i += 1;
    }

    Normalized {
        name: out.join(" "),
        confidence: (total / tokens.len() as f32).clamp(0.0, 1.0),
        fixes,
    }
}

fn is_known(token: &str) -> bool {
    gazetteer::NAMES.iter().any(|n| *n == token)
}

/// Highest-scoring single-word gazetteer entry, shorter entry wins a tie.
fn best_match(token: &str) -> Option<(&'static str, f32)> {
    let mut best: Option<(&'static str, f32)> = None;
    for candidate in gazetteer::NAMES.iter().copied().filter(|n| !n.contains(' ')) {
        let score = jaro_winkler(token, candidate);
        if score < MIN_SCORE {
            continue;
        }
        let better = match best {
            None => true,
            Some((bc, bs)) => score > bs || (score == bs && candidate.len() < bc.len()),
        };
        if better {
            best = Some((candidate, score));
        }
    }
    best
}

/// Jaro-Winkler similarity, 0.0..=1.0.
fn jaro_winkler(a: &str, b: &str) -> f32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }

    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_hit = vec![false; a.len()];
    let mut b_hit = vec![false; b.len()];
    let mut matches = 0usize;

    for i in 0..a.len() {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(b.len());
        for j in start..end {
            if !b_hit[j] && a[i] == b[j] {
                a_hit[i] = true;
                b_hit[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }

    let mut transpositions = 0usize;
    let mut k = 0usize;
    for i in 0..a.len() {
        if a_hit[i] {
            while !b_hit[k] {
                k += 1;
            }
            if a[i] != b[k] {
                transpositions += 1;
            }
            k += 1;
        }
    }

    let m = matches as f32;
    let jaro = (m / a.len() as f32
        + m / b.len() as f32
        + (m - transpositions as f32 / 2.0) / m)
        / 3.0;
    let prefix = a
        .iter()
        .zip(b.iter())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count() as f32;
    (jaro + prefix * 0.1 * (1.0 - jaro)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_name_is_unchanged() {
        let n = normalize("SUDHA DAIRY");
        assert_eq!(n.name, "SUDHA DAIRY");
        assert_eq!(n.confidence, 1.0);
        assert!(n.fixes.is_empty());
    }

    #[test]
    fn ocr_typo_is_repaired() {
        let n = normalize("SUDDHA DAIRY");
        assert_eq!(n.name, "SUDHA DAIRY");
        assert_eq!(n.fixes, vec!["SUDDHA -> SUDHA (gazetteer, 0.96)"]);
    }

    #[test]
    fn speech_typo_is_repaired() {
        let n = normalize("manish kumar divedi");
        assert_eq!(n.name, "MANISH KUMAR DWIVEDI");
        assert!(n.fixes[0].starts_with("DIVEDI -> DWIVEDI (gazetteer, 0."));
        assert!(n.confidence > 0.9);
    }

    #[test]
    fn initials_are_preserved() {
        let n = normalize("V K SHARMA");
        assert_eq!(n.name, "V K SHARMA");
        assert_eq!(n.confidence, 1.0);
        assert!(n.fixes.is_empty());
    }

    #[test]
    fn empty_input_is_empty() {
        let n = normalize("");
        assert_eq!(n.name, "");
        assert_eq!(n.confidence, 0.0);
        assert!(n.fixes.is_empty());
    }

    #[test]
    fn unknown_name_is_kept_with_low_confidence() {
        let n = normalize("XOLANI ZWANE");
        assert_eq!(n.name, "XOLANI ZWANE");
        assert_eq!(n.confidence, 0.45);
        assert!(n.fixes.is_empty());
    }

    #[test]
    fn punctuation_is_cleaned() {
        let n = normalize("V.K. SHARMA");
        assert_eq!(n.name, "V K SHARMA");
        assert!(n.fixes.is_empty());
    }

    #[test]
    fn ledger_name_is_repaired() {
        let n = normalize("SUNIL KUMAR MSITRI");
        assert_eq!(n.name, "SUNIL KUMAR MISTRI");
        assert_eq!(n.fixes, vec!["MSITRI -> MISTRI (gazetteer, 0.95)"]);
    }

    #[test]
    fn header_words_and_digits_are_dropped() {
        let n = normalize("CASH No 123 SUDHA DAIRY");
        assert_eq!(n.name, "SUDHA DAIRY");
        assert_eq!(n.confidence, 1.0);
    }

    #[test]
    fn devanagari_is_kept_unchanged() {
        let n = normalize("सुधा");
        assert_eq!(n.name, "सुधा");
        assert!(n.confidence < 0.6);
    }
}
