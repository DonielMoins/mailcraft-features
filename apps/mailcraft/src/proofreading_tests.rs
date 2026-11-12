// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Grammar checking's host half, against servers of this test's own on loopback: an answer with a
//! place in it, a server that is not there, and a server that is not a grammar server.

use std::io::{Read, Write};
use std::net::TcpListener;

use mailcraft_model::proof::Language;
use mailcraft_ui_egui::proof::Request;

use crate::proofreading::*;

/// A server on this machine that answers one request with `answer` (the whole HTTP response) and
/// stops. Its address.
fn server(answer: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let at = listener.local_addr().expect("an address");
    std::thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut buf = [0u8; 8192];
            let _ = socket.read(&mut buf);
            let _ = socket.write_all(answer.as_bytes());
        }
    });
    format!("http://{at}")
}

/// An HTTP answer with `body`.
fn answered(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn request(url: &str, text: &str) -> Request {
    Request { url: url.to_string(), text: text.to_string(), language: Language::FrCa }
}

#[test]
fn a_place_the_server_found_comes_back_with_its_bytes() {
    let text = "Les document sont important.";
    let url = server(Box::leak(
        answered(r#"{"matches":[{"message":"Accord","replacements":[{"value":"documents"}],"offset":4,"length":8,"rule":{"id":"FR_ACCORD","issueType":"grammar"}}]}"#)
            .into_boxed_str(),
    ));
    let reply = answer(&request(&url, text));
    assert!(reply.error.is_none(), "{:?}", reply.error);
    assert_eq!(reply.text, text);
    let place = reply.diagnostics.first().expect("one place");
    assert_eq!(text.get(place.span.start..place.span.end), Some("document"));
    assert_eq!(place.suggestions.first().map(String::as_str), Some("documents"));
}

#[test]
fn a_server_that_is_not_there_says_so_and_says_nothing_else() {
    // A port nobody listens on: bound and let go, so the number is one this machine has free.
    let port = TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a port").port();
    let url = format!("http://127.0.0.1:{port}");
    let text = "the document is here";
    let reply = answer(&request(&url, text));
    let error = reply.error.clone().expect("an error");
    assert!(!error.contains("document"), "the message's text is never in an error: {error}");
    assert!(reply.diagnostics.is_empty());
    assert_eq!(reply.text, text, "the text comes back with its answer, so that a stale one is dropped");
}

#[test]
fn a_server_that_is_not_a_grammar_server_is_read_as_no_places() {
    let url = server(Box::leak(answered("this is not JSON").into_boxed_str()));
    let reply = answer(&request(&url, "Bonjour"));
    assert!(reply.diagnostics.is_empty());
    assert!(reply.error.is_some());
}

#[test]
fn a_server_on_another_computer_is_never_asked() {
    let reply = answer(&request("https://languagetool.example", "Bonjour le monde"));
    assert!(reply.diagnostics.is_empty());
    assert!(reply.error.is_some(), "refused before anything is sent");
}

#[test]
fn the_service_answers_through_a_channel() {
    let text = "Les document sont important.";
    let url = server(Box::leak(
        answered(r#"{"matches":[{"message":"Accord","replacements":[],"offset":0,"length":3,"rule":{"id":"R","issueType":"grammar"}}]}"#)
            .into_boxed_str(),
    ));
    let service = service();
    let channel = service(request(&url, text));
    let reply = channel.recv_timeout(std::time::Duration::from_secs(10)).expect("an answer");
    assert_eq!(reply.diagnostics.len(), 1);
}
