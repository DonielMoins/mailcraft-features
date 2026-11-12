// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Spelling and grammar while writing (#954): the checks, Review › Spelling & Grammar's walk, and
//! the settings that say what is checked and where the words come from.
//!
//! Spelling is checked here, on this computer, against every dictionary the owner turned on at
//! once: a word is right if any of them knows it, so an English word in a French sentence is not a
//! mistake and neither is the other way round ([`mailcraft_model::proof::Speller`]). Grammar is
//! the host's (a LanguageTool server, asked by the window or the CLI with `mailcraft_net`); what
//! these commands do with it is take its places in, check that they still fit the text, and walk
//! the owner through them.
//!
//! Nothing here writes to the mailbox, and nothing here opens a connection: `proof.check` and
//! `proof.suggest` change nothing at all, and a correction is one step of the message's own undo
//! ([`RichBody::replace_range`]).
//!
//! Message text is data: it is not logged, and no answer of these commands carries anything of a
//! message that was not asked for (the places found, their words and what could stand in them).

use mailcraft_model::proof::{Diagnostic, Kind, Language, ProofSettings, Span};
use mailcraft_model::rich::RichBody;
use serde_json::{Value, json};

use super::bad;
use super::format::{needs_doc, state};
use crate::proof::{Proofing, is_sound};
use crate::{CmdResult, CommandSpec, Session, p};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("proof.check", "Check Spelling", "Review › Proofing", check)
            .pure()
            .params(PARAMS)
            .when_writing(needs_doc),
        CommandSpec::new("proof.suggest", "Suggestions", "Review › Proofing", suggest).pure().params(SUGGEST).when_writing(needs_doc),
        CommandSpec::new("proof.status", "Spelling & Grammar Settings", "Review › Proofing", status).pure(),
        CommandSpec::new("proof.set", "Spelling & Grammar Settings", "Review › Proofing", set).pure().params(SET),
        CommandSpec::new("proof.addWord", "Add to Dictionary", "Review › Proofing", add_word)
            .pure()
            .params(r#"{"word": string} — the owner's own word, kept in their settings; answers the settings as they are now"#),
        CommandSpec::new("proof.removeWord", "Remove from Dictionary", "Review › Proofing", remove_word)
            .pure()
            .params(r#"{"word": string} — a word of the owner's own out of the personal dictionary again"#),
        CommandSpec::new("proof.ignoreRule", "Ignore Grammar Rule", "Review › Proofing", ignore_rule)
            .pure()
            .params(r#"{"rule": string} — a LanguageTool rule id kept quiet from now on; answers the settings"#),
        CommandSpec::new("proof.forgetRules", "Check Every Rule Again", "Review › Proofing", forget_rules).pure(),
        CommandSpec::new("review.spelling", "Spelling & Grammar", "Review › Proofing", review)
            .pure()
            .params(REVIEW)
            .when_writing(needs_doc),
        CommandSpec::new("proof.change", "Change", "Review › Proofing", change)
            .params(r#"{"text"?: string, "from"?: number, "to"?: number} — puts `text` (default: the first suggestion) in place of `from`..`to` (characters of the body; default: the place Review › Spelling & Grammar is at) as one step of the message's undo; answers the walk"#)
            .when_writing(needs_doc),
        CommandSpec::new("proof.ignore", "Ignore Once", "Review › Proofing", ignore_once)
            .pure()
            .params(r#"{"rule"?: string} — leaves the place on screen alone this time; with a rule, that grammar rule is left alone for the rest of the walk too"#)
            .when_writing(needs_doc),
        CommandSpec::new("proof.close", "Close Spelling & Grammar", "Review › Proofing", close).pure(),
    ]
}

const PARAMS: &str = r#"{"text"?: string, "from"?: number, "to"?: number} (default: the selection when there is one, else the whole body) — the places a dictionary of this computer finds, in bytes of that text; answers {"diagnostics": [...], "text": the text checked, "languages": [...], "loaded": bool, "note"?: why no dictionary}"#;

const SUGGEST: &str = r#"{"word": string, "language"?: "en-CA"|"fr-CA"|"en-US"|"fr-FR", "limit"?: number (1–8, default 5)} — what could stand in for it; answers {"suggestions": [...]}"#;

const SET: &str = r##"{"checkSpelling"?: bool, "checkGrammar"?: bool, "asYouType"?: bool, "languages"?: ["en-CA","fr-CA",…], "grammarUrl"?: string (http:// or https://; not this computer's unless "allowRemote"), "allowRemote"?: bool, "dictionaryDir"?: string (a folder, "" for the usual places)} — answers the settings as they are now"##;

const REVIEW: &str = r#"{"grammar"?: [{"start": number, "end": number, "kind": "grammar"|"style", "message": string, "suggestions": [string], "rule": string, "language": string}], "again"?: bool} (the grammar places the host brought back for the body as it is now; "again" starts over, else the walk goes on where it was) — answers {"done": bool, "position": number, "total": number, "at"?: {"start", "end", "kind", "word", "message", "suggestions", "rule", "language"}, "text": the body as it was checked}"#;

/// The whole body, when `from` and `to` name nothing.
fn text_of(body: &RichBody, from: Option<usize>, to: Option<usize>) -> String {
    let plain = body.plain();
    match (from, to) {
        (None, None) => plain,
        (from, to) => {
            let (a, b) = (from.unwrap_or(0), to.unwrap_or(usize::MAX));
            let (a, b) = (a.min(b), a.max(b));
            chars_between(&plain, a, b)
        }
    }
}

/// The characters `from..to` (in characters) of `text`.
fn chars_between(text: &str, from: usize, to: usize) -> String {
    text.chars().skip(from).take(to.saturating_sub(from)).collect()
}

/// The characters `span` covers, in a text whose positions are characters.
fn char_range(text: &str, span: Span) -> Option<(usize, usize)> {
    if !span.fits(text) {
        return None;
    }
    let start = text.get(..span.start)?.chars().count();
    let end = text.get(..span.end)?.chars().count();
    (start < end).then_some((start, end))
}

fn check(s: &mut Session, v: &Value) -> CmdResult {
    let (body, _) = s.doc().ok_or_else(|| bad("no message is being written"))?;
    let from = p::u64(v, "from").and_then(|n| usize::try_from(n).ok());
    let to = p::u64(v, "to").and_then(|n| usize::try_from(n).ok());
    let text = text_of(body, from, to);
    let settings = s.prefs.proof.clone();
    let diagnostics = if settings.check_spelling { s.proof.check(&settings, &text) } else { Vec::new() };
    let note = s.proof.note.clone();
    let loaded = !s.proof.words(&settings).is_empty();
    let languages: Vec<&str> = s.proof.words(&settings).languages().iter().map(|l| l.code()).collect();
    Ok(json!({
        "diagnostics": diagnostics,
        "text": text,
        "languages": languages,
        "loaded": loaded,
        "note": note,
    }))
}

fn suggest(s: &mut Session, v: &Value) -> CmdResult {
    let word = p::req_str(v, "word")?;
    let limit = p::u64(v, "limit").and_then(|n| usize::try_from(n).ok()).unwrap_or(5).clamp(1, 8);
    let settings = s.prefs.proof.clone();
    Ok(json!({ "suggestions": s.proof.suggest(&settings, word, limit) }))
}

/// The settings as they are, and whether a dictionary was read.
fn settings_state(s: &mut Session) -> Value {
    let settings = s.prefs.proof.clone();
    let loaded = !s.proof.words(&settings).is_empty();
    let note = s.proof.note.clone();
    let languages: Vec<&str> = s.proof.words(&settings).languages().iter().map(|l| l.code()).collect();
    json!({
        "settings": settings,
        "loaded": loaded,
        "read": languages,
        "note": note,
        "grammarAllowed": settings.grammar_allowed(),
    })
}

fn status(s: &mut Session, _: &Value) -> CmdResult {
    Ok(settings_state(s))
}

fn set(s: &mut Session, v: &Value) -> CmdResult {
    let mut settings = s.prefs.proof.clone();
    if let Some(b) = p::opt_bool(v, "checkSpelling")? {
        settings.check_spelling = b;
    }
    if let Some(b) = p::opt_bool(v, "checkGrammar")? {
        settings.check_grammar = b;
    }
    if let Some(b) = p::opt_bool(v, "asYouType")? {
        settings.as_you_type = b;
    }
    if let Some(b) = p::opt_bool(v, "allowRemote")? {
        // Turning remote servers off leaves an address that can no longer be spoken to: the
        // server is turned off with it, so that nothing is sent on an old yes.
        if !b {
            settings.check_grammar = settings.check_grammar && mailcraft_model::proof::address_allowed(&settings.grammar_url, false);
        }
        settings.allow_remote = b;
    }
    if let Some(values) = v.get("languages") {
        let list = values.as_array().ok_or_else(|| bad("`languages` is a list of language codes"))?;
        let mut languages: Vec<Language> = Vec::new();
        for value in list {
            let code = value.as_str().ok_or_else(|| bad("`languages` is a list of language codes"))?;
            let lang = Language::parse(code).ok_or_else(|| bad(format!("`{code}` is no language proofing knows")))?;
            if !languages.contains(&lang) {
                languages.push(lang);
            }
        }
        if languages.is_empty() {
            return Err(bad("`languages` names at least one language"));
        }
        settings.languages = languages;
    }
    if let Some(url) = p::str(v, "grammarUrl") {
        let url = url.trim();
        if !url.is_empty() && !mailcraft_model::proof::address_allowed(url, settings.allow_remote) {
            return Err(bad("the grammar server's address must be http:// or https://, and on another computer only when that is allowed"));
        }
        if !url.is_empty() && url.len() > 2_000 {
            return Err(bad("the grammar server's address is too long"));
        }
        settings.grammar_url = url.to_string();
    }
    if let Some(dir) = p::str(v, "dictionaryDir") {
        if dir.len() > 1_024 {
            return Err(bad("`dictionaryDir` is too long"));
        }
        settings.dictionary_dir = dir.trim().to_string();
        // Another folder: the dictionaries are read again at the next check.
    }
    s.prefs.proof = settings;
    Ok(settings_state(s))
}

fn add_word(s: &mut Session, v: &Value) -> CmdResult {
    let word = p::req_str(v, "word")?.to_string();
    let mut settings = s.prefs.proof.clone();
    let added = settings.add_word(&word);
    s.prefs.proof = settings;
    let mut answer = settings_state(s);
    if let Some(map) = answer.as_object_mut() {
        map.insert("added".into(), json!(added));
    }
    Ok(answer)
}

fn remove_word(s: &mut Session, v: &Value) -> CmdResult {
    let word = p::req_str(v, "word")?.to_string();
    let mut settings = s.prefs.proof.clone();
    let removed = settings.remove_word(&word);
    s.prefs.proof = settings;
    let mut answer = settings_state(s);
    if let Some(map) = answer.as_object_mut() {
        map.insert("removed".into(), json!(removed));
    }
    Ok(answer)
}

fn ignore_rule(s: &mut Session, v: &Value) -> CmdResult {
    let rule = p::req_str(v, "rule")?.trim().to_string();
    if rule.is_empty() || rule.len() > 64 {
        return Err(bad("`rule` is a grammar rule's id"));
    }
    let mut settings = s.prefs.proof.clone();
    if !settings.ignored_rules.iter().any(|r| *r == rule) && settings.ignored_rules.len() < 500 {
        settings.ignored_rules.push(rule);
    }
    s.prefs.proof = settings;
    Ok(settings_state(s))
}

fn forget_rules(s: &mut Session, _: &Value) -> CmdResult {
    s.prefs.proof.ignored_rules.clear();
    Ok(settings_state(s))
}

/// The body as it is, with what has been found in it.
fn review(s: &mut Session, v: &Value) -> CmdResult {
    let (body, _) = s.doc().ok_or_else(|| bad("no message is being written"))?;
    let text = body.plain();
    let settings = s.prefs.proof.clone();
    let again = p::bool(v, "again").unwrap_or(false) || s.proof.current_walk(&text).is_none();
    if !again {
        // The walk goes on where it was: no dictionary is read again and nothing is searched for.
        return Ok(walk_state(s, &text));
    }
    let mut found: Vec<Diagnostic> = if settings.check_spelling { s.proof.check(&settings, &text) } else { Vec::new() };
    found.extend(grammar_places(v, &text).into_iter().filter(|d| d.rule.as_ref().is_none_or(|r| !settings.ignored_rules.contains(r))));
    s.proof.start(&text, found);
    Ok(walk_state(s, &text))
}

/// The grammar places the host brought back, taken only where they still fit the text.
fn grammar_places(v: &Value, text: &str) -> Vec<Diagnostic> {
    let Some(list) = v.get("grammar").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<Diagnostic> = Vec::new();
    for item in list.iter().take(mailcraft_model::proof::MAX_DIAGNOSTICS) {
        let Some(start) = p::u64(item, "start").and_then(|n| usize::try_from(n).ok()) else { continue };
        let Some(end) = p::u64(item, "end").and_then(|n| usize::try_from(n).ok()) else { continue };
        let Some(span) = Span::of(text, start, end) else { continue };
        let kind = match p::str(item, "kind") {
            Some("style") => Kind::Style,
            Some("spelling") => Kind::Spelling,
            _ => Kind::Grammar,
        };
        let mut d = Diagnostic::new(span, kind, mailcraft_model::proof::Source::Server);
        if let Some(message) = p::str(item, "message") {
            d = d.saying(message);
        }
        if let Some(suggestions) = item.get("suggestions").and_then(Value::as_array) {
            d = d.suggesting(suggestions.iter().filter_map(Value::as_str).map(str::to_string));
        }
        d.rule = p::str(item, "rule").map(str::to_string);
        d.language = p::str(item, "language").and_then(Language::parse);
        if is_sound(&d, text) {
            out.push(d);
        }
    }
    out
}

/// What Review › Spelling & Grammar shows: the place on screen, where it is, and the body it was
/// found in (so that the window can compare it with the body as it is now).
fn walk_state(s: &mut Session, text: &str) -> Value {
    let Some(walk) = s.proof.current_walk(text) else {
        return json!({ "done": true, "position": 0, "total": 0 });
    };
    let total = walk.items.len();
    let at = walk.current().map(|d| {
        json!({
            "start": d.span.start,
            "end": d.span.end,
            "kind": d.kind,
            "word": text.get(d.span.start..d.span.end),
            "message": d.message,
            "suggestions": d.suggestions,
            "rule": d.rule,
            "language": d.language.map(|l| l.code()),
        })
    });
    json!({
        "done": at.is_none() || walk.at >= total,
        "position": walk.at.min(total),
        "total": total,
        "at": at,
        "rules": walk.items.iter().filter_map(|d| d.rule.clone()).collect::<Vec<String>>(),
        "text": walk.text,
        "left": walk.left(),
    })
}

fn change(s: &mut Session, v: &Value) -> CmdResult {
    let (body, _) = s.doc().ok_or_else(|| bad("no message is being written"))?;
    let text = body.plain();
    // The place named by `from` and `to` (a right click's, a script's), else the one the walk is at.
    let named = match (p::u64(v, "from"), p::u64(v, "to")) {
        (Some(from), Some(to)) => Some((usize::try_from(from).unwrap_or(usize::MAX), usize::try_from(to).unwrap_or(usize::MAX))),
        _ => None,
    };
    let (_place, from, to, with) = match named {
        Some((from, to)) => {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            let with = p::str(v, "text").map(str::to_string).ok_or_else(|| bad("`text` (string) is required with `from` and `to`"))?;
            let len = text.chars().count();
            if from >= to || to > len {
                return Err(bad("`from` and `to` are not a stretch of the message (in characters)"));
            }
            (None, from, to, with)
        }
        _ => {
            let Some(walk) = s.proof.current_walk(&text) else {
                return Err(bad("Review › Spelling & Grammar was not started, or the message has changed since"));
            };
            let Some(place) = walk.current().cloned() else {
                return Err(bad("there is nothing to change"));
            };
            let with = match p::str(v, "text") {
                Some(t) => t.to_string(),
                None => place.suggestions.first().cloned().ok_or_else(|| bad("this one has no suggestion: put the right words in `text`"))?,
            };
            let (from, to) = char_range(&text, place.span).ok_or_else(|| bad("that place is no longer in the message"))?;
            (Some(place), from, to, with)
        }
    };
    let d = s.doc_mut().ok_or_else(|| bad("no message is being written"))?;
    d.body.replace_range(from, to, &with).map_err(|e| bad(e.to_string()))?;
    *d.dirty = true;
    // The message has changed, so the places found in it no longer belong to it: the walk starts
    // again, at the first place after the correction.
    let after = body_now(s);
    let settings = s.prefs.proof.clone();
    let mut found = if settings.check_spelling { s.proof.check(&settings, &after) } else { Vec::new() };
    let moved = from + with.chars().count();
    found.retain(|d| char_range(&after, d.span).is_some_and(|(a, _)| a >= moved));
    s.proof.start(&after, found);
    let mut answer = walk_state(s, &after);
    if let Some(map) = answer.as_object_mut() {
        map.insert("changed".into(), json!(with));
        map.insert("state".into(), state(s));
    }
    Ok(answer)
}

/// The body's text as it is now.
fn body_now(s: &Session) -> String {
    s.doc().map(|(b, _)| b.plain()).unwrap_or_default()
}

fn ignore_once(s: &mut Session, v: &Value) -> CmdResult {
    let text = body_now(s);
    if let Some(rule) = p::str(v, "rule").filter(|r| !r.is_empty() && r.chars().count() <= 64) {
        let mut settings = s.prefs.proof.clone();
        if !settings.ignored_rules.iter().any(|r| r == rule) && settings.ignored_rules.len() < 500 {
            settings.ignored_rules.push(rule.to_string());
        }
        s.prefs.proof = settings;
    }
    s.proof.ignore(&text);
    Ok(walk_state(s, &text))
}

fn close(s: &mut Session, _: &Value) -> CmdResult {
    s.proof.close();
    Ok(json!({ "done": true }))
}

/// Proofing's state for the window: whether a walk is open.
pub fn walking(proofing: &Proofing) -> bool {
    proofing.walk.is_some()
}

/// The settings, for a caller that has a session but no proofing state of its own.
pub fn settings_of(s: &Session) -> ProofSettings {
    s.prefs.proof.clone()
}
