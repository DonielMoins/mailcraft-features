// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The grammar server's answer, read: offsets, sizes, and what a hostile one can make it do.

#[test]
fn offsets_are_utf16_are_bytes() {
    let text = "café noir";
    let index = super::grammar::utf16_index(text);
    assert_eq!(super::grammar::utf16_to_byte(text, &index, 4), Some(5));
}
