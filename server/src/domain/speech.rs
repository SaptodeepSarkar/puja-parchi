//! Parse the 6-second voice note into receipt fields.
//!
//! The volunteer speaks in this order: name, receipt number, payment mode,
//! amount. Real clips are messy, so we accept any order and rely on cues.
//!
//! Contract (do not change the signature):
//! - pure function, no I/O.
//! - `receipt`: digits only, 1-4 digits, from "receipt / recipt / parchi / slip /
//!   number <digits>", or a run of spoken single digits ("eight five one").
//! - `name`: the words that are not cues or numbers; strips "name", "naam",
//!   "ji", "ji ka naam". Keep original casing, UPPERCASE it.
//! - `amount`: integer rupees. Accept digits ("1100", "1,100"), "₹1100",
//!   and Hindi/English words ("eleven hundred", "ek hazar sau", "do sau").
//! - `mode`: Online if "online/upi/gpay/google pay/paytm/phonepe/transfer/neft"
//!   appears, Cash if "cash/nakad"; else None (caller defaults to cash).
//! - `evidence`: one short sentence per field that was found, for the audit
//!   trail. Empty when nothing parsed.
//! - Never panic on empty / unicode / very long input.

use super::receipt::Mode;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spoken {
    pub receipt: Option<String>,
    pub name: Option<String>,
    pub amount: Option<String>,
    pub mode: Option<Mode>,
    pub evidence: Vec<String>,
}

pub fn parse(input: &str) -> Spoken {
    let mut out = Spoken::default();
    let tokens: Vec<String> = normalize(input)
        .split(|c: char| !is_word_char(c))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if tokens.is_empty() {
        return out;
    }
    let mut used = vec![false; tokens.len()];

    for t in &tokens {
        if let Some(m) = mode_of(t) {
            out.mode = Some(m);
            out.evidence.push(format!(
                "mode: {} (heard '{}')",
                match m {
                    Mode::Online => "online",
                    Mode::Cash => "cash",
                },
                t
            ));
            break;
        }
    }

    // Receipt: a cue word followed by digits or spoken digits.
    for (i, t) in tokens.iter().enumerate() {
        if !is_receipt_word(t) {
            continue;
        }
        let Some((val, start, end)) = receipt_after(&tokens, i) else { continue };
        out.evidence
            .push(format!("receipt {val}: heard as '{}'", tokens[start..end].join(" ")));
        out.receipt = Some(val);
        for u in used[i..end].iter_mut() {
            *u = true;
        }
        break;
    }

    // Amount explicitly attached to a currency cue wins.
    if out.amount.is_none() {
        for (i, t) in tokens.iter().enumerate() {
            if used[i] || !is_currency(t) {
                continue;
            }
            let Some((v, end)) = take_number(&tokens, i + 1) else { continue };
            if v == 0 || v > 99_999 {
                continue;
            }
            out.evidence
                .push(format!("amount {v}: from '{}'", tokens[i..end].join(" ")));
            out.amount = Some(v.to_string());
            for u in used[i..end].iter_mut() {
                *u = true;
            }
            break;
        }
    }

    // Every remaining number expression, left to right.
    let mut candidates: Vec<(u64, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if used[i] || !starts_number(&tokens[i]) {
            i += 1;
            continue;
        }
        let Some((v, end)) = take_number(&tokens, i) else { i += 1; continue };
        if end > i && v > 0 && v <= 99_999 {
            candidates.push((v, i, end));
        }
        i = end.max(i + 1);
    }

    // No receipt cue: the first short bare number before the amount is the receipt.
    if out.receipt.is_none() && (out.amount.is_some() || candidates.len() >= 2) {
        let pick = candidates
            .iter()
            .find(|c| {
                c.2 == c.1 + 1
                    && c.0 > 0
                    && c.0 <= 9999
                    && ascii_digits(&tokens[c.1]).is_some_and(|s| s.len() <= 4)
            })
            .copied();
        if let Some((v, start, end)) = pick {
            if out.amount.is_some() || candidates.last().is_some_and(|l| l.1 != start) {
                out.evidence.push(format!(
                    "receipt {v}: heard as '{}'",
                    tokens[start..end].join(" ")
                ));
                out.receipt = Some(v.to_string());
                for u in used[start..end].iter_mut() {
                    *u = true;
                }
                candidates.retain(|c| c.1 != start);
            }
        }
    }

    // Otherwise the last number that is not the receipt is the amount.
    if out.amount.is_none() {
        if let Some(&(v, start, end)) = candidates.last() {
            out.evidence
                .push(format!("amount {v}: from '{}'", tokens[start..end].join(" ")));
            out.amount = Some(v.to_string());
            for u in used[start..end].iter_mut() {
                *u = true;
            }
        }
    }

    // Name: whatever is left that is not a cue, number or filler.
    let mut name_words: Vec<&str> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if used[i]
            || is_name_stop(t)
            || is_receipt_word(t)
            || is_currency(t)
            || mode_of(t).is_some()
            || digits(t).is_some()
            || word_value(t).is_some()
        {
            continue;
        }
        name_words.push(t);
    }
    while name_words.first().is_some_and(|w| is_edge_filler(w)) {
        name_words.remove(0);
    }
    while name_words.last().is_some_and(|w| is_edge_filler(w)) {
        name_words.pop();
    }
    let name = name_words.join(" ");
    if name.chars().filter(|c| c.is_alphabetic()).count() >= 2 {
        let upper = name.to_uppercase();
        out.evidence.push(format!("name {upper}: heard as '{name}'"));
        out.name = Some(upper);
    }

    out
}

/// Lowercase, drop digit-group commas, and turn ₹ into a currency cue word.
fn normalize(input: &str) -> String {
    let lower = input.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = String::with_capacity(lower.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c == '₹' {
            out.push_str(" rs ");
        } else if c == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())
        {
            continue;
        } else {
            out.push(c);
        }
    }
    out
}

/// Keep Devanagari combining marks (matras, virama) inside one token.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
        || (('\u{0900}'..='\u{097F}').contains(&c) && !matches!(c, '\u{0964}' | '\u{0965}'))
}

fn mode_of(t: &str) -> Option<Mode> {
    match t {
        "online" | "upi" | "gpay" | "google" | "phonepe" | "paytm" | "transfer" | "neft"
        | "ऑनलाइन" | "यूपीआई" => Some(Mode::Online),
        "cash" | "nakad" | "नकद" => Some(Mode::Cash),
        _ => None,
    }
}

fn is_receipt_word(t: &str) -> bool {
    matches!(
        t,
        "receipt"
            | "recipt"
            | "reciept"
            | "raseed"
            | "rasid"
            | "parchi"
            | "parchee"
            | "slip"
            | "number"
            | "no"
            | "rno"
            | "r"
            | "नंबर"
            | "रसीद"
            | "पर्ची"
    )
}

fn is_filler(t: &str) -> bool {
    matches!(t, "is" | "hai" | "hain" | "है" | "हैं" | "ka" | "ki" | "का" | "की" | "of")
}

fn is_currency(t: &str) -> bool {
    matches!(
        t,
        "rs" | "rupee" | "rupees" | "rupaye" | "rupay" | "paisa" | "inr" | "amount" | "rakam"
            | "रुपये" | "रुपए" | "रकम"
    )
}

fn is_name_stop(t: &str) -> bool {
    matches!(
        t,
        "name"
            | "naam"
            | "नाम"
            | "hai"
            | "hain"
            | "है"
            | "हैं"
            | "the"
            | "is"
            | "ji"
            | "jee"
            | "ka"
            | "ki"
            | "ke"
            | "का"
            | "की"
            | "के"
            | "of"
            | "my"
            | "mera"
            | "meri"
            | "and"
            | "aur"
            | "और"
            | "amount"
            | "rakam"
            | "रकम"
    )
}

fn is_edge_filler(t: &str) -> bool {
    matches!(t, "ji" | "jee" | "ka" | "ki" | "ke" | "hai" | "the" | "is" | "of")
}

fn is_scale(t: &str) -> bool {
    matches!(
        t,
        "hundred" | "sau" | "सौ" | "thousand" | "hazar" | "hajar" | "हजार" | "lakh" | "लाख"
    )
}

/// Digits only, accepting Devanagari digits. Returns the ASCII string.
fn ascii_digits(t: &str) -> Option<String> {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        let d = match c {
            '0'..='9' => c,
            '०'..='९' => char::from(b'0' + (c as u32 - '०' as u32) as u8),
            _ => return None,
        };
        s.push(d);
    }
    (!s.is_empty()).then_some(s)
}

fn digits(t: &str) -> Option<u64> {
    Some(ascii_digits(t)?.parse().unwrap_or(u64::MAX))
}

fn single_digit(t: &str) -> Option<u8> {
    if let Some(v) = digits(t) {
        return (v <= 9).then_some(v as u8);
    }
    let v = word_value(t)?;
    (v <= 9).then_some(v as u8)
}

fn starts_number(t: &str) -> bool {
    digits(t).is_some() || word_value(t).is_some_and(|v| v < 100)
}

fn word_value(t: &str) -> Option<u64> {
    Some(match t {
        "zero" | "shunya" | "शून्य" => 0,
        "one" | "ek" | "एक" => 1,
        "two" | "do" | "दो" => 2,
        "three" | "teen" | "तीन" => 3,
        "four" | "char" | "chaar" | "चार" => 4,
        "five" | "panch" | "पांच" | "पाँच" => 5,
        "six" | "chhe" | "chah" | "छह" | "छे" => 6,
        "seven" | "saat" | "सात" => 7,
        "eight" | "aath" | "आठ" => 8,
        "nine" | "nau" | "नौ" => 9,
        "ten" | "das" | "दस" => 10,
        "eleven" | "gyarah" | "ग्यारह" => 11,
        "twelve" | "barah" | "बारह" => 12,
        "thirteen" | "terah" | "तेरह" => 13,
        "fourteen" | "chaudah" | "चौदह" => 14,
        "fifteen" | "pandrah" | "पंद्रह" => 15,
        "sixteen" | "solah" | "सोलह" => 16,
        "seventeen" | "satrah" | "सत्रह" => 17,
        "eighteen" | "atharah" | "अठारह" => 18,
        "nineteen" | "unnis" | "उन्नीस" => 19,
        "twenty" | "bees" | "बीस" => 20,
        "thirty" | "tees" | "तीस" => 30,
        "forty" | "chalees" | "चालीस" => 40,
        "fifty" | "pachas" | "पचास" => 50,
        "fiftyfive" | "pachpan" | "पचपन" => 55,
        "sixty" | "saath" | "साठ" => 60,
        "seventy" | "sattar" | "सत्तर" => 70,
        "eighty" | "assi" | "अस्सी" => 80,
        "ninety" | "nabbe" | "नब्बे" => 90,
        "hundred" | "sau" | "सौ" => 100,
        "thousand" | "hazar" | "hajar" | "हजार" => 1_000,
        "lakh" | "लाख" => 100_000,
        "इक्यावन" => 51,
        _ => return None,
    })
}

/// Parse a number expression starting at `start`, return value and end index.
fn take_number(tokens: &[String], start: usize) -> Option<(u64, usize)> {
    let mut total: u64 = 0;
    let mut current: u64 = 0;
    let mut any = false;
    let mut i = start;
    while i < tokens.len() {
        let tok = &tokens[i];
        if let Some(d) = digits(tok) {
            if any {
                break; // adjacent digit tokens are separate numbers
            }
            current = d;
            any = true;
            i += 1;
            continue;
        }
        let Some(v) = word_value(tok) else { break };
        if is_scale(tok) {
            if v == 100 {
                current = current.max(1).saturating_mul(100);
            } else {
                total = total.saturating_add(current.max(1).saturating_mul(v));
                current = 0;
            }
        } else {
            current = current.saturating_add(v);
        }
        any = true;
        i += 1;
    }
    any.then_some((total.saturating_add(current), i))
}

fn receipt_after(t: &[String], k: usize) -> Option<(String, usize, usize)> {
    let mut i = k + 1;
    while i < t.len() && (is_receipt_word(&t[i]) || is_filler(&t[i])) {
        i += 1;
    }
    if i >= t.len() {
        return None;
    }

    if let Some(s) = ascii_digits(&t[i]) {
        return (s.len() <= 4 && s != "0").then_some((s, i, i + 1));
    }

    let mut j = i;
    let mut s = String::new();
    while j < t.len() {
        let Some(d) = single_digit(&t[j]) else { break };
        s.push(char::from(b'0' + d));
        j += 1;
    }
    let run = j - i;
    if run > 4 {
        return None; // 5+ digits is never a receipt
    }
    if run >= 2 {
        return (s != "0").then_some((s, i, j));
    }
    if run == 1 && !(j < t.len() && is_scale(&t[j])) {
        return (s != "0").then_some((s, i, j));
    }

    // "eighteen fifty two" is how a stub number is usually said out loud.
    // Two 10..99 groups concatenate as zero-padded pairs; a scale word
    // ("hundred"/"hazar") makes take_number return >99 and falls through.
    if let Some((s, end)) = pair_run(t, i) {
        return (s.len() <= 4 && s != "0000").then_some((s, i, end));
    }

    let (v, end) = take_number(t, i)?;
    (1..=9999).contains(&v).then_some((v.to_string(), i, end))
}

/// "eighteen fifty two" -> "1852". Exactly two groups of 10..99, no scale words.
fn pair_run(t: &[String], start: usize) -> Option<(String, usize)> {
    let (a, e1) = small_group(t, start)?;
    if !(10..=99).contains(&a) {
        return None;
    }
    let (b, e2) = small_group(t, e1)?;
    if !(10..=99).contains(&b) {
        return None;
    }
    Some((format!("{a:02}{b:02}"), e2))
}

/// One spoken group below 100, consuming at most two words: "fifty two" -> 52,
/// "eighteen" -> 18, "hundred" -> None. Never swallows a following group, which
/// is what `take_number` does on purpose for amounts.
fn small_group(t: &[String], start: usize) -> Option<(u64, usize)> {
    let v = word_value(t.get(start)?)?;
    if v >= 100 {
        return None;
    }
    let mut n = v;
    let mut end = start + 1;
    if (20..=90).contains(&v) && v % 10 == 0 {
        if let Some(u) = t.get(end).and_then(|x| word_value(x)) {
            if (1..=9).contains(&u) {
                n += u;
                end += 1;
            }
        }
    }
    Some((n, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_example_fields() {
        let s = parse("name suddha dairy receipt number eight five one eleven hundred cash");
        assert_eq!(s.receipt.as_deref(), Some("851"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Cash));
        assert_eq!(s.name.as_deref(), Some("SUDDHA DAIRY"));
        assert_eq!(s.evidence.len(), 4);
    }

    #[test]
    fn receipt_spoken_in_two_digit_pairs() {
        let s = parse("v p mishra receipt number eighteen fifty two online one thousand one hundred");
        assert_eq!(s.receipt.as_deref(), Some("1852"), "18+52 must not become 70");
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Online));
        assert_eq!(s.name.as_deref(), Some("V P MISHRA"));
    }

    #[test]
    fn receipt_pairs_stop_before_a_scale_word() {
        let s = parse("receipt number twelve thirty cash four hundred");
        assert_eq!(s.receipt.as_deref(), Some("1230"));
        assert_eq!(s.amount.as_deref(), Some("400"));
    }

    #[test]
    fn receipt_eighteen_hundred_fifty_two() {
        let s = parse("receipt number eighteen hundred fifty two cash 1100");
        assert_eq!(s.receipt.as_deref(), Some("1852"));
    }

    #[test]
    fn name_receipt_amount_without_cue() {
        let s = parse("ram kumar 852 online one thousand one hundred");
        assert_eq!(s.receipt.as_deref(), Some("852"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Online));
        assert_eq!(s.name.as_deref(), Some("RAM KUMAR"));
    }

    #[test]
    fn parchi_number_and_upi() {
        let s = parse("suddha dairy parchi number 851 upi 1100");
        assert_eq!(s.receipt.as_deref(), Some("851"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Online));
        assert_eq!(s.name.as_deref(), Some("SUDDHA DAIRY"));
    }

    #[test]
    fn hindi_words_and_receipt_phrase() {
        let s = parse("नाम राम कुमार रसीद नंबर आठ सौ इक्यावन");
        assert_eq!(s.receipt.as_deref(), Some("851"));
        assert_eq!(s.name.as_deref(), Some("राम कुमार"));
        assert_eq!(s.amount, None);
    }

    #[test]
    fn ek_hazar_sau_is_1100() {
        let s = parse("receipt 700 ek hazar sau cash");
        assert_eq!(s.receipt.as_deref(), Some("700"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Cash));
    }

    #[test]
    fn do_sau_pachas_is_250() {
        let s = parse("receipt 12 do sau pachas");
        assert_eq!(s.receipt.as_deref(), Some("12"));
        assert_eq!(s.amount.as_deref(), Some("250"));
    }

    #[test]
    fn comma_amount() {
        let s = parse("receipt 852 1,100");
        assert_eq!(s.receipt.as_deref(), Some("852"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
    }

    #[test]
    fn rupee_symbol_amount() {
        let s = parse("receipt 852 ₹1100 cash");
        assert_eq!(s.receipt.as_deref(), Some("852"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Cash));
    }

    #[test]
    fn name_after_amount() {
        let s = parse("receipt 700 1100 ram kumar cash");
        assert_eq!(s.receipt.as_deref(), Some("700"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.name.as_deref(), Some("RAM KUMAR"));
    }

    #[test]
    fn mode_only() {
        let s = parse("online");
        assert_eq!(s.mode, Some(Mode::Online));
        assert!(s.name.is_none());
        assert!(s.receipt.is_none());
        assert!(s.amount.is_none());
        assert_eq!(s.evidence.len(), 1);
    }

    #[test]
    fn empty_input() {
        let s = parse("");
        assert_eq!(s, Spoken::default());
        assert!(s.evidence.is_empty());
    }

    #[test]
    fn long_input_does_not_panic() {
        let long = "eight ".repeat(2000);
        let s = parse(&long);
        let _ = s;
        let _ = parse(&"abc ".repeat(3000));
    }

    #[test]
    fn receipt_is_not_the_amount() {
        let s = parse("receipt 852 1100");
        assert_eq!(s.receipt.as_deref(), Some("852"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
    }

    #[test]
    fn spoken_single_digits_receipt() {
        let s = parse("receipt eight five one two cash 500");
        assert_eq!(s.receipt.as_deref(), Some("8512"));
        assert_eq!(s.amount.as_deref(), Some("500"));
    }

    #[test]
    fn devanagari_digits_receipt() {
        let s = parse("रसीद ८५१ 1100 नकद");
        assert_eq!(s.receipt.as_deref(), Some("851"));
        assert_eq!(s.amount.as_deref(), Some("1100"));
        assert_eq!(s.mode, Some(Mode::Cash));
    }

    #[test]
    fn first_mode_wins() {
        assert_eq!(parse("upi cash").mode, Some(Mode::Online));
        assert_eq!(parse("cash upi").mode, Some(Mode::Cash));
    }

    #[test]
    fn unicode_and_junk_never_panic() {
        for input in ["🙂🎉", "12345", "receipt number", "name hai ji ka", "₹", "1,1,1"] {
            let _ = parse(input);
        }
        let s = parse("12345");
        assert!(s.receipt.is_none());
        assert_eq!(s.amount.as_deref(), Some("12345"));
    }
}
