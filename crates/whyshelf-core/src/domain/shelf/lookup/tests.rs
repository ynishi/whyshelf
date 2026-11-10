//! Tests of the lookup.

use crate::domain::id::ShortId;
use crate::domain::shelf::fixtures_tests::{approved, docid, eid, fr, ledger, shelf};

use super::*;

#[test]
fn a_key_is_found_with_or_without_its_book() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![]);
    assert_eq!(
        resolve(&shelf, &ledger, "FR-1"),
        Resolution::Found(docid(1))
    );
    assert_eq!(
        resolve(&shelf, &ledger, "finding:FR-2"),
        Resolution::Found(docid(2))
    );
    assert_eq!(resolve(&shelf, &ledger, "FR-9"), Resolution::Missing);
    assert_eq!(resolve(&shelf, &ledger, "nobook:FR-1"), Resolution::Missing);
}

#[test]
fn an_identity_and_a_short_id_name_a_document() {
    let lettered = shelf(vec![fr(0xabc, &[])]);
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = ledger(vec![]);
    let full = eid(1).to_string();
    assert_eq!(resolve(&shelf, &ledger, &full), Resolution::Found(docid(1)));
    let short = ShortId::of(eid(2)).to_string();
    assert_eq!(
        resolve(&shelf, &ledger, &short),
        Resolution::Found(docid(2))
    );
    // An identity with letters in it, written in capitals, is no identity.
    let written = eid(0xabc).to_string();
    assert_eq!(
        resolve(&lettered, &ledger, &written),
        Resolution::Found(docid(0xabc))
    );
    assert_eq!(
        resolve(&lettered, &ledger, &written.to_uppercase()),
        Resolution::Missing
    );
}

#[test]
fn a_short_id_that_two_identities_end_with_is_listed_not_chosen() {
    let other = 0x1_0000_0001;
    let shelf = shelf(vec![fr(1, &[]), fr(other, &[])]);
    match resolve(&shelf, &ledger(vec![]), "00000001") {
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
        resolve(&now, &ledger, &eid(1).to_string()),
        Resolution::LedgerOnly { id, .. } if id == docid(1)
    ));
    // A key has no file to be read from, so it names nothing.
    assert_eq!(resolve(&now, &ledger, "FR-1"), Resolution::Missing);
}
