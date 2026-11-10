//! Tests of the paths of links to artifacts that name no file
//! ([`Doc::absent_artifacts`] and [`Notice::ArtifactAbsent`]).

use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    approved, deny, deprecate, docid, fr, ledger, retire, shelf_with, sp_testing,
};
use crate::domain::shelf::ledger::JudgedAt;

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn absent_notices(assessment: &Assessment, n: u128) -> Vec<(String, String)> {
    assessment
        .notices_of(docid(n))
        .iter()
        .filter_map(|notice| match notice {
            Notice::ArtifactAbsent { link, artifact } => {
                Some((link.to_string(), artifact.name().to_string()))
            }
            _ => None,
        })
        .collect()
}

fn the_shelf() -> Shelf {
    shelf_with(
        vec![
            fr(1, &[]),
            sp_testing(
                2,
                &["FR-1"],
                &["tests/a.rs", "tests/none.rs", "tests/gone.rs"],
            ),
            sp_testing(3, &["FR-1"], &["tests/a.rs"]),
        ],
        &[("tests/a.rs", Some(h("a"))), ("tests/none.rs", None)],
    )
}

#[test]
fn a_document_lists_the_paths_that_name_no_file_as_written() {
    let s = the_shelf();
    let absent: Vec<_> = s
        .doc(docid(2))
        .unwrap()
        .absent_artifacts()
        .into_iter()
        .map(|(link, artifact)| (link.to_string(), artifact.name().to_string()))
        .collect();
    // Two paths the adapter found no file at, in the order they are written.
    assert_eq!(
        absent,
        vec![
            ("tests".to_owned(), "tests/none.rs".to_owned()),
            ("tests".to_owned(), "tests/gone.rs".to_owned()),
        ]
    );
    assert!(s.doc(docid(3)).unwrap().absent_artifacts().is_empty());
}

#[test]
fn a_pending_or_approved_document_has_a_notice_for_each_and_a_denied_one_none() {
    let s = the_shelf();
    let pending = assess(&s, &ledger(Vec::new()), JudgedAt::new(100));
    assert_eq!(absent_notices(&pending, 2).len(), 2);
    assert!(absent_notices(&pending, 3).is_empty());

    let approved_all = approved(&s, &[1, 2, 3]);
    let approved_now = assess(&s, &approved_all, JudgedAt::new(100));
    assert_eq!(absent_notices(&approved_now, 2).len(), 2);
    // The notice changes no freshness: the approval holds.
    assert_eq!(
        approved_now.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );

    // A denied, deprecated or retired document is not expected to keep its evidence.
    for decided in [
        deny(&s, 2, 2001, 20),
        deprecate(&s, 2, 2001, 20),
        retire(&s, 2, 2001, 20),
    ] {
        let l = ledger(vec![decided]);
        assert!(absent_notices(&assess(&s, &l, JudgedAt::new(100)), 2).is_empty());
    }
}
