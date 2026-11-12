// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The dictionary engine's own tests: the affixes it reads, the words they make, the ones it
//! refuses, and what it suggests in their place.
//!
//! Every dictionary here is invented (`example` words and made-up stems), so that what a test says
//! a word is never changes with a dictionary's version or with what this computer has installed.

use std::time::{Duration, Instant};

use super::spell::MAX_ENTRIES;
use super::{Dictionary, Language, MAX_DIAGNOSTICS, MAX_WORD, Speller};

/// A dictionary read from a `.dic` alone, or a panic with the error in it.
fn dic(lang: Language, text: &str) -> Dictionary {
    Dictionary::from_dic(lang, text).expect("a dictionary")
}

/// A dictionary read with its `.aff`.
fn hunspell(aff: &str, text: &str) -> Dictionary {
    Dictionary::from_hunspell(Language::EnCa, aff, text).expect("a dictionary with affixes")
}

/// A speller with just this dictionary on it: what a suggestion is asked of.
fn spell(d: &Dictionary) -> Speller {
    let mut s = Speller::new();
    s.put(d.clone());
    s
}

#[test]
fn a_dictionary_is_read() {
    let d = dic(Language::EnCa, "1\ncolour\n");
    assert!(d.holds("colour"));
    assert!(d.holds("COLOUR"), "case is not what a word is");
    assert!(!d.holds("colourful"));
    assert_eq!(d.len(), 1);
    assert_eq!(d.language(), Language::EnCa);
}

/// A prefix and a suffix that both cross the product make the word both of them make: this is the
/// gap a reader that applies one rule per stem leaves open.
#[test]
fn cross_product_affixes_make_the_paired_word() {
    let aff = "SET UTF-8\nPFX A Y 1\nPFX A 0 re .\nSFX B Y 1\nSFX B 0 s .\n";
    let d = hunspell(aff, "1\nwrite/AB\n");
    assert!(d.holds("write"), "the stem is a word");
    assert!(d.holds("rewrite"), "the prefix's word");
    assert!(d.holds("writes"), "the suffix's word");
    assert!(d.holds("rewrites"), "the word both of them make");
    assert!(!d.holds("rewritess"));
    assert!(!d.holds("rerewrite"));
}

/// A rule that does not cross (`N`) is never joined to the other end, and neither is one whose
/// partner does not, so the cross product cannot be made up by the reader.
#[test]
fn a_rule_that_does_not_cross_is_never_joined() {
    let aff = "PFX A N 1\nPFX A 0 re .\nSFX B Y 1\nSFX B 0 s .\n";
    let d = hunspell(aff, "1\nwrite/AB\n");
    assert!(d.holds("rewrite"));
    assert!(d.holds("writes"));
    assert!(!d.holds("rewrites"), "the prefix says it does not cross");
    let aff = "PFX A Y 1\nPFX A 0 re .\nSFX B N 1\nSFX B 0 s .\n";
    let d = hunspell(aff, "1\nwrite/AB\n");
    assert!(!d.holds("rewrites"), "the suffix says it does not cross");
}

/// `NEEDAFFIX`: a stem listed so that its affixed forms exist is not a word by itself.
#[test]
fn a_needaffix_stem_is_not_a_word_by_itself() {
    let aff = "NEEDAFFIX X\nSFX S Y 1\nSFX S 0 s .\n";
    let d = hunspell(aff, "2\nrun/XS\ntalk/S\n");
    assert!(!d.holds("run"), "the stem is not a word on its own");
    assert!(d.holds("runs"), "the form its suffix makes is");
    assert!(d.holds("talk"), "a stem that is not NEEDAFFIX is a word");
    assert!(d.holds("talks"));
    // And the same through a whole speller: nothing accepts it, so it is flagged.
    let mut s = Speller::new();
    s.put(d);
    assert!(!s.is_word("run"));
    assert!(s.is_word("runs"));
    let text = "run runs";
    let flagged: Vec<&str> = s.check(text).iter().filter_map(|x| text.get(x.span.start..x.span.end)).collect();
    assert_eq!(flagged, ["run"]);
}

/// A word the dictionary forbids is never a word, whichever stem and whichever affix makes it, and
/// it is never suggested either.
#[test]
fn a_forbidden_word_is_never_a_word_nor_a_suggestion() {
    // `foos` is listed as a wrong spelling of `foo`, and the suffix of `foo` would make it too.
    let aff = "FORBIDDENWORD X\nSFX S Y 1\nSFX S 0 s .\n";
    let d = hunspell(aff, "2\nfoo/S\nfoos/X\n");
    assert!(d.holds("foo"));
    assert!(!d.holds("foos"), "the forbidden word is not a word");
    assert!(d.forbids("foos"));
    assert!(!d.has("foos"), "it is not in the word list at all, so nothing can let it through");
    assert!(!d.all().iter().any(|w| w == "foos"));
    // What could stand in its place never includes it.
    let got = spell(&d).suggest("fooz", 8);
    assert!(!got.iter().any(|w| w.eq_ignore_ascii_case("foos")), "{got:?}");
    assert!(got.iter().any(|w| w == "foo"), "{got:?}");
    // The forbidden stem's own affixed forms are wrong too: the flag is the entry's.
    let d = hunspell(aff, "2\nfoo/S\nfoos/X\n");
    assert!(!d.holds("foos"));
    let mut s = Speller::new();
    s.put(d);
    assert!(!s.is_word("foos"));
    assert!(s.is_word("foo"));
}

/// Every way a `.dic` can write its flags is read: one character, two (`long`), a number (`num`)
/// and one character of any script (`UTF-8`).
#[test]
fn every_kind_of_flag_is_read() {
    // One character, the default.
    let aff = "SFX S Y 1\nSFX S 0 s .\n";
    let d = hunspell(aff, "1\ncolour/S\n");
    assert!(d.holds("colours"), "a one-character flag");
    // Two characters.
    let aff = "FLAG long\nSFX AB Y 1\nSFX AB 0 s .\n";
    let d = hunspell(aff, "2\ncolour/AB\nfleur/CD\n");
    assert!(d.holds("colours"), "a two-character flag");
    assert!(!d.holds("fleurs"), "the flags are read two characters at a time, not as one");
    // A flag that is written as another pair of characters is another flag.
    let d = hunspell(aff, "1\nfleur/AC\n");
    assert!(!d.holds("fleurs"), "`AC` is not `AB`");
    // Numbers, with several on one line.
    let aff = "FLAG num\nSFX 101 Y 1\nSFX 101 0 s .\nPFX 102 Y 1\nPFX 102 0 re .\n";
    let d = hunspell(aff, "1\nwrite/101,102\n");
    assert!(d.holds("writes"), "a numbered suffix");
    assert!(d.holds("rewrite"), "a numbered prefix");
    assert!(d.holds("rewrites"), "and both together");
    assert!(!d.holds("101"));
    // UTF-8: a flag written as a letter of the language's own alphabet.
    let aff = "FLAG UTF-8\nSFX é Y 1\nSFX é 0 s .\nPFX ő Y 1\nPFX ő 0 re .\n";
    let d = hunspell(aff, "1\ncafé/éő\n");
    assert!(d.holds("cafés"), "a suffix named by a multi-byte flag");
    assert!(d.holds("recafé"), "a prefix named by another");
    assert!(d.holds("recafés"), "and both together");
    assert!(!d.holds("cafées"));
}

/// An affix may say which flags the word it makes carries, after a `/` (`SFX N aire aît/n'q' plaire`
/// in a French dictionary). Those flags are not part of the word, and a reader that keeps them
/// makes a word nobody could ever write.
#[test]
fn an_affix_that_carries_flags_makes_the_word_without_them() {
    let aff = "FLAG long\nSFX wN Y 2\nSFX wN aire aît/n'q'l' plaire\nSFX wN aire ait/n'q'l' taire\n";
    let d = hunspell(aff, "2\nplaire/wNq.\ntaire/wN\n");
    assert!(d.holds("plaît"), "the affix's word, without the flags it carries");
    assert!(d.holds("tait"), "and the same rule for the stem its condition names");
    assert!(!d.holds("plaît/n'q'l'"), "the flags are not part of it");
    assert!(d.all().iter().all(|w| !w.contains('/')), "no word of a dictionary holds a slash: {:?}", d.all());
    assert!(d.holds("plaire"), "the stem is a word too");
}

/// A hyphenated word the dictionary knows as one word is not taken apart: a French dictionary lists
/// `rendez-vous`, and its halves are not words of it.
#[test]
fn a_hyphenated_word_the_dictionary_knows_is_not_flagged() {
    let mut s = Speller::new();
    s.put(dic(Language::FrCa, "3\nrendez-vous\naujourd'hui\nun\n"));
    assert!(s.is_word("Rendez-vous"));
    let text = "un rendez-vous aujourd'hui.";
    let found = s.check(text);
    assert!(found.is_empty(), "nothing here is a mistake: {found:?}");
    // A misspelt one: this dictionary lists the whole word and not its halves, so the half that is
    // not a word is what is flagged.
    let text = "un rendez-vuos";
    let found = s.check(text);
    let flagged: Vec<&str> = found.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect();
    assert_eq!(flagged, ["rendez"]);
    // In a dictionary that lists the halves, only the half that is wrong is flagged.
    let mut parts = Speller::new();
    parts.put(dic(Language::FrCa, "3\nrendez\nvous\nun\n"));
    let found = parts.check(text);
    let flagged: Vec<&str> = found.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect();
    assert_eq!(flagged, ["vuos"]);
}

/// Conditions are read from the right end of the stem for a suffix, and a set or a negated set is
/// one character of it — which is what keeps `-s` off the words `y → ies` is for.
#[test]
fn conditions_are_read_from_the_end_of_the_stem() {
    let aff = "SFX S Y 3\nSFX S y ies [^aeiou]y\nSFX S 0 s [aeiou]y\nSFX S 0 s [^y]\n";
    let d = hunspell(aff, "3\nhappy/S\nboy/S\ncolour/S\n");
    assert!(d.holds("happies"), "a consonant before the y takes `-ies`");
    assert!(!d.holds("happys"), "and not `-s`");
    assert!(d.holds("boys"), "a vowel before the y takes `-s`");
    assert!(!d.holds("boies"), "and not `-ies`");
    assert!(d.holds("colours"), "a word not ending in y takes `-s`");
    assert!(!d.holds("coloury"));
    // A dot is one character of any kind, and a prefix's condition is read from the start.
    let aff = "PFX P Y 2\nPFX P 0 un .\nPFX P 0 in [^aeiou]\n";
    let d = hunspell(aff, "3\nkind/P\ntrue/P\naudible/P\n");
    assert!(d.holds("unkind"));
    assert!(d.holds("untrue"));
    assert!(d.holds("unaudible"), "a dot holds whatever the stem starts with");
    assert!(d.holds("inkind"), "a negated set at the start of the stem");
    assert!(d.holds("intrue"));
    assert!(!d.holds("inaudible"), "but not where the stem starts with a vowel");
}

/// Accents, ligatures and the typographic apostrophe: a word is the word it is written as, and a
/// missing accent is a different spelling, which is what a correction is for.
#[test]
fn accents_ligatures_and_typographic_apostrophes_are_words() {
    let d = dic(Language::FrCa, "4\ncœur\nœuvre\naujourd’hui\nfrançais\n");
    assert!(d.holds("cœur"));
    assert!(d.holds("CŒUR"));
    assert!(d.holds("œuvre"));
    assert!(d.holds("aujourd’hui"), "a dictionary's typographic apostrophe is the plain one too");
    assert!(d.holds("aujourd'hui"));
    assert!(d.holds("AUJOURD’HUI"));
    assert!(d.holds("français"));
    assert!(!d.holds("coeur"), "a ligature is not two letters: that is another spelling");
    assert!(!d.holds("francais"), "nor is a letter without its accent");
    // Both of those are what a suggestion is for.
    let got = spell(&d).suggest("coeur", 5);
    assert!(got.iter().any(|w| w == "cœur"), "{got:?}");
    let got = spell(&d).suggest("francais", 5);
    assert!(got.iter().any(|w| w == "français"), "{got:?}");
    let got = spell(&d).suggest("aujourd'hui", 5);
    assert!(got.is_empty(), "the word as it is written is not its own suggestion: {got:?}");
}

/// The owner's own words are theirs: they are accepted, in any case, in the language that is on.
#[test]
fn personal_words_are_accepted_and_bounded() {
    let mut s = Speller::new();
    s.put(dic(Language::FrCa, "1\nbonjour\n"));
    s.set_personal(&["Hochelaga".into(), "  ".into(), "hochelaga".into(), "x".repeat(MAX_WORD + 40)]);
    assert_eq!(s.accept("hochelaga"), Some(Language::FrCa));
    assert_eq!(s.accept("HOCHELAGA"), Some(Language::FrCa), "case is folded");
    assert_eq!(s.accept("Hochelaga"), Some(Language::FrCa));
    assert!(s.is_word("hochelaga"));
    assert_eq!(s.accept("ailleurs"), None);
    assert_eq!(s.personal.len(), 1, "an empty word and one that is too long are not words");
    // A word of the owner's is not a word of the dictionary, and it is never suggested back.
    let got = s.suggest("hochelago", 5);
    assert!(!got.iter().any(|w| w.eq_ignore_ascii_case("hochelaga")), "{got:?}");
    // The speller is not empty even with no dictionary at all, once a word is added.
    let mut only = Speller::new();
    assert!(only.is_empty());
    only.set_personal(&["Hochelaga".into()]);
    assert!(!only.is_empty());
    assert!(only.is_word("Hochelaga"));
}

/// Suggestions are the nearest words first, in the case they were asked in, never the word itself
/// and never a word the dictionary keeps out of them.
#[test]
fn suggestions_are_ranked_capitalised_and_never_the_word_itself() {
    let s = {
        let mut s = Speller::new();
        s.put(dic(Language::EnCa, "6\ndocument\ndocuments\ndoctor\ndoctrine\ncolour\ncolours\n"));
        s
    };
    let got = s.suggest("documant", 5);
    assert_eq!(got.first().map(String::as_str), Some("document"), "the nearest word comes first: {got:?}");
    assert!(got.iter().all(|w| !w.is_empty()));
    // A word written with a capital comes back with one, and in the same place in the order.
    let got = s.suggest("Documant", 5);
    assert_eq!(got.first().map(String::as_str), Some("Document"), "{got:?}");
    // The word itself is never the answer, even when the dictionary has it.
    let got = s.suggest("document", 8);
    assert!(!got.iter().any(|w| w.eq_ignore_ascii_case("document")), "{got:?}");
    assert!(got.iter().any(|w| w == "documents"), "{got:?}");
    // Nothing in common, nothing offered; and no more than asked for.
    assert!(s.suggest("zzzz", 5).is_empty());
    assert!(s.suggest("documant", 2).len() <= 2);
    assert!(s.suggest("", 5).is_empty());
    assert!(s.suggest("documant", 0).is_empty());
    assert!(s.suggest("documant", 10_000).len() <= super::MAX_SUGGESTIONS);
}

/// `NOSUGGEST`: a word that is a word, and is not offered — its affixed forms neither.
#[test]
fn a_quiet_word_is_a_word_but_is_never_suggested() {
    let aff = "NOSUGGEST !\nSFX S Y 1\nSFX S 0 s .\n";
    let d = hunspell(aff, "2\ncolour/!S\ndocument/S\n");
    assert!(d.holds("colour"));
    assert!(d.holds("colours"));
    assert!(d.is_quiet("colour"));
    assert!(d.is_quiet("colours"), "the forms of a quiet stem are quiet too");
    for word in ["coluor", "coloures"] {
        let got = spell(&d).suggest(word, 8);
        assert!(!got.iter().any(|w| w.starts_with("colou")), "{word} gave {got:?}");
    }
    // A word that is not quiet is still offered, so the test would catch an over-wide filter.
    let got = spell(&d).suggest("documant", 5);
    assert!(got.iter().any(|w| w == "document"), "{got:?}");
}

/// Every place a diagnostic points at is a range of the text it was found in, on its character
/// boundaries, and never empty: a correction goes where the wrong word is or nowhere.
#[test]
fn every_diagnostic_span_is_a_char_boundary_range_of_its_text() {
    let mut s = Speller::new();
    s.put(dic(Language::EnCa, "5\ndocument\ncolour\ncentre\nneighbour\nthe\n"));
    s.put(dic(Language::FrCa, "3\nfrançais\naujourd'hui\nMontréal\n"));
    let texts = [
        "Bonjour, I reviewed les documents hier soir.",
        "a\u{300}b\u{fffd}\u{1F600} 🙂 ok, the documant is hear",
        "«Ça va?» dit-il, en français — aujourd’hui, à Montréal.",
        "e\u{301}\u{302}\u{303} documant\u{200d}colour",
        "rendez-vuos et aujourd'hui",
        "  ",
        "",
    ];
    for text in texts {
        for d in s.check(text) {
            assert!(d.span.fits(text), "{d:?} does not fit {text:?}");
            assert!(d.span.start < d.span.end, "{d:?} is empty");
            assert!(text.is_char_boundary(d.span.start) && text.is_char_boundary(d.span.end));
            assert!(text.get(d.span.start..d.span.end).is_some(), "{d:?}");
            assert!(text.get(d.span.start..d.span.end).is_some_and(|w| !w.is_empty()));
        }
    }
}

/// A dictionary far larger than the one on a desktop still loads, and is held to
/// [`MAX_ENTRIES`] however many words its affixes would make.
#[test]
fn a_hundred_thousand_words_load_within_the_caps() {
    let mut text = String::with_capacity(1_100_000);
    text.push_str("100000\n");
    for n in 0..100_000usize {
        text.push_str(&format!("word{n}\n"));
    }
    let started = Instant::now();
    let d = dic(Language::EnCa, &text);
    assert_eq!(d.len(), 100_000, "every word of it is a word");
    assert!(d.holds("word0"));
    assert!(d.holds("word99999"));
    assert!(!d.holds("word100000"));
    assert!(started.elapsed() < Duration::from_secs(30), "loading stays quick: {:?}", started.elapsed());
}

/// A big `.dic` with affixes, and lines that cannot be read: it finishes, keeps to the caps, and
/// nothing panics.
#[test]
fn a_huge_dictionary_with_affixes_finishes() {
    let aff = "SET UTF-8\nFLAG long\nFORBIDDENWORD XX\nNEEDAFFIX YY\nSFX SS Y 2\nSFX SS 0 s .\nSFX SS 0 es [sxz]\nPFX PP Y 1\nPFX PP 0 re .\n";
    let mut text = String::with_capacity(4_000_000);
    text.push_str("200000\n");
    for n in 0..200_000usize {
        match n % 5 {
            0 => text.push_str(&format!("stem{n}/SSPP\n")),
            1 => text.push_str(&format!("stem{n}/SS\n")),
            2 => text.push_str(&format!("stem{n}/YYSS\n")),
            3 => text.push_str(&format!("stem{n}/XX\n")),
            _ => text.push_str(&format!("stem{n}\n")),
        }
    }
    let started = Instant::now();
    let d = hunspell(aff, &text);
    assert!(d.len() <= MAX_ENTRIES, "the cap is kept: {}", d.len());
    assert!(d.len() > 200_000, "the affixes made words of the stems: {}", d.len());
    assert!(d.holds("stem0"));
    assert!(d.holds("restem0"));
    assert!(d.holds("stem0s"));
    assert!(d.holds("restem0s"), "both ends of the cross product");
    assert!(!d.holds("stem2"), "a NEEDAFFIX stem is not a word");
    assert!(d.holds("stem2s"), "but the form its suffix makes is");
    assert!(!d.holds("stem3"), "a forbidden stem is not a word");
    assert!(!d.holds("restem3"), "nor any form of it");
    assert!(started.elapsed() < Duration::from_secs(60), "loading stays bounded: {:?}", started.elapsed());
}

/// A `.aff` that is truncated, random, or full of lines that cannot be read is read for what it
/// says and skipped for the rest: no panic, no loop, and the words it can make are still made.
#[test]
fn a_corrupt_or_truncated_affix_file_is_read_without_a_panic() {
    let dictionary = "2\ndocument/S\ncolour/S\n";
    let cases = [
        "",
        "\n\n\n",
        "SFX",
        "SFX S",
        "SFX S Y",
        "FLAG",
        "FLAG long extra words here",
        "FLAG UTF-8",
        "SFX S Y 999999999999999999999",
        "SFX S Y 3\nSFX S 0 s .\n",
        "SFX S Y 1\nSFX S 0\n",
        "SFX S Y 1\nSFX S 0 s [abc",
        "SFX S Y 1\nSFX S 0 s [",
        "SFX S Y 1\nPFX S Y 1\nPFX S 0 re .",
        "NEEDAFFIX\nFORBIDDENWORD\nNOSUGGEST\n",
        "PFX A Y 1\nPFX A 0 re .\ntrailing garbage\n\x01\x02\x03\n",
        "# a comment\nSFX S Y 1\nSFX S 0 s .\n# another\n",
        "SFX S Y 1\r\nSFX S 0 s .\r\n",
        "SFX S Y 1\nSFX S 0 s .\nSFX S Y 1\nSFX S 0 s .\n",
    ];
    for aff in cases {
        let d = hunspell(aff, dictionary);
        assert!(d.len() >= 2, "{aff:?} made {} words", d.len());
        assert!(d.holds("document"), "{aff:?}");
    }
    // A condition nobody can read is taken to hold, so the rule still makes its word.
    let d = hunspell("SFX S Y 1\nSFX S 0 s [abc", dictionary);
    assert!(d.holds("colours"), "an unreadable condition does not lose the word");
    // Random text, a very long line, and a line with thousands of flags: read, and bounded.
    let noise = noise(0x5eed_1234_9876_abcd, 4_000);
    let d = hunspell(&noise, dictionary);
    assert!(d.holds("document"));
    let long = "x".repeat(100_000);
    let d = hunspell(&long, dictionary);
    assert!(d.holds("document"));
    let flags = "S".repeat(10_000);
    let d = hunspell("SFX S Y 1\nSFX S 0 s .\n", &format!("1\ndocument/{flags}\n"));
    assert!(d.len() <= 4, "a line is cut to a bounded number of flags: {}", d.len());
}

/// The edits between two words, worked out in full with no cut-off and no shortcuts: one letter
/// changed, put in or taken out, or two neighbours swapped — the format's distance, and what a
/// suggestion is measured with. The engine may stop early; this may not, because it is what the
/// engine is checked against.
fn exact(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d: Vec<Vec<usize>> = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    if let Some(row) = d.first_mut() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = j;
        }
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let swap = if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i - 2][j - 2] + 1 } else { usize::MAX };
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1])).min(swap);
        }
    }
    d[a.len()][b.len()]
}

/// An edit-distance table worked out by hand, so that no change to the search can make a word look
/// nearer than it is. Each pair is also measured in full, so a wrong number in the table is a
/// failure of the table.
#[test]
fn an_edit_distance_table_is_exact() {
    // (the word in the dictionary, what was typed, the edits between them)
    let table = [
        ("colour", "colours", 1),
        ("colour", "colur", 1),
        ("colour", "coluor", 1),
        ("colour", "calour", 1),
        ("colour", "coluors", 2),
        ("colour", "coluosr", 2),
        ("colour", "cluoor", 3),
        ("colour", "colourful", 3),
        ("please", "pleese", 1),
        ("please", "pealse", 2),
        ("please", "pealses", 3),
        // A swap and two letters changed: three, and only a table that charges one edit for the
        // swap says so.
        ("abcdef", "acbxyf", 3),
        ("abcdef", "acbdef", 1),
        ("pierre", "pirre", 1),
        ("pierre", "pierr", 1),
        ("pierre", "please", 3),
        ("pierre", "pour", 4),
    ];
    for (word, typed, edits) in table {
        assert_eq!(exact(typed, word), edits, "{typed:?} → {word:?} by hand");
        let s = spell(&dic(Language::EnCa, &format!("1\n{word}\n")));
        let got = s.suggest(typed, 5);
        let offered = got.iter().any(|w| w.eq_ignore_ascii_case(word));
        assert_eq!(offered, edits <= 2, "{typed:?} → {word:?} is {edits} edits apart: {got:?}");
    }
}

/// Every suggestion the search makes is what a full edit distance says it is, in the order that
/// distance says: every word of one, two and three letters over three letters is asked about, and
/// the answer is compared with what the same distance worked out in full would offer.
#[test]
fn suggestions_agree_with_an_exact_edit_distance() {
    let mut all: Vec<String> = Vec::new();
    for n in 1..=3usize {
        let mut level = vec![String::new()];
        for _ in 0..n {
            let mut next: Vec<String> = Vec::new();
            for base in &level {
                for c in ['a', 'b', 'c'] {
                    let mut word = base.clone();
                    word.push(c);
                    next.push(word);
                }
            }
            level = next;
        }
        all.extend(level);
    }
    let mut text = format!("{}\n", all.len());
    for word in &all {
        text.push_str(word);
        text.push('\n');
    }
    let s = spell(&dic(Language::EnCa, &text));
    assert!(s.is_word("abc"));
    for query in &all {
        let got = s.suggest(query, 8);
        let first = query.chars().next();
        for word in &got {
            let edits = exact(query, word);
            assert!((1..=2).contains(&edits), "{query:?} was offered {word:?}, {edits} edits away");
            assert!(word.chars().next() == first, "only words that begin as it does are looked at");
        }
        let mut want: Vec<String> = all.iter().filter(|w| *w != query && w.chars().next() == first && exact(query, w) <= 2).cloned().collect();
        want.sort_by(|a, b| exact(query, a).cmp(&exact(query, b)).then(a.chars().count().cmp(&b.chars().count())).then(a.cmp(b)));
        want.truncate(8);
        assert_eq!(got, want, "the suggestions for {query:?}");
    }
}

/// The dictionaries the owner has, together: Canadian English and Canadian French. What a wrong
/// word is offered is a word near its letters, and what only shares its first letter is not
/// offered at all.
#[test]
fn a_bilingual_dictionary_offers_only_what_is_near() {
    let mut s = Speller::new();
    s.put(dic(Language::EnCa, "7\ndocument\ncolour\nplease\nproject\nplus\nsee\nthe\n"));
    s.put(dic(Language::FrCa, "7\nbonjour\npierre\npeut\npour\nprojet\nmerci\nrevoir\n"));
    // `pierre` is three edits from `please` and four from `pour`: a reader that offers them for it
    // has measured the letters wrongly, and its corrections cannot be trusted.
    let got = s.suggest("Pierre", 8);
    assert!(got.iter().all(|w| exact("pierre", &w.to_lowercase()) <= 2), "nothing far away: {got:?}");
    for far in ["please", "project", "projet", "pour", "peut", "plus"] {
        assert!(!got.iter().any(|w| w.eq_ignore_ascii_case(far)), "{far} is not near pierre: {got:?}");
    }
    // And the words that are near it are offered, nearest first, in the case they were typed in.
    let got = s.suggest("Pirre", 5);
    assert_eq!(got.first().map(String::as_str), Some("Pierre"), "{got:?}");
    let got = s.suggest("pierr", 5);
    assert_eq!(got.first().map(String::as_str), Some("pierre"), "{got:?}");
    // The one word that is not a word is the one flagged, with a word of the same dictionary.
    let text = "Bonjour Pierre, see the documant please.";
    let found = s.check(text);
    let flagged: Vec<&str> = found.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect();
    assert_eq!(flagged, ["documant"], "only the word that is not one: {flagged:?}");
    assert_eq!(found.first().and_then(|d| d.suggestions.first()).map(String::as_str), Some("document"));
}

/// Random text of the alphabet a `.aff` is written in, from a seed, so a failure can be run again.
fn noise(mut seed: u64, lines: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 /-^[]#.\t\\\"'";
    let mut out = String::with_capacity(lines * 24);
    for _ in 0..lines {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let len = (seed >> 33) as usize % 40;
        for _ in 0..len {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            if let Some(c) = ALPHABET.get((seed >> 33) as usize % ALPHABET.len()) {
                out.push(*c as char);
            }
        }
        out.push('\n');
    }
    out
}

/// A dictionary of 20,000 stems, as a package installs one: four 2-letter syllables, a suffix that
/// crosses with a prefix, and enough words for a suggestion to be a search.
fn twenty_thousand_stems() -> (String, String) {
    const SYL: [&str; 16] = ["ka", "lo", "ri", "men", "tor", "sa", "vi", "nel", "dar", "po", "lin", "qua", "ber", "sto", "mi", "ra"];
    let aff = "SET UTF-8\nFLAG UTF-8\nPFX P Y 1\nPFX P 0 re .\nSFX S Y 2\nSFX S 0 s .\nSFX S 0 es [sxz]\n";
    let mut text = String::with_capacity(20_000 * 16);
    text.push_str("20000\n");
    for n in 0..20_000usize {
        let a = SYL.get(n % 16).copied().unwrap_or("ka");
        let b = SYL.get((n / 16) % 16).copied().unwrap_or("ka");
        let c = SYL.get((n / 256) % 16).copied().unwrap_or("ka");
        let d = SYL.get((n / 4_096) % 16).copied().unwrap_or("ka");
        text.push_str(&format!("{a}{b}{c}{d}/PS\n"));
    }
    (aff.into(), text)
}

/// How long loading one is, one word of it, and a suggestion for a misspelling of one: the numbers
/// a right click and a fresh start are felt in.
///
/// Run with `cargo test -p mailcraft-model --lib proof -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not an assertion: run it with --ignored --nocapture"]
fn a_twenty_thousand_stem_dictionary_is_quick() {
    let (aff, text) = twenty_thousand_stems();
    let started = Instant::now();
    let d = hunspell(&aff, &text);
    let loading = started.elapsed();
    assert!(d.len() > 20_000, "the affixes made words of the stems: {}", d.len());
    assert_eq!(d.len(), 80_000, "stem, prefix, suffix and cross product for each: {}", d.len());

    // One word of it, asked the way the window asks: through the speller, which folds the case
    // and looks in every dictionary that is on.
    let mut s = Speller::new();
    s.put(d.clone());
    let rounds = 1_000u32;
    let started_word = Instant::now();
    let mut known = 0usize;
    for _ in 0..rounds {
        if s.is_word("kalorimen") && s.is_word("kalorimens") {
            known += 1;
        }
    }
    let word_total = started_word.elapsed();
    let per_word = word_total / rounds;
    assert_eq!(known as u32, rounds, "the words asked about are the dictionary's own");

    // A misspelling of an 8-letter stem, two letters the wrong way round, twenty times over.
    let started_suggest = Instant::now();
    let mut worst = Duration::ZERO;
    let mut got = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        got = s.suggest("kalorimne", 5);
        worst = worst.max(started.elapsed());
    }
    let per_suggest = started_suggest.elapsed() / 20;
    let one = s.suggest("kalorimne", 5);
    println!(
        "20k stems ({} words, 3 affix rules): load {loading:?}, is_word {per_word:?} ({rounds} calls in {:?}), \
         suggest 8 letters {per_suggest:?} (worst {worst:?}), {one:?}",
        d.len(),
        word_total,
    );
    assert!(!got.is_empty(), "the stem itself is not its own suggestion, its forms are: {got:?}");
    assert!(loading < Duration::from_secs(10), "loading a real dictionary stays quick: {loading:?}");
    assert!(per_word < Duration::from_millis(1), "one word is a lookup: {per_word:?}");
    assert!(worst < Duration::from_millis(500), "a suggestion stays quick: {worst:?}");
}

/// A smoke test against the dictionaries a distribution installs, when they are on this computer:
/// the words Canadian English and Canadian French are expected to know, and a few sentences, read
/// and measured rather than assumed.
///
/// It reads no dictionary of its own and ships none: with no folder named it says so and stops.
/// Run with
/// `MAILCRAFT_DICT_DIR=/usr/share/hunspell cargo test -p mailcraft-model --lib
/// proof::spell_tests::the_installed_dictionaries -- --ignored --nocapture`
/// (Arch: the packages `hunspell-en_ca` and `hunspell-fr-comprehensive`).
#[test]
#[ignore = "reads the dictionaries installed on this computer: name their folder in MAILCRAFT_DICT_DIR"]
fn the_installed_dictionaries_accept_their_own_words() {
    let Some(dir) = std::env::var("MAILCRAFT_DICT_DIR").ok().filter(|d| !d.is_empty()) else {
        println!("no MAILCRAFT_DICT_DIR: nothing to read, nothing to say");
        return;
    };
    let mut s = Speller::new();
    for (lang, stem) in [(Language::EnCa, "en_CA"), (Language::FrCa, "fr_CA")] {
        let aff = std::fs::read_to_string(format!("{dir}/{stem}.aff"));
        let dic = std::fs::read_to_string(format!("{dir}/{stem}.dic"));
        let (Ok(aff), Ok(dic)) = (aff, dic) else {
            println!("{stem}.aff/.dic is not in {dir}");
            continue;
        };
        let started = Instant::now();
        match Dictionary::from_hunspell(lang, &aff, &dic) {
            Ok(d) => {
                println!(
                    "{stem}: {} words out of {:.2} MB of .dic and {} bytes of .aff in {:?}",
                    d.len(),
                    dic.len() as f64 / 1_048_576.0,
                    aff.len(),
                    started.elapsed()
                );
                s.put(d);
            }
            Err(e) => println!("{stem}: {e}"),
        }
    }
    assert!(!s.is_empty(), "a dictionary was read: {dir}");
    // The words a Canadian pair is expected to know, in both spellings of the apostrophe.
    let known = ["colour", "centre", "neighbour", "Montréal", "Québec", "français", "aujourd'hui", "aujourd’hui", "rendez-vous"];
    let missing: Vec<&str> = known.iter().copied().filter(|w| !s.is_word(w)).collect();
    println!("words refused by the installed dictionaries: {missing:?}");
    assert!(missing.is_empty(), "{missing:?} should be known: {dir}");
    // What a person writes: nothing in these is a spelling mistake, so nothing may be flagged.
    for text in [
        "The colour of the centre is fine, neighbour.",
        "Bonjour Pierre, j'ai vu le document sur la table à Montréal.",
        "Rendez-vous aujourd’hui à Québec, s'il vous plaît.",
        "I reviewed les documents hier soir and everything looks correct.",
    ] {
        let found = s.check(text);
        let flagged: Vec<&str> = found.iter().filter_map(|d| text.get(d.span.start..d.span.end)).collect();
        let said: Vec<&String> = found.iter().filter_map(|d| d.suggestions.first()).collect();
        println!("{text:?}\n  flagged {flagged:?}\n  offered {said:?}");
        for w in &flagged {
            assert!(!known.iter().any(|k| k.eq_ignore_ascii_case(w)), "{w:?} is a word of the dictionary: {text:?}");
        }
    }
    // A misspelling of one, so that a suggestion is seen against a real dictionary too.
    let got = s.suggest("documant", 5);
    println!("documant → {got:?}");
    assert!(got.iter().any(|w| w == "document"), "{got:?}");
}

proptest::proptest! {
    /// Whatever it is given — combining marks, emoji, ligatures, unpaired brackets — checking,
    /// suggesting and asking about a word never panics, and every diagnostic is a range of the
    /// text it was found in.
    #[test]
    fn arbitrary_unicode_is_checked_without_a_panic(text in "\\PC{0,160}", word in "\\PC{0,40}") {
        let mut s = Speller::new();
        s.put(dic(Language::EnCa, "4\ndocument\ncolour\nthe\nand\n"));
        s.put(dic(Language::FrCa, "3\nfrançais\naujourd'hui\nMontréal\n"));
        s.set_personal(&["Hochelaga".into(), "œ".into()]);
        let found = s.check(&text);
        proptest::prop_assert!(found.len() <= MAX_DIAGNOSTICS);
        for d in &found {
            proptest::prop_assert!(d.span.fits(&text), "{:?} does not fit {:?}", d, text);
            proptest::prop_assert!(d.span.start < d.span.end);
            proptest::prop_assert!(text.is_char_boundary(d.span.start));
            proptest::prop_assert!(text.is_char_boundary(d.span.end));
            for said in &d.suggestions {
                proptest::prop_assert!(!said.is_empty());
                proptest::prop_assert!(!said.chars().any(char::is_control), "{said:?}");
            }
        }
        let offered = s.suggest(&word, 5);
        proptest::prop_assert!(offered.len() <= super::MAX_SUGGESTIONS);
        proptest::prop_assert!(!offered.iter().any(|w| w.is_empty()));
        let _ = s.is_word(&word);
        let _ = s.is_word(&text);
        let _ = s.accept(&text);
        let _ = super::words(&text);
    }

    /// A `.dic` and its `.aff` written by somebody hostile: a line of any bytes loads without a
    /// panic, and what comes out is a word list within its caps.
    #[test]
    fn a_hostile_dictionary_loads_without_a_panic(dic_text in "\\PC{0,400}", aff_text in "\\PC{0,400}") {
        if let Ok(d) = Dictionary::from_hunspell(Language::EnCa, &aff_text, &dic_text) {
            proptest::prop_assert!(d.len() <= MAX_ENTRIES);
            for w in d.all() {
                proptest::prop_assert!(!w.is_empty());
                proptest::prop_assert!(w.chars().count() <= MAX_WORD.max(64));
                proptest::prop_assert!(!w.chars().any(char::is_control));
                // A flag an affix carries is not part of the word it makes, and no word is two.
                proptest::prop_assert!(!w.contains('/'));
                proptest::prop_assert!(!w.chars().any(char::is_whitespace));
            }
        }
    }
}
