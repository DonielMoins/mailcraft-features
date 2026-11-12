// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Review › Spelling & Grammar (#954): one place at a time, what could stand there instead, and
//! Change, Ignore Once, Ignore This Rule, Add to Dictionary or Close.
//!
//! The places are the engine's ([`mailcraft_engine::proof::Walk`]): the words this computer's
//! dictionaries do not know, and the grammar places the window already brought back from the
//! server for this text. The dialog changes nothing itself — every button runs the command a
//! script or an agent runs, so a message is corrected the same way from the window, the CLI or
//! MCP, and a correction is one step of the message's own undo.

use egui::{RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::MailApp;
use crate::dialogs::button_row;
use crate::theme::Tokens;
use crate::widgets::primary_button;

pub const WIDTH: f32 = 460.0;

/// The dialog: what is to be put in place of the place on screen, and the last thing that was done.
#[derive(Clone, Debug, Default)]
pub struct Spelling {
    /// What Change puts in: the first suggestion to begin with, and whatever the owner types.
    pub replacement: String,
    /// What the last button came to, in a line (a word was added, a rule is quiet…).
    pub note: String,
}

impl Spelling {
    /// Opens over the message being written (the walk itself starts at the dialog's first frame).
    pub fn open(app: &MailApp) -> Option<Spelling> {
        app.session.doc()?;
        Some(Spelling::default())
    }

    /// The dialog's contents; whether it is done.
    pub fn show(&mut self, app: &mut MailApp, ui: &mut Ui, enter: bool) -> bool {
        let t = Tokens::get(ui.ctx());
        let mut done = false;
        // The first frame starts the walk, with the grammar places the window has for the body as
        // it is now; afterwards it goes on where it was.
        let body = app.session.doc().map(|(b, _)| b.plain()).unwrap_or_default();
        if app.session.proof.current_walk(&body).is_none() {
            let grammar = crate::proof::grammar_places(app, &body);
            let answer = app.execute("review.spelling", json!({ "again": true, "grammar": grammar }));
            if let Ok(answer) = &answer {
                self.replacement = first_suggestion(answer);
            }
        }
        let walk = app.session.proof.walk.clone();
        let Some(walk) = walk.filter(|_| app.session.doc().is_some()) else {
            ui.label(crate::tl!("Spelling check is complete."));
            ui.add_space(6.0);
            button_row(ui, |ui| {
                if ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(80.0, 26.0))).clicked() {
                    done = true;
                }
            });
            return done;
        };
        let total = walk.items.len();
        let Some(place) = walk.current() else {
            ui.label(crate::tl!("Spelling check is complete."));
            ui.add_space(6.0);
            button_row(ui, |ui| {
                if ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(80.0, 26.0))).clicked() {
                    done = true;
                }
            });
            return done;
        };
        let word = walk.text.get(place.span.start..place.span.end).unwrap_or_default().to_string();
        ui.label(RichText::new(crate::tl!("Not in dictionary:")).small().color(t.text_dim));
        ui.label(RichText::new(&word).strong().size(15.0));
        if !place.message.is_empty() {
            ui.label(RichText::new(&place.message).color(t.text_dim));
        }
        ui.add_space(4.0);
        ui.label(crate::tl!("Suggestions"));
        // The suggestions, each a button: the one clicked goes in the field (Change then puts it
        // in), and the field can always be typed in instead.
        if !place.suggestions.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for s in &place.suggestions {
                    if ui.add(egui::Button::new(s).frame(false)).clicked() {
                        self.replacement = s.clone();
                    }
                }
            });
        } else {
            ui.label(RichText::new(crate::tl!("No suggestions")).color(t.text_dim));
        }
        ui.add_space(4.0);
        ui.label(crate::tl!("Change to"));
        let resp = ui.add(egui::TextEdit::singleline(&mut self.replacement).desired_width(f32::INFINITY));
        let resp = crate::text_menu::field(resp);
        app.hit.add("proofChangeTo", 0, resp.rect);
        if !self.note.is_empty() {
            ui.label(RichText::new(self.note.as_str()).color(t.text_dim));
        }
        ui.add_space(6.0);
        button_row(ui, |ui| {
            let b = ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(72.0, 26.0)));
            app.hit.add("proofClose", 0, b.rect);
            if b.clicked() {
                done = true;
            }
            let b = ui.add(egui::Button::new(crate::tl!("Ignore Once")).min_size(vec2(72.0, 26.0)));
            app.hit.add("proofIgnore", 0, b.rect);
            if b.clicked() {
                self.note = said(app.execute("proof.ignore", json!({})));
            }
            if let Some(rule) = place.rule.clone() {
                let b = ui.add(egui::Button::new(crate::tl!("Ignore This Rule")).min_size(vec2(72.0, 26.0)));
                app.hit.add("proofIgnoreRule", 0, b.rect);
                if b.clicked() {
                    let _ = app.execute("proof.ignoreRule", json!({ "rule": rule }));
                    self.note = said(app.execute("proof.ignore", json!({})));
                }
            }
            let b = ui.add(egui::Button::new(crate::tl!("Add to Dictionary")).min_size(vec2(72.0, 26.0)));
            app.hit.add("proofAdd", 0, b.rect);
            if b.clicked() {
                let _ = app.execute("proof.addWord", json!({ "word": word }));
                self.note = said(app.execute("proof.ignore", json!({})));
            }
            let can = !self.replacement.trim().is_empty() && self.replacement != word;
            let b = primary_button(ui, crate::tl!("Change"), can);
            app.hit.add("proofChange", 0, b.rect);
            if (b.clicked() || (enter && can)) && can {
                let answer = app.execute("proof.change", json!({ "text": self.replacement }));
                self.note = said(answer);
                self.replacement = next_suggestion(app);
            }
        });
        ui.add_space(2.0);
        ui.label(RichText::new(format!("{} / {}", walk.at.min(total).saturating_add(1), total)).small().color(t.text_dim));
        done
    }
}

/// What a command's answer says, or why it said no.
fn said(answer: Result<Value, String>) -> String {
    match answer {
        Ok(_) => String::new(),
        Err(e) => crate::tl!(e.as_str()).to_string(),
    }
}

/// The first suggestion of the place on screen now, for the field after a change.
fn next_suggestion(app: &MailApp) -> String {
    app.session.proof.walk.as_ref().and_then(|w| w.current()).and_then(|d| d.suggestions.first().cloned()).unwrap_or_default()
}

/// The first suggestion of the place on screen, for the field.
fn first_suggestion(answer: &Value) -> String {
    answer
        .get("at")
        .and_then(|at| at.get("suggestions"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
