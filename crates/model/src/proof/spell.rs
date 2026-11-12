// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Words this computer knows: the owner's dictionaries, read as they are installed
//! (`/usr/share/hunspell/*.dic` and `.aff`, and the folder File › Options names), plus the words
//! the owner added.
//!
//! A dictionary is a Hunspell one ([the format](https://manpages.debian.org/hunspell)): a `.dic`
//! of stems, each with the flags its affixes are named by, and a `.aff` of those affixes
//! (prefixes and suffixes with a condition on the stem, and whether the stem is a word by
//! itself). Only the part of the format that says what a word is is read — `SET`, `FLAG`, `PFX`,
//! `SFX`, `NEEDAFFIX`, `FORBIDDENWORD`, `NOSUGGEST` — and nothing else is: no suggestion engine of
//! the format's own, no compound rules, and no `ICONV`. Everything is capped, a line that cannot
//! be read is skipped, and a dictionary that cannot be read at all is an error the window says in
//! words instead of a crash.
//!
//! A word is accepted when **any** dictionary that is on accepts it ([`Speller::accept`]): the
//! owner writes Canadian English and Canadian French in the same paragraph, and a word is not
//! wrong for being in the other one.

use std::collections::HashSet;

use super::{Language, MAX_PERSONAL, ProofError, key};

/// Most words one dictionary holds once its affixes are worked out; a dictionary that would hold
/// more is cut there (a real one holds well under it).
pub const MAX_ENTRIES: usize = 1_500_000;

/// Most affix rules read from one `.aff`.
const MAX_RULES: usize = 20_000;

/// Longest stem or word kept, in characters.
const MAX_STEM: usize = 64;

/// Longest condition of an affix rule; longer ones are skipped.
const MAX_CONDITION: usize = 32;

/// Most probes the suggester makes for one word: what keeps a right click quick on a bad word.
const MAX_PROBES: usize = 20_000;

/// One language's words, sorted, as keys ([`key`]).
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    lang: Language,
    words: Vec<String>,
    /// Stems that are not words by themselves (`NEEDAFFIX`): never a word, whatever they look like.
    stems_only: HashSet<String>,
    /// Words the dictionary says are wrong (`FORBIDDENWORD`), such as a misspelling a dictionary
    /// lists so that it can be suggested as corrected.
    forbidden: HashSet<String>,
    /// Words that are fine but not to be suggested.
    no_suggest: HashSet<String>,
    /// Whether anything was read at all.
    loaded: bool,
}

impl Dictionary {
    /// An empty dictionary for `lang`.
    pub fn new(lang: Language) -> Dictionary {
        Dictionary { lang, ..Dictionary::default() }
    }

    /// The language it is.
    pub fn language(&self) -> Language {
        self.lang
    }

    /// Whether it read a dictionary.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// How many words it holds.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether it holds no word at all.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Reads a `.dic` alone (no affixes): one word a line, a line's flags after its `/`.
    pub fn from_dic(lang: Language, dic: &str) -> Result<Dictionary, ProofError> {
        Dictionary::from_hunspell(lang, "", dic)
    }

    /// Reads a `.dic` with its `.aff`: the stems, then every word the affixes make of them.
    pub fn from_hunspell(lang: Language, aff: &str, dic: &str) -> Result<Dictionary, ProofError> {
        let flags = Flags::read(aff);
        let mut out = Dictionary { lang, loaded: true, ..Dictionary::default() };
        let mut stems: Vec<(String, Vec<String>)> = Vec::with_capacity(4096);
        for (n, line) in dic.lines().enumerate() {
            // The first line is how many words follow, which is only ever a hint.
            if n == 0 && line.trim().chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
                continue;
            }
            let line = line.trim_end_matches(['\r', '\n']).trim();
            if line.is_empty() || line.starts_with('\t') || line.starts_with('#') {
                continue;
            }
            let (stem, own) = match line.split_once('/') {
                Some((stem, own)) => (stem.trim(), own.trim()),
                None => (line, ""),
            };
            if stem.is_empty() || stem.chars().count() > MAX_STEM || stem.chars().any(|c| c.is_whitespace() || c.is_control()) {
                continue;
            }
            let own = flags.split(own);
            if own.iter().any(|f| f == &flags.need_affix) && !flags.need_affix.is_empty() {
                out.stems_only.insert(key(stem));
                stems.push((stem.to_string(), own));
                continue;
            }
            if own.iter().any(|f| f == &flags.forbidden) && !flags.forbidden.is_empty() {
                out.forbidden.insert(key(stem));
            }
            if own.iter().any(|f| f == &flags.no_suggest) && !flags.no_suggest.is_empty() {
                out.no_suggest.insert(key(stem));
            }
            stems.push((stem.to_string(), own));
            if stems.len() >= MAX_ENTRIES {
                break;
            }
        }
        if stems.is_empty() {
            return Err(ProofError::Dictionary(lang.code().into()));
        }
        for (stem, own) in &stems {
            out.words.push(key(stem));
            for rule in flags.affixes_for(own) {
                if let Some(word) = rule.apply(stem) {
                    out.words.push(key(&word));
                    if out.words.len() >= MAX_ENTRIES {
                        break;
                    }
                }
            }
        }
        // A stem listed twice, or two rules making the same word, is one word.
        out.words.sort_unstable();
        out.words.dedup();
        out.words.retain(|w| !w.is_empty() && w.chars().count() <= MAX_STEM);
        if out.words.len() > MAX_ENTRIES {
            out.words.truncate(MAX_ENTRIES);
        }
        Ok(out)
    }

    /// Whether `word` (already a [`key`]) is one the dictionary knows.
    pub fn has(&self, word: &str) -> bool {
        self.words.binary_search_by(|w| w.as_str().cmp(word)).is_ok()
    }

    /// Whether the dictionary knows `word` well enough that it is a word and no mistake.
    fn knows(&self, word: &str) -> bool {
        !self.forbidden.contains(word) && self.has(word)
    }

    /// Every word of the dictionary, in order.
    pub fn all(&self) -> &[String] {
        &self.words
    }
}

/// Which flags of a `.dic` mean what, and the affixes they name.
#[derive(Clone, Debug, Default)]
struct Flags {
    /// How flags are written in the `.dic`: one character (`FLAG` absent), two (`long`), a number
    /// (`num`) or a character (`UTF-8`).
    kind: FlagKind,
    need_affix: String,
    forbidden: String,
    no_suggest: String,
    /// The affixes, by the flag they are named by.
    rules: Vec<(String, Rule)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FlagKind {
    #[default]
    Char,
    Long,
    Num,
}

impl Flags {
    fn read(aff: &str) -> Flags {
        let mut out = Flags::default();
        let lines: Vec<&str> = aff.lines().take(MAX_RULES * 8).collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines.get(i).map_or("", |l| l.split('#').next().unwrap_or_default()).trim();
            i += 1;
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let head = parts.next().unwrap_or_default();
            match head {
                "FLAG" => {
                    out.kind = match parts.next().unwrap_or_default() {
                        "long" => FlagKind::Long,
                        "num" => FlagKind::Num,
                        _ => FlagKind::Char,
                    };
                }
                "NEEDAFFIX" | "PSEUDOROOT" => out.need_affix = parts.next().unwrap_or_default().to_string(),
                "FORBIDDENWORD" => out.forbidden = parts.next().unwrap_or_default().to_string(),
                "NOSUGGEST" => out.no_suggest = parts.next().unwrap_or_default().to_string(),
                "PFX" | "SFX" => {
                    // `SFX S Y 3`, then three lines `SFX S 0 s .`.
                    let Some(flag) = parts.next() else { continue };
                    let cross = parts.next().is_some_and(|c| c.eq_ignore_ascii_case("Y"));
                    let count: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    let prefix = head == "PFX";
                    let mut taken = 0;
                    while taken < count.min(MAX_RULES) {
                        let Some(next) = lines.get(i) else { break };
                        let next = next.split('#').next().unwrap_or_default().trim();
                        let mut f = next.split_whitespace();
                        let (Some(kind), Some(same)) = (f.next(), f.next()) else { break };
                        if kind != head || same != flag {
                            break;
                        }
                        i += 1;
                        taken += 1;
                        let (Some(strip), Some(add), condition) = (f.next(), f.next(), f.next().unwrap_or(".")) else { continue };
                        if out.rules.len() >= MAX_RULES || condition.chars().count() > MAX_CONDITION {
                            continue;
                        }
                        let rule = Rule {
                            prefix,
                            strip: if strip == "0" { String::new() } else { strip.to_string() },
                            add: if add == "0" { String::new() } else { add.to_string() },
                            condition: if condition.is_empty() || condition == "." { String::new() } else { condition.to_string() },
                            cross,
                        };
                        if rule.strip.chars().count() <= MAX_STEM && rule.add.chars().count() <= MAX_STEM {
                            out.rules.push((flag.to_string(), rule));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The flags written after a stem's `/`, as they are.
    fn split(&self, text: &str) -> Vec<String> {
        match self.kind {
            FlagKind::Char | FlagKind::Long => {
                let size = if self.kind == FlagKind::Char { 1 } else { 2 };
                text.chars()
                    .collect::<Vec<char>>()
                    .chunks(size)
                    .map(|c| c.iter().collect::<String>())
                    .filter(|f| !f.is_empty() && f != "/")
                    .collect()
            }
            FlagKind::Num => text.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect(),
        }
    }

    /// The affix rules the flags `own` name.
    fn affixes_for<'a>(&'a self, own: &[String]) -> impl Iterator<Item = &'a Rule> + 'a {
        let own = own.to_vec();
        self.rules.iter().filter(move |(flag, _)| own.iter().any(|o| o == flag)).map(|(_, rule)| rule)
    }
}

/// The flag of an affix line: the second word of `PFX A Y 1`.
fn parts_flag(line: &str) -> String {
    let mut parts = line.split_whitespace();
    let (Some(_), Some(flag)) = (parts.next(), parts.next()) else {
        return String::new();
    };
    flag.to_string()
}

/// One affix: a letter or letters taken off one end of a stem (`strip`) and put on the other end of
/// what is left (`add`, `0` for nothing), when the stem's own end matches `condition`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    /// Whether it is a prefix.
    prefix: bool,
    strip: String,
    add: String,
    /// The stem's start (a prefix) or end (a suffix) this applies to; empty for any.
    condition: String,
    cross: bool,
}

impl Rule {
    /// The word this rule makes of `stem`, when it applies to it; `None` when it does not.
    fn apply(&self, stem: &str) -> Option<String> {
        let stem_chars: Vec<char> = stem.chars().collect();
        if stem_chars.len() > MAX_STEM {
            return None;
        }
        let (near, far) = if self.prefix { (0, stem_chars.len().min(MAX_CONDITION)) } else { (stem_chars.len().saturating_sub(MAX_CONDITION), stem_chars.len()) };
        let near = near.min(far);
        if !matches_condition(&stem_chars.get(near..far)?, &self.condition) {
            return None;
        }
        let strip: Vec<char> = self.strip.chars().collect();
        let rest: String = if self.prefix {
            let head = stem_chars.get(..strip.len())?;
            if head != strip.as_slice() {
                return None;
            }
            stem_chars.get(strip.len()..)?.iter().collect()
        } else {
            let tail = stem_chars.get(stem_chars.len().checked_sub(strip.len())?..)?;
            if tail != strip.as_slice() {
                return None;
            }
            stem_chars.get(..stem_chars.len() - strip.len())?.iter().collect()
        };
        let made = if self.prefix { format!("{}{rest}", self.add) } else { format!("{rest}{}", self.add) };
        (!made.is_empty() && made.chars().count() <= MAX_STEM).then_some(made)
    }
}

/// Whether `chars` matches an affix `condition`: literal characters, `.` for any one, `[abc]` for
/// one of a set and `[^abc]` for one that is not. The condition is matched against the stem's end
/// (a suffix) or start (a prefix), so a shorter part than the condition never matches.
fn matches_condition(chars: &[char], condition: &str) -> bool {
    if condition.is_empty() {
        return true;
    }
    let pattern: Vec<char> = condition.chars().collect();
    if chars.len() < pattern.len() {
        return false;
    }
    let tail = chars.get(chars.len() - pattern.len()..).unwrap_or_default();
    let mut p = 0;
    let mut c = 0;
    while p < pattern.len() {
        let Some(&want) = pattern.get(p) else { return true };
        match want {
            '.' => {
                if c >= tail.len() {
                    return false;
                }
                p += 1;
                c += 1;
            }
            '[' => {
                let mut negate = false;
                p += 1;
                if pattern.get(p) == Some(&'^') {
                    negate = true;
                    p += 1;
                }
                let mut matched = false;
                while let Some(&inside) = pattern.get(p) {
                    if inside == ']' {
                        break;
                    }
                    if tail.get(c) == Some(&inside) {
                        matched = true;
                    }
                    p += 1;
                }
                if pattern.get(p) != Some(&']') || c >= tail.len() {
                    return false;
                }
                if matched == negate {
                    return false;
                }
                p += 1;
                c += 1;
            }
            other => {
                if tail.get(c) != Some(&other) {
                    return false;
                }
                p += 1;
                c += 1;
            }
        }
    }
    true
}

impl Dictionary {
    /// Whether `word` is one the dictionary knows, for a lookup of a whole word written as it is.
    pub fn holds(&self, word: &str) -> bool {
        self.knows(&key(word))
    }
}

impl Speller {
    /// Every word of a dictionary, for the suggester: nothing else is looked at.
    fn each_word<F: FnMut(&str)>(dict: &Dictionary, mut f: F) {
        for w in &dict.words {
            f(w);
        }
    }

    /// Whether a word would be suggested: a dictionary may say a word is not to be.
    fn suggestable(&self, word: &str) -> bool {
        let k = key(word);
        !self.dicts.iter().any(|d| d.no_suggest.contains(&k))
    }
}

/// The owner's dictionaries, all of them on at once, and the words they added.
#[derive(Clone, Debug, Default)]
pub struct Speller {
    pub(super) dicts: Vec<Dictionary>,
    personal: Vec<String>,
}

impl Speller {
    /// No dictionary yet.
    pub fn new() -> Speller {
        Speller::default()
    }

    /// Adds a dictionary, replacing one of the same language.
    pub fn put(&mut self, dict: Dictionary) {
        self.dicts.retain(|d| d.lang != dict.lang);
        self.dicts.push(dict);
    }

    /// Sets the personal words, as they are in the settings.
    pub fn set_personal(&mut self, words: &[String]) {
        self.personal = words.iter().take(MAX_PERSONAL).map(|w| key(w)).filter(|w| !w.is_empty()).collect();
        self.personal.sort_unstable();
        self.personal.dedup();
    }

    /// The languages it has a dictionary for.
    pub fn languages(&self) -> Vec<Language> {
        let mut out: Vec<Language> = self.dicts.iter().filter(|d| d.is_loaded()).map(|d| d.lang).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Whether there is nothing to check against.
    pub fn is_empty(&self) -> bool {
        self.dicts.iter().all(|d| d.is_empty()) && self.personal.is_empty()
    }

    /// The language that accepts `word`, if one does: the first of those asked for.
    pub fn accept(&self, word: &str) -> Option<Language> {
        let k = key(word);
        if k.is_empty() {
            return None;
        }
        if self.personal.binary_search(&k).is_ok() {
            // A personal word is the owner's own: it counts for the first language on.
            return Some(self.dicts.first().map_or(Language::EnCa, |d| d.lang));
        }
        self.dicts.iter().find(|d| d.knows(&k)).map(|d| d.lang)
    }

    /// Whether `word` is a word: any dictionary that is on says so, or the owner added it.
    pub fn is_word(&self, word: &str) -> bool {
        self.accept(word).is_some()
    }

    /// The spelling mistakes of `text`, with what could stand in their place.
    pub fn check(&self, text: &str) -> Vec<super::Diagnostic> {
        let mut out: Vec<super::Diagnostic> = Vec::new();
        for w in super::words(text) {
            if out.len() >= super::MAX_DIAGNOSTICS {
                break;
            }
            if let Some(d) = self.bad(text, w) {
                out.push(d);
            }
        }
        out
    }

    /// `word` when nothing accepts it, with its suggestions: what one word is worth.
    pub fn bad(&self, text: &str, word: super::Word<'_>) -> Option<super::Diagnostic> {
        // A hyphen or a dot joins words that are each checked as one: `rendez-vous` is two, and
        // its halves are looked at where they are, so that a correction replaces only that part.
        let parts: Vec<(usize, &str)> = word
            .text
            .split_inclusive(['-', '.', '_'])
            .scan(word.span.start, |at, piece| {
                let start = *at;
                *at += piece.len();
                Some((start, piece.trim_end_matches(['-', '.', '_'])))
            })
            .filter(|(_, p)| !p.is_empty())
            .collect();
        if parts.len() > 1 {
            for (start, piece) in parts {
                let Some(span) = super::Span::of(text, start, start + piece.len()) else { continue };
                if self.is_word(piece) {
                    continue;
                }
                let one = super::Word { text: piece, span };
                if let Some(d) = self.bad_single(text, one) {
                    return Some(d);
                }
            }
            return None;
        }
        if self.is_word(word.text) { None } else { self.bad_single(text, word) }
    }

    fn bad_single(&self, _text: &str, word: super::Word<'_>) -> Option<super::Diagnostic> {
        let lang = self.dicts.first().map_or(Language::EnCa, |d| d.lang);
        let mut d = super::Diagnostic::new(word.span, super::Kind::Spelling, super::Source::Local);
        d.language = Some(lang);
        let suggestions = self.suggest(word.text, 5);
        Some(if suggestions.is_empty() { d } else { d.suggesting(suggestions) })
    }

    /// The words that could stand in for `word`, the best first: those a dictionary has, nearest
    /// by the edits it takes to get there, and in the language the word was written in when one
    /// can be told.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let limit = limit.min(super::MAX_SUGGESTIONS);
        let written = key(word);
        if written.is_empty() || limit == 0 {
            return Vec::new();
        }
        let upper = word.chars().next().is_some_and(|c| c.is_uppercase());
        let mut found: Vec<(u8, usize, String)> = Vec::new();
        let mut probes = 0usize;
        for dist in 1..=2u8 {
            for candidate in edits(&written, dist) {
                if probes >= MAX_PROBES {
                    break;
                }
                probes += 1;
                if candidate == written || self.personal.binary_search(&candidate).is_ok() {
                    continue;
                }
                let Some(accepting) = self.dicts.iter().find(|d| d.knows(&candidate)) else { continue };
                if accepting.no_suggest.contains(&candidate) {
                    continue;
                }
                let same_start = candidate.chars().next() == written.chars().next();
                found.push((dist, if same_start { 0 } else { 1 }, candidate));
                if found.len() >= 64 {
                    break;
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.len().cmp(&b.2.len())).then(a.2.cmp(&b.2)));
        let mut out: Vec<String> = Vec::new();
        for (_, _, mut candidate) in found {
            if out.len() >= limit {
                break;
            }
            if upper {
                if let Some(first) = candidate.chars().next() {
                    candidate = first.to_uppercase().collect::<String>() + candidate.get(first.len_utf8()..).unwrap_or_default();
                }
            }
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
        out
    }
}

/// The words one edit (for `dist` 1) or two (2) away from `word`, bounded: only letters, the
/// apostrophe and the hyphen are tried, and a shorter word is never made longer than by two.
fn edits(word: &str, dist: u8) -> Vec<String> {
    let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyzàâäçéèêëîïôöùûüÿœæ'-".chars().collect();
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    fn push(out: &mut Vec<String>, word: &str, s: String) {
        if out.len() < 4096 && s != word {
            out.push(s);
        }
    }
    // A letter taken out.
    for i in 0..n {
        let mut c = chars.clone();
        c.remove(i);
        push(&mut out, word, c.into_iter().collect());
    }
    // Two letters swapped.
    for i in 0..n.saturating_sub(1) {
        let mut c = chars.clone();
        c.swap(i, i + 1);
        push(&mut out, word, c.into_iter().collect());
    }
    // A letter changed.
    for i in 0..n {
        for a in &alphabet {
            if *a == chars[i] {
                continue;
            }
            let mut c = chars.clone();
            c[i] = *a;
            push(&mut out, word, c.into_iter().collect());
        }
    }
    // A letter put in.
    for i in 0..=n {
        for a in &alphabet {
            let mut c = chars.clone();
            c.insert(i, *a);
            push(&mut out, word, c.into_iter().collect());
        }
    }
    if dist >= 2 {
        let first = out.clone();
        for w in first.iter().take(1200) {
            for again in edits(w, 1).into_iter().take(400) {
                push(&mut out, word, again);
            }
        }
    }
    out
}
