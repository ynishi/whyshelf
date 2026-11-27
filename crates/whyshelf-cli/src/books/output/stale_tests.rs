//! Tests of what `stale` tells of the approvals the frame refuses: `withheld` in the
//! answer, and a line on stderr for each.

use whyshelf_core::app::shelf::{Gate, Health, Names, SettledOrigin, StaleReport};
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::nonempty::NonEmptySet;
use whyshelf_core::domain::shelf::doc::DocKey;
use whyshelf_core::domain::shelf::freshness::{Origin, OriginEvent, OriginGroup};
use whyshelf_core::domain::shelf::ledger::{FrameRefusal, NotStanding};
use whyshelf_core::domain::shelf::schema::{BookName, EntryName, KeyPattern};
use whyshelf_core::domain::shelf::settle::{Approval, Settlement, Withheld};

use super::*;

fn docid(n: u8) -> DocId {
    DocId::new(
        format!("01900000-0000-7000-8000-0000000000{n:02x}")
            .parse::<EntityId>()
            .unwrap(),
    )
}

/// The refusal of an approval that stands on `note:one`, which was denied.
fn refusal() -> FrameRefusal {
    FrameRefusal::UpstreamNotStanding {
        link: EntryName::new("base").unwrap(),
        target: docid(9),
        key: DocKey {
            book: BookName::new("note").unwrap(),
            form: KeyPattern::Slug.parse("one").unwrap(),
        },
        status: NotStanding::Denied,
    }
}

const SENTENCE: &str =
    "the link `base` names note:one, which is denied: an approval cannot stand on it";

fn origin(n: u8, next: Vec<Settlement>, withheld: Vec<Withheld>) -> SettledOrigin {
    SettledOrigin {
        group: OriginGroup {
            origin: Origin::Doc(docid(n)),
            degraded: [docid(n)].into(),
            held: NonEmptySet::new((OriginEvent::UpstreamPending, docid(n)), []),
        },
        next,
        withheld,
    }
}

/// An origin at document 1 that two approvals of the same document are withheld of,
/// and one at 2 that none is.
fn report() -> StaleReport {
    StaleReport {
        entries: Vec::new(),
        origins: vec![
            origin(
                1,
                Vec::new(),
                vec![
                    Withheld {
                        approval: Approval::AsAnother(docid(1)),
                        why: refusal(),
                    },
                    Withheld {
                        approval: Approval::Until(docid(1)),
                        why: refusal(),
                    },
                ],
            ),
            origin(
                2,
                vec![Settlement::Approve(Approval::Pending(docid(2)))],
                Vec::new(),
            ),
        ],
        unknown: Vec::new(),
        held: Gate::Holds,
        names: Names::default(),
        health: Health::default(),
    }
}

#[test]
fn each_approval_the_frame_refuses_is_told_with_its_document_kind_and_sentence() {
    let report = report();
    let answer = stale(&report, NextRoot::Current);
    let first = &answer.origins[0].withheld;
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].doc.id, docid(1).to_string());
    assert_eq!(first[0].approve, "as_another");
    assert_eq!(first[1].approve, "until");
    for one in first {
        assert_eq!(one.detail, SENTENCE);
    }
    // An origin the frame refuses nothing of tells nothing, and keeps its command.
    assert!(answer.origins[1].withheld.is_empty());
    assert_eq!(answer.origins[1].next.len(), 1);
}

#[test]
fn what_is_withheld_is_told_when_the_root_is_no_text_and_no_command_is_written() {
    let report = report();
    let answer = stale(&report, NextRoot::NotText);
    assert!(answer.origins[0].next.is_empty());
    assert_eq!(answer.origins[0].withheld.len(), 2);
    assert!(answer.origins[1].next.is_empty());
    assert!(answer.origins[1].withheld.is_empty());
}

#[test]
fn each_withheld_approval_is_one_line_with_its_kind_and_the_same_document_twice_is_two_lines() {
    let id = docid(1);
    assert_eq!(
        stale_warnings(&report()),
        [
            format!("warning: {id} is not offered an approval by another name: {SENTENCE}"),
            format!("warning: {id} is not offered an approval with a new expiry: {SENTENCE}"),
        ]
    );
    // And the third kind, which has a phrase of its own.
    let mut pending = report();
    pending.origins = vec![origin(
        3,
        Vec::new(),
        vec![Withheld {
            approval: Approval::Pending(docid(3)),
            why: refusal(),
        }],
    )];
    assert_eq!(
        stale_warnings(&pending),
        [format!(
            "warning: {} is not offered an approval as pending: {SENTENCE}",
            docid(3)
        )]
    );
}

#[test]
fn nothing_withheld_is_no_line() {
    let mut none = report();
    none.origins.remove(0);
    assert!(stale_warnings(&none).is_empty());
}
