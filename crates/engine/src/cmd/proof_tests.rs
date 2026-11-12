// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Spelling and grammar while writing (#954), through the commands: what is found, what a
//! correction does to the message and its undo, and what Review › Spelling & Grammar walks
//! through.
//!
//! The dictionaries here are invented ones written into a folder of this test's own, so that what
//! a test says is a word never depends on what this computer has installed.

use mailcraft_model::proof::{Kind, Language};
use serde_json::json;
use tempdir::Dir;

use crate::Session;
use crate::sample;

/// The instant these tests are at: the sample's mailbox, as its commands see it.
fn now() -> mailcraft_time::Timestamp {
    1_750_000_000
}

/// Two dictionaries of a few words each, in a folder of their own, and a session told to use it.
fn session(name: &str) -> (Session, Dir) {
    let dir = Dir::new(name);
    dir.write("en_CA.dic", "6\ncolour\ncentre\ndocument\ndocuments\nthe\ntomorrow\n");
    dir.write("fr_CA.dic", "5\nbonjour\nfrançais\naujourd'hui\nMontréal\ndocument\n");
    let mut s = Session::at(sample::mailbox(now(), 0), now(), 0);
    s.prefs.proof.dictionary_dir = dir.path().to_string_lossy().to_string();
    (s, dir)
}

/// A message being written with `text` in it, and nothing else: the signature a new message is
/// given is taken out again, so that what a test counts is what it wrote.
fn composing(s: &mut Session, text: &str) {
    s.run("mail.new", &json!({})).expect("a message");
    s.run("compose.type", &json!({ "text": text })).expect("the text");
    let len = s.doc().map(|(b, _)| b.text().len()).unwrap_or(0);
    let end = text.chars().count();
    if len > end {
        s.run("compose.delete", &json!({ "from": end, "to": len })).expect("the signature taken out");
    }
}

/// The words of `sentence` the answer says are wrong, in order (the body also holds the
/// signature a new message is given, whose words are nobody's business here).
fn flagged(v: &serde_json::Value) -> Vec<String> {
    let in_it = |w: &String, whole: &str| whole.contains(w.as_str());
    v.get("diagnostics")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    let span = d.get("span")?;
                    span.get("start").and_then(|x| x.as_u64()).zip(span.get("end").and_then(|x| x.as_u64()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(start, end)| v.get("text").and_then(|t| t.as_str()).and_then(|t| t.get(start as usize..end as usize)).map(str::to_string))
        .filter(|w| in_it(w, v.get("text").and_then(|t| t.as_str()).unwrap_or_default()))
        .collect()
}

#[test]
fn a_word_of_either_language_is_right() {
    let (mut s, _dir) = session("either");
    composing(&mut s, "Bonjour Pierre, the colour of the centre is right.");
    let answer = s.run("proof.check", &json!({})).expect("a check");
    let wrong = flagged(&answer);
    assert!(!wrong.contains(&"colour".to_string()), "a Canadian dictionary knows it: {wrong:?}");
    assert!(!wrong.contains(&"centre".to_string()));
    assert!(!wrong.contains(&"bonjour".to_string()), "the French dictionary knows it: {wrong:?}");
    assert!(wrong.contains(&"Pierre".to_string()), "no dictionary here knows the name: {wrong:?}");
    assert_eq!(answer.get("loaded").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn the_two_languages_are_both_read() {
    let (mut s, _dir) = session("both");
    let answer = s.run("proof.status", &json!({})).expect("the settings");
    let read = answer.get("read").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    assert!(read.len() >= 2, "both dictionaries were read: {read:?}");
    assert_eq!(answer.get("loaded").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn no_dictionary_means_no_places_rather_than_every_word() {
    let mut s = Session::at(sample::mailbox(now(), 0), now(), 0);
    s.prefs.proof.dictionary_dir = "/nonexistent/folder/of/dictionaries".into();
    composing(&mut s, "Bonjour, this is a message with words in it.");
    let answer = s.run("proof.check", &json!({})).expect("a check");
    assert!(flagged(&answer).is_empty(), "nothing is flagged when there is nothing to check against");
    assert_eq!(answer.get("loaded").and_then(|v| v.as_bool()), Some(false));
    assert!(answer.get("note").and_then(|n| n.as_str()).is_some(), "and it says why");
}

#[test]
fn a_word_the_owner_added_is_a_word() {
    let (mut s, _dir) = session("personal");
    composing(&mut s, "Hochelaga is where the mountain is.");
    assert!(flagged(&s.run("proof.check", &json!({})).expect("a check")).contains(&"Hochelaga".to_string()));
    let answer = s.run("proof.addWord", &json!({ "word": "Hochelaga" })).expect("a word added");
    assert_eq!(answer.get("added").and_then(|v| v.as_bool()), Some(true));
    assert!(!flagged(&s.run("proof.check", &json!({})).expect("a check")).contains(&"Hochelaga".to_string()), "it is a word now");
    let again = s.run("proof.addWord", &json!({ "word": "hochelaga" })).expect("the same word again");
    assert_eq!(again.get("added").and_then(|v| v.as_bool()), Some(false), "but only once");
    let removed = s.run("proof.removeWord", &json!({ "word": "HOCHELAGA" })).expect("taken out");
    assert_eq!(removed.get("removed").and_then(|v| v.as_bool()), Some(true));
    assert!(flagged(&s.run("proof.check", &json!({})).expect("a check")).contains(&"Hochelaga".to_string()));
}

#[test]
fn the_personal_dictionary_is_kept_in_the_settings() {
    let (mut s, _dir) = session("kept");
    s.run("proof.addWord", &json!({ "word": "Hochelaga" })).expect("a word added");
    let text = serde_json::to_string(&s.prefs).expect("settings");
    let back: crate::Prefs = serde_json::from_str(&text).expect("the same settings");
    assert_eq!(back.proof.personal_words(), ["Hochelaga"]);
}

#[test]
fn suggestions_are_words_and_a_correction_is_one_step_of_undo() {
    let (mut s, _dir) = session("change");
    composing(&mut s, "the documet is here");
    let answer = s.run("proof.check", &json!({})).expect("a check");
    let places = flagged(&answer);
    assert_eq!(places, ["documet", "is", "here"], "the test dictionary knows only its own words");
    let suggest = s.run("proof.suggest", &json!({ "word": "documet" })).expect("suggestions");
    let list = suggest.get("suggestions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    assert!(list.iter().any(|s| s == "document"), "{list:?}");

    // The place, in characters of the body: `documet` is the second word.
    let start = 4;
    let end = start + "documet".chars().count();
    s.run("proof.change", &json!({ "text": "document", "from": start, "to": end })).expect("changed");
    let body = s.doc().map(|(b, _)| b.plain()).unwrap_or_default();
    assert_eq!(body.trim_end(), "the document is here");
    // One step back, and the message is as it was: a correction is one step of its undo.
    s.run("compose.undo", &json!({})).expect("undone");
    let body = s.doc().map(|(b, _)| b.plain()).unwrap_or_default();
    assert_eq!(body.trim_end(), "the documet is here");
    assert!(s.run("compose.undo", &json!({})).is_ok(), "and the typing before it is another step");
}

#[test]
fn a_correction_keeps_the_look_of_the_word() {
    let (mut s, _dir) = session("look");
    composing(&mut s, "the documet is here");
    s.run("compose.select", &json!({ "from": 4, "to": 11 })).expect("selected");
    s.run("format.bold", &json!({})).expect("bold");
    // A right click's correction names the place it was made at.
    s.run("proof.change", &json!({ "text": "document", "from": 4, "to": 11 })).expect("changed");
    let (body, _) = s.doc().expect("a message");
    let bold: String = body.text().paragraphs().iter().flat_map(|p| p.runs.iter()).filter(|r| r.style.bold).map(|r| r.text.clone()).collect();
    assert_eq!(bold, "document", "what is put in is bold as what it replaced was, and nothing else is");
}

#[test]
fn review_walks_the_places_and_changes_one() {
    let (mut s, _dir) = session("walk");
    composing(&mut s, "the documet and the sentance");
    let opened = s.run("review.spelling", &json!({ "again": true })).expect("the walk");
    assert!(opened.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= 2, "the places of the message and of its signature");
    let at = opened.get("at").cloned().unwrap_or_default();
    assert_eq!(at.get("word").and_then(|v| v.as_str()), Some("documet"));
    s.run("proof.change", &json!({ "text": "document" })).expect("changed");
    let body = s.doc().map(|(b, _)| b.plain()).unwrap_or_default();
    assert!(body.starts_with("the document and the sentance"), "{body:?}");
    // The walk goes on with what is left, and ends.
    let rest = s.run("review.spelling", &json!({})).expect("the walk again");
    let next = rest.get("at").and_then(|a| a.get("word")).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    assert_ne!(next, "documet", "the corrected word is not looked at again");
    assert!(["is", "here", "and", "the", "sentance"].contains(&next.as_str()), "{next}");
    // Whatever is left is left alone, one at a time, until the walk is over.
    let mut done = false;
    for _ in 0..50 {
        let answer = s.run("proof.ignore", &json!({})).expect("left alone");
        if answer.get("done").and_then(|v| v.as_bool()) == Some(true) {
            done = true;
            break;
        }
    }
    assert!(done, "the walk ends");
}

#[test]
fn a_walk_of_a_message_that_changed_is_not_applied_to_it() {
    let (mut s, _dir) = session("stale");
    composing(&mut s, "the documet is here");
    s.run("review.spelling", &json!({ "again": true })).expect("the walk");
    // The owner types somewhere else: the place no longer belongs to this text.
    s.run("compose.type", &json!({ "text": "more " })).expect("typed");
    assert!(s.run("proof.change", &json!({ "text": "document" })).is_err(), "a stale place is refused");
    let body = s.doc().map(|(b, _)| b.plain()).unwrap_or_default();
    assert!(body.contains("the documet is here"), "the text is as it was: {body:?}");
    assert!(body.contains("heremore"), "and the typing was all that happened: {body:?}");
}

#[test]
fn settings_are_changed_and_read_back() {
    let (mut s, _dir) = session("set");
    let answer = s
        .run("proof.set", &json!({ "checkSpelling": false, "checkGrammar": true, "languages": ["fr-CA"], "grammarUrl": "http://127.0.0.1:8081" }))
        .expect("set");
    let settings = answer.get("settings").cloned().unwrap_or_default();
    assert_eq!(settings.get("checkSpelling").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(settings.get("checkGrammar").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(settings.get("languages"), Some(&json!(["fr-CA"])));
    // A server on another computer needs the owner's yes, said in words.
    assert!(s.run("proof.set", &json!({ "grammarUrl": "https://languagetool.example" })).is_err());
    assert!(s.run("proof.set", &json!({ "allowRemote": true, "grammarUrl": "https://languagetool.example" })).is_ok());
    assert_eq!(s.prefs.proof.grammar_url, "https://languagetool.example");
    assert!(s.run("proof.set", &json!({ "grammarUrl": "file:///etc/passwd" })).is_err(), "and only http(s) addresses");
}

#[test]
fn grammar_places_from_the_host_are_taken_only_where_they_fit() {
    let (mut s, _dir) = session("grammar");
    composing(&mut s, "She have sent the documents.");
    let walk = s
        .run(
            "review.spelling",
            &json!({
                "again": true,
                "grammar": [
                    { "start": 0, "end": 8, "kind": "grammar", "message": "The verb does not agree.", "suggestions": ["She has"], "rule": "HE_VERB_AGR" },
                    { "start": 20, "end": 900, "kind": "grammar", "message": "not this text's" },
                    { "start": 7, "end": 7, "kind": "grammar", "message": "empty" }
                ]
            }),
        )
        .expect("the walk");
    let rules = walk.get("rules").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    assert_eq!(rules, vec![json!("HE_VERB_AGR")], "the two that do not fit are left out");
    assert!(walk.get("at").and_then(|a| a.get("word")).is_some(), "there is something to look at");
}

#[test]
fn a_quiet_rule_is_left_out() {
    let (mut s, _dir) = session("quiet");
    composing(&mut s, "She have sent the documents.");
    s.run("proof.ignoreRule", &json!({ "rule": "HE_VERB_AGR" })).expect("quiet");
    let walk = s
        .run("review.spelling", &json!({ "again": true, "grammar": [{ "start": 0, "end": 8, "kind": "grammar", "rule": "HE_VERB_AGR" }] }))
        .expect("the walk");
    assert!(walk.get("rules").and_then(|v| v.as_array()).is_none_or(|r| r.is_empty()), "the rule is the owner's to leave quiet");
    s.run("proof.forgetRules", &json!({})).expect("forgotten");
    assert!(s.prefs.proof.ignored_rules.is_empty());
}

#[test]
fn a_language_can_be_turned_off() {
    let (mut s, _dir) = session("off");
    composing(&mut s, "Bonjour le monde");
    s.run("proof.set", &json!({ "languages": ["en-CA"] })).expect("English alone");
    let wrong = flagged(&s.run("proof.check", &json!({})).expect("a check"));
    assert!(wrong.contains(&"Bonjour".to_string()), "French is off, so a French word is not a word: {wrong:?}");
    assert_eq!(s.prefs.proof.languages, vec![Language::EnCa]);
}

#[test]
fn a_place_is_never_outside_the_text() {
    let (mut s, _dir) = session("fits");
    for text in ["Bonjour 🙂 Montréal", "a\u{300}bc documet", "", "   ", "\u{fffc}"] {
        composing(&mut s, text);
        let answer = s.run("proof.check", &json!({})).expect("a check");
        let span_text = answer.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
        for d in answer.get("diagnostics").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
            let span = d.get("span").cloned().unwrap_or_default();
            let (Some(start), Some(end)) = (span.get("start").and_then(|v| v.as_u64()), span.get("end").and_then(|v| v.as_u64())) else {
                panic!("a place with no ends: {d:?}");
            };
            let (start, end) = (start as usize, end as usize);
            assert!(end > start && end <= span_text.len(), "{d:?} does not fit");
            assert!(span_text.is_char_boundary(start) && span_text.is_char_boundary(end), "{d:?} splits a character");
        }
        let _ = s.run("compose.discard", &json!({}));
    }
}

#[test]
fn the_kind_of_a_place_says_what_found_it() {
    let (mut s, _dir) = session("kind");
    composing(&mut s, "the documet");
    let answer = s.run("proof.check", &json!({})).expect("a check");
    let d = answer.get("diagnostics").and_then(|d| d.as_array()).and_then(|a| a.first()).cloned().unwrap_or_default();
    assert_eq!(d.get("kind").and_then(|v| v.as_str()), Some("spelling"));
    assert_eq!(d.get("source").and_then(|v| v.as_str()), Some("local"));
    assert_eq!(d.get("kind").and_then(|v| v.as_str()), Some("spelling"));
    assert_eq!(Kind::Spelling, Kind::Spelling);
}

/// A folder of this test's own, taken away when the test ends.
pub(crate) mod tempdir {
    use std::path::{Path, PathBuf};

    /// A folder under the system's temporary folder, named for the test that asked for it.
    pub struct Dir(PathBuf);

    impl Dir {
        /// Makes the folder.
        pub fn new(name: &str) -> Dir {
            let path = std::env::temp_dir().join(format!("mailcraft-proof-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
            Dir(path)
        }

        /// Writes `text` into `file`.
        pub fn write(&self, file: &str, text: &str) {
            let _ = std::fs::write(self.0.join(file), text);
        }

        /// The folder.
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
