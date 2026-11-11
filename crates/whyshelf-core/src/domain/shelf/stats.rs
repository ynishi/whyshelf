//! How the work went, as the ledger tells it: [`of`] derives [`Stats`] from the
//! decisions and the documents, and from nothing else.
//!
//! Every time is one a record or an identity carries: a decision's
//! [`at`](Decision::at), and the time a document's identity was made
//! ([`DocId::made_at_millis`]). Nothing reads a clock, so the same ledger and the
//! same documents always give the same answer, the order of every list included.
//!
//! # The measures
//!
//! An *approval* is a decision that approves; every one is counted, revoked or not
//! (a revocation withdraws what it decided, not that it was made), and those revoked
//! are counted apart as well. The approvals of a document are taken in the order
//! [`Ledger::standing_decision`] reads them, by time and then by identity. A revoked
//! approval never made the document approved: an approval is *again* when an earlier
//! approval of the same document is in the ledger and is not revoked, and the
//! approval it is *made again from* is the latest such one.
//!
//! - **Approvals** ([`Approvals`]): per document, the number of its approvals, how
//!   many of them are again, and how many are revoked; per book, the totals.
//! - **Pending time** ([`BookPending`]): per book, for each document with an
//!   approval that is not revoked, the milliseconds from the time of its identity to
//!   the first such approval (negative when the identity carries a later time than
//!   the approval).
//! - **Runs** ([`Runs`]): the decisions of any verb written by one run of
//!   `judge`. Every decision of a run takes one time, the time the project was read
//!   at (the rule is on [`judge_all`](crate::app::shelf::ShelfApp::judge_all)), so a
//!   run is the decisions that share their time and their actor. Two runs by one
//!   actor in the same millisecond are counted as one. A run that writes no decision
//!   (a revocation, a declaration of the same meaning) is no run here.
//! - **Origins over time** ([`OriginCount`]): for each approval that is again, its
//!   frame is set beside the frame of the approval it is made again from. Each pin
//!   both frames hold whose hash differs is an *origin* of that approval made again:
//!   an upstream by its identity, an artifact by its space and name (an artifact
//!   that came or went counts, as its hash differs from none). A pin of an artifact
//!   is compared only with a pin of the same comparison: when the comparison
//!   differs, the hashes were made differently and are not compared (as
//!   [`Cause::ArtifactComparisonChanged`](super::freshness::Cause::ArtifactComparisonChanged)
//!   says), and it is no origin. A pin only one of the two frames holds is a link
//!   that changed, not a hash, and is no origin. An origin counts once per approval
//!   made again.
//!
//! # `since`
//!
//! A time given as `since` keeps only the records at or after it: the approvals
//! counted, the decisions of a run, the approvals made again whose origins are
//! counted, and, for the pending time, the documents whose first approval that is
//! not revoked is at or after it. Whether an approval is again, and the approval it
//! is made again from, are read from the whole ledger: an approval after `since` of
//! a document first approved before it is again.
//!
//! # Books and keys
//!
//! A document is named by its identity and by the key of its valid document now,
//! as every answer names it; one that is no valid document now has no key and is
//! counted under no book. The books are listed in the order the declaration writes
//! them, each whether it has a document or not, and then, when there is any, the
//! documents of no book.
//!
//! # Distributions
//!
//! A [`Spread`] gives the count, the median, the 90th percentile and the maximum of
//! a list of whole numbers. The percentiles are nearest-rank: of `n` values sorted
//! up, the `p`th percentile is the value at the rank `ceil(p * n / 100)`, counting
//! from 1. The median is the 50th percentile, so of an even count it is the lower of
//! the two middle values; every value is one of the list.
//!
//! # The order of the lists
//!
//! - documents: the most approvals again first, then by key (the documents with no
//!   key after those with one), then by identity;
//! - books: the order of the declaration, the documents of no book last;
//! - the largest runs: the largest first, then the earliest, then by actor;
//! - origins: the most counted first, then the documents before the artifacts, the
//!   documents by key and identity as above, the artifacts by space and name.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::id::DocId;

use super::artifact::{ArtifactKey, ArtifactState, Comparison};
use super::build::Shelf;
use super::doc::{DocHash, DocKey};
use super::ledger::{Decision, DecisionVerb, JudgedAt, Ledger};
use super::schema::BookName;

/// How many of the largest runs [`Runs::largest`] lists.
pub const LARGEST_RUNS: usize = 10;

/// What [`of`] derives: the four measures of the module page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// The approvals, per document and per book.
    pub approvals: Approvals,
    /// The time from identity to first approval, per book.
    pub pending: Vec<BookPending>,
    /// The runs of `judge`.
    pub runs: Runs,
    /// The pins that changed under an approval made again, the most counted first.
    pub origins: Vec<OriginCount>,
}

/// The approvals, per document and per book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approvals {
    /// Each document with an approval counted, the most approved again first.
    pub docs: Vec<DocApprovals>,
    /// Each book, in the order of the declaration, then the documents of no book.
    pub books: Vec<BookApprovals>,
}

/// The approvals of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocApprovals {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is a valid document now.
    pub key: Option<DocKey>,
    /// How many approvals of it there are, revoked or not.
    pub approvals: usize,
    /// How many of them came after an earlier approval of it that is not revoked.
    pub again: usize,
    /// How many of them are revoked.
    pub revoked: usize,
}

/// The approvals of the documents of one book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookApprovals {
    /// The book; none for the documents that are no valid document now.
    pub book: Option<BookName>,
    /// How many documents of it have an approval counted.
    pub docs: usize,
    /// The approvals of its documents.
    pub approvals: usize,
    /// How many of them are again.
    pub again: usize,
    /// How many of them are revoked.
    pub revoked: usize,
}

/// The pending times of the documents of one book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookPending {
    /// The book; none for the documents that are no valid document now.
    pub book: Option<BookName>,
    /// The milliseconds from identity to first approval.
    pub millis: Spread,
}

/// The count, median, 90th percentile and maximum of a list, nearest-rank (see the
/// module page); the three values are none when the list is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spread {
    /// How many values.
    pub count: usize,
    /// The 50th percentile.
    pub median: Option<i64>,
    /// The 90th percentile.
    pub p90: Option<i64>,
    /// The largest.
    pub max: Option<i64>,
}

impl Spread {
    /// The spread of these values, in any order.
    pub fn of(mut values: Vec<i64>) -> Self {
        values.sort_unstable();
        let count = values.len();
        let rank = |p: usize| values.get((p * count).div_ceil(100).max(1) - 1).copied();
        Self {
            count,
            median: rank(50),
            p90: rank(90),
            max: values.last().copied(),
        }
    }
}

/// The runs of `judge`: the decisions that share their time and their actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runs {
    /// How many decisions each run held; its count is the number of runs.
    pub sizes: Spread,
    /// The largest runs, at most [`LARGEST_RUNS`], the largest first.
    pub largest: Vec<Run>,
}

/// One run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// When it was made.
    pub at: JudgedAt,
    /// Who made it.
    pub actor: Actor,
    /// How many decisions it holds.
    pub size: usize,
}

/// A pin that changed under approvals made again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginCount {
    /// What it pinned.
    pub origin: Origin,
    /// Under how many approvals made again its hash differed from the one before.
    pub count: usize,
}

/// What a changed pin names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// An upstream document.
    Doc {
        /// The document.
        doc: DocId,
        /// Its key, when it is a valid document now.
        key: Option<DocKey>,
    },
    /// An artifact.
    Artifact(ArtifactKey),
}

/// The measures of the module page, from every decision of `ledger`, the documents
/// of `shelf` naming them, and the records at or after `since` when it is given.
pub fn of(ledger: &Ledger, shelf: &Shelf, since: Option<JudgedAt>) -> Stats {
    let kept = |at: JudgedAt| since.is_none_or(|since| at >= since);
    let key_of = |id: DocId| shelf.doc(id).map(|doc| doc.key().clone());

    let mut by_doc: BTreeMap<DocId, Vec<&Decision>> = BTreeMap::new();
    for decision in ledger.decisions() {
        if matches!(decision.verb(), DecisionVerb::Approve { .. }) {
            by_doc.entry(decision.subject()).or_default().push(decision);
        }
    }

    let mut docs = Vec::new();
    let mut pending: BTreeMap<Option<BookName>, Vec<i64>> = BTreeMap::new();
    let mut origins: BTreeMap<OriginKey, usize> = BTreeMap::new();
    for (id, approvals) in &mut by_doc {
        approvals.sort_by_key(|d| (d.at(), d.id()));
        let key = key_of(*id);
        let book = key.as_ref().map(|key| key.book.clone());
        if let Some(first) = approvals.iter().find(|d| !ledger.is_revoked(d.id()))
            && kept(first.at())
        {
            pending
                .entry(book)
                .or_default()
                .push(first.at().millis() - id.made_at_millis());
        }
        let mut counted = DocApprovals {
            doc: *id,
            key,
            approvals: 0,
            again: 0,
            revoked: 0,
        };
        // The latest earlier approval that is not revoked.
        let mut standing: Option<&Decision> = None;
        for approval in approvals.iter() {
            let revoked = ledger.is_revoked(approval.id());
            if kept(approval.at()) {
                counted.approvals += 1;
                if revoked {
                    counted.revoked += 1;
                }
                if let Some(before) = standing {
                    counted.again += 1;
                    for origin in changed_pins(before, approval) {
                        *origins.entry(origin).or_default() += 1;
                    }
                }
            }
            if !revoked {
                standing = Some(approval);
            }
        }
        if counted.approvals > 0 {
            docs.push(counted);
        }
    }
    docs.sort_by(|a, b| {
        b.again
            .cmp(&a.again)
            .then_with(|| by_key(&a.key, &b.key))
            .then(a.doc.cmp(&b.doc))
    });

    let declared: Vec<Option<BookName>> =
        shelf.schema().books().keys().cloned().map(Some).collect();
    let mut books = declared;
    if docs.iter().any(|d| d.key.is_none()) || pending.contains_key(&None) {
        books.push(None);
    }
    let book_approvals = books
        .iter()
        .map(|book| {
            let of_book = docs
                .iter()
                .filter(|d| d.key.as_ref().map(|k| &k.book) == book.as_ref());
            let mut total = BookApprovals {
                book: book.clone(),
                docs: 0,
                approvals: 0,
                again: 0,
                revoked: 0,
            };
            for doc in of_book {
                total.docs += 1;
                total.approvals += doc.approvals;
                total.again += doc.again;
                total.revoked += doc.revoked;
            }
            total
        })
        .collect();
    let pending = books
        .iter()
        .map(|book| BookPending {
            book: book.clone(),
            millis: Spread::of(pending.remove(book).unwrap_or_default()),
        })
        .collect();

    let mut runs: BTreeMap<(JudgedAt, &Actor), usize> = BTreeMap::new();
    for decision in ledger.decisions().filter(|d| kept(d.at())) {
        *runs.entry((decision.at(), decision.actor())).or_default() += 1;
    }
    let mut largest: Vec<Run> = runs
        .iter()
        .map(|((at, actor), size)| Run {
            at: *at,
            actor: (*actor).clone(),
            size: *size,
        })
        .collect();
    let sizes = Spread::of(largest.iter().map(|run| run.size as i64).collect());
    largest.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then(a.at.cmp(&b.at))
            .then_with(|| a.actor.cmp(&b.actor))
    });
    largest.truncate(LARGEST_RUNS);

    let mut origins: Vec<OriginCount> = origins
        .into_iter()
        .map(|(origin, count)| OriginCount {
            origin: match origin {
                OriginKey::Doc(doc) => Origin::Doc {
                    doc,
                    key: key_of(doc),
                },
                OriginKey::Artifact(artifact) => Origin::Artifact(artifact),
            },
            count,
        })
        .collect();
    origins.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| by_origin(&a.origin, &b.origin))
    });

    Stats {
        approvals: Approvals {
            docs,
            books: book_approvals,
        },
        pending,
        runs: Runs { sizes, largest },
        origins,
    }
}

/// What a changed pin names, before it is named.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum OriginKey {
    Doc(DocId),
    Artifact(ArtifactKey),
}

/// The pins both approvals hold whose hash differs, an artifact's only under the
/// same comparison, each once.
fn changed_pins(before: &Decision, after: &Decision) -> BTreeSet<OriginKey> {
    let (
        DecisionVerb::Approve {
            frame: frame_before,
            artifacts: artifacts_before,
            ..
        },
        DecisionVerb::Approve {
            frame, artifacts, ..
        },
    ) = (before.verb(), after.verb())
    else {
        return BTreeSet::new();
    };
    let docs_before: BTreeMap<DocId, &DocHash> = frame_before
        .iter()
        .map(|pin| (pin.target(), pin.hash()))
        .collect();
    let artifacts_before: BTreeMap<(&ArtifactKey, &Comparison), ArtifactState> = artifacts_before
        .iter()
        .map(|pin| ((pin.artifact(), pin.comparison()), pin.seen().state()))
        .collect();
    let docs = frame
        .iter()
        .filter(|pin| {
            docs_before
                .get(&pin.target())
                .is_some_and(|h| *h != pin.hash())
        })
        .map(|pin| OriginKey::Doc(pin.target()));
    let artifacts = artifacts
        .iter()
        .filter(|pin| {
            artifacts_before
                .get(&(pin.artifact(), pin.comparison()))
                .is_some_and(|state| *state != pin.seen().state())
        })
        .map(|pin| OriginKey::Artifact(pin.artifact().clone()));
    docs.chain(artifacts).collect()
}

/// Keys in order, no key after every key.
fn by_key(a: &Option<DocKey>, b: &Option<DocKey>) -> Ordering {
    (a.is_none(), a).cmp(&(b.is_none(), b))
}

/// Documents before artifacts; documents by key then identity, artifacts by space
/// and name.
fn by_origin(a: &Origin, b: &Origin) -> Ordering {
    match (a, b) {
        (Origin::Doc { doc: x, key: kx }, Origin::Doc { doc: y, key: ky }) => {
            by_key(kx, ky).then(x.cmp(y))
        }
        (Origin::Doc { .. }, Origin::Artifact(_)) => Ordering::Less,
        (Origin::Artifact(_), Origin::Doc { .. }) => Ordering::Greater,
        (Origin::Artifact(x), Origin::Artifact(y)) => x.cmp(y),
    }
}

#[cfg(test)]
mod tests;
