//! Tests of the one refusal of a group of pins and of which pins stale reads now, over
//! pins told apart by hand: each step of the order, for a hash named and for none, and
//! the pins `check` reads, in their order.

use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{ArtifactHash, Untold};
use crate::domain::shelf::fixtures_tests::{
    approved, docid, file_artifact, fr, jid, read_pins, shelf, shelf_with, sp_testing, standing,
};

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

/// The pin of `tests/a.rs` of an approval of SP-2 made while the file held `held`, or
/// nothing.
fn pin(held: Option<&str>) -> ReadPin {
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", held.map(h))],
    );
    read_pins(&approved(&s, &[1, 2]), docid(2)).remove(0)
}

/// What [`refusal`] says of `pins`, for the hash of `named` or none.
fn refused(pins: &[Compared<'_>], named: Option<&str>) -> SameMeaningRefusal {
    let pins: Vec<&Compared<'_>> = pins.iter().collect();
    refusal(&pins, named.map(|text| PinnedHash::Artifact(h(text))))
}

fn not_compared(named: Option<&str>, why: NotComparedWhy) -> SameMeaningRefusal {
    SameMeaningRefusal::NotComparedNow {
        hash: named.map(|text| h(text).to_string()),
        why,
    }
}

/// The edits of `tests/a.rs` from "a": the same, kept in its meaning, and edited.
struct Edits {
    same: Edit<ArtifactHash, NonEmptyVec<JudgementId>>,
    kept: Edit<ArtifactHash, NonEmptyVec<JudgementId>>,
}

fn edits() -> Edits {
    Edits {
        same: Edit::Same { then: h("a") },
        kept: Edit::KeptMeaning {
            then: h("a"),
            now: h("b"),
            declared: NonEmptyVec::new(jid(2001), Vec::new()),
        },
    }
}

#[test]
fn unnamed_the_refusal_follows_one_order_over_the_pins_that_pinned_a_hash() {
    let (read, absent) = (pin(Some("a")), pin(None));
    let edits = edits();
    let untold = Untold::NoAnswer;
    let rest = |read, rest| Compared::Rest { read, rest };
    let content = |edit| Compared::Content(Content::Artifact { read: &read, edit });
    let unknown = SameMeaningRefusal::ArtifactUnknown {
        artifact: file_artifact("tests/a.rs"),
        why: Untold::NoAnswer,
    };
    let gone = SameMeaningRefusal::ArtifactAbsent {
        artifact: file_artifact("tests/a.rs"),
    };
    let not_written = |read| Compared::NotCompared(NotCompared::NotWritten(read));
    let of_invalid = || {
        Compared::NotCompared(NotCompared::SubjectNotValid {
            pin: PinRef::Artifact(Box::new(pin(Some("a")))),
        })
    };

    for (pins, expected) in [
        (vec![rest(&read, Rest::Untold(&untold))], unknown.clone()),
        (vec![rest(&read, Rest::Gone { then: h("a") })], gone),
        // Not told comes before gone, whatever the order of the pins.
        (
            vec![
                rest(&read, Rest::Gone { then: h("a") }),
                rest(&read, Rest::Untold(&untold)),
            ],
            unknown,
        ),
        // A pin that holds comes before all.
        (
            vec![content(&edits.same), rest(&read, Rest::Untold(&untold))],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (
            vec![
                content(&edits.kept),
                rest(&read, Rest::Gone { then: h("a") }),
            ],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (
            vec![content(&edits.same), not_written(&read)],
            SameMeaningRefusal::NothingToDeclare,
        ),
        // A pin stale does not compare now, with why.
        (
            vec![rest(&read, Rest::Remade)],
            not_compared(None, NotComparedWhy::ComparisonChanged),
        ),
        (
            vec![not_written(&read)],
            not_compared(None, NotComparedWhy::LinkDropped),
        ),
        (
            vec![of_invalid()],
            not_compared(None, NotComparedWhy::DocumentNotValid),
        ),
        // The first such pin's reason, in the order of the pins.
        (
            vec![not_written(&read), rest(&read, Rest::Remade)],
            not_compared(None, NotComparedWhy::LinkDropped),
        ),
        (
            vec![rest(&read, Rest::Remade), not_written(&read)],
            not_compared(None, NotComparedWhy::ComparisonChanged),
        ),
        // A pin that pinned no hash is not counted.
        (
            vec![not_written(&absent)],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (
            vec![rest(&absent, Rest::StillAbsent)],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (
            vec![rest(&absent, Rest::Arrived)],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (
            vec![rest(&absent, Rest::Untold(&untold))],
            SameMeaningRefusal::NothingToDeclare,
        ),
        (Vec::new(), SameMeaningRefusal::NothingToDeclare),
    ] {
        assert_eq!(refused(&pins, None), expected);
    }
}

#[test]
fn named_the_refusal_reads_the_pins_of_that_hash_and_ends_not_pinned() {
    let read = pin(Some("a"));
    let edits = edits();
    let untold = Untold::NoAnswer;
    let rest = |rest| Compared::Rest { read: &read, rest };
    let content = |edit| Compared::Content(Content::Artifact { read: &read, edit });
    let not_pinned = |text: &str| SameMeaningRefusal::NotPinned {
        hash: h(text).to_string(),
    };

    for (pins, named, expected) in [
        // The hash now, or one that already means the same: not pinned, as before.
        (vec![content(&edits.same)], "a", not_pinned("a")),
        (vec![content(&edits.kept)], "a", not_pinned("a")),
        (
            vec![rest(Rest::Untold(&untold))],
            "a",
            SameMeaningRefusal::ArtifactUnknown {
                artifact: file_artifact("tests/a.rs"),
                why: Untold::NoAnswer,
            },
        ),
        (
            vec![rest(Rest::Gone { then: h("a") })],
            "a",
            SameMeaningRefusal::ArtifactAbsent {
                artifact: file_artifact("tests/a.rs"),
            },
        ),
        (
            vec![rest(Rest::Remade)],
            "a",
            not_compared(Some("a"), NotComparedWhy::ComparisonChanged),
        ),
        (
            vec![Compared::NotCompared(NotCompared::NotWritten(&read))],
            "a",
            not_compared(Some("a"), NotComparedWhy::LinkDropped),
        ),
        (
            vec![Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Artifact(Box::new(pin(Some("a")))),
            })],
            "a",
            not_compared(Some("a"), NotComparedWhy::DocumentNotValid),
        ),
        // A pin of another hash is not read: no pin holds the one named.
        (vec![rest(Rest::Untold(&untold))], "z", not_pinned("z")),
        (vec![rest(Rest::Remade)], "z", not_pinned("z")),
    ] {
        assert_eq!(refused(&pins, Some(named)), expected, "{named}");
    }
}

#[test]
fn the_refusal_of_a_document_reads_its_pins_as_an_artifact_s_are_read() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = approved(&s, &[1, 2]);
    let view = standing(&l, docid(2)).view();
    let [upstream] = view.frame().iter().collect::<Vec<_>>()[..] else {
        panic!("FR-2 pins FR-1");
    };
    let hash = *upstream.hash();
    let same = Edit::Same { then: hash };
    let named = Some(PinnedHash::Doc(hash));
    let own = Compared::Content(Content::Doc {
        id: docid(1),
        role: DocPinRole::Own,
        edit: &same,
    });
    let of_invalid = Compared::NotCompared(NotCompared::SubjectNotValid {
        pin: PinRef::Upstream(upstream),
    });
    assert_eq!(
        refusal(&[&own], named),
        SameMeaningRefusal::NotPinned {
            hash: hash.to_string()
        }
    );
    assert_eq!(
        refusal(&[&of_invalid], named),
        SameMeaningRefusal::NotComparedNow {
            hash: Some(hash.to_string()),
            why: NotComparedWhy::DocumentNotValid,
        }
    );
    assert_eq!(
        refusal(&[&of_invalid], None),
        SameMeaningRefusal::NotComparedNow {
            hash: None,
            why: NotComparedWhy::DocumentNotValid,
        }
    );
}

#[test]
fn check_reads_the_pins_stale_reads_now_with_the_hash_each_pinned_in_their_order() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = approved(&s, &[1, 2]);
    let view = standing(&l, docid(2)).view();
    let [upstream] = view.frame().iter().collect::<Vec<_>>()[..] else {
        panic!("FR-2 pins FR-1");
    };
    let (read, absent) = (pin(Some("a")), pin(None));
    let edited = Edit::Edited {
        then: h("a"),
        now: h("b"),
    };
    let own = Edit::Edited {
        then: *view.seen(),
        now: *upstream.hash(),
    };
    let untold = Untold::NoAnswer;
    let rest = |read, rest| Compared::Rest { read, rest };
    let now = ComparedNow {
        shelf: &s,
        pins: vec![
            // Read now: a content, an upstream that is not valid, a pin gone, and one
            // not told now that pinned a hash.
            Compared::Content(Content::Doc {
                id: docid(2),
                role: DocPinRole::Own,
                edit: &own,
            }),
            Compared::NotCompared(NotCompared::UpstreamNotValid(upstream)),
            Compared::Content(Content::Artifact {
                read: &read,
                edit: &edited,
            }),
            rest(&read, Rest::Gone { then: h("a") }),
            rest(&read, Rest::Untold(&untold)),
            // Not read now: not compared, or no hash pinned.
            rest(&read, Rest::Remade),
            Compared::NotCompared(NotCompared::NotWritten(&read)),
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Own(docid(3), *view.seen()),
            }),
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Upstream(upstream),
            }),
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Artifact(Box::new(pin(Some("a")))),
            }),
            rest(&absent, Rest::Arrived),
            rest(&absent, Rest::StillAbsent),
            rest(&absent, Rest::Untold(&untold)),
        ],
    };
    let read_now: Vec<PinnedHash> = now.thens().iter().map(Then::hash).collect();
    assert_eq!(
        read_now,
        [
            PinnedHash::Doc(*view.seen()),
            PinnedHash::Doc(*upstream.hash()),
            PinnedHash::Artifact(h("a")),
            PinnedHash::Artifact(h("a")),
            PinnedHash::Artifact(h("a")),
        ]
    );
    let documents: Vec<DocId> = now
        .thens()
        .iter()
        .filter_map(|then| match then {
            Then::Doc(id, ..) => Some(*id),
            Then::Artifact(..) => None,
        })
        .collect();
    assert_eq!(documents, [docid(2), docid(1)]);
}
