//! The table of what a pinned content comes to now ([`super::Edit`], [`super::DocDrift`],
//! [`super::ArtifactChange`], [`super::ArtifactDrift`]): every pair of what was pinned and
//! what is there now, for both kinds of content, and that a declaration is asked of two
//! hashes that differ only.

use std::convert::Infallible;

use super::super::artifact::{ArtifactHash, ArtifactSeen, Comparison, Untold};
use super::super::build::FailureTag;
use super::super::doc::DocHash;
use super::super::fixtures_tests::{docid, fr_titled, lines, shelf};
use super::{ArtifactChange, ArtifactDrift, Edit, doc_drift, edit_of};

fn doc_hash(title: &str) -> DocHash {
    *shelf(vec![fr_titled(1, title, &[])])
        .doc(docid(1))
        .unwrap()
        .hash()
}

fn hash(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

/// A join of two hashes: the declarations that join them, if any.
type Join<H> = fn(&H, &H) -> Option<&'static str>;

/// A row of what a pinned artifact comes to: what was pinned, what is there now, the
/// join, and what it comes to.
type ArtifactRow<'u> = (
    ArtifactSeen,
    Result<ArtifactSeen, &'u Untold>,
    Join<ArtifactHash>,
    ArtifactChange<&'static str>,
);

/// A join that must not be asked: two hashes that are the same need no declaration.
fn never<H>(_: &H, _: &H) -> Option<&'static str> {
    panic!("a declaration was asked of two hashes that are the same")
}

fn declared<H>(_: &H, _: &H) -> Option<&'static str> {
    Some("declared")
}

fn none<H>(_: &H, _: &H) -> Option<&'static str> {
    None
}

#[test]
fn a_document_is_the_same_edited_kept_or_not_valid() {
    let (a, b) = (doc_hash("a"), doc_hash("b"));
    let rows: [(Result<DocHash, FailureTag>, Join<DocHash>, _); 4] = [
        (Ok(a), never, Ok(Edit::Same { then: a })),
        (Ok(b), none, Ok(Edit::Edited { then: a, now: b })),
        (
            Ok(b),
            declared,
            Ok(Edit::KeptMeaning {
                then: a,
                now: b,
                declared: "declared",
            }),
        ),
        (Err(FailureTag::Gone), never, Err(FailureTag::Gone)),
    ];
    for (now, join, expected) in rows {
        assert_eq!(doc_drift(a, now, join), expected, "{now:?}");
    }
}

#[test]
fn an_edit_tells_the_hash_then_and_now() {
    let (a, b) = (doc_hash("a"), doc_hash("b"));
    let same: Edit<DocHash, Infallible> = edit_of(a, a, |_, _| None);
    assert_eq!((same.then(), same.now()), (a, a));
    let edited: Edit<DocHash, Infallible> = edit_of(a, b, |_, _| None);
    assert_eq!((edited.then(), edited.now()), (a, b));
    let kept = edit_of(a, b, declared);
    assert_eq!((kept.then(), kept.now()), (a, b));
}

#[test]
fn an_artifact_is_there_gone_arrived_still_absent_or_not_told() {
    let (a, b) = (hash("a"), hash("b"));
    let untold = Untold::NoAnswer;
    let present = ArtifactSeen::Present(a);
    let rows: [ArtifactRow<'_>; 10] = [
        (
            present,
            Ok(present),
            never,
            ArtifactChange::Content(Edit::Same { then: a }),
        ),
        (
            present,
            Ok(ArtifactSeen::Present(b)),
            none,
            ArtifactChange::Content(Edit::Edited { then: a, now: b }),
        ),
        (
            present,
            Ok(ArtifactSeen::Present(b)),
            declared,
            ArtifactChange::Content(Edit::KeptMeaning {
                then: a,
                now: b,
                declared: "declared",
            }),
        ),
        (
            present,
            Ok(ArtifactSeen::Absent),
            never,
            ArtifactChange::Gone { then: a },
        ),
        (
            present,
            Err(&untold),
            never,
            ArtifactChange::Untold(Untold::NoAnswer),
        ),
        (
            ArtifactSeen::Absent,
            Ok(ArtifactSeen::Present(b)),
            never,
            ArtifactChange::Arrived { now: b },
        ),
        (
            ArtifactSeen::Absent,
            Ok(ArtifactSeen::Present(a)),
            never,
            ArtifactChange::Arrived { now: a },
        ),
        (
            ArtifactSeen::Absent,
            Ok(ArtifactSeen::Present(b)),
            declared,
            ArtifactChange::Arrived { now: b },
        ),
        (
            ArtifactSeen::Absent,
            Ok(ArtifactSeen::Absent),
            never,
            ArtifactChange::StillAbsent,
        ),
        (
            ArtifactSeen::Absent,
            Err(&untold),
            never,
            ArtifactChange::Untold(Untold::NoAnswer),
        ),
    ];
    for (seen, now, join, expected) in rows {
        assert_eq!(
            ArtifactChange::of(seen, now, join),
            expected,
            "{seen:?} {now:?}"
        );
    }
}

#[test]
fn an_artifact_there_then_and_now_under_another_comparison_is_remade_and_nothing_else_is() {
    let (a, b) = (hash("a"), hash("b"));
    let untold = Untold::NoAnswer;
    let pinned = Comparison::Bytes;
    let other = lines(&[]);
    let present = ArtifactSeen::Present(a);
    // The comparison changed: only a thing there then and now is remade, its hashes not
    // compared; one that cannot be told now is not told, whatever the comparison, and
    // one gone or come has no two hashes.
    let changed: [(
        ArtifactSeen,
        Result<ArtifactSeen, &Untold>,
        ArtifactDrift<&'static str>,
    ); 5] = [
        (
            present,
            Ok(ArtifactSeen::Present(b)),
            ArtifactDrift::Remade { now: other.clone() },
        ),
        (
            present,
            Ok(present),
            ArtifactDrift::Remade { now: other.clone() },
        ),
        (
            present,
            Err(&untold),
            ArtifactDrift::Change(ArtifactChange::Untold(Untold::NoAnswer)),
        ),
        (
            present,
            Ok(ArtifactSeen::Absent),
            ArtifactDrift::Change(ArtifactChange::Gone { then: a }),
        ),
        (
            ArtifactSeen::Absent,
            Ok(ArtifactSeen::Present(b)),
            ArtifactDrift::Change(ArtifactChange::Arrived { now: b }),
        ),
    ];
    for (seen, now, expected) in changed {
        assert_eq!(
            ArtifactDrift::of(seen, &pinned, now, &other, never),
            expected,
            "{seen:?} {now:?}"
        );
    }
    // The same comparison: what the artifact came to.
    assert_eq!(
        ArtifactDrift::of(
            present,
            &pinned,
            Ok(ArtifactSeen::Present(b)),
            &pinned,
            none
        ),
        ArtifactDrift::Change(ArtifactChange::Content(Edit::Edited { then: a, now: b }))
    );
    assert_eq!(
        ArtifactDrift::of(present, &pinned, Ok(present), &pinned, never),
        ArtifactDrift::Change(ArtifactChange::Content(Edit::Same { then: a }))
    );
}

#[test]
fn a_reader_that_asks_no_declaration_never_holds_a_kept_meaning() {
    // With no declaration asked (`Infallible`), an edit is an edit: `KeptMeaning` of
    // `Infallible` holds a value no one can make.
    let (a, b) = (hash("a"), hash("b"));
    let change: ArtifactChange<Infallible> = ArtifactChange::of(
        ArtifactSeen::Present(a),
        Ok(ArtifactSeen::Present(b)),
        |_, _| None,
    );
    assert_eq!(
        change,
        ArtifactChange::Content(Edit::Edited { then: a, now: b })
    );
}
