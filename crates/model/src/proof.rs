// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Proofing the message being written (#954): spelling checked against every dictionary the owner
//! turned on at once, and grammar read from a LanguageTool server on this computer.
//!
//! Nothing here does I/O and nothing here knows about a window. The text comes in as a string
//! whose positions are **byte** offsets (they are what [`crate::rich`] uses), the diagnostics go
//! out as [`Diagnostic`]s over the same positions, and what to check is [`ProofSettings`].
//!
//! - A word is accepted if **any** enabled language accepts it ([`Speller::accept`]); a sentence
//!   written half in English and half in French is never flagged for the mixing itself.
//! - Grammar comes from a server (LanguageTool's `/v2/check`); its answers are read by
//!   [`grammar`], which is as suspicious of them as of any other bytes from outside, and whose
//!   offsets are converted from UTF-16 code units to byte offsets here, once, with tests.
//! - A dictionary is a file on this computer ([`spell`]): no dictionary is fetched.
//!
//! What a diagnostic spans is a byte range that is always on character boundaries, always inside
//! the text it was found in, and never empty; a diagnostic that could not be placed is dropped
//! rather than clamped into something that would replace the wrong text.

use serde::{Deserialize, Serialize};

use crate::ModelError;

mod grammar;
mod spell;
#[cfg(test)]
mod tests;

pub use grammar::{CheckRequest, parse_answer};
pub use spell::{Dictionary, Speller};

/// Most characters of one text that are checked at once: a message body is capped well below
/// this ([`crate::rich::MAX_CHARS`]); a text longer than this is checked only up to here.
pub const MAX_CHECKED: usize = 100_000;

/// Longest word looked at, in characters; longer runs are a pasted address or a code, not a word.
pub const MAX_WORD: usize = 48;

/// Most diagnostics one check answers with.
pub const MAX_DIAGNOSTICS: usize = 4_000;

/// Most suggestions one diagnostic carries.
pub const MAX_SUGGESTIONS: usize = 8;

/// Most words a personal dictionary holds.
pub const MAX_PERSONAL: usize = 10_000;

/// Longest personal word, in characters.
pub const MAX_PERSONAL_WORD: usize = 64;

/// Longest grammar server answer read, in bytes.
pub const MAX_ANSWER: usize = 1024 * 1024;

/// The languages proofing knows. A dictionary says which one it is; the owner picks which are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// Canadian English.
    #[serde(rename = "en-CA")]
    #[default]
    EnCa,
    /// Canadian French.
    #[serde(rename = "fr-CA")]
    FrCa,
    /// American English, for an owner who wants it beside the Canadian one.
    #[serde(rename = "en-US")]
    EnUs,
    /// French of France.
    #[serde(rename = "fr-FR")]
    FrFr,
}

impl Language {
    /// Every language proofing can be set to, in the order the settings list them.
    pub const ALL: [Language; 4] = [Language::EnCa, Language::FrCa, Language::EnUs, Language::FrFr];

    /// Its code, as a dictionary is named and as LanguageTool writes it: `en-CA`.
    pub fn code(self) -> &'static str {
        match self {
            Language::EnCa => "en-CA",
            Language::FrCa => "fr-CA",
            Language::EnUs => "en-US",
            Language::FrFr => "fr-FR",
        }
    }

    /// Its name in English, for the settings.
    pub fn name(self) -> &'static str {
        match self {
            Language::EnCa => "English (Canada)",
            Language::FrCa => "French (Canada)",
            Language::EnUs => "English (United States)",
            Language::FrFr => "French (France)",
        }
    }

    /// The language `code` names: `en-CA`, `en_CA`, `EN-ca`, or just `en` (Canadian English) or
    /// `fr` (Canadian French), which is what a system setting usually says.
    pub fn parse(code: &str) -> Option<Language> {
        let code = code.trim();
        if code.is_empty() || code.len() > 16 {
            return None;
        }
        let mut parts = code.split(['-', '_']);
        let tongue = parts.next()?.to_ascii_lowercase();
        let region = parts.next().unwrap_or_default().to_ascii_uppercase();
        if parts.next().is_some() {
            return None;
        }
        match (tongue.as_str(), region.as_str()) {
            ("en", "" | "CA" | "US") => Some(if region == "US" { Language::EnUs } else { Language::EnCa }),
            ("fr", "" | "CA" | "FR") => Some(if region == "FR" { Language::FrFr } else { Language::FrCa }),
            ("en", _) | ("fr", _) => None,
            _ => None,
        }
    }

    /// Whether it is English.
    pub fn is_english(self) -> bool {
        matches!(self, Language::EnCa | Language::EnUs)
    }

    /// The language on another side of the same tongue, or itself: an American dictionary stands
    /// in for a Canadian one that is missing, and the other way round.
    pub fn kin(self) -> Language {
        match self {
            Language::EnCa => Language::EnUs,
            Language::EnUs => Language::EnCa,
            Language::FrCa => Language::FrFr,
            Language::FrFr => Language::FrCa,
        }
    }
}

/// Why a piece of proofing could not be done.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProofError {
    /// A dictionary is not a dictionary: empty, or with a line that cannot be read.
    #[error("the dictionary {0} cannot be read")]
    Dictionary(String),
    /// The grammar server's answer is not one.
    #[error("the grammar server's answer cannot be read")]
    Answer,
    /// A text is too long to check.
    #[error("the text is too long to check")]
    TooLong,
    /// The settings name an address proofing does not speak to.
    #[error("{0}")]
    Refused(String),
}

impl From<ProofError> for ModelError {
    fn from(e: ProofError) -> ModelError {
        ModelError::Invalid(e.to_string())
    }
}

/// What a diagnostic is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A word no enabled dictionary knows.
    Spelling,
    /// Grammar, from the server.
    Grammar,
    /// Style, from the server.
    Style,
}

/// Where a diagnostic came from, which is also where the text went for it: nowhere, or a server
/// named in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// This computer's dictionaries.
    Local,
    /// The grammar server.
    Server,
}

/// A range of the checked text, in bytes, on character boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// The span `start..end` of `text`, or `None` when either end is past the text, is not on a
    /// character boundary, or the span is empty: what would put a correction in the wrong place.
    pub fn of(text: &str, start: usize, end: usize) -> Option<Span> {
        let len = text.len();
        (start < end && end <= len && text.is_char_boundary(start) && text.is_char_boundary(end)).then_some(Span { start, end })
    }

    /// Whether it is a span of `text`, on its boundaries and inside it.
    pub fn fits(&self, text: &str) -> bool {
        Span::of(text, self.start, self.end).is_some()
    }

    /// How many characters it spans.
    pub fn chars(&self, text: &str) -> usize {
        text.get(self.start..self.end).map_or(0, |s| s.chars().count())
    }
}

/// One thing to look at in the text, and what could be put in its place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub span: Span,
    pub kind: Kind,
    /// The language it was found against, when one can be told: the dictionary that refused it, or
    /// the language the server was asked in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// What is wrong, in the server's words (English, from LanguageTool) or in none for spelling.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    /// What could stand there instead, the best first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    /// The server's rule id, for a rule the owner may ignore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub source: Source,
}

impl Diagnostic {
    /// A diagnostic, with its suggestions cut to [`MAX_SUGGESTIONS`] and each of them made safe to
    /// put in the text (one line, no control characters), and a message cut short.
    pub fn new(span: Span, kind: Kind, source: Source) -> Diagnostic {
        Diagnostic { span, kind, language: None, message: String::new(), suggestions: Vec::new(), rule: None, source }
    }

    /// The same with what could stand there instead, cleaned and cut.
    pub fn suggesting(mut self, suggestions: impl IntoIterator<Item = String>) -> Diagnostic {
        let mut kept: Vec<String> = Vec::new();
        for s in suggestions {
            if kept.len() >= MAX_SUGGESTIONS {
                break;
            }
            let clean = clean_text(&s);
            if !clean.is_empty() && !kept.contains(&clean) {
                kept.push(clean);
            }
        }
        self.suggestions = kept;
        self
    }

    /// The same, with the text the owner is told.
    pub fn saying(mut self, message: &str) -> Diagnostic {
        self.message = clean_text(message).chars().take(300).collect();
        self
    }

    /// Whether it is worth showing: a place in the text and something to say about it.
    pub fn is_sound(&self) -> bool {
        self.span.start < self.span.end
    }
}

/// One line of text, without control characters (except a tab, which becomes a space).
fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(1_024));
    for c in s.chars() {
        match c {
            '\t' => out.push(' '),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

/// What proofing is set to do, kept in the owner's settings between runs ([`crate::proof::Prefs`]
/// is where the front end stores it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProofSettings {
    /// Spelling is checked against the dictionaries below.
    pub check_spelling: bool,
    /// Grammar is checked by the LanguageTool server at [`ProofSettings::grammar_url`].
    pub check_grammar: bool,
    /// Checking happens as the owner types, not only when asked (Review › Spelling & Grammar).
    pub as_you_type: bool,
    /// The dictionaries that are on, at once: a word is right if any of them says so.
    pub languages: Vec<Language>,
    /// The words the owner added, as typed, in no order in particular.
    pub personal: Vec<String>,
    /// Grammar rules the owner told to be quiet (LanguageTool's rule ids).
    pub ignored_rules: Vec<String>,
    /// Where the grammar server is.
    pub grammar_url: String,
    /// The owner allows a grammar server that is not on this computer. Off: an address that is not
    /// this machine's is refused before anything of the message leaves it.
    pub allow_remote: bool,
    /// The folder the owner keeps dictionaries in. Empty: the usual places
    /// (`/usr/share/hunspell`, `~/.local/share/hunspell`, `/Library/Spelling`…).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dictionary_dir: String,
}

impl Default for ProofSettings {
    fn default() -> Self {
        ProofSettings {
            check_spelling: true,
            check_grammar: false,
            as_you_type: true,
            languages: vec![Language::EnCa, Language::FrCa],
            personal: Vec::new(),
            ignored_rules: Vec::new(),
            grammar_url: "http://127.0.0.1:8081".into(),
            allow_remote: false,
            dictionary_dir: String::new(),
        }
    }
}

impl ProofSettings {
    /// The languages in effect, without repeats and never empty: the two Canadian ones when the
    /// settings name none that proofing knows.
    pub fn active(&self) -> Vec<Language> {
        let mut out: Vec<Language> = Vec::new();
        for l in &self.languages {
            if !out.contains(l) && out.len() < Language::ALL.len() {
                out.push(*l);
            }
        }
        if out.is_empty() {
            out.push(Language::EnCa);
        }
        out
    }

    /// Whether checking anything is on at all.
    pub fn any(&self) -> bool {
        self.check_spelling || self.check_grammar
    }

    /// Whether the grammar server may be spoken to at the address in the settings: only this
    /// computer's, unless the owner allowed another ([`ProofSettings::allow_remote`]). A refused
    /// address is not spoken to at all, so that a message never leaves this computer because of a
    /// settings file.
    pub fn grammar_allowed(&self) -> bool {
        crate::proof::address_allowed(&self.grammar_url, self.allow_remote)
    }

    /// The words of the personal dictionary, in the order they were added, without repeats and no
    /// more than [`MAX_PERSONAL`].
    pub fn personal_words(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in &self.personal {
            let w = w.trim();
            if w.is_empty() || w.chars().count() > MAX_PERSONAL_WORD || out.len() >= MAX_PERSONAL {
                continue;
            }
            if !out.iter().any(|kept| same_word(kept, w)) {
                out.push(w.to_string());
            }
        }
        out
    }

    /// Adds a word to the personal dictionary; `false` when it is already there, is empty, or the
    /// dictionary is full.
    pub fn add_word(&mut self, word: &str) -> bool {
        let w = word.trim();
        if w.is_empty() || w.chars().count() > MAX_PERSONAL_WORD || !w.chars().any(char::is_alphabetic) {
            return false;
        }
        if self.personal_words().iter().any(|kept| same_word(kept, w)) || self.personal.len() >= MAX_PERSONAL {
            return false;
        }
        self.personal.push(w.to_string());
        true
    }

    /// Takes a word out of the personal dictionary; whether it was there.
    pub fn remove_word(&mut self, word: &str) -> bool {
        let before = self.personal.len();
        self.personal.retain(|w| !same_word(w, word.trim()));
        self.personal.len() != before
    }
}

/// Whether two words are the same word, whatever their case and whichever apostrophe: what the
/// personal dictionary has no two of.
pub fn same_word(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

/// A word as it is looked up: lower case, with the typographic apostrophe folded to the plain one.
pub fn key(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        match c {
            '\u{2019}' | '\u{2018}' | '\u{02bc}' => out.push('\''),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Whether an address is one the grammar server may be spoken at: `http://` or `https://` to this
/// computer (loopback, by name or by address), or anywhere when `allow_remote`.
pub fn address_allowed(url: &str, allow_remote: bool) -> bool {
    let Some((scheme, rest)) = url.trim().split_once("://") else {
        return false;
    };
    let secure = scheme.eq_ignore_ascii_case("https");
    if !secure && !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    let host = host.to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let _ = secure;
    allow_remote || is_this_machine(&host)
}

/// Whether `host` is this computer: loopback by name or by address (and nothing that merely ends
/// the same way, such as `localhost.example`).
fn is_this_machine(host: &str) -> bool {
    if host == "localhost" || host == "::1" || host == "ip6-localhost" {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// A run of letters that proofing looks at, with where it is in the text (bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word<'a> {
    pub text: &'a str,
    pub span: Span,
}

/// The words of `text`, in order: runs of letters (and the apostrophe and hyphen inside them),
/// leaving out what is not a word to check: an address, an e-mail address, a number, a word longer
/// than [`MAX_WORD`], a picture (the object character), and anything in a script with no space
/// between words. Only the first [`MAX_CHECKED`] characters are looked at.
pub fn words(text: &str) -> Vec<Word<'_>> {
    let text = match text.char_indices().nth(MAX_CHECKED) {
        Some((cut, _)) => text.get(..cut).unwrap_or_default(),
        None => text,
    };
    let skip = skipped(text);
    let mut out: Vec<Word<'_>> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let part = is_word_char(c);
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(from)) => {
                push_word(text, from, i, &skip, &mut out);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        push_word(text, from, text.len(), &skip, &mut out);
    }
    out
}

/// Adds the word `from..to`, trimmed of the punctuation at its ends, when it is one to check.
fn push_word<'a>(text: &'a str, from: usize, to: usize, skip: &[(usize, usize)], out: &mut Vec<Word<'a>>) {
    let Some(piece) = text.get(from..to) else {
        return;
    };
    let trimmed = piece.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        return;
    }
    let lead = piece.len() - piece.trim_start_matches(|c: char| !c.is_alphanumeric()).len();
    let start = from + lead;
    let end = start + trimmed.len();
    let Some(span) = Span::of(text, start, end) else {
        return;
    };
    if skip.iter().any(|(a, b)| start < *b && *a < end) {
        return;
    }
    if trimmed.chars().count() > MAX_WORD || !trimmed.chars().any(char::is_alphabetic) {
        return;
    }
    // A word written in a script that puts no space between words (Chinese, Japanese, Thai…): the
    // run is a sentence, and checking it as one word would flag every one of them.
    if trimmed.chars().any(no_spaces_between_words) {
        return;
    }
    out.push(Word { text: trimmed, span });
}

/// A character a word is made of: a letter or a digit, or the apostrophe or hyphen joining two.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-' || c == '_' || c == '.'
}

/// Scripts written without spaces between words: a run of them is not a word.
fn no_spaces_between_words(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF | 0x2E80..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF65..=0xFF9F | 0x20000..=0x3FFFF)
}

/// The places in `text` that are looked at as a whole and never as words: a web address, an
/// e-mail address, a path, and what a name of a file or a variable looks like.
fn skipped(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let expand = |from: usize, to: usize| {
        let mut a = from;
        while a > 0 {
            let Some(c) = text.get(..a).and_then(|s| s.chars().next_back()) else { break };
            if c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '«' | '»' | ',' | ';') {
                break;
            }
            a -= c.len_utf8();
        }
        let mut b = to;
        while b < bytes.len() {
            let Some(c) = text.get(b..).and_then(|s| s.chars().next()) else { break };
            if c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '«' | '»' | ',' | ';') {
                break;
            }
            b += c.len_utf8();
        }
        (a, b)
    };
    for marker in ["://", "www.", "\\\\"] {
        let mut from = 0;
        while let Some(at) = text.get(from..).and_then(|s| s.find(marker)) {
            let i = from + at;
            let (a, b) = expand(i, i + marker.len());
            out.push((a, b));
            from = b.max(i + marker.len());
        }
    }
    // An e-mail address or a path: `@` or a slash with a name on both sides.
    for (i, c) in text.char_indices() {
        if c != '@' && c != '/' {
            continue;
        }
        let (a, b) = expand(i, i + c.len_utf8());
        let piece = text.get(a..b).unwrap_or_default();
        let spaced = piece.split(['@', '/']).filter(|p| p.chars().filter(|c| c.is_alphabetic()).count() > 1).count();
        if spaced >= 2 {
            out.push((a, b));
        }
    }
    out.sort_unstable();
    out
}
