// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Asking a grammar server for a check, against stand-in servers on this machine: one that
//! answers a match, one that refuses, one that hangs up, one that is not there at all, one that
//! is too slow, and one whose answer is too long. Nothing here reaches a real LanguageTool.

use std::time::{Duration, Instant};

use mailcraft_model::proof::{Language, MAX_ANSWER, parse_answer};

use crate::fake::{FakeHttp, json_answer};
use crate::http::pairs;
use crate::languagetool::*;

/// Long enough for anything on this machine.
const SOON: Duration = Duration::from_secs(5);

/// The one match the stand-in servers answer with: `document` in "Les document sont important.",
/// counted the way LanguageTool counts (UTF-16 code units, which is bytes here, the text being
/// plain ASCII).
fn matches() -> &'static str {
    r#"{"matches":[{"message":"Accord","replacements":[{"value":"documents"}],"offset":4,"length":8,"rule":{"id":"FR_ACCORD","issueType":"grammar"}}]}"#
}

const TEXT: &str = "Les document sont important.";

#[test]
fn a_check_is_asked_and_its_answer_given_back() {
    let fake = FakeHttp::answering(json_answer("200 OK", matches()));
    let got = check(&fake.url(""), TEXT, "fr-CA", SOON).expect("an answer");
    assert_eq!(got, matches().as_bytes());

    let request = fake.request(0);
    let (head, form) = request.split_once("\r\n\r\n").expect("a head and a body");
    assert!(head.starts_with("POST /v2/check HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("Content-Type: application/x-www-form-urlencoded\r\n"), "{head}");
    assert!(head.contains("Host: 127.0.0.1:"), "{head}");
    // Nothing of this program but its name, and no cookie or credential of any kind.
    for never in ["cookie:", "authorization:", "referer:", "accept-encoding:"] {
        assert!(!head.to_ascii_lowercase().contains(never), "{never} in {head}");
    }
    // The form carries the text whole and the language asked for.
    let asked_for = pairs(form);
    assert_eq!(asked_for.len(), 4, "{form}");
    assert_eq!(asked_for.first().map(|(name, value)| (name.as_str(), value.as_str())), Some(("text", TEXT)));
    assert_eq!(asked_for.get(1).map(|(name, value)| (name.as_str(), value.as_str())), Some(("language", "fr-CA")));
    // And what came back is what the model's reader makes a diagnostic of.
    let got = parse_answer(&got, TEXT, Language::FrCa, &[]).expect("an answer");
    assert_eq!(got.len(), 1);
    let span = got.first().expect("one").span;
    assert_eq!(TEXT.get(span.start..span.end), Some("document"));
}

#[test]
fn the_text_goes_whole_whatever_it_holds() {
    // A `&`, an `=`, a `%` and a space in the message never change what the request says.
    let text = "Bonjour & à bientôt = 100%";
    let fake = FakeHttp::answering(json_answer("200 OK", r#"{"matches":[]}"#));
    check(&fake.url("/"), text, "fr-CA", SOON).expect("an answer");
    let request = fake.request(0);
    let (_, form) = request.split_once("\r\n\r\n").expect("a head and a body");
    assert_eq!(form, "text=Bonjour+%26+%C3%A0+bient%C3%B4t+%3D+100%25&language=fr-CA&enabledOnly=false&level=picky");
    assert_eq!(pairs(form).first().map(|(name, value)| (name.as_str(), value.as_str())), Some(("text", text)));
}

#[test]
fn a_trailing_slash_does_not_double_the_path() {
    let fake = FakeHttp::answering(json_answer("200 OK", r#"{"matches":[]}"#));
    for base in [fake.url(""), fake.url("/"), format!("{}//", fake.url(""))] {
        check(&base, "Bonjour.", "fr-CA", SOON).expect("an answer");
    }
    assert_eq!(fake.asked(), 3);
    for n in 0..3 {
        assert!(fake.request(n).starts_with("POST /v2/check HTTP/1.1\r\n"), "{}", fake.request(n));
    }
}

#[test]
fn a_server_on_another_machine_is_not_spoken_to_at_all() {
    let fake = FakeHttp::answering(json_answer("200 OK", matches()));
    for bad in [
        // Plain http to a machine that is not this one, by address or by name.
        "http://192.0.2.7:8081",
        "http://languagetool.example",
        // Secure or not, a server on another machine is told nothing of the message here.
        "https://languagetool.example/v2/check",
        // What is no web address at all.
        "ftp://127.0.0.1:8081",
        "file:///etc/passwd",
        "127.0.0.1:8081",
        "//127.0.0.1:8081",
        "",
        "   ",
        // An address with a query or a fragment is not where a check is posted.
        "http://127.0.0.1:8081/?x=1",
        "http://127.0.0.1:8081/#top",
        // A name and a password in the address are nobody's here.
        "http://user:secret@127.0.0.1:8081",
    ] {
        assert_eq!(check(bad, TEXT, "fr-CA", SOON), Err(LanguageToolError::Refused), "{bad}");
    }
    // The stand-in server itself, through an address that is not one: not a byte of a request
    // reaches it, so the refusal happened before any connection.
    assert_eq!(check(&format!("http://user@{}/", fake.url("")), TEXT, "fr-CA", SOON), Err(LanguageToolError::Refused));
    assert_eq!(fake.asked(), 0);
}

#[test]
fn a_language_that_is_not_one_is_refused_before_a_connection() {
    let fake = FakeHttp::answering(json_answer("200 OK", matches()));
    for bad in ["", "xx-YY", "tlh", "en-CA-et-plus"] {
        assert_eq!(check(&fake.url(""), TEXT, bad, SOON), Err(LanguageToolError::Language), "{bad}");
    }
    assert_eq!(fake.asked(), 0);
}

#[test]
fn a_server_that_says_it_cannot_is_an_error() {
    for status in ["500 Internal Server Error", "400 Bad Request", "429 Too Many Requests", "503 Service Unavailable"] {
        let fake = FakeHttp::answering(json_answer(status, "no"));
        assert_eq!(check(&fake.url(""), TEXT, "fr-CA", SOON), Err(LanguageToolError::Failed), "{status}");
        assert_eq!(fake.asked(), 1, "{status}");
    }
}

#[test]
fn a_server_that_hangs_up_without_an_answer_is_told_apart() {
    let fake = FakeHttp::answering(Vec::new());
    assert_eq!(check(&fake.url(""), TEXT, "fr-CA", SOON), Err(LanguageToolError::Closed));
    assert_eq!(fake.asked(), 1);
    // Half an answer is no answer either.
    let fake = FakeHttp::answering(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n{\"matches\"".to_vec());
    assert!(matches!(check(&fake.url(""), TEXT, "fr-CA", SOON), Err(LanguageToolError::Closed | LanguageToolError::Garbled)));
}

#[test]
fn an_answer_that_is_not_json_is_given_back_for_the_reader_to_refuse() {
    // Asking worked; what came back is the model's to read, and it refuses it there.
    let fake = FakeHttp::answering(json_answer("200 OK", "<html>not a check</html>"));
    let body = check(&fake.url(""), TEXT, "fr-CA", SOON).expect("bytes");
    assert!(parse_answer(&body, TEXT, Language::FrCa, &[]).is_err());
}

#[test]
fn an_answer_longer_than_a_grammar_servers_may_be_is_not_taken() {
    let full = format!(r#"{{"matches":[],"pad":"{}"}}"#, "x".repeat(MAX_ANSWER));
    let fake = FakeHttp::answering(json_answer("200 OK", &full));
    assert_eq!(check(&fake.url(""), TEXT, "fr-CA", SOON), Err(LanguageToolError::TooLong));
    // As much as one may be is read whole.
    let fits = "x".repeat(MAX_ANSWER);
    let fake = FakeHttp::answering(json_answer("200 OK", &fits));
    assert_eq!(check(&fake.url(""), TEXT, "fr-CA", SOON).map(|body| body.len()), Ok(MAX_ANSWER));
}

#[test]
fn nothing_listening_is_a_clean_error_at_once() {
    // A port with nobody on it: bound once, so that it was free, and let go.
    let port = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port").local_addr().expect("an address").port();
    let started = Instant::now();
    assert_eq!(check(&format!("http://127.0.0.1:{port}"), TEXT, "fr-CA", SOON), Err(LanguageToolError::Unreachable));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn a_slow_server_is_not_waited_for() {
    let fake = FakeHttp::start(|_, _| {
        std::thread::sleep(Duration::from_secs(3));
        json_answer("200 OK", matches())
    });
    let started = Instant::now();
    assert_eq!(check(&fake.url(""), TEXT, "fr-CA", Duration::from_millis(200)), Err(LanguageToolError::TooSlow));
    assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
}

#[test]
fn the_errors_say_what_happened_and_nothing_of_the_text() {
    let fake = FakeHttp::answering(json_answer("200 OK", matches()));
    let text = "Le mot de passe est hunter2.";
    let told = |e: LanguageToolError| e.to_string();
    // Not one of the owner's sentences holds a word of the message or the server's address.
    for error in [
        LanguageToolError::Refused,
        LanguageToolError::Language,
        LanguageToolError::Unreachable,
        LanguageToolError::TooSlow,
        LanguageToolError::TooLong,
        LanguageToolError::Closed,
        LanguageToolError::Failed,
        LanguageToolError::Garbled,
    ] {
        let said = told(error);
        assert!(!said.contains("hunter2") && !said.contains("mot de passe"), "{said}");
        assert!(!said.contains("127.0.0.1") && !said.contains("http"), "{said}");
        assert!(!said.is_empty(), "{error:?}");
    }
    // And a real failure is one of them, with nothing of the message in it.
    let said = check(&format!("http://127.0.0.1:{}/", "1"), text, "fr-CA", Duration::from_millis(50)).expect_err("no server").to_string();
    assert!(!said.contains("hunter2") && !said.contains("1"), "{said}");
    assert_eq!(fake.asked(), 0);
}
