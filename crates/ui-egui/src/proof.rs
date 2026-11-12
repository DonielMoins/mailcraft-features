// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Spelling and grammar under the text of a message being written (#954): what is looked at, when,
//! and what comes back.
//!
//! - **Spelling** is this computer's, against every dictionary the owner turned on at once
//!   (`mailcraft_model::proof`), so a word is never wrong for being in the other language.
//! - **Grammar** is a LanguageTool server's, and this crate never opens a connection: it hands the
//!   host an address and a text ([`Services::proof_check`]) and takes back the places that came
//!   of it, or why there are none. Without a host that does it (the browser build, the tests) the
//!   grammar part is simply never asked for.
//! - **Nothing is asked for while the owner types**: the text may rest for [`REST`] seconds first,
//!   one grammar request is on its way at a time, and two are never asked for closer together than
//!   [`ASK_EVERY`]. An answer that is for a text the message no longer has is dropped.
//!
//! A place is a stretch of **characters** of the body's text (the positions the editor and the
//! commands count in), worked out once from the byte positions a check answers with. A place is
//! drawn only while the text under it is still the word it was found in: a correction can never
//! land on another word because the message was edited meanwhile.
//!
//! [`Services::proof_check`]: crate::Services::proof_check

use std::sync::mpsc::{Receiver, TryRecvError};

use egui::Ui;
use mailcraft_model::proof::{Diagnostic, Kind, Language, ProofSettings};
use serde_json::{Value, json};

use crate::MailApp;

/// How long the text may rest before it is looked at again, in seconds.
const REST: f64 = 0.25;

/// The least time between two grammar requests while typing, in seconds.
const ASK_EVERY: f64 = 0.8;

/// Most places drawn at once: a message with more mistakes than this shows the first ones.
pub const MOST_PLACES: usize = 2_000;

/// One place under the text, in characters of the body's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub from: usize,
    pub to: usize,
    pub kind: Kind,
    /// The word as it was when it was found: a place is drawn only while the text still says it.
    pub word: String,
    /// The grammar rule it came of, for leaving that rule alone from now on.
    pub rule: Option<String>,
    /// What the server said of it, in its own words.
    pub message: String,
    /// What the server said could stand there instead.
    pub suggestions: Vec<String>,
}

/// What the host is asked for: the grammar server's address, the text, and the language to check
/// it as.
#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub text: String,
    pub language: Language,
}

/// What the host answers: the text that was checked, the places in it (bytes), or why there are
/// none.
#[derive(Clone, Debug)]
pub struct Reply {
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
    pub error: Option<String>,
}

/// Runs proofing for a frame: the body's text looked at when it has rested, the grammar server
/// asked through the host, and any answer taken. Called by the composer, once a frame.
pub(crate) fn run(app: &mut MailApp, ctx: &egui::Context, text: &str) {
    // Taken out and put back so that the window is not borrowed twice: a check needs the session
    // and the places side by side, and both live in it.
    let mut proof = std::mem::take(&mut app.proof);
    proof.frame(app, ctx, text);
    app.proof = proof;
}

/// Asks the grammar server, as File › Options' Test the connection does: one short text, and the
/// answer (or why there is none) said in words on the page, never in a dialog that would stop the
/// window.
pub(crate) fn test(app: &mut MailApp) {
    let settings = app.session.prefs.proof.clone();
    let Some(service) = app.services.proof_check.as_ref() else {
        app.proof.tested = Some(crate::tl!("This build has no grammar server to ask.").to_string());
        return;
    };
    if !settings.grammar_allowed() {
        app.proof.tested = Some(crate::tl!("That server is on another computer, and File › Options does not allow one.").to_string());
        return;
    }
    if settings.grammar_url.trim().is_empty() {
        app.proof.tested = Some(crate::tl!("Name the grammar server's address first.").to_string());
        return;
    }
    let language = settings.active().first().copied().unwrap_or(Language::EnCa);
    // A text with something in it for every language's rules to look at, and nothing of the
    // owner's: the server is told only that this computer is checking a grammar server's answers.
    let text = match language {
        Language::FrFr | Language::FrCa => "Je vais envoyer le document demain.".to_string(),
        _ => "I will send the document tomorrow.".to_string(),
    };
    app.proof.tested = Some(crate::tl!("Asking…").to_string());
    app.proof.testing = Some(service(Request { url: settings.grammar_url.clone(), text, language }));
}

/// What the connection test said, once it has an answer; `None` when no test was made.
pub(crate) fn test_result(app: &mut MailApp) -> Option<String> {
    let channel = app.proof.testing.as_ref()?;
    let reply = match channel.try_recv() {
        Ok(reply) => reply,
        Err(TryRecvError::Empty) => return Some(crate::tl!("Asking…").to_string()),
        Err(TryRecvError::Disconnected) => Reply { text: String::new(), diagnostics: Vec::new(), error: Some("the answer was lost".into()) },
    };
    app.proof.testing = None;
    let said = match reply.error {
        Some(error) => error,
        None => format!("{} {}", crate::tl!("The server answered:"), crate::tln!(reply.diagnostics.len(), "{n} note", "{n} notes")),
    };
    app.proof.tested = Some(said.clone());
    Some(said)
}

/// What proofing has found, and what it is waiting for.
#[derive(Default)]
pub struct Proof {
    /// The places of the body, in characters.
    found: Vec<Place>,
    /// The body's text as the last frame saw it, and when it last changed.
    seen: String,
    edited_at: Option<f64>,
    /// A grammar answer on its way, with the text it was asked for.
    asking: Option<(String, Receiver<Reply>)>,
    /// When the grammar server was last asked.
    asked_at: f64,
    /// The last thing the grammar server said, or why it was not asked, in words for the owner.
    pub(crate) note: Option<String>,
    /// Whether the grammar server answered the last time it was asked.
    pub(crate) online: Option<bool>,
    /// Look again now, whatever the rest period says.
    pub(crate) again: bool,
    /// How many grammar requests were made this session, for the tests and the benchmarks.
    pub(crate) asked: usize,
    /// A connection test on its way (File › Options), and what the last one came to.
    pub(crate) testing: Option<Receiver<Reply>>,
    pub(crate) tested: Option<String>,
}

impl std::fmt::Debug for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proof").field("places", &self.found.len()).field("asking", &self.asking.is_some()).finish()
    }
}

impl Proof {
    /// The places of `text` as they are now: those of the words they were found in, and only those
    /// (the rest would underline the wrong text after an edit).
    pub fn places(&self, text: &str) -> Vec<Place> {
        self.found.iter().filter(|p| still_there(text, p)).cloned().collect()
    }

    /// The place under the character `at` of `text`, if there is one.
    pub fn place_at(&self, text: &str, at: usize) -> Option<Place> {
        self.found.iter().find(|p| p.from <= at && at <= p.to && still_there(text, p)).cloned()
    }

    /// Whether anything was found at all.
    pub fn is_empty(&self) -> bool {
        self.found.is_empty()
    }

    /// Forgets what was found, when checking is turned off.
    pub(crate) fn clear(&mut self, text: &str) {
        self.found.clear();
        self.seen = text.to_string();
        self.asking = None;
    }

    /// Looks at `text` again if it has rested, asks the grammar server when it is on, and takes
    /// any answer that has come. Called once a frame while a message is being written.
    pub(crate) fn frame(&mut self, app: &mut MailApp, ctx: &egui::Context, text: &str) {
        let now = ctx.input(|i| i.time);
        let settings = app.session.prefs.proof.clone();
        if !settings.any() {
            self.clear(text);
            return;
        }
        if self.seen != text {
            self.seen = text.to_string();
            self.edited_at = Some(now);
        }
        self.take_answer(app, ctx, text);
        let rested = self.again || self.edited_at.is_none_or(|t| now - t >= REST);
        if rested && settings.check_spelling {
            self.found = spelling(app, text);
        } else if rested && !settings.check_spelling {
            self.found.retain(|p| p.kind != Kind::Spelling);
        }
        if rested && settings.check_grammar && settings.grammar_allowed() {
            self.ask(app, ctx, now, &settings, text);
            if self.edited_at.is_none() || self.again {
                self.edited_at = Some(now);
            }
        }
        self.again = false;
        if self.asking.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    /// Asks the grammar server through the host, when one is on, nothing is on its way, and the
    /// last request is not too near.
    fn ask(&mut self, app: &mut MailApp, ctx: &egui::Context, now: f64, settings: &ProofSettings, text: &str) {
        let Some(service) = app.services.proof_check.as_ref() else {
            self.note = Some("Grammar is checked by a LanguageTool server; this build has none to ask.".to_string());
            return;
        };
        if self.asking.is_some() || text.trim().is_empty() || now - self.asked_at < ASK_EVERY {
            return;
        }
        self.asked_at = now;
        self.asked += 1;
        let language = settings.active().first().copied().unwrap_or(Language::EnCa);
        let request = Request { url: settings.grammar_url.clone(), text: text.to_string(), language };
        let reply = service(request);
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
        self.asking = Some((text.to_string(), reply));
    }

    /// Takes the answer that has come, when it is for the text the message has now.
    fn take_answer(&mut self, app: &mut MailApp, ctx: &egui::Context, text: &str) {
        let Some((asked_for, channel)) = &self.asking else { return };
        let same = asked_for == text;
        let answer = match channel.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Reply { text: asked_for.clone(), diagnostics: Vec::new(), error: Some("the grammar check was lost".into()) })
            }
        };
        let Some(reply) = answer else { return };
        self.asking = None;
        self.online = Some(reply.error.is_none());
        self.note = reply.error.clone();
        // What the server said of a text the message no longer has is not this text's.
        if !same || reply.text != text {
            return;
        }
        let grammar: Vec<Place> = reply.diagnostics.iter().filter_map(|d| as_place(text, d)).collect();
        self.found.retain(|p| p.kind == Kind::Spelling);
        self.found.extend(grammar);
        self.found.sort_by_key(|p| (p.from, p.to));
        self.found.truncate(MOST_PLACES);
        let _ = (app, ctx);
    }
}

/// Whether the text at `p`'s place is still the word the place was found in.
fn still_there(text: &str, p: &Place) -> bool {
    chars_slice(text, p.from, p.to).is_some_and(|w| w == p.word)
}

/// The characters `from..to` of `text`.
pub fn chars_slice(text: &str, from: usize, to: usize) -> Option<&str> {
    if from >= to {
        return None;
    }
    let start = byte_of(text, from)?;
    let end = byte_of(text, to)?;
    text.get(start..end)
}

/// The byte of the character at `at` of `text`.
fn byte_of(text: &str, at: usize) -> Option<usize> {
    if at == 0 {
        return Some(0);
    }
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        seen += 1;
        if seen == at {
            return Some(i + c.len_utf8());
        }
    }
    (seen == at).then_some(text.len())
}

/// A diagnostic's place, in characters, with the word it is on.
fn as_place(text: &str, d: &Diagnostic) -> Option<Place> {
    if !d.span.fits(text) {
        return None;
    }
    let from = text.get(..d.span.start)?.chars().count();
    let to = text.get(..d.span.end)?.chars().count();
    let word = text.get(d.span.start..d.span.end)?.to_string();
    (from < to).then_some(Place {
        from,
        to,
        kind: d.kind,
        word,
        rule: d.rule.clone(),
        message: d.message.clone(),
        suggestions: d.suggestions.clone(),
    })
}

/// The spelling places of `text`, from this computer's dictionaries, in characters.
fn spelling(app: &mut MailApp, text: &str) -> Vec<Place> {
    let settings = app.session.prefs.proof.clone();
    if !settings.check_spelling {
        return Vec::new();
    }
    let found = app.session.proof.check(&settings, text);
    let mut out: Vec<Place> = found.iter().filter_map(|d| as_place(text, d)).collect();
    out.truncate(MOST_PLACES);
    out
}

/// The whole body checked as the owner asked (Review › Spelling & Grammar): the spelling places
/// here, and the grammar places the window already has for this text.
pub fn grammar_places(app: &MailApp, text: &str) -> Vec<Value> {
    app.proof
        .found
        .iter()
        .filter(|p| p.kind != Kind::Spelling && still_there(text, p))
        .filter_map(|p| {
            let start = byte_of(text, p.from)?;
            let end = byte_of(text, p.to)?;
            Some(json!({
                "start": start,
                "end": end,
                "kind": if p.kind == Kind::Style { "style" } else { "grammar" },
                "message": p.message,
                "suggestions": p.suggestions,
                "rule": p.rule,
            }))
        })
        .collect()
}

/// The menu a right click on a place shows: what could stand there instead, then Add to
/// Dictionary, Ignore Once and Ignore All of This Rule. Every item runs the command the keyboard
/// and a script run, so nothing here changes the text by itself. Answers whether it should close.
pub(crate) fn menu(app: &mut MailApp, ui: &Ui, place: &Place) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique(("mailcraft-proof-menu", place.from, place.to)))
        .order(egui::Order::Foreground)
        .fixed_pos(ui.input(|i| i.pointer.interact_pos()).unwrap_or(egui::Pos2::ZERO))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(320.0);
                if !place.word.is_empty() {
                    ui.label(egui::RichText::new(&place.word).strong());
                }
                let suggestions = app.run("proof.suggest", json!({ "word": place.word, "limit": 6 })).ok().and_then(|v| {
                    v.get("suggestions").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
                });
                match suggestions {
                    Some(list) if !list.is_empty() => {
                        for s in list {
                            if ui.add(egui::Button::new(egui::RichText::new(&s)).frame(false)).clicked() {
                                let _ = app.execute("proof.change", json!({ "text": s, "from": place.from, "to": place.to }));
                                close = true;
                                break;
                            }
                        }
                    }
                    _ => {
                        ui.label(crate::tl!("No suggestions"));
                    }
                }
                if !place.message.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(&place.message).small());
                }
                ui.separator();
                if ui.button(crate::tl!("Ignore Once")).clicked() {
                    let _ = app.execute("proof.ignore", json!({}));
                    close = true;
                }
                if ui.button(crate::tl!("Add to Dictionary")).clicked() {
                    let _ = app.execute("proof.addWord", json!({ "word": place.word }));
                    close = true;
                }
                if let Some(rule) = place.rule.clone()
                    && ui.button(crate::tl!("Ignore This Rule")).clicked()
                {
                    let _ = app.execute("proof.ignoreRule", json!({ "rule": rule }));
                    close = true;
                }
            });
        });
    close
}
