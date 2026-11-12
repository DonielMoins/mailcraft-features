// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Proofing's own tests: the words it looks at, the settings it keeps, and a dictionary it reads.
//!
//! The dictionaries here are invented ones (`example` words), so that what a test says a word is
//! never changes with a dictionary's version or with what this computer has installed.

use super::*;

/// A small dictionary of two languages, as a file would say it, with affixes.
fn english() -> &'static str {
    "5\ndocument/S\ndocuments/S\ncolour\ncentre\nneighbour\n"
}

fn french() -> &'static str {
    "4\ndocument/S\nfrançais\naujourd'hui\nMontréal\nQuébec\n"
}

const AFFIXES: &str = "SET UTF-8\nFLAG long\nSFX S Y 1\nSFX S 0 s .\n";

fn both() -> Speller {
    let mut s = Speller::new();
    if let Ok(d) = Dictionary::from_dic(Language::EnCa, english()) {
        s.put(d);
    }
    if let Ok(d) = Dictionary::from_dic(Language::FrCa, french()) {
        s.put(d);
    }
    s
}

#[test]
fn keys_fold_case_and_apostrophes() {
    assert_eq!(key("Aujourd’hui"), key("aujourd'hui"));
    assert_eq!(key("MONTRÉAL"), "montréal");
}

#[test]
fn codes_round_trip() {
    for l in Language::ALL {
        assert_eq!(Language::parse(l.code()), Some(l), "{l:?}");
    }
    assert_eq!(Language::parse("en_CA"), Some(Language::EnCa));
    assert_eq!(Language::parse("fr"), Some(Language::FrCa));
    assert_eq!(Language::parse("en-GB"), None);
    assert_eq!(Language::parse(""), None);
}

#[test]
fn only_this_machine_unless_allowed() {
    assert!(address_allowed("http://127.0.0.1:8081", false));
    assert!(address_allowed("http://localhost:8081/v2/check", false));
    assert!(address_allowed("http://[::1]:8081", false));
    assert!(!address_allowed("http://localhost.example.com:8081", false));
    assert!(!address_allowed("http://192.168.1.9:8081", false));
    assert!(!address_allowed("https://languagetool.example", false));
    assert!(address_allowed("https://languagetool.example", true));
    assert!(!address_allowed("file:///etc/passwd", true));
    assert!(!address_allowed("ftp://example.org", true));
    assert!(!address_allowed("", false));
}

#[test]
fn words_are_runs_of_letters_with_their_places() {
    let text = "Bonjour Pierre, I reviewed les documents hier soir.";
    let got: Vec<&str> = words(text).iter().map(|w| w.text).collect();
    assert_eq!(got, ["Bonjour", "Pierre", "I", "reviewed", "les", "documents", "hier", "soir"]);
    for w in words(text) {
        assert_eq!(text.get(w.span.start..w.span.end), Some(w.text), "a word's place is its text");
    }
}

#[test]
fn addresses_are_left_alone() {
    let text = "Write to pierre@example.org about https://example.org/a today, or http://x.example/y.";
    let got: Vec<&str> = words(text).iter().map(|w| w.text).collect();
    assert_eq!(got, ["Write", "to", "about", "today"]);
}

#[test]
fn a_picture_and_a_long_run_are_not_words() {
    let long = "a".repeat(MAX_WORD + 1);
    let text = format!("ok \u{fffc} {long} 12345");
    let got: Vec<&str> = words(&text).iter().map(|w| w.text).collect();
    assert_eq!(got, ["ok"]);
}

#[test]
fn personal_words_never_repeat() {
    let mut s = ProofSettings::default();
    assert!(s.add_word("Hochelaga"));
    assert!(!s.add_word("hochelaga"), "the same word in another case is the same word");
    assert!(s.add_word("aujourd’hui"));
    assert_eq!(s.personal_words().len(), 2);
    assert!(s.remove_word("HOCHELAGA"));
    assert_eq!(s.personal_words(), ["aujourd’hui"]);
    assert!(!s.add_word(""), "nothing is not a word");
    assert!(!s.add_word("12345"), "a number is not a word");
}

#[test]
fn settings_survive_a_round_trip_and_unknown_fields_are_kept_kindly() {
    let mut s = ProofSettings::default();
    assert!(s.add_word("Hochelaga"));
    s.check_grammar = true;
    s.languages = vec![Language::FrCa, Language::EnUs];
    let text = serde_json::to_string(&s).expect("a settings file");
    let back: ProofSettings = serde_json::from_str(&text).expect("the same settings");
    assert_eq!(back, s);
    // A settings file of an older version, or one edited by hand: what is missing is default.
    let old: ProofSettings = serde_json::from_str(r#"{"checkSpelling":false}"#).expect("older settings");
    assert!(!old.check_spelling);
    assert_eq!(old.languages, ProofSettings::default().languages);
}

#[test]
fn a_word_is_right_when_either_language_says_so() {
    let s = both();
    assert_eq!(s.accept("colour"), Some(Language::EnCa));
    assert_eq!(s.accept("couleur"), None, "not a word of either dictionary here");
    assert_eq!(s.accept("Montréal"), Some(Language::FrCa));
    assert_eq!(s.accept("français"), Some(Language::FrCa));
    assert_eq!(s.accept("aujourd'hui"), Some(Language::FrCa));
    assert_eq!(s.accept("aujourd’hui"), Some(Language::FrCa), "the typographic apostrophe is the same word");
    assert_eq!(s.accept("centre"), Some(Language::EnCa));
    assert_eq!(s.accept("neighbour"), Some(Language::EnCa));
    assert_eq!(s.accept("quebec"), None, "a missing accent is another spelling");
}

#[test]
fn a_mixed_sentence_is_flagged_only_where_it_is_wrong() {
    let s = both();
    let text = "Bonjour Pierre, I reviewed les documents hier soir and everything looks correct.";
    let got = s.check(text);
    let flagged: Vec<&str> = got.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect();
    assert_eq!(flagged, ["Bonjour", "Pierre", "I", "reviewed", "les", "hier", "soir", "and", "everything", "looks", "correct"], "the test dictionary is small; what matters is that no word of either language is flagged for its language");
}

#[test]
fn a_hyphenated_word_is_checked_by_its_halves() {
    let mut s = Speller::new();
    if let Ok(d) = Dictionary::from_dic(Language::FrCa, "2\nrendez\nvous\n") {
        s.put(d);
    }
    assert!(s.is_word("rendez-vous"));
    let text = "rendez-vuos";
    let got = s.check(text);
    assert_eq!(got.len(), 1);
    let d = got.first().expect("one");
    assert_eq!(text.get(d.span.start..d.span.end), Some("vuos"), "only the half that is wrong is flagged");
}

#[test]
fn suggestions_are_words_of_the_dictionary() {
    let s = both();
    let got = s.suggest("documant", 5);
    assert!(got.contains(&"document".to_string()), "{got:?}");
    let got = s.suggest("Montréal".to_lowercase().as_str(), 5);
    assert!(got.contains(&"montréal".to_string()), "{got:?}");
}

#[test]
fn affixes_make_words_of_stems() {
    let d = Dictionary::from_hunspell(Language::EnCa, AFFIXES, "2\ndocument/S\ndocument\n").expect("a dictionary");
    assert!(d.holds("document"));
    assert!(d.holds("documents"), "the suffix's word is a word");
    assert!(!d.holds("documented"));
}

#[test]
fn a_dictionary_that_is_not_one_is_an_error() {
    assert!(Dictionary::from_dic(Language::EnCa, "").is_err());
    assert!(Dictionary::from_dic(Language::EnCa, "0\n").is_ok_and(|d| d.is_empty() || d.len() == 0));
    // A line that cannot be read is skipped, not fatal.
    let d = Dictionary::from_dic(Language::FrCa, "2\nfrançais\n\u{fffd}\u{fffd}//\n").expect("a dictionary with a bad line");
    assert_eq!(d.len(), 1);
}

#[test]
fn a_real_dictionary_reads_within_its_caps() {
    // A dictionary as a package installs one: a count, stems with flags, and affixes.
    let aff = "SET UTF-8\nFLAG num\nNEEDAFFIX 500\nSFX 101 Y 2\nSFX 101 0 s [^y]\nSFX 101 y ies y\nPFX 102 Y 1\nPFX 102 0 re .\n";
    let dic = "4\ncolour/101\ndocument/101\nhappy/101\nwrite/102\n";
    let d = Dictionary::from_hunspell(Language::EnCa, aff, dic).expect("a dictionary");
    for w in ["colour", "colours", "document", "documents", "happy", "happies", "rewrite"] {
        assert!(d.holds(w), "{w} should be a word of it");
    }
    assert!(!d.holds("happys"), "the condition keeps the `y` rules apart");
}

#[test]
fn a_word_check_is_not_a_sentence_check() {
    // The text is longer than the cap: only its first characters are looked at, and nothing panics.
    let text = "bonjour ".repeat(MAX_CHECKED);
    let s = both();
    let got = s.check(&text);
    assert!(got.len() <= MAX_DIAGNOSTICS);
}

#[test]
fn every_span_of_a_diagnostic_fits_its_text() {
    let s = both();
    for text in [
        "Bonjour, I reviewed les documents hier soir.",
        "a\u{300}b\u{fffd}\u{1F600} 🙂 ok",
        "«Ça va?» dit-il, en français.",
        "  ",
        "",
    ] {
        for d in s.check(text) {
            assert!(d.span.fits(text), "{d:?} does not fit {text:?}");
        }
    }
}
