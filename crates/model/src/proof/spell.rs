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

/// Most words of one dictionary that are measured for one misspelling, before the rest of it is
/// left for another day: a dictionary holds far more words that begin as a word does than could
/// be within two edits of it.
const MOST_PER_DICTIONARY: usize = 8_000;

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

    /// Whether `word` is one the dictionary knows, written as it is (any case, either apostrophe).
    pub fn holds(&self, word: &str) -> bool {
        self.knows(&key(word))
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
                text.chars().collect::<Vec<char>>().chunks(size).map(|c| c.iter().collect::<String>()).filter(|f| !f.is_empty() && f != "/").collect()
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
        if !matches_condition(&stem_chars, &self.condition, self.prefix) {
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
/// one of a set and `[^abc]` for one that is not. Each element of a condition matches exactly one
/// character, read from the stem's start for a prefix and its end for a suffix — which is what keeps
/// a dictionary's `-s` rule off the words its `y → ies` rule is for.
fn matches_condition(chars: &[char], condition: &str, prefix: bool) -> bool {
    if condition.is_empty() {
        return true;
    }
    // A condition this reader cannot read is taken to hold: one word too many is a mistake the
    // owner can add to their dictionary, one word too few is one they cannot correct.
    let Some(pattern) = elements(condition) else {
        return true;
    };
    if chars.len() < pattern.len() {
        return false;
    }
    let part = if prefix { chars.get(..pattern.len()) } else { chars.get(chars.len() - pattern.len()..) };
    let Some(part) = part else {
        return false;
    };
    pattern.iter().zip(part).all(|(want, got)| match want {
        One::Any => true,
        One::Of(inside) => inside.contains(got),
        One::Not(inside) => !inside.contains(got),
        One::This(c) => c == got,
    })
}

/// One character of an affix condition.
enum One {
    /// `.`
    Any,
    /// `[abc]`
    Of(Vec<char>),
    /// `[^abc]`
    Not(Vec<char>),
    /// A character written as itself.
    This(char),
}

/// The characters a condition matches, one element each; `None` for one that is not written as this
/// reader understands (an unclosed `[`, a condition longer than [`MAX_CONDITION`]).
fn elements(condition: &str) -> Option<Vec<One>> {
    let chars: Vec<char> = condition.chars().collect();
    if chars.is_empty() || chars.len() > MAX_CONDITION {
        return None;
    }
    let mut out: Vec<One> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars.get(i)? {
            '.' => {
                out.push(One::Any);
                i += 1;
            }
            '[' => {
                let mut j = i + 1;
                let negate = chars.get(j) == Some(&'^');
                if negate {
                    j += 1;
                }
                let mut inside: Vec<char> = Vec::new();
                while *chars.get(j)? != ']' {
                    if inside.len() >= MAX_CONDITION {
                        return None;
                    }
                    inside.push(*chars.get(j)?);
                    j += 1;
                }
                out.push(if negate { One::Not(inside) } else { One::Of(inside) });
                i = j + 1;
            }
            c => {
                out.push(One::This(*c));
                i += 1;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The owner's dictionaries, all of them on at once, and the words they added: a word is right when
/// any of them knows it, and a wrong word is looked at in every one of them for what could stand in
/// its place.
#[derive(Clone, Debug, Default)]
pub struct Speller {
    pub(super) dicts: Vec<Dictionary>,
    /// The owner's own words, as keys ([`key`]), sorted.
    pub(super) personal: Vec<String>,
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

    /// Whether `word` is a word: any dictionary that is on says so, or the owner added it. A word
    /// joined by a hyphen or a dot (`rendez-vous`, `aujourd’hui`, `example.org`) is a word when it is
    /// one of a dictionary's own or when every part of it is one, so that a dictionary that lists the
    /// parts is not made to look like one that does not.
    pub fn is_word(&self, word: &str) -> bool {
        if self.accept(word).is_some() {
            return true;
        }
        let mut parts = 0usize;
        for part in word.split(['-', '.', '\u{2010}', '\u{2011}']) {
            let part = part.trim_matches(|c: char| !c.is_alphanumeric());
            if part.is_empty() {
                continue;
            }
            parts += 1;
            if self.accept(part).is_none() {
                return false;
            }
        }
        parts > 1
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

    /// The words that could stand in for `word`, the best first: those a dictionary has within two
    /// edits of it, nearest first, then those that begin as it does, then the shorter, then in order.
    /// A word written with a capital comes back with one.
    ///
    /// The search is bounded on both sides: the candidates are the words of the dictionaries that
    /// begin with the word's first letter and are of a length two of its own, and no more than
    /// [`MAX_PROBES`] of them are measured at all, so a right click on the longest word there is
    /// stays quick.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let limit = limit.min(super::MAX_SUGGESTIONS);
        let written = key(word);
        let letters = written.chars().count();
        if written.is_empty() || limit == 0 {
            return Vec::new();
        }
        let upper = word.chars().next().is_some_and(|c| c.is_uppercase());
        let mut found: Vec<(u8, u8, usize, String)> = Vec::new();
        let mut probes = 0usize;
        for dict in &self.dicts {
            // The words that begin as it does: a word two edits away almost always does.
            let (mut from, mut to) = span_beginning_with(&dict.words, written.chars().next().unwrap_or_default());
            if to.saturating_sub(from) > MOST_PER_DICTIONARY {
                to = from.saturating_add(MOST_PER_DICTIONARY);
            }
            while from < to {
                let Some(candidate) = dict.words.get(from) else { break };
                from += 1;
                if probes >= MAX_PROBES {
                    break;
                }
                probes += 1;
                let size = candidate.chars().count();
                if size.abs_diff(letters) > 2 || candidate == &written || dict.no_suggest.contains(candidate) {
                    continue;
                }
                let Some(distance) = distance(&written, candidate, 2) else { continue };
                let same_start = if candidate.chars().next() == written.chars().next() { 0 } else { 1 };
                found.push((distance, same_start, size, candidate.clone()));
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
        let mut out: Vec<String> = Vec::new();
        for (_, _, _, candidate) in found {
            if out.len() >= limit {
                break;
            }
            let said = if upper { capitalised(&candidate) } else { candidate };
            if !out.iter().any(|kept| super::same_word(kept, &said)) && self.personal.binary_search(&key(&said)).is_err() {
                out.push(said);
            }
        }
        out
    }
}

/// The words of `words` (sorted) that begin with `first`, whichever case it is written in: where
/// that stretch is, as a range of the whole.
fn span_beginning_with(words: &[String], first: char) -> (usize, usize) {
    let mut from = words.len();
    let mut to = 0usize;
    for c in [first.to_lowercase().next().unwrap_or(first), first.to_uppercase().next().unwrap_or(first)] {
        let mut prefix = String::new();
        prefix.push(c);
        let start = words.partition_point(|w| w.as_str() < prefix.as_str());
        let run = words.get(start..).map_or(0, |rest| rest.partition_point(|w| w.starts_with(&prefix)));
        if run > 0 {
            from = from.min(start);
            to = to.max(start.saturating_add(run));
        }
    }
    (from.min(words.len()), to.min(words.len()).max(from.min(words.len())))
}

/// `word` with its first character in capitals.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// How many edits apart `a` and `b` are, when that is no more than `most` (`None` when it is more):
/// one letter changed, put in or taken out, or two neighbours swapped. Work stops as soon as the
/// answer is too large, which is what keeps the whole search quick.
fn distance(a: &str, b: &str, most: u8) -> Option<u8> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > usize::from(most) {
        return None;
    }
    // The rows of the table, two at a time.
    let mut before: Vec<usize> = (0..=b.len()).collect();
    let mut now: Vec<usize> = vec![0; b.len() + 1];
    let mut before_before: Vec<usize> = Vec::new();
    for (i, ca) in a.iter().enumerate() {
        if let Some(row) = now.first_mut() {
            *row = i + 1;
        }
        let mut row_least = usize::MAX;
        for (j, cb) in b.iter().enumerate() {
            let swap = if i > 0 && j > 0 && *ca == b.get(j - 1).copied().unwrap_or_default() && a.get(i - 1) == Some(cb) {
                before_before.get(j.saturating_sub(1)).copied().unwrap_or(usize::MAX)
            } else {
                usize::MAX
            };
            let value = (before.get(j).copied().unwrap_or(0) + usize::from(ca != cb))
                .min(now.get(j).copied().unwrap_or(0) + 1)
                .min(now.get(j + 1).copied().unwrap_or(0) + 1)
                .min(swap);
            if let Some(cell) = now.get_mut(j + 1) {
                *cell = value;
            }
            row_least = row_least.min(value);
        }
        if row_least > usize::from(most) {
            return None;
        }
        before_before = std::mem::replace(&mut before, std::mem::replace(&mut now, vec![0; b.len() + 1]));
    }
    let answer = before.get(b.len()).copied()?;
    (answer <= usize::from(most)).then_some(answer as u8)
}
