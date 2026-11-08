//! An `id:` is put into the bytes of a file and nothing else changes.

use super::*;
use crate::domain::id::test_support::{docid, iid};

fn number(n: u32) -> ItemNumber {
    ItemNumber::new(n).expect("a number")
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("text")
}

fn id_line() -> String {
    format!("id: {}", docid(1))
}

#[test]
fn the_id_goes_right_after_the_opening_fence() {
    let out = text(with_doc_id(b"---\nkind: finding\n---\nbody\n", docid(1)));
    assert_eq!(
        out,
        format!("---\n{}\nkind: finding\n---\nbody\n", id_line())
    );
}

#[test]
fn a_file_with_crlf_gets_a_crlf() {
    let out = text(with_doc_id(
        b"---\r\nkind: finding\r\n---\r\nbody\r\n",
        docid(1),
    ));
    assert_eq!(
        out,
        format!("---\r\n{}\r\nkind: finding\r\n---\r\nbody\r\n", id_line())
    );
}

#[test]
fn a_file_with_no_front_matter_gets_one_before_its_first_byte() {
    let out = text(with_doc_id(b"just a body\n", docid(1)));
    assert_eq!(out, format!("---\n{}\n---\njust a body\n", id_line()));
    // An empty file too.
    assert_eq!(
        text(with_doc_id(b"", docid(1))),
        format!("---\n{}\n---\n", id_line())
    );
}

#[test]
fn an_opening_fence_that_ends_the_file_is_given_a_line_end() {
    let out = text(with_doc_id(b"---", docid(1)));
    assert_eq!(out, format!("---\n{}\n", id_line()));
}

#[test]
fn only_the_inserted_line_is_new() {
    let original = "---\nkind: finding\ntitle: \"é\"\n---\n\nbody\n";
    let out = text(with_doc_id(original.as_bytes(), docid(1)));
    assert_eq!(out.replacen(&format!("{}\n", id_line()), "", 1), original);
}

const DESIGN: &str = "---\nspec: spec/s\nitems:\n  - number: 1\n    summary: one\n  - id: 0192f3a4-5b6c-7d8e-8f90-123456789abc\n    number: 2\n    summary: two\n  - summary: three\n    number: 3\nother: x\n---\nnotes\n";

#[test]
fn the_ids_go_into_the_entries_that_have_none() {
    let ids = BTreeMap::from([(number(1), iid(11)), (number(3), iid(13))]);
    let out = text(with_item_ids(DESIGN.as_bytes(), &ids).expect("changed").0);
    assert_eq!(
        out,
        format!(
            "---\nspec: spec/s\nitems:\n  - number: 1\n    id: {}\n    summary: one\n  - id: 0192f3a4-5b6c-7d8e-8f90-123456789abc\n    number: 2\n    summary: two\n  - summary: three\n    id: {}\n    number: 3\nother: x\n---\nnotes\n",
            iid(11),
            iid(13)
        )
    );
}

#[test]
fn an_entry_with_an_id_is_left_alone_even_when_its_number_is_asked_for() {
    let ids = BTreeMap::from([(number(2), iid(12))]);
    assert_eq!(with_item_ids(DESIGN.as_bytes(), &ids), None);
}

#[test]
fn a_number_nobody_asked_for_gets_nothing() {
    let ids = BTreeMap::from([(number(9), iid(19))]);
    assert_eq!(with_item_ids(DESIGN.as_bytes(), &ids), None);
}

#[test]
fn entries_at_the_indent_of_the_key_and_crlf_files_are_read() {
    let file = "---\r\nitems:\r\n- number: 1\r\n  summary: one\r\nother: x\r\n---\r\n";
    let ids = BTreeMap::from([(number(1), iid(11))]);
    let out = text(with_item_ids(file.as_bytes(), &ids).expect("changed").0);
    assert_eq!(
        out,
        format!(
            "---\r\nitems:\r\n- number: 1\r\n  id: {}\r\n  summary: one\r\nother: x\r\n---\r\n",
            iid(11)
        )
    );
}

#[test]
fn what_is_not_a_block_of_entries_is_not_changed() {
    let ids = BTreeMap::from([(number(1), iid(11))]);
    // A flow mapping has no line to put the id on.
    let flow = "---\nitems:\n  - {number: 1, summary: one}\n---\n";
    assert_eq!(with_item_ids(flow.as_bytes(), &ids), None);
    // No items key.
    assert_eq!(with_item_ids(b"---\nspec: spec/s\n---\n", &ids), None);
    // No front matter.
    assert_eq!(with_item_ids(b"items:\n  - number: 1\n", &ids), None);
    // An unclosed front matter.
    assert_eq!(with_item_ids(b"---\nitems:\n  - number: 1\n", &ids), None);
    // Not text.
    assert_eq!(with_item_ids(&[0xff, 0xfe], &ids), None);
    // An empty list.
    assert_eq!(with_item_ids(b"---\nitems:\n---\n", &ids), None);
}
