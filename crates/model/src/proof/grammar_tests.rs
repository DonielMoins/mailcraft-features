// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The grammar server's answer read from this side (#954): offsets counted in UTF-16 code units
//! turned into byte spans of a French sentence, matches that the text does not hold, rules the
//! owner silenced, and answers that are enormous, malformed or hostile. Every one of these is
//! read by [`parse_answer`], which never panics and never clamps a place onto other text.

use std::time::{Duration, Instant};

use super::*;

/// An answer holding `matches`, as JSON.
fn answer(matches: &str) -> Vec<u8> {
    format!("{{\"matches\":[{matches}]}}").into_bytes()
}

/// One match over `length` UTF-16 code units from `offset`, which is how LanguageTool counts.
fn at(offset: i64, length: i64) -> String {
    format!("{{\"message\":\"Accord\",\"offset\":{offset},\"length\":{length}}}")
}

/// The text under each diagnostic's span, in the answer's order.
fn under<'a>(found: &[Diagnostic], text: &'a str) -> Vec<&'a str> {
    found.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect()
}

#[test]
fn offsets_counted_in_utf16_land_on_the_bytes_of_a_french_sentence() {
    // `é` and `è` are one UTF-16 unit and two bytes each: a server counting units is further on
    // by a byte for every one of them that came before.
    let text = "Le café est très bon.";
    let got = parse_answer(&answer(&[at(3, 4), at(12, 4)].join(",")), text, Language::FrCa, &[]).expect("an answer");
    assert_eq!(under(&got, text), ["café", "très"]);
    assert_eq!(got.first().map(|d| d.span), Some(Span { start: 3, end: 8 }));
    assert_eq!(got.get(1).map(|d| d.span), Some(Span { start: 13, end: 18 }));
}

#[test]
fn a_character_outside_the_basic_plane_is_two_units_and_four_bytes() {
    // "Très 🦀 bon": the crab starts at unit 5 and byte 6.
    let text = "Très 🦀 bon";
    let got = parse_answer(&answer(&at(5, 2)), text, Language::FrCa, &[]).expect("an answer");
    assert_eq!(under(&got, text), ["🦀"]);
    assert_eq!(got.first().map(|d| d.span), Some(Span { start: 6, end: 10 }));
    // A single unit of it is half a character, and half a character is nobody's place.
    assert!(parse_answer(&answer(&at(5, 1)), text, Language::FrCa, &[]).expect("an answer").is_empty());
    assert!(parse_answer(&answer(&at(6, 1)), text, Language::FrCa, &[]).expect("an answer").is_empty());
}

#[test]
fn a_match_the_text_does_not_hold_is_dropped_rather_than_clamped() {
    // A place that does not fit would put a correction in the wrong text: it is left out whole.
    let text = "a🦀b";
    for (offset, length) in [(1, 1), (2, 1), (0, 0), (1, 0), (-1, 1), (99, 1), (i64::MAX, 1), (0, i64::MAX), (3, 9), (1, -2)] {
        let got = parse_answer(&answer(&at(offset, length)), text, Language::EnCa, &[]).expect("an answer");
        assert!(got.is_empty(), "offset {offset}, length {length}");
    }
    // And one that does fit is read.
    assert_eq!(under(&parse_answer(&answer(&at(1, 2)), text, Language::EnCa, &[]).expect("an answer"), text), ["🦀"]);
}

#[test]
fn the_rules_the_owner_silenced_are_left_out() {
    let text = "Bonjour tout le monde";
    let quiet = r#"{"message":"x","offset":0,"length":7,"rule":{"id":"FR_QUIET"}}"#;
    let named = r#"{"message":"x","offset":8,"length":4,"rule_id":"OLD_RULE"}"#;
    let loud = r#"{"message":"x","offset":13,"length":5,"rule":{"id":"FR_LOUD"}}"#;
    let body = answer(&format!("{quiet},{named},{loud}"));
    let silenced = ["FR_QUIET".to_string(), "OLD_RULE".to_string()];

    let got = parse_answer(&body, text, Language::FrCa, &silenced).expect("an answer");
    assert_eq!(got.len(), 1);
    assert_eq!(got.first().and_then(|d| d.rule.clone()), Some("FR_LOUD".to_string()));
    // Nothing is left out when nothing was silenced.
    assert_eq!(parse_answer(&body, text, Language::FrCa, &[]).expect("an answer").len(), 3);
    // A rule is silenced by its whole name alone, never by part of one.
    assert_eq!(parse_answer(&body, text, Language::FrCa, &["FR_".to_string()]).expect("an answer").len(), 3);
}

#[test]
fn what_a_match_is_about_and_where_it_came_from_say_themselves() {
    for (issue, kind) in [("style", Kind::Style), ("misspelling", Kind::Spelling), ("grammar", Kind::Grammar), ("typography", Kind::Grammar)] {
        let body = format!(r#"{{"matches":[{{"message":"x","offset":0,"length":1,"rule":{{"id":"R","issueType":"{issue}"}}}}]}}"#);
        let got = parse_answer(body.as_bytes(), "a", Language::FrCa, &[]).expect("an answer");
        let d = got.first().expect("one");
        assert_eq!(d.kind, kind, "{issue}");
        assert_eq!(d.language, Some(Language::FrCa));
        assert_eq!(d.source, Source::Server);
        assert_eq!(d.rule.as_deref(), Some("R"));
    }
}

#[test]
fn an_answer_that_is_not_one_is_an_error_and_never_a_panic() {
    for body in [
        b"".as_slice(),
        b"not json",
        b"{",
        b"null",
        b"[]",
        b"\"matches\"",
        b"{\"matches\":3}",
        b"{\"matches\":null}",
        b"{\"matches\":[3]}",
        b"{\"matches\":[{\"offset\":\"four\"}]}",
        b"\xff\xfe\x00\x00",
    ] {
        assert_eq!(parse_answer(body, "text", Language::EnCa, &[]), Err(ProofError::Answer), "{:?}", String::from_utf8_lossy(body));
    }
    // Nesting deeper than any answer has is refused, not followed down.
    let deep = "[".repeat(2_000);
    assert_eq!(parse_answer(deep.as_bytes(), "text", Language::EnCa, &[]), Err(ProofError::Answer));
    // An answer nobody would send is not read at all, and is let go of at once.
    let huge = vec![b'x'; MAX_ANSWER + 1];
    let started = Instant::now();
    assert_eq!(parse_answer(&huge, "text", Language::EnCa, &[]), Err(ProofError::Answer));
    assert!(started.elapsed() < Duration::from_secs(2));
    // An answer of nothing at all is one with nothing in it.
    assert_eq!(parse_answer(b"{}", "text", Language::EnCa, &[]), Ok(Vec::new()));
}

#[test]
fn a_list_of_matches_is_bounded_however_many_there_are() {
    // Nine thousand matches fit in an answer, and make no more places than there may be.
    let mut many = String::new();
    for _ in 0..9_000 {
        many.push_str(r#"{"offset":0,"length":1},"#);
    }
    many.pop();
    let got = parse_answer(&answer(&many), "a", Language::EnCa, &[]).expect("an answer");
    assert_eq!(got.len(), MAX_DIAGNOSTICS);

    // A hundred thousand of them is longer than a grammar server's answer may be: refused
    // before a match of it is read.
    let mut hundred = String::new();
    for _ in 0..100_000 {
        hundred.push_str(r#"{"offset":0,"length":1},"#);
    }
    hundred.pop();
    let body = answer(&hundred);
    assert!(body.len() > MAX_ANSWER);
    assert_eq!(parse_answer(&body, "a", Language::EnCa, &[]), Err(ProofError::Answer));
}

#[test]
fn what_could_stand_there_instead_is_cleaned_and_cut() {
    // A control character in a suggestion would be put into the message: none is kept, a tab
    // becomes a space, an empty one is no suggestion, and two that say the same say it once.
    let body = br#"{"matches":[{"message":"x\u0007y","offset":0,"length":1,"replacements":[
        {"value":"do\u0007c"},{"value":"\u0000"},{"value":"a\tb"},{"value":"one"},{"value":"two"},
        {"value":"three"},{"value":"four"},{"value":"five"},{"value":"six"},{"value":"one"}]}]}"#;
    let got = parse_answer(body, "a", Language::EnCa, &[]).expect("an answer");
    let d = got.first().expect("one");
    assert_eq!(d.suggestions, ["doc", "a b", "one", "two", "three", "four", "five", "six"]);
    assert_eq!(d.suggestions.len(), MAX_SUGGESTIONS);
    assert!(d.suggestions.iter().all(|s| !s.chars().any(char::is_control) && !s.contains('\u{2028}')));
    // What the server says of the mistake is one line too, and never longer than a sentence.
    assert_eq!(d.message, "xy");

    let long = format!(r#"{{"matches":[{{"message":"{}","offset":0,"length":1}}]}}"#, "é".repeat(1_000));
    let got = parse_answer(long.as_bytes(), "a", Language::EnCa, &[]).expect("an answer");
    assert_eq!(got.first().map(|d| d.message.chars().count()), Some(300));
}

#[test]
fn a_span_is_always_bytes_of_the_text_that_was_sent() {
    // Whatever the answer says, every place is on a character boundary of the text and inside it.
    let text = "Où est l’accord ? Très important, ça.";
    let mut matches = String::new();
    for offset in -3..80 {
        for length in -3..12 {
            matches.push_str(&at(offset, length));
            matches.push(',');
        }
    }
    matches.pop();
    let got = parse_answer(&answer(&matches), text, Language::FrCa, &[]).expect("an answer");
    assert!(!got.is_empty());
    for d in &got {
        assert!(d.span.fits(text), "{:?}", d.span);
        assert!(text.get(d.span.start..d.span.end).is_some());
    }
}
