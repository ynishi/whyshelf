//! Tests of the lookup.

use crate::domain::id::ShortId;
use crate::domain::shelf::fixtures_tests::{
    approved, docid, eid, fr, ledger, path, shelf, sided, unreadable_at,
};

use super::*;

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

#[test]
fn a_key_is_found_with_or_without_its_book() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![]);
    assert_eq!(
        resolve(&shelf, &ledger, &r("FR-1")),
        Resolution::Found(docid(1))
    );
    assert_eq!(
        resolve(&shelf, &ledger, &r("finding:FR-2")),
        Resolution::Found(docid(2))
    );
    assert_eq!(resolve(&shelf, &ledger, &r("FR-9")), Resolution::Missing);
    assert_eq!(
        resolve(&shelf, &ledger, &r("nobook:FR-1")),
        Resolution::UnknownBook(BookName::new("nobook").unwrap())
    );
}

#[test]
fn an_identity_and_a_short_id_name_a_document() {
    let lettered = shelf(vec![fr(0xabc, &[])]);
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![]);
    let full = eid(1).to_string();
    assert_eq!(
        resolve(&shelf, &ledger, &r(&full)),
        Resolution::Found(docid(1))
    );
    let short = ShortId::of(eid(2)).to_string();
    assert_eq!(
        resolve(&shelf, &ledger, &r(&short)),
        Resolution::Found(docid(2))
    );
    // An identity with letters in it, written in capitals, is no identity,
    // and no book's grammar takes it for a key.
    let written = eid(0xabc).to_string();
    assert_eq!(
        resolve(&lettered, &ledger, &r(&written)),
        Resolution::Found(docid(0xabc))
    );
    assert_eq!(
        resolve(&lettered, &ledger, &r(&written.to_uppercase())),
        Resolution::NotAKeyOfAnyBook
    );
}

#[test]
fn a_short_id_that_two_identities_end_with_is_listed_not_chosen() {
    let other = 0x1_0000_0001;
    let shelf = shelf(vec![fr(1, &[]), fr(other, &[])]);
    match resolve(&shelf, &ledger(vec![]), &r("00000001")) {
        Resolution::Ambiguous(candidates) => assert_eq!(candidates.len(), 2),
        found => panic!("expected an ambiguous reference, got {found:?}"),
    }
}

#[test]
fn a_document_whose_file_is_gone_is_still_named_by_the_ledger() {
    let at_approval = shelf(vec![fr(1, &[])]);
    let ledger = approved(&at_approval, &[1]);
    let now = shelf(vec![]);
    assert!(matches!(
        resolve(&now, &ledger, &r(&eid(1).to_string())),
        Resolution::LedgerOnly { id, .. } if id == docid(1)
    ));
    // A key has no file to be read from, so it names nothing.
    assert_eq!(resolve(&now, &ledger, &r("FR-1")), Resolution::Missing);
}

#[test]
fn a_key_only_a_file_with_no_identity_writes_names_the_file_not_a_document() {
    use crate::domain::shelf::build::DocReading;
    use crate::domain::shelf::fixtures_tests::{entries, path, text};

    let no_id = DocReading::Parsed {
        origin: path("findings/a.md"),
        entries: entries(&[("key", text("FR-5")), ("title", text("t"))]),
        body: "body\n".to_owned(),
    };
    let shelf = shelf(vec![fr(1, &[]), no_id]);
    let ledger = ledger(vec![]);
    let files = NonEmptyVec::new(path("findings/a.md"), Vec::new());
    assert_eq!(
        resolve(&shelf, &ledger, &r("finding:FR-5")),
        Resolution::NoIdentity(files.clone())
    );
    // A key alone is told so as well, when no document of any book is found.
    assert_eq!(
        resolve(&shelf, &ledger, &r("FR-5")),
        Resolution::NoIdentity(files)
    );
    // A document that is found is not hidden by it.
    assert_eq!(
        resolve(&shelf, &ledger, &r("FR-1")),
        Resolution::Found(docid(1))
    );
}

#[test]
fn a_reference_is_read_into_one_of_four_shapes() {
    assert!(matches!(
        r("finding:FR-2"),
        Reference::InBook { book, key }
            if book.as_str() == "finding" && key.as_str() == "FR-2"
    ));
    assert_eq!(r(&eid(1).to_string()), Reference::Id(docid(1)));
    assert_eq!(
        r(&ShortId::of(eid(2)).to_string()),
        Reference::Short(ShortId::of(eid(2)))
    );
    assert!(matches!(r("FR-1"), Reference::Key(key) if key.as_str() == "FR-1"));
}

#[test]
fn an_empty_text_is_no_reference() {
    assert_eq!("".parse::<Reference>(), Err(ReferenceProblem::Empty));
}

#[test]
fn a_book_that_is_no_name_is_refused_at_the_reading() {
    // Once a `Missing` of the lookup; now the text is no reference at all.
    assert!(matches!(
        "No Book:FR-1".parse::<Reference>(),
        Err(ReferenceProblem::BadBook { book, .. }) if book == "No Book"
    ));
    assert!(matches!(
        ":FR-1".parse::<Reference>(),
        Err(ReferenceProblem::BadBook { .. })
    ));
}

#[test]
fn a_key_with_a_space_a_colon_or_nothing_is_refused() {
    assert!(matches!(
        "FR 1".parse::<Reference>(),
        Err(ReferenceProblem::BadKey { key }) if key == "FR 1"
    ));
    assert!(matches!(
        "finding:FR 1".parse::<Reference>(),
        Err(ReferenceProblem::BadKey { .. })
    ));
    assert!(matches!(
        "finding:".parse::<Reference>(),
        Err(ReferenceProblem::BadKey { .. })
    ));
    assert!(matches!(
        "finding:a:b".parse::<Reference>(),
        Err(ReferenceProblem::BadKey { .. })
    ));
}

#[test]
fn a_reference_written_out_reads_back_the_same() {
    for text in [
        "FR-1".to_owned(),
        "finding:FR-2".to_owned(),
        eid(1).to_string(),
        ShortId::of(eid(2)).to_string(),
    ] {
        let reference = r(&text);
        assert_eq!(reference.to_string(), text);
        assert_eq!(r(&reference.to_string()), reference);
    }
}

#[test]
fn a_book_the_schema_does_not_declare_is_an_unknown_book_not_a_missing_key() {
    let shelf = shelf(vec![fr(1, &[])]);
    assert_eq!(
        resolve(&shelf, &ledger(vec![]), &r("nobook:FR-1")),
        Resolution::UnknownBook(BookName::new("nobook").unwrap())
    );
}

#[test]
fn a_key_the_books_grammar_refuses_is_not_a_key_of_the_book() {
    let shelf = shelf(vec![fr(1, &[])]);
    for text in ["finding:FR-0", "finding:FR-007", "finding:SP-1"] {
        match resolve(&shelf, &ledger(vec![]), &r(text)) {
            Resolution::NotAKeyOf { book, .. } => assert_eq!(book.as_str(), "finding", "{text}"),
            other => panic!("{text}: expected NotAKeyOf, got {other:?}"),
        }
    }
}

#[test]
fn a_bare_key_no_book_accepts_is_not_a_key_of_any_book() {
    let shelf = shelf(vec![fr(1, &[])]);
    for text in ["FR-0", "FR-007", "ZZ-1"] {
        assert_eq!(
            resolve(&shelf, &ledger(vec![]), &r(text)),
            Resolution::NotAKeyOfAnyBook,
            "{text}"
        );
    }
}

#[test]
fn a_well_formed_key_nothing_carries_is_missing() {
    let shelf = shelf(vec![fr(1, &[])]);
    assert_eq!(
        resolve(&shelf, &ledger(vec![]), &r("finding:FR-9")),
        Resolution::Missing
    );
}

// X holds both sides of a merge conflict whose sides show A and B; C is in no file.
const A: u128 = 10;
const B: u128 = 11;
const C: u128 = 12;

#[test]
fn a_document_a_side_shows_with_no_record_is_named_by_its_id_and_its_short_id() {
    let now = shelf(vec![sided("findings/x.md", &[A, B])]);
    let nothing = ledger(vec![]);
    for n in [A, B] {
        assert_eq!(
            resolve(&now, &nothing, &r(&eid(n).to_string())),
            Resolution::Shown(docid(n)),
            "{n}"
        );
        assert_eq!(
            resolve(&now, &nothing, &r(&ShortId::of(eid(n)).to_string())),
            Resolution::Shown(docid(n)),
            "{n}"
        );
    }
    // No side shows C.
    assert_eq!(
        resolve(&now, &nothing, &r(&eid(C).to_string())),
        Resolution::Missing
    );
}

#[test]
fn a_document_a_side_shows_with_a_record_is_named_by_the_ledger_with_the_file_that_shows_it() {
    let ledger = approved(&shelf(vec![fr(A, &[]), fr(C, &[])]), &[A, C]);
    let now = shelf(vec![sided("findings/x.md", &[A, B])]);
    let Resolution::LedgerOnly {
        id,
        state: Absence::Unknown(may),
    } = resolve(&now, &ledger, &r(&eid(A).to_string()))
    else {
        panic!("ledger only, unknown");
    };
    assert_eq!(id, docid(A));
    assert_eq!(may.shown(), [path("findings/x.md")]);
    assert!(may.any().is_empty());
    // C, which no side shows, is gone, not unknown.
    assert_eq!(
        resolve(&now, &ledger, &r(&eid(C).to_string())),
        Resolution::LedgerOnly {
            id: docid(C),
            state: Absence::Gone,
        }
    );
    // Beside a file that may hold any, C is unknown in that file only.
    let with_u = shelf(vec![
        sided("findings/x.md", &[A, B]),
        unreadable_at("findings/u.md"),
    ]);
    let Resolution::LedgerOnly {
        state: Absence::Unknown(may),
        ..
    } = resolve(&with_u, &ledger, &r(&eid(C).to_string()))
    else {
        panic!("ledger only, unknown");
    };
    assert!(may.shown().is_empty());
    assert_eq!(may.any(), [path("findings/u.md")]);
}

#[test]
fn a_document_only_a_file_that_may_hold_any_may_hold_and_no_record_names_is_missing() {
    let now = shelf(vec![unreadable_at("findings/u.md")]);
    let nothing = ledger(vec![]);
    assert_eq!(
        resolve(&now, &nothing, &r(&eid(A).to_string())),
        Resolution::Missing
    );
    assert_eq!(
        resolve(&now, &nothing, &r(&ShortId::of(eid(A)).to_string())),
        Resolution::Missing
    );
}
