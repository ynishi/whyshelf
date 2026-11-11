//! Tests of the drawing of `stats`: exactly, for a small answer and for an empty
//! one; its durations and times, those RFC 3339 cannot write included; display
//! widths of CJK names; artifacts by space; lists drawn short; its lines under the
//! width; and the same answer drawn twice.

use unicode_width::UnicodeWidthStr;

use super::summary::{LINE, draw, duration, time};
use super::*;

fn doc_ref(id: &str, key: Option<&str>) -> DocRef {
    DocRef {
        id: id.to_owned(),
        key: key.map(str::to_owned),
    }
}

fn health() -> HealthOut {
    HealthOut {
        clean: true,
        invalid: vec![],
        cycles: vec![],
        ledger_problems: vec![],
        unreadable_records: vec![],
    }
}

fn pending(book: Option<&str>, count: usize, spread: Option<[i64; 3]>) -> BookPendingOut {
    BookPendingOut {
        book: book.map(str::to_owned),
        count,
        median_ms: spread.map(|s| s[0]),
        p90_ms: spread.map(|s| s[1]),
        max_ms: spread.map(|s| s[2]),
    }
}

fn books(rows: &[(Option<&str>, [usize; 4])]) -> Vec<BookApprovalsOut> {
    rows.iter()
        .map(
            |(book, [approvals, again, revoked, docs])| BookApprovalsOut {
                book: book.map(str::to_owned),
                docs: *docs,
                approvals: *approvals,
                again: *again,
                revoked: *revoked,
            },
        )
        .collect()
}

/// Two books (one of them the documents of no book), three documents, one of them
/// approved again, two runs and two origins.
fn small() -> StatsOut {
    StatsOut {
        since: None,
        approvals: ApprovalsOut {
            docs: vec![
                DocApprovalsOut {
                    doc: doc_ref("01a1", Some("decision:DEC-1")),
                    approvals: 2,
                    again: 1,
                    revoked: 0,
                },
                DocApprovalsOut {
                    doc: doc_ref("01a2", Some("decision:DEC-2")),
                    approvals: 1,
                    again: 0,
                    revoked: 0,
                },
                DocApprovalsOut {
                    doc: doc_ref("01a3", None),
                    approvals: 1,
                    again: 0,
                    revoked: 1,
                },
            ],
            books: books(&[(Some("decision"), [4, 1, 0, 3]), (None, [1, 0, 1, 1])]),
        },
        pending: vec![
            pending(
                Some("decision"),
                3,
                Some([13 * 60_000 + 5_000, 7_500_000, 7_500_000]),
            ),
            pending(None, 0, None),
        ],
        runs: RunsOut {
            count: 2,
            median: Some(1),
            p90: Some(2),
            max: Some(2),
            largest: vec![
                RunOut {
                    at: 1_762_744_365_000,
                    actor: "ann".to_owned(),
                    size: 2,
                },
                RunOut {
                    at: 1_762_750_000_000,
                    actor: "bo".to_owned(),
                    size: 1,
                },
            ],
        },
        origins: vec![
            OriginCountOut {
                origin: ChangedOut::Doc(DocNamed {
                    doc: doc_ref("01a2", Some("decision:DEC-2")),
                }),
                count: 2,
            },
            OriginCountOut {
                origin: ChangedOut::Artifact(ArtifactNamed {
                    path: "src/lib.rs".to_owned(),
                    name: "src/lib.rs".to_owned(),
                    space: "file".to_owned(),
                }),
                count: 1,
            },
        ],
        health: health(),
        drawing: String::new(),
    }
}

/// A ledger with no record, on a declaration of one book.
fn empty() -> StatsOut {
    StatsOut {
        since: None,
        approvals: ApprovalsOut {
            docs: vec![],
            books: books(&[(Some("decision"), [0, 0, 0, 0])]),
        },
        pending: vec![pending(Some("decision"), 0, None)],
        runs: RunsOut {
            count: 0,
            median: None,
            p90: None,
            max: None,
            largest: vec![],
        },
        origins: vec![],
        health: health(),
        drawing: String::new(),
    }
}

#[test]
fn a_small_answer_is_drawn_section_by_section_in_aligned_columns() {
    let expected = "\
Approvals per book
  book       approvals  again  revoked  documents
  decision           4      1        0          3
  (no book)          1      0        1          1

Approved again most often
  again  approvals  document
      1          2  decision:DEC-1

Pending per book, from a document's identity to its first approval
  book       count  median    p90    max
  decision       3     13m  2h 5m  2h 5m
  (no book)      0       -      -      -

Runs of judge
  count  median  p90  max
      2       1    2    2
  largest
    size  actor  at
       2  ann    2025-11-10T03:12:45Z
       1  bo     2025-11-10T04:46:40Z

Origins under approvals made again
  count  origin
      2  decision:DEC-2
      1  src/lib.rs
";
    assert_eq!(draw(&small()), expected);
}

#[test]
fn an_empty_ledger_says_so_in_every_section() {
    let expected = "\
Approvals per book
  no approval

Approved again most often
  no document approved again

Pending per book, from a document's identity to its first approval
  no document approved

Runs of judge
  no run

Origins under approvals made again
  no origin under an approval made again
";
    assert_eq!(draw(&empty()), expected);
}

#[test]
fn a_duration_is_hours_and_minutes_and_a_time_is_utc_to_the_second() {
    for (millis, text) in [
        (0, "< 1m"),
        (59_999, "< 1m"),
        (-30_000, "< 1m"),
        (60_000, "1m"),
        (13 * 60_000 + 59_999, "13m"),
        (3_600_000, "1h 0m"),
        ((2 * 60 + 13) * 60_000 + 59_000, "2h 13m"),
        (-(2 * 60 + 13) * 60_000, "-2h 13m"),
        (100 * 3_600_000, "100h 0m"),
    ] {
        assert_eq!(duration(millis), text, "{millis}");
    }
    assert_eq!(time(0), "1970-01-01T00:00:00Z");
    assert_eq!(time(1_762_744_365_999), "2025-11-10T03:12:45Z");
}

#[test]
fn a_time_rfc_3339_cannot_write_is_its_milliseconds_and_said_so() {
    // The first instant RFC 3339 writes (a four-digit year), and the latest whole
    // millisecond jiff reads, a little before the end of 9999; a millisecond later
    // is out of its range.
    assert_eq!(time(-62_167_219_200_000), "0000-01-01T00:00:00Z");
    let last = jiff::Timestamp::MAX.as_second() * 1000;
    assert_eq!(time(last), "9999-12-30T22:00:00Z");
    for millis in [-62_167_219_200_001, last + 1, i64::MAX, i64::MIN] {
        assert_eq!(time(millis), format!("unix ms {millis}"));
    }
}

#[test]
fn a_cjk_name_is_aligned_by_its_display_width_and_cut_by_it() {
    let mut answer = small();
    answer.runs.largest[0].actor = "西村豊".to_owned();
    let drawing = draw(&answer);
    let expected = "  largest
    size  actor   at
       2  西村豊  2025-11-10T03:12:45Z
       1  bo      2025-11-10T04:46:40Z
";
    assert!(drawing.contains(expected), "{drawing}");

    answer.runs.largest[0].actor = "西".repeat(50);
    let drawing = draw(&answer);
    let row = drawing
        .lines()
        .find(|line| line.contains('西'))
        .expect("the row of the actor");
    let actor = row.split("  ").find(|cell| cell.contains('西')).unwrap();
    // 19 characters of two columns, then `…`: 39 columns, under the 40 of a name.
    assert_eq!(actor, format!("{}…", "西".repeat(19)));
    for line in drawing.lines() {
        assert!(line.width() < LINE, "{line}");
    }
}

#[test]
fn an_artifact_outside_the_files_is_named_with_its_space() {
    let mut answer = small();
    answer.origins[1].origin = ChangedOut::Artifact(ArtifactNamed {
        path: "whyshelf::stats".to_owned(),
        name: "whyshelf::stats".to_owned(),
        space: "rust-item".to_owned(),
    });
    let drawing = draw(&answer);
    assert!(
        drawing.contains("      1  rust-item:whyshelf::stats\n"),
        "{drawing}"
    );
    assert!(draw(&small()).contains("      1  src/lib.rs\n"));
}

#[test]
fn a_list_drawn_short_says_how_many_it_shows_of_how_many() {
    let mut answer = small();
    answer.approvals.docs = (1..=12)
        .map(|n| DocApprovalsOut {
            doc: doc_ref(&format!("01b{n}"), Some(&format!("decision:DEC-{n}"))),
            approvals: 30 - n,
            again: 20 - n,
            revoked: 0,
        })
        .collect();
    answer.origins = (1..=12)
        .map(|n| OriginCountOut {
            origin: ChangedOut::Doc(DocNamed {
                doc: doc_ref(&format!("01c{n}"), Some(&format!("decision:DEC-{n}"))),
            }),
            count: 20 - n,
        })
        .collect();
    answer.runs.count = 58;
    answer.runs.largest = (0..7)
        .map(|n| RunOut {
            at: 1_762_744_365_000 + n,
            actor: "ann".to_owned(),
            size: 10 - n as usize,
        })
        .collect();
    let drawing = draw(&answer);
    let lines: Vec<&str> = drawing.lines().collect();
    for heading in [
        "Approved again most often (10 of 12)",
        "  largest (5 of 58)",
        "Origins under approvals made again (10 of 12)",
    ] {
        assert!(lines.contains(&heading), "{heading}: {drawing}");
    }
    assert!(drawing.contains("decision:DEC-10\n"));
    assert!(!drawing.contains("decision:DEC-11"));
    // A list drawn whole has no count in its heading.
    let whole = draw(&small());
    assert!(whole.lines().any(|l| l == "Approved again most often"));
    assert!(whole.lines().any(|l| l == "  largest"));
}

#[test]
fn every_line_stays_under_the_width_and_a_long_name_is_cut() {
    let mut answer = small();
    let long = "x".repeat(150);
    answer.approvals.docs[0].doc.key = Some(long.clone());
    answer.approvals.books[0].book = Some(long.clone());
    answer.pending[0].book = Some(long.clone());
    answer.runs.largest[0].actor = long.clone();
    answer.origins[1].origin = ChangedOut::Artifact(ArtifactNamed {
        path: long.clone(),
        name: long.clone(),
        space: "file".to_owned(),
    });
    let drawing = draw(&answer);
    for line in drawing.lines() {
        assert!(line.width() < LINE, "{line}");
    }
    assert!(!drawing.contains(&long));
    assert_eq!(drawing.matches('…').count(), 5, "{drawing}");
}

#[test]
fn the_same_answer_is_the_same_text() {
    assert_eq!(draw(&small()), draw(&small()));
    let answer = small();
    assert_eq!(draw(&answer), draw(&answer));
}
