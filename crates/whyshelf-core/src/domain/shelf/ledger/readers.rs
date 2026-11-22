//! Who has read the content an approval stands on, and what a new approval of the
//! document would be beside them: [`Ledger::readers`] and [`Ledger::kind_of`], the one
//! place two approvals are matched for what they stand on and who made them; and
//! whether enough of them did for the approval that stands to settle,
//! [`StandingApproval::quorum`], from the same walk that tells who counts now,
//! [`StandingApproval::reading`]. The rules are on [`Readers`], [`ReadBy`],
//! [`ApprovalKind`], [`ApprovalKind::left_out`] and [`Quorum`].
//!
//! ```text
//!   the decisions of a document, the latest first in the order of the records (RecordOrder)
//!        │ each a view of its record (ApprovalView), up to one that is no approval
//!        ▼ or stands on other content
//!     Readers  ◄── what an approval stands on (the one comparison)
//!        │
//!        ├── at(now) ──► ReadBy ──► count: Nobody / One / Several
//!        ├──────────────► Quorum (Met / Short) of the StandingApproval: freshness,
//!        │                order, check read it
//!        ▼
//!   ApprovalKind ──► left_out(taking, again)
//!        │
//!   judge approve, --stale / --pending, stats: read the answer, decide nothing
//! ```

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptySet;

use super::super::doc::DocHash;
use super::super::schema::Approvals;
use super::{
    ApprovalView, ApproveFrame, ArtifactPin, Decision, FramePin, JudgedAt, JudgementId, Ledger,
    Moved, RecordOrder, StandingApproval, StandsOn,
};

/// Up to which record the ledger is read by [`Ledger::readers`] and
/// [`Ledger::kind_of`]: which records are read, decisions and declarations of moves
/// alike. Whether a record is revoked is read from the whole ledger either way, and
/// whether a move it keeps reaches a pin is a rule of time of its own (a move reaches
/// the pins of an approval made at or before it, see [`Moved`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before {
    /// Every record: the ledger as it is now.
    Now,
    /// The records before one in the order of the records ([`RecordOrder`]), the order
    /// in which [`Ledger::standing_decision`] takes the latest.
    Record(RecordOrder),
}

impl Before {
    /// The records before `decision`.
    pub fn decision(decision: &Decision) -> Self {
        Self::Record(decision.order())
    }

    /// Whether a decision is read.
    pub(super) fn keeps(self, decision: &Decision) -> bool {
        self.keeps_order(decision.order())
    }

    /// Whether a declaration of a move is read, in the same order as a decision.
    pub(super) fn keeps_move(self, moved: &Moved) -> bool {
        self.keeps_order(moved.order())
    }

    fn keeps_order(self, order: RecordOrder) -> bool {
        match self {
            Self::Now => true,
            Self::Record(cut) => order < cut,
        }
    }
}

/// The readers of the content that stands about a document: the approvals of the same
/// content that stand together now, the latest first ([`Ledger::readers`]), each a view
/// of its record ([`ApprovalView`]).
///
/// Walking back from the latest decision about the document that is not revoked, in
/// the order of the records, they are the approvals that stand on what it stands on,
/// until a decision that does not (a decision that is no approval, or an approval of
/// other content). Every row is read from its record as the first is. So an edit
/// approved again by one actor starts a new list, and content approved again after
/// other content is a new list too, not the readers it had before. What an approval
/// stands on is the hash of the document, every pin of an upstream (link, identity,
/// hash; its key is not compared) and every pin of an artifact (link, space and name,
/// comparison, what was there and its hash), compared as values; the addresses, the
/// revision, the expiry, the actor and the reason are not, and freshness is not
/// asked. The pins compared are the stored ones, never as read through a move: an
/// approval made after a move pins
/// the name it finds, and while a move reads a pin of the approval that stands, a new
/// one of the same values is a renewal ([`Ledger::kind_of`]).
///
/// A reading is a name declared, as every actor is
/// ([`Actor`](crate::domain::actor::Actor)): there is no authentication, so one person
/// who gives two names reads as two readers. The readers say who declared they read
/// the content, not that two people did.
///
/// Two lists of readers are not compared: a row is a view tied to its ledger, which has
/// no equality ([`ApprovalView`]).
#[derive(Debug, Clone)]
pub struct Readers<'l> {
    approvals: Vec<ApprovalView<'l>>,
}

impl<'l> Readers<'l> {
    /// The approvals, the latest first; the first is the approval that stands. Empty
    /// when no approval stands.
    pub fn approvals(&self) -> &[ApprovalView<'l>] {
        &self.approvals
    }

    /// The approval that stands, when one does.
    pub fn standing(&self) -> Option<ApprovalView<'l>> {
        self.approvals.first().copied()
    }

    /// The names that read the content, each once.
    pub fn actors(&self) -> BTreeSet<&'l Actor> {
        self.approvals.iter().map(ApprovalView::actor).collect()
    }

    /// The latest reading by `actor`, when it is a reader.
    pub fn latest_of(&self, actor: &Actor) -> Option<ApprovalView<'l>> {
        self.approvals.iter().copied().find(|v| v.actor() == actor)
    }

    /// The names that read the content, told by whether their own latest reading has
    /// lapsed at `now` ([`JudgedAt::lapsed_by`]): the one place who reads it now is
    /// decided, so a name counts once, however many of its approvals are among the
    /// readers, and no other name stands in for one that lapsed. No readers, none
    /// counted and none lapsed.
    pub fn at(&self, now: JudgedAt) -> ReadBy {
        let mut read_by = ReadBy {
            counted: BTreeSet::new(),
            lapsed: BTreeSet::new(),
        };
        for actor in self.actors() {
            let alive = self
                .latest_of(actor)
                .is_some_and(|own| !own.expires().is_some_and(|e| e.lapsed_by(now)));
            let side = if alive {
                &mut read_by.counted
            } else {
                &mut read_by.lapsed
            };
            side.insert(actor.clone());
        }
        read_by
    }
}

/// The names that read the content an approval stands on, at a time: those whose own
/// latest reading has not lapsed, and those whose has ([`Readers::at`]). A name is in
/// one of the two, never both. Both may be empty, and `counted` is empty when every
/// reading lapsed: what the approval needs to settle is [`Quorum`]'s, which counts the
/// approval that stands alone then, and nothing of that is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBy {
    counted: BTreeSet<Actor>,
    lapsed: BTreeSet<Actor>,
}

impl ReadBy {
    /// Read by these names, none lapsed: the input of a test that needs a standing.
    #[cfg(test)]
    pub fn of_counted(counted: BTreeSet<Actor>) -> Self {
        Self {
            counted,
            lapsed: BTreeSet::new(),
        }
    }

    /// The names whose reading has not lapsed, each once.
    pub fn counted(&self) -> &BTreeSet<Actor> {
        &self.counted
    }

    /// The names whose own latest reading has lapsed, each once.
    pub fn lapsed(&self) -> &BTreeSet<Actor> {
        &self.lapsed
    }

    /// How many names count: the one place it is told, for the filter and the
    /// answers.
    pub fn count(&self) -> ReaderCount {
        match self.counted.len() {
            0 => ReaderCount::Nobody,
            1 => ReaderCount::One,
            _ => ReaderCount::Several,
        }
    }
}

/// How many names read the content an approval stands on now ([`ReadBy::count`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReaderCount {
    /// No reading counts: every one lapsed (or no approval stands).
    Nobody,
    /// One name's reading counts.
    One,
    /// Two names' or more.
    Several,
}

impl ReaderCount {
    /// Every count, in order: the one list of them, so that a surface that names each
    /// count reads them all from here and a word can be read back.
    pub const ALL: [Self; 3] = [Self::Nobody, Self::One, Self::Several];
}

/// What one walk of the readers of the approval that stands gives
/// ([`StandingApproval::reading`]): who reads its content now, and whether that is
/// enough for the approval to settle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The names that read the content, told at the time asked.
    pub read_by: ReadBy,
    /// Whether enough of them did for the rule that reads the approval.
    pub quorum: Quorum,
}

/// A new approval of a document, asked about: what it would stand on, who would make
/// it, when it would lapse, and when it is made. What an approval of the ledger was
/// when it was made is asked of its view, from its record, in the ledger it is of.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    subject: DocId,
    seen: &'a DocHash,
    frame: &'a BTreeSet<FramePin>,
    artifacts: &'a BTreeSet<ArtifactPin>,
    by: &'a Actor,
    until: Option<JudgedAt>,
    at: JudgedAt,
}

impl<'a> Asked<'a> {
    /// An approval to be made on `frame` by `by` at `at`, lapsing at `until`.
    pub fn frame(
        frame: &'a ApproveFrame,
        by: &'a Actor,
        until: Option<JudgedAt>,
        at: JudgedAt,
    ) -> Self {
        Self {
            subject: frame.subject,
            seen: &frame.seen,
            frame: &frame.frame,
            artifacts: &frame.artifacts,
            by,
            until,
            at,
        }
    }
}

impl<'l> ApprovalView<'l> {
    /// The approval, as it was asked when it was made.
    fn asked(&self) -> Asked<'l> {
        Asked {
            subject: self.subject(),
            seen: self.seen,
            frame: self.frame,
            artifacts: self.artifacts,
            by: self.actor(),
            until: self.expires,
            at: self.at(),
        }
    }

    /// What the approval was when it was made, beside the readers before it in its
    /// ledger ([`Ledger::kind_of`]).
    pub(in crate::domain::shelf) fn kind_before(&self) -> ApprovalKind {
        self.ledger
            .kind_of(&self.asked(), Before::Record(self.order()))
    }
}

/// What a new approval would be beside the [`Readers`] of its document
/// ([`Ledger::kind_of`]), asked about by what it would stand on, who would make it,
/// when it would lapse and when it is made ([`Asked`]):
///
/// | the readers | the one who approves | kind |
/// |---|---|---|
/// | none, or of other content | anyone | [`New`](ApprovalKind::New) |
/// | of the same content, the approval that stands lapsed, lapsing at another time than the new one would, or with a pin read through a move | anyone | [`Renewal`](ApprovalKind::Renewal) |
/// | of the same content, the approval that stands not lapsed and lapsing when the new one would | no reader | [`SecondReading`](ApprovalKind::SecondReading) |
/// | the same | a reader whose latest reading lapses when the new one would | [`Repeat`](ApprovalKind::Repeat) |
/// | the same | a reader otherwise | [`Renewal`](ApprovalKind::Renewal) |
///
/// Freshness reads the approval that stands (the latest), with its expiry and its
/// pins through the moves since it, so an approval that would approve a lapsed one
/// again, move when the document lapses, or end a move's reading, is a renewal,
/// whoever makes it. Of the others, which change nothing freshness reads, a repeat
/// would change nothing at all; a reader's with another expiry than their own latest
/// reading renews that reading, which a reader keeps whoever's approval stands; and a
/// second reading adds a reader: a reading of its own in every project, whether or
/// not it declares who may decide.
/// `standing` is the approval that stands,
/// which a run that leaves the new one out names. Before, a new approval of the
/// content that stands with its expiry repeated it whoever approved; another actor's
/// is now a second reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalKind {
    /// Of other content than the approval that stands, or with none standing.
    New,
    /// Of the content that stands, whose approval that stands has not lapsed and
    /// lapses when the new one would, by an actor who is not among its readers: a
    /// reading of its own, which changes nothing freshness reads.
    SecondReading {
        /// The approval that stands.
        standing: JudgementId,
    },
    /// Of the content that stands, whose approval that stands has not lapsed, by a
    /// reader whose latest reading and the approval that stands both lapse when the
    /// new one would: it adds nothing.
    Repeat {
        /// The approval that stands.
        standing: JudgementId,
    },
    /// Of the content that stands, by anyone once the approval that stands lapsed,
    /// when it lapses at another time than the new one would, or while a pin of it is
    /// read through a move (freshness reads them, so the new one approves the
    /// document again, moves when it lapses, or ends the move's reading), or by a
    /// reader asking for another expiry than their own latest reading has.
    Renewal,
}

/// How a run took the document it approves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taking {
    /// Named by the person: `judge approve <ref>`.
    Named,
    /// Chosen by a rule: `judge approve --stale` or `--pending`.
    Chosen,
}

/// Why a run makes no approval it was asked for, by its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveOut {
    /// It would repeat the reading of the one who approves.
    Repeats {
        /// The approval that stands.
        approval: JudgementId,
    },
    /// It would be a second reading, in a run that chose the document by a rule.
    SecondReading {
        /// The approval that stands.
        approval: JudgementId,
    },
}

impl ApprovalKind {
    /// Whether a run that took the document by `taking` leaves the approval out, and
    /// why. `again` is the request's `--again`, which only a named run takes: it
    /// writes a repeat anyway.
    ///
    /// | kind | named (`judge approve <ref>`) | chosen (`--stale`, `--pending`) |
    /// |---|---|---|
    /// | `Repeat` | left out as `repeats`, written with `--again` | left out as `repeats` |
    /// | `SecondReading` | written | left out as `second_reading` |
    /// | `New`, `Renewal` | written | written |
    ///
    /// A run that chooses its documents by a rule did not read them, so it writes no
    /// second reading: a second reading is said by naming the document. Neither kind
    /// left out holds back what stands on it: the approval it would have made stands.
    pub fn left_out(self, taking: Taking, again: bool) -> Option<LeaveOut> {
        match self {
            Self::Repeat { standing } if !(again && taking == Taking::Named) => {
                Some(LeaveOut::Repeats { approval: standing })
            }
            Self::SecondReading { standing } if taking == Taking::Chosen => {
                Some(LeaveOut::SecondReading { approval: standing })
            }
            Self::Repeat { .. } | Self::SecondReading { .. } | Self::New | Self::Renewal => None,
        }
    }
}

/// Whether the approval that stands about a document has as many different readers
/// as the rule that reads it asks for ([`StandingApproval::quorum`]).
///
/// It is a fact of its own beside the freshness of the approval, never one of its
/// causes: a level of degradation is the strongest of the causes, so a short quorum
/// among them would be hidden by any stronger one, and an approval that also lapsed
/// would let it through where an upstream that only expired holds nothing back. So
/// an approval is fresh or degraded as before, and short or met besides; how the two
/// are shown together is [`Standing::kind`](super::super::freshness::Standing::kind)'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quorum {
    /// Enough different names read the content (or no approval stands).
    Met,
    /// Fewer different names read the content than the rule asks for.
    Short {
        /// The names whose reading counts now, each once.
        readers: NonEmptySet<Actor>,
        /// How many the rule asks for.
        need: Approvals,
    },
}

impl Ledger {
    /// The readers of the content that stands about `subject`, reading the decisions
    /// `before` (the rule is on [`Readers`]): the one place they are read, by
    /// [`Ledger::kind_of`], by the approval that stands
    /// ([`StandingApproval::reading`]) and by whatever lists who read a document.
    pub fn readers(&self, subject: DocId, before: Before) -> Readers<'_> {
        self.walk(self.decisions_latest_first(subject, before))
    }

    /// The readers among `decisions`, the latest first: each a view of its record, up
    /// to a decision that is no approval or an approval of other content than the first.
    fn walk<'l>(&'l self, decisions: impl Iterator<Item = &'l Decision>) -> Readers<'l> {
        let mut approvals = Vec::new();
        let mut content: Option<StandsOn<'l>> = None;
        for decision in decisions {
            let Ok(view) = ApprovalView::of(self, decision) else {
                break;
            };
            let on = view.stands_on();
            if content.as_ref().is_some_and(|content| *content != on) {
                break;
            }
            content.get_or_insert(on);
            approvals.push(view);
        }
        Readers { approvals }
    }

    /// Whether the `read_by` of the readers of the `standing` approval are as many as
    /// the rule that reads it asks for. Only [`StandingApproval::reading`] asks it, so
    /// that the two are of the same readers and the same time: the name that stands in
    /// for every reading lapsed is here only, never in what `read_by` says.
    fn quorum_of(&self, standing: ApprovalView<'_>, read_by: &ReadBy) -> Quorum {
        if self.admit.is_unruled() {
            return Quorum::Met;
        }
        let need = self.need(standing.judgement(), standing.at());
        let mut counted = read_by.counted.iter();
        let names = match counted.next() {
            Some(first) => NonEmptySet::new(first.clone(), counted.cloned()),
            None => NonEmptySet::new(standing.actor().clone(), []),
        };
        if names.len() >= need.count() {
            Quorum::Met
        } else {
            Quorum::Short {
                readers: names,
                need,
            }
        }
    }

    /// What the approval `asked` would be beside the readers of its document, reading
    /// the decisions `before` (the table is on [`ApprovalKind`]): the one place a new
    /// approval is told apart. An expiry not after the time it is made is never a
    /// repeat: no approval can be made with it, and it is refused as one.
    pub fn kind_of(&self, asked: &Asked<'_>, before: Before) -> ApprovalKind {
        let readers = self.readers(asked.subject, before);
        let Some(standing) = readers.standing() else {
            return ApprovalKind::New;
        };
        if standing.stands_on() != StandsOn::of(asked.seen, asked.frame, asked.artifacts) {
            return ApprovalKind::New;
        }
        // Freshness reads the expiry of the approval that stands, not a reader's own:
        // once it lapsed, a new approval makes the document approved again, whoever
        // makes it; and one asked with another expiry moves when the document lapses.
        // Freshness reads a pin of it through a move the cut keeps, and a new approval
        // pins what it finds, read through none, so it would change what freshness
        // reads even where its pins are the same values.
        let standing_expires = standing.expires();
        if standing_expires.is_some_and(|e| e.lapsed_by(asked.at))
            || standing_expires != asked.until
            || standing.read_through_a_move(before)
        {
            return ApprovalKind::Renewal;
        }
        let standing = standing.judgement();
        match readers.latest_of(asked.by) {
            None => ApprovalKind::SecondReading { standing },
            Some(own) if own.expires() == asked.until => ApprovalKind::Repeat { standing },
            Some(_) => ApprovalKind::Renewal,
        }
    }
}

impl StandingApproval<'_> {
    /// Who reads the content of the approval that stands at `now`, and whether that is
    /// enough ([`StandingApproval::quorum`]), from one walk of its readers
    /// ([`Ledger::readers`]): the one place they are read for either.
    pub fn reading(&self, now: JudgedAt) -> Reading {
        let view = self.view();
        let readers = view.ledger.readers(view.subject(), Before::Now);
        let read_by = readers.at(now);
        let quorum = view.ledger.quorum_of(view, &read_by);
        Reading { read_by, quorum }
    }

    /// Whether the approval that stands has, at `now`, as many different readers
    /// ([`Ledger::readers`]) as the rule that reads it asks for (its need: the
    /// `approvals` of that rule; one when no rule reads it): the quorum of
    /// [`StandingApproval::reading`] alone, the same walk, for a caller that needs no
    /// readers.
    ///
    /// A name counts once, however many of its approvals are among the readers, and
    /// only while its own latest reading has not lapsed at `now`
    /// ([`JudgedAt::lapsed_by`]), whoever approved last: so once any reading lapses,
    /// an approval that needed it waits again. When every reading lapsed, the reader
    /// of the approval that stands is counted alone: the approval is expired, which
    /// freshness tells ([`Cause::Expired`](super::super::freshness::Cause::Expired)),
    /// and one that needs one name is never short. A project that declares no rule
    /// asks for one name of every approval, so its quorum is met whoever read it.
    pub fn quorum(&self, now: JudgedAt) -> Quorum {
        self.reading(now).quorum
    }
}
