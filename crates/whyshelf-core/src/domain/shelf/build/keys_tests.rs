//! Tests of who writes a key, and who writes it in a link: the one reading each of the
//! lookup, a link, the next key, the duplicate-key check and `rekey` ask.

use super::*;
use crate::domain::shelf::doc::KeyWriter;
use crate::domain::shelf::fixtures_tests::{
    docid, entries, fr, list, path, reading, schema, shelf, sp_testing, text,
};

fn finding() -> BookName {
    BookName::new("finding").unwrap()
}

fn key(written: &str) -> DocKey {
    DocKey {
        book: finding(),
        form: schema()
            .book("finding")
            .unwrap()
            .key()
            .parse(written)
            .unwrap(),
    }
}

/// A finding that writes `key` and has no `id:`.
fn no_id(file: &str, key: &str) -> DocReading {
    DocReading::Parsed {
        origin: path(file),
        entries: entries(&[("key", text(key)), ("title", text("t"))]),
        body: "body\n".to_owned(),
    }
}

/// A file whose front matter was never closed.
fn unclosed(file: &str) -> DocReading {
    DocReading::Unparsed {
        origin: path(file),
        id: ReadId::Unreadable,
        why: RawProblem::Unclosed,
    }
}

fn link_of(shelf: &Shelf, n: u128, link: &str) -> LinkState {
    shelf
        .doc(docid(n))
        .unwrap()
        .links()
        .get(link)
        .unwrap()
        .states()
        .first()
        .cloned()
        .expect("a link is written")
}

#[test]
fn a_file_with_no_identity_still_writes_its_key() {
    let s = shelf(vec![no_id("findings/a.md", "FR-5"), fr(1, &[])]);
    let writers = s.key_writers(&key("FR-5"));
    assert_eq!(
        *writers.writers(),
        Writers::One(KeyWriter::new(path("findings/a.md"), None))
    );
    // The key is taken, whichever way it is asked: the next number counts it, and a
    // key given for a new document is refused.
    assert_eq!(
        s.next_key(&finding()).unwrap().key().to_string(),
        "finding:FR-6"
    );
    assert!(matches!(
        s.claim_key(&finding(), "FR-5"),
        Err(ClaimKeyProblem::Taken { .. })
    ));
    assert_eq!(s.files_writing(&key("FR-5")), [path("findings/a.md")]);
}

#[test]
fn a_link_to_a_key_only_a_file_with_no_identity_writes_says_so() {
    let s = shelf(vec![no_id("findings/a.md", "FR-5"), fr(1, &["FR-5"])]);
    match link_of(&s, 1, "base") {
        LinkState::Unresolved(unresolved) => {
            assert_eq!(
                *unresolved.why(),
                UnresolvedWhy::NoIdentity(path("findings/a.md"))
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        s.find_key(&finding(), &key("FR-5").form),
        KeyHit::NoIdentity(NonEmptyVec::new(path("findings/a.md"), Vec::new()))
    );
}

#[test]
fn a_collision_names_every_file_and_the_identity_of_each_that_could_be_read() {
    let s = shelf(vec![
        reading("findings/a.md", 5, "FR-5", &[("title", text("a"))]),
        no_id("findings/b.md", "FR-5"),
        fr(7, &["FR-5"]),
    ]);
    let collisions = s.collisions();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].key(), &key("FR-5"));
    assert_eq!(
        collisions[0]
            .writers()
            .iter()
            .map(|w| (w.file().to_string(), w.id()))
            .collect::<Vec<_>>(),
        [
            ("findings/a.md".to_owned(), Some(docid(5))),
            ("findings/b.md".to_owned(), None)
        ]
    );
    // Each of the files is invalid, and its problem names the other file.
    for (file, other) in [
        ("findings/a.md", "findings/b.md"),
        ("findings/b.md", "findings/a.md"),
    ] {
        let problem = s
            .unreadable()
            .iter()
            .chain(s.anonymous())
            .chain(s.invalid().values().flat_map(NonEmptyVec::iter))
            .find(|invalid| invalid.origin().as_str() == file)
            .unwrap()
            .problems()
            .iter()
            .find(|p| matches!(p, DocProblem::DuplicateKey { .. }))
            .unwrap()
            .to_string();
        assert_eq!(
            problem,
            format!("the key `finding:FR-5` is also written by {other}")
        );
    }
    // A link to the key is ambiguous between the files, not resolved to the one with
    // an identity.
    match link_of(&s, 7, "base") {
        LinkState::Unresolved(unresolved) => match unresolved.why() {
            UnresolvedWhy::Ambiguous(collision) => assert_eq!(collision.files().count(), 2),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_that_could_not_be_read_may_write_the_keys_of_its_own_book_only() {
    let s = shelf(vec![fr(1, &[]), unclosed("findings/broken.md")]);
    match s.key_writers(&key("FR-1")) {
        KeyWritersRead::MaybeMore { writers, unread } => {
            assert!(matches!(writers, Writers::One(_)));
            assert_eq!(unread.into_vec(), [path("findings/broken.md")]);
        }
        other => panic!("{other:?}"),
    }
    // A key of another book is not asked of it.
    let spec_key = DocKey {
        book: BookName::new("spec").unwrap(),
        form: schema().book("spec").unwrap().key().parse("SP-1").unwrap(),
    };
    assert!(matches!(
        s.key_writers(&spec_key),
        KeyWritersRead::Complete(Writers::None)
    ));
}

#[test]
fn the_links_that_write_a_key_are_read_from_invalid_files_too() {
    let mut invalid = sp_testing(12, &["FR-1"], &[]);
    if let DocReading::Parsed { entries, .. } = &mut invalid {
        entries.insert("surprise".to_owned(), text("x"));
    }
    let s = shelf(vec![fr(1, &[]), sp_testing(11, &["FR-1"], &[]), invalid]);
    assert!(
        s.doc(docid(12)).is_none(),
        "the spec with a surprise is invalid"
    );
    let writers = s.link_writers(&key("FR-1"));
    assert_eq!(
        writers
            .found()
            .iter()
            .map(|w| (
                w.file().to_string(),
                w.id(),
                w.link().to_string(),
                w.written().to_owned()
            ))
            .collect::<Vec<_>>(),
        [
            (
                "specs/sp-11.md".to_owned(),
                Some(docid(11)),
                "grounds".to_owned(),
                "FR-1".to_owned()
            ),
            (
                "specs/sp-12.md".to_owned(),
                Some(docid(12)),
                "grounds".to_owned(),
                "FR-1".to_owned()
            ),
        ]
    );
    assert!(writers.unread().is_empty());
}

#[test]
fn a_place_a_link_may_write_a_key_in_and_could_not_be_read_is_named() {
    // A spec whose `grounds` is a text and no list, and a spec that could not be read
    // at all: either may write the key of a finding in a link.
    let mut text_link = sp_testing(11, &[], &[]);
    if let DocReading::Parsed { entries, .. } = &mut text_link {
        entries.insert("grounds".to_owned(), text("FR-1"));
    }
    let s = shelf(vec![fr(1, &[]), text_link, unclosed("specs/broken.md")]);
    let writers = s.link_writers(&key("FR-1"));
    assert!(writers.found().is_empty());
    assert_eq!(
        writers.unread(),
        [
            Unread::File(path("specs/broken.md")),
            Unread::Link {
                file: path("specs/sp-11.md"),
                link: EntryName::new("grounds").unwrap(),
            },
        ]
    );
    // The designs link into the spec book and not into the findings: a file of the
    // designs that could not be read is no place for a key of the findings.
    let s = shelf(vec![fr(1, &[]), unclosed("designs/broken.md")]);
    assert!(s.link_writers(&key("FR-1")).unread().is_empty());
}

#[test]
fn what_an_identity_is_called_now_keeps_the_four_states() {
    let s = shelf(vec![
        fr(1, &[]),
        // Invalid, and writes one key.
        reading(
            "findings/a.md",
            2,
            "FR-2",
            &[("title", text("a")), ("surprise", text("x"))],
        ),
        // The same identity in two files that write two keys.
        reading("findings/b.md", 3, "FR-3", &[("title", text("b"))]),
        reading("findings/c.md", 3, "FR-4", &[("title", text("c"))]),
    ]);
    assert_eq!(s.now_named(docid(1)), NowNamed::Valid(key("FR-1")));
    assert_eq!(s.now_named(docid(2)), NowNamed::Invalid(Some(key("FR-2"))));
    assert_eq!(s.now_named(docid(3)), NowNamed::Invalid(None));
    assert_eq!(s.now_named(docid(9)), NowNamed::Gone);
    let s = shelf(vec![fr(1, &[]), unclosed("findings/broken.md")]);
    assert_eq!(s.now_named(docid(9)), NowNamed::Unknown);
}

#[test]
fn a_list_in_a_link_is_read_for_its_keys_even_when_the_file_is_invalid() {
    // A link with a blank item is a problem of the file; the key written beside it is
    // still written.
    let mut file = sp_testing(11, &[], &[]);
    if let DocReading::Parsed { entries, .. } = &mut file {
        entries.insert("grounds".to_owned(), list(&["FR-1", " "]));
    }
    let s = shelf(vec![fr(1, &[]), file]);
    assert!(
        s.doc(docid(11)).is_none(),
        "a blank item makes the file invalid"
    );
    assert_eq!(s.link_writers(&key("FR-1")).found().len(), 1);
}
