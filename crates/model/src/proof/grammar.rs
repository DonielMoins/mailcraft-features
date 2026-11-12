// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The grammar server: LanguageTool's `POST /v2/check`, asked and read.
//!
//! Nothing here opens a connection — the host does that (`mailcraft_net::languagetool`, on a
//! thread of the window's own) — and nothing here trusts an answer: every length and offset in it
//! is checked against the text that was sent, a match that does not fit is dropped rather than
//! guessed at, and the count is capped ([`super::MAX_DIAGNOSTICS`]).
//!
//! **Offsets.** LanguageTool counts its offsets in UTF-16 code units (Java's `String`), while a
//! MailCraft text is counted in bytes (a Rust `&str`). The two agree only for plain ASCII, which
//! is exactly where a French sentence with `é`, `à` and `œ` stops being ASCII, so every offset is
//! converted through [`utf16_to_byte`] and a match whose offset does not land on a character is
//! dropped. There is no other conversion anywhere: a diagnostic's [`Span`] is always bytes.

use serde::Deserialize;

use super::{Diagnostic, Kind, Language, MAX_DIAGNOSTICS, ProofError, Source, Span};

/// How the text is asked to be checked: LanguageTool's own language, and no rule switched off by
/// the request (the owner's quiet rules are dropped from the answer here instead, so that one
/// answer is the same for every window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRequest<'a> {
    /// The text, which is the message being written or one paragraph of it.
    pub text: &'a str,
    /// Which language to check it as.
    pub language: Language,
}

impl CheckRequest<'_> {
    /// The body of the request, `application/x-www-form-urlencoded`.
    pub fn form(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + 64);
        out.push_str("text=");
        encode_into(&mut out, self.text);
        out.push_str("&language=");
        encode_into(&mut out, self.language.code());
        out.push_str("&enabledOnly=false&level=picky");
        out
    }
}

/// `text` written for a form: everything but an unreserved letter or digit as `%XX`, a space as
/// `+`. A `&`, a `=` or a `%` in the message never changes what the request says.
fn encode_into(out: &mut String, text: &str) {
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*byte as char),
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(hex(other >> 4));
                out.push(hex(other & 0x0f));
            }
        }
    }
}

fn hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

/// The answer, as far as this program reads it.
#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    matches: Vec<Match>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Match {
    #[serde(default)]
    message: String,
    #[serde(default)]
    offset: i64,
    #[serde(default)]
    length: i64,
    #[serde(default)]
    replacements: Vec<Replacement>,
    #[serde(default)]
    rule: Option<Rule>,
    #[serde(default, alias = "rule_id")]
    rule_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Replacement {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    issue_type: Option<String>,
}

/// The diagnostics in an answer to a check of `text` in `language`, in the answer's order, each
/// with what could stand in its place.
///
/// An answer that is not JSON at all, or that does not have the shape of one, is [`ProofError::Answer`];
/// a match that the text does not hold is left out (it is not this text's, whatever the server
/// says). `ignored` names the rule ids the owner told to be quiet.
pub fn parse_answer(body: &[u8], text: &str, language: Language, ignored: &[String]) -> Result<Vec<Diagnostic>, ProofError> {
    if body.len() > super::MAX_ANSWER {
        return Err(ProofError::Answer);
    }
    // An answer is one object: `[]`, a bare string or a number is not one, whatever it parses to.
    if !body.trim_ascii_start().starts_with(b"{") {
        return Err(ProofError::Answer);
    }
    let answer: Answer = serde_json::from_slice(body).map_err(|_| ProofError::Answer)?;
    let lines = utf16_index(text);
    let mut out: Vec<Diagnostic> = Vec::new();
    for m in answer.matches.iter().take(MAX_DIAGNOSTICS * 4) {
        if out.len() >= MAX_DIAGNOSTICS {
            break;
        }
        let id = m.rule.as_ref().and_then(|r| r.id.clone()).or_else(|| m.rule_id.clone());
        if let Some(id) = &id
            && ignored.iter().any(|quiet| quiet == id)
        {
            continue;
        }
        let Some(start) = utf16_to_byte(text, &lines, m.offset) else { continue };
        let Some(end) = utf16_to_byte(text, &lines, m.offset.saturating_add(m.length.max(0))) else { continue };
        let Some(span) = Span::of(text, start, end) else { continue };
        let kind = match m.rule.as_ref().and_then(|r| r.issue_type.as_deref()) {
            Some("style") => Kind::Style,
            Some("misspelling") => Kind::Spelling,
            _ => Kind::Grammar,
        };
        let mut d = Diagnostic::new(span, kind, Source::Server).saying(&m.message).suggesting(m.replacements.iter().map(|r| r.value.clone()));
        d.language = Some(language);
        d.rule = id;
        if d.is_sound() {
            out.push(d);
        }
    }
    Ok(out)
}

/// Where each character of `text` is, in UTF-16 code units and in bytes: only the starts of
/// characters, in order, with the UTF-16 count of each (`2` for a character outside the basic
/// plane).
pub(super) fn utf16_index(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(text.len().min(4_096));
    let mut units = 0usize;
    for (at, c) in text.char_indices() {
        out.push((units, at));
        units += c.len_utf16();
    }
    out.push((units, text.len()));
    out
}

/// The byte offset of the character starting at UTF-16 code unit `units` of `text`: `None` when
/// no character starts there (past the end, or between the two halves of a character outside the
/// basic plane), which is where a correction would cut a character in two.
pub fn utf16_to_byte(text: &str, index: &[(usize, usize)], units: i64) -> Option<usize> {
    let units = usize::try_from(units).ok()?;
    let at = index.binary_search_by_key(&units, |(u, _)| *u).ok()?;
    let byte = index.get(at).map(|(_, b)| *b)?;
    (byte <= text.len() && text.is_char_boundary(byte)).then_some(byte)
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn utf16_offsets_are_converted() {
        let text = "Les documents sont important.";
        let index = utf16_index(text);
        for (units, byte) in &index {
            assert_eq!(utf16_to_byte(text, &index, *units as i64), Some(*byte));
        }
        // `é` is one UTF-16 unit and two bytes: a server counting units lands after it by two bytes.
        let text = "café noir";
        let index = utf16_index(text);
        assert_eq!(utf16_to_byte(text, &index, 4), Some(5));
        assert_eq!(text.get(5..), Some(" noir"));
        // A character outside the basic plane is two units and four bytes.
        let text = "a🦀b";
        let index = utf16_index(text);
        assert_eq!(utf16_to_byte(text, &index, 1), Some(1));
        assert_eq!(utf16_to_byte(text, &index, 2), None, "the middle of the crab is no character's start");
        assert_eq!(utf16_to_byte(text, &index, 3), Some(5));
        assert_eq!(utf16_to_byte(text, &index, 99), None);
        assert_eq!(utf16_to_byte(text, &index, -1), None);
    }

    #[test]
    fn a_form_carries_the_text_whole() {
        let req = CheckRequest { text: "Bonjour & à bientôt = 100%", language: Language::FrCa };
        let form = req.form();
        assert!(form.starts_with("text=Bonjour+%26+%C3%A0+bient%C3%B4t+%3D+100%25&language=fr-CA"));
        assert!(!form.contains(' '));
    }

    #[test]
    fn an_answer_is_read_with_its_offsets_in_bytes() {
        let text = "Les document sont important.";
        let body = br#"{"matches":[{"message":"Accord","replacements":[{"value":"documents"}],"offset":4,"length":8,"rule":{"id":"FR_ACCORD","issueType":"grammar"}}]}"#;
        let got = parse_answer(body, text, Language::FrCa, &[]).expect("an answer");
        assert_eq!(got.len(), 1);
        let d = got.first().expect("one");
        assert_eq!(text.get(d.span.start..d.span.end), Some("document"));
        assert_eq!(d.suggestions.first().map(String::as_str), Some("documents"));
        assert_eq!(d.kind, Kind::Grammar);
        assert_eq!(d.rule.as_deref(), Some("FR_ACCORD"));
    }

    #[test]
    fn a_match_that_does_not_fit_is_dropped() {
        let text = "Bonjour";
        for body in [
            r#"{"matches":[{"message":"x","offset":2,"length":99}]}"#,
            r#"{"matches":[{"message":"x","offset":-4,"length":1}]}"#,
            r#"{"matches":[{"message":"x","offset":0,"length":0}]}"#,
            r#"{"matches":[{"message":"x","offset":900,"length":1}]}"#,
        ] {
            let got = parse_answer(body.as_bytes(), text, Language::EnCa, &[]).expect("an answer");
            assert!(got.is_empty(), "{body}");
        }
    }

    #[test]
    fn a_quiet_rule_is_left_out() {
        let text = "Bonjour";
        let body = br#"{"matches":[{"message":"x","offset":0,"length":7,"rule":{"id":"QUIET"}}]}"#;
        assert!(parse_answer(body, text, Language::FrCa, &["QUIET".to_string()]).expect("an answer").is_empty());
    }

    #[test]
    fn an_answer_that_is_not_one_is_an_error_and_never_a_panic() {
        for body in [b"not json".as_slice(), b"{}[]".as_slice(), b"null".as_slice(), b"".as_slice()] {
            assert!(
                parse_answer(body, "text", Language::EnCa, &[]).is_err()
                    || parse_answer(body, "text", Language::EnCa, &[]).is_ok_and(|d| d.is_empty())
            );
        }
    }
}
