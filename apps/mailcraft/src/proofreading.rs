// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The host's half of grammar checking (#954): the window hands a text and a LanguageTool server's
//! address here (`mailcraft_ui_egui::proof::Request`), and this asks that server on a thread of its
//! own and sends back the places it found — or why there are none — through a channel the window
//! looks at each frame. The window never waits on it, and never sees a socket.
//!
//! Nothing here tries again, logs or keeps anything: the text is asked for, its places are read,
//! and both are forgotten. A thread that cannot be started answers at once saying so, and a server
//! that is not on this computer is refused before a connection is made
//! (`mailcraft_net::languagetool`).
//!
//! Two things arrive from outside and neither is trusted: the server's answer (read by
//! `mailcraft_model::proof::parse_answer`, which checks every offset against the text that was
//! sent) and the thread's own channel.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use mailcraft_model::proof::{self, Language};
use mailcraft_net::languagetool;
use mailcraft_ui_egui::proof::{Reply, Request};

/// Everything the window needs to ask a grammar server: the service it installs in
/// [`mailcraft_ui_egui::Services::proof_check`].
pub fn service() -> Box<dyn Fn(Request) -> Receiver<Reply>> {
    Box::new(|request: Request| {
        let (tx, rx) = channel();
        // A thread of its own for each request: the window asks at most one at a time, and a check
        // of a paragraph is over in the time a frame takes.
        let fallback = tx.clone();
        let made = std::thread::Builder::new().name("mailcraft-proof".into()).spawn(move || {
            let _ = tx.send(answer(&request));
        });
        if let Err(e) = made {
            let _ = fallback.send(Reply {
                text: String::new(),
                diagnostics: Vec::new(),
                error: Some(format!("grammar checking could not be started: {e}")),
            });
        }
        rx
    })
}

/// Asks the server `request` names, and reads its answer: what the window would have got, without
/// the thread.
pub fn answer(request: &Request) -> Reply {
    let language = request.language;
    let asked = languagetool::check(&request.url, &request.text, language.code(), patience());
    let body = match asked {
        Ok(body) => body,
        Err(e) => return Reply { text: request.text.clone(), diagnostics: Vec::new(), error: Some(e.to_string()) },
    };
    match proof::parse_answer(&body, &request.text, language, &[]) {
        Ok(diagnostics) => Reply { text: request.text.clone(), diagnostics, error: None },
        Err(_) => Reply { text: request.text.clone(), diagnostics: Vec::new(), error: Some("the grammar server's answer could not be read".into()) },
    }
}

/// How long a server on this computer is given: it is a process on this machine, and a check of a
/// paragraph is a fraction of a second's work.
fn patience() -> Duration {
    languagetool::PATIENCE
}

/// The language a check is asked for, named as the settings name it (`fr-CA`).
pub fn code_of(language: Language) -> &'static str {
    language.code()
}
