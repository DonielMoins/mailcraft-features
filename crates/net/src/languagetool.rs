// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Grammar from a LanguageTool server on this computer (#954): one `POST /v2/check`, and the
//! answer's bytes.
//!
//! The window opens no connection of its own: the host asks here on a thread that is not the
//! window's, and hands what comes back to `mailcraft_model::proof::parse_answer`, which is where
//! UTF-16 offsets become bytes and every length is checked against the text that was sent. This
//! module's whole job is to ask, and to say plainly why there is no answer.
//!
//! - **Only this computer.** A grammar server on the internet would be told the message being
//!   written. An address that is not `http(s)`, or that is not this machine's, is refused before
//!   a connection is made — refused here, on this module's own account, whatever the caller has
//!   checked ([`mailcraft_model::proof::address_allowed`], with no remote server allowed) — and
//!   the crate's HTTP client refuses `http://` to anything but loopback as well.
//! - **The answer is hostile**: at most [`MAX_ANSWER`] bytes of it, no redirect read, no
//!   redirect followed, and one patience for connecting and for the answer alike.
//! - **Nothing of the text is said.** An error says what happened in the owner's words and
//!   carries neither the text nor the server's address; the log line the HTTP client writes
//!   names the host and the status, and nothing else.
//! - **Asked once.** Nothing here tries again: the window asks again when it is ready to.

use std::time::Duration;

use mailcraft_model::proof::{self, CheckRequest, Language, MAX_ANSWER};

use crate::NetError;
use crate::http::{self, Request, Url};

/// How long connecting may take, and so may the answer. A check of a paragraph on a server on
/// this computer takes a fraction of a second; a server that has to think longer than this is
/// one the window asks again rather than waits for.
pub const PATIENCE: Duration = Duration::from_secs(8);

/// What is posted to, under the server's address.
const CHECK: &str = "/v2/check";

/// Why a check has no answer. Each says which of the things that can go wrong did, and none of
/// them holds the text that was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LanguageToolError {
    /// The address is not one a grammar server is spoken to at: it is no address at all, or it is
    /// not this computer's.
    #[error("grammar is checked by a LanguageTool server on this computer, and that address is not one of this computer's")]
    Refused,
    /// The language asked for is not one proofing knows.
    #[error("that is not a language a grammar check can be asked for")]
    Language,
    /// No server answered at that address: nothing listens there.
    #[error("no grammar server on this computer answered")]
    Unreachable,
    /// The server did not answer in the time it had.
    #[error("the grammar server took too long")]
    TooSlow,
    /// The answer is longer than a grammar server's answers may be.
    #[error("the grammar server's answer is too long to read")]
    TooLong,
    /// The server hung up without an answer.
    #[error("the grammar server hung up without an answer")]
    Closed,
    /// The server answered, and said it could not check the text.
    #[error("the grammar server could not check the text")]
    Failed,
    /// What came back is not HTTP, or not the shape of an answer.
    #[error("the grammar server answered with something that is not a grammar check")]
    Garbled,
}

/// Asks the grammar server at `url` to check `text` as `language` (`fr-CA`), waiting `patience`
/// to connect and as long again for the answer, and gives back the bytes of its answer — for
/// [`proof::parse_answer`], which is the only thing that reads them.
///
/// `url` is the server's address, which may end in a `/`; `/v2/check` is posted under it. An
/// address with a query or a fragment is not one a check is posted to.
pub fn check(url: &str, text: &str, language: &str, patience: Duration) -> Result<Vec<u8>, LanguageToolError> {
    let at = endpoint(url).ok_or(LanguageToolError::Refused)?;
    // This module's own refusal, never the caller's alone: a live server on another machine is
    // told nothing of the message.
    if !proof::address_allowed(&at, false) {
        return Err(LanguageToolError::Refused);
    }
    let language = Language::parse(language).ok_or(LanguageToolError::Language)?;
    // The client's own rule too: `https://` anywhere, plain `http://` to this machine alone.
    let at = Url::parse(&at).map_err(|_| LanguageToolError::Refused)?;
    let body = CheckRequest { text, language }.form();
    let headers = [("Content-Type", "application/x-www-form-urlencoded"), ("Accept", "application/json")];
    let request = Request { method: "POST", headers: &headers, body: Some(body.as_bytes()), patience, max_body: MAX_ANSWER };
    let reply = http::request(&at, &request).map_err(told)?;
    if !(200..300).contains(&reply.status) {
        return Err(LanguageToolError::Failed);
    }
    Ok(reply.body)
}

/// The address a check is posted to: the server's own, with one `/` between it and [`CHECK`],
/// however many trailing ones it was written with. `None` for what no check is posted to.
fn endpoint(url: &str) -> Option<String> {
    let base = url.trim().trim_end_matches('/');
    // A query or a fragment after the address would be read as part of the path: neither is one.
    if base.is_empty() || base.contains(['?', '#']) {
        return None;
    }
    Some(format!("{base}{CHECK}"))
}

/// What an error of the crate's HTTP client means for a check: one of the owner's sentences,
/// with nothing of the text and nothing of the answer in it.
fn told(e: NetError) -> LanguageToolError {
    if http::is_too_slow(&e) {
        return LanguageToolError::TooSlow;
    }
    if http::is_too_long(&e) {
        return LanguageToolError::TooLong;
    }
    if http::is_closed(&e) {
        return LanguageToolError::Closed;
    }
    match e {
        // No connection was made at all: the address, the name, or the secure connection.
        NetError::Connect { .. } | NetError::Tls { .. } | NetError::Certificate(_) | NetError::Config(_) => LanguageToolError::Unreachable,
        _ => LanguageToolError::Garbled,
    }
}
