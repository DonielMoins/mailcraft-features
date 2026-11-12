// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The dictionaries' own tests: affixes, what a word is, and what is suggested in its place.

#[test]
fn a_dictionary_is_read() {
    let d = super::Dictionary::from_dic(super::Language::EnCa, "1\ncolour\n").expect("a dictionary");
    assert!(d.holds("colour"));
}
