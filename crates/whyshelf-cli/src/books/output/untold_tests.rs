//! Tests of the lines `check` and `stale` write on stderr for names that cannot be
//! told now: one line per cause, and for a cause of one (document, name) pair the
//! line they wrote before names were gathered.

use whyshelf_core::app::shelf::{Absent, BooksCheck, Health, Names, StaleReport};
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{ArtifactKey, ArtifactName, Space, Untold};
use whyshelf_core::domain::shelf::doc::DocKey;
use whyshelf_core::domain::shelf::schema::EntryName;

use super::*;

fn docid(n: u8) -> DocId {
    DocId::new(
        format!("01900000-0000-7000-8000-0000000000{n:02x}")
            .parse::<EntityId>()
            .unwrap(),
    )
}

fn key(text: &str) -> Option<DocKey> {
    Some(DocKey {
        book: BookName::new("note").unwrap(),
        form: KeyPattern::Slug.parse(text).unwrap(),
    })
}

fn not_built(krate: &str) -> Untold {
    Untold::Said(NonBlank::new(format!("the docs of the crate `{krate}` are not built")).unwrap())
}

fn artifact(space: &str, name: &str) -> ArtifactKey {
    ArtifactKey::new(Space::new(space).unwrap(), ArtifactName::new(name).unwrap())
}

/// The name `name` of `rust-item`, written by the document `doc` (keyed `note:one`
/// for 1 and `note:two` for 2, no key for the others) under the link `link`.
fn unknown(doc: u8, link: &str, name: &str, why: &Untold) -> Unknown {
    Unknown {
        doc: docid(doc),
        key: match doc {
            1 => key("one"),
            2 => key("two"),
            _ => None,
        },
        link: EntryName::new(link).unwrap(),
        artifact: artifact("rust-item", name),
        why: why.clone(),
    }
}

fn check_of(unknown: Vec<Unknown>) -> BooksCheck {
    BooksCheck::Checked {
        books: Vec::new(),
        health: Health::default(),
        uncovered: Vec::new(),
        absent: Vec::new(),
        unknown,
        moved: Vec::new(),
        exempt: Vec::new(),
        unneeded: Vec::new(),
        repeated: Vec::new(),
        names: Names::default(),
    }
}

fn stale_of(unknown: Vec<Unknown>) -> StaleReport {
    StaleReport {
        entries: Vec::new(),
        origins: Vec::new(),
        unknown,
        names: Names::default(),
        health: Health::default(),
    }
}

#[test]
fn one_pair_is_the_line_it_always_was() {
    let why = not_built("other");
    let items = vec![unknown(1, "items", "other::Item", &why)];
    assert_eq!(
        check_warnings(&check_of(items.clone())),
        [
            "warning: note:one names other::Item in rust-item, which cannot be told: the docs of the crate `other` are not built"
        ]
    );
    assert_eq!(
        stale_warnings(&stale_of(items)),
        [
            "warning: note:one pins other::Item in rust-item, which cannot be told now, so whether its approval holds cannot be said: the docs of the crate `other` are not built"
        ]
    );
    // A document with no key is named by its identity.
    let keyless = vec![unknown(3, "items", "other::Item", &Untold::NoAnswer)];
    assert_eq!(
        check_warnings(&check_of(keyless)),
        [format!(
            "warning: {} names other::Item in rust-item, which cannot be told: the adapter gave no answer for it",
            docid(3)
        )]
    );
}

#[test]
fn two_names_of_one_document_are_one_line() {
    let why = not_built("other");
    let items = vec![
        unknown(1, "items", "other::A", &why),
        unknown(1, "items", "other::B", &why),
    ];
    assert_eq!(
        check_warnings(&check_of(items.clone())),
        [
            "warning: note:one names 2 names in rust-item, which cannot be told, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B"
        ]
    );
    assert_eq!(
        stale_warnings(&stale_of(items)),
        [
            "warning: note:one pins 2 names in rust-item, which cannot be told now, so whether its approval holds cannot be said, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B"
        ]
    );
    // A document with no key is named by its identity.
    let keyless = vec![
        unknown(3, "items", "other::A", &Untold::NoAnswer),
        unknown(3, "items", "other::B", &Untold::NoAnswer),
    ];
    assert!(
        check_warnings(&check_of(keyless))[0]
            .starts_with(&format!("warning: {} names 2 names in", docid(3)))
    );
}

#[test]
fn two_documents_are_named_by_their_keys() {
    let why = not_built("other");
    let items = vec![
        unknown(1, "items", "other::A", &why),
        unknown(2, "items", "other::B", &why),
    ];
    assert_eq!(
        check_warnings(&check_of(items.clone())),
        [
            "warning: 2 names in rust-item, named by 2 documents (note:one, note:two), cannot be told, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B"
        ]
    );
    assert_eq!(
        stale_warnings(&stale_of(items)),
        [
            "warning: 2 names in rust-item, pinned by the approvals of 2 documents (note:one, note:two), cannot be told now, so whether those approvals hold cannot be said, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B"
        ]
    );
}

#[test]
fn four_documents_show_three_keys_and_say_how_many_more() {
    let why = not_built("other");
    let items: Vec<Unknown> = (1..=4)
        .map(|n| unknown(n, "items", &format!("other::{n}"), &why))
        .collect();
    let line = check_warnings(&check_of(items));
    assert_eq!(
        line,
        [format!(
            "warning: 4 names in rust-item, named by 4 documents (note:one, note:two, {}, and 1 more), cannot be told, each for this reason: the docs of the crate `other` are not built; names: other::1, other::2, other::3, and 1 more",
            docid(3)
        )]
    );
}

#[test]
fn four_names_show_three_and_say_how_many_more() {
    let why = not_built("other");
    let items: Vec<Unknown> = ["A", "B", "C", "D"]
        .iter()
        .map(|n| unknown(1, "items", &format!("other::{n}"), &why))
        .collect();
    assert_eq!(
        check_warnings(&check_of(items)),
        [
            "warning: note:one names 4 names in rust-item, which cannot be told, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B, other::C, and 1 more"
        ]
    );
    // Three names are all shown, with no "more".
    let three: Vec<Unknown> = ["A", "B", "C"]
        .iter()
        .map(|n| unknown(1, "items", &format!("other::{n}"), &why))
        .collect();
    let line = &check_warnings(&check_of(three))[0];
    assert!(
        line.ends_with("names: other::A, other::B, other::C"),
        "{line}"
    );
}

#[test]
fn one_name_in_two_documents_is_one_name_and_two_documents() {
    let why = not_built("other");
    let items = vec![
        unknown(1, "items", "other::A", &why),
        unknown(2, "items", "other::A", &why),
    ];
    assert_eq!(
        check_warnings(&check_of(items)),
        [
            "warning: 1 name in rust-item, named by 2 documents (note:one, note:two), cannot be told, each for this reason: the docs of the crate `other` are not built; names: other::A"
        ]
    );
}

#[test]
fn no_answer_reads_in_the_plural_form_too() {
    let items = vec![
        unknown(1, "items", "other::A", &Untold::NoAnswer),
        unknown(1, "items", "other::B", &Untold::NoAnswer),
    ];
    assert_eq!(
        check_warnings(&check_of(items)),
        [
            "warning: note:one names 2 names in rust-item, which cannot be told, each for this reason: the adapter gave no answer for it; names: other::A, other::B"
        ]
    );
}

#[test]
fn one_document_writing_one_name_by_two_links_is_the_singular_line() {
    let why = not_built("other");
    let items = vec![
        unknown(1, "items", "other::A", &why),
        unknown(1, "also", "other::A", &why),
    ];
    assert_eq!(
        check_warnings(&check_of(items)),
        [
            "warning: note:one names other::A in rust-item, which cannot be told: the docs of the crate `other` are not built"
        ]
    );
}

#[test]
fn each_cause_is_a_line_in_the_order_it_first_appears() {
    let (other, third) = (not_built("other"), not_built("third"));
    let items = vec![
        unknown(1, "items", "other::A", &other),
        unknown(1, "items", "third::A", &third),
        unknown(1, "items", "other::B", &other),
    ];
    let lines = check_warnings(&check_of(items));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("warning: note:one names 2 names in rust-item"),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("warning: note:one names third::A"),
        "{lines:?}"
    );
}

#[test]
fn the_lines_of_causes_stand_between_the_gaps_and_the_names_with_nothing_behind_them() {
    let why = not_built("other");
    let mut check = check_of(vec![
        unknown(1, "items", "other::A", &why),
        unknown(1, "items", "other::B", &why),
    ]);
    if let BooksCheck::Checked { absent, .. } = &mut check {
        absent.push(Absent {
            doc: docid(1),
            key: key("one"),
            link: EntryName::new("items").unwrap(),
            artifact: artifact("rust-item", "demo::Missing"),
        });
    }
    let lines = check_warnings(&check);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("warning: note:one names 2 names in rust-item"),
        "{lines:?}"
    );
    assert!(
        lines[1].ends_with("which has nothing behind it"),
        "{lines:?}"
    );
}

#[test]
fn stale_writes_a_cause_once_with_its_own_words() {
    let why = not_built("other");
    let items = vec![
        unknown(1, "items", "other::A", &why),
        unknown(2, "items", "other::B", &why),
        unknown(2, "items", "other::C", &why),
        unknown(2, "items", "other::D", &why),
        unknown(1, "items", "demo::E", &Untold::NotAsked),
    ];
    assert_eq!(
        stale_warnings(&stale_of(items)),
        [
            "warning: 4 names in rust-item, pinned by the approvals of 2 documents (note:one, note:two), cannot be told now, so whether those approvals hold cannot be said, each for this reason: the docs of the crate `other` are not built; names: other::A, other::B, other::C, and 1 more",
            "warning: note:one pins demo::E in rust-item, which cannot be told now, so whether its approval holds cannot be said: the adapter was not asked for this comparison",
        ]
    );
    assert!(stale_warnings(&stale_of(Vec::new())).is_empty());
}
