//! Whether what was decided still holds: derived, never stored.
//!
//! [`assess`] is the one entry. It reads the shelf (what the documents are now) and
//! the ledger (what people decided) and returns an [`Assessment`] of every document.
//! Nothing it computes is written down: ask again and it is derived again, which is
//! why two worktrees that decided differently cannot leave a stale answer behind.
//!
//! # What it separates
//!
//! The ledger's [`DeclaredStatus`] says what people decided. A [`Standing`] says
//! where the document is now: only an approval has a [`Freshness`], and an approval
//! that no longer holds is [`Degraded`](Freshness::Degraded) with the [`Cause`]s that
//! say why, each a single fact. How strong a cause is is
//! [`Cause::severity`]'s table, and the level of a [`Degradation`] is the strongest
//! of its causes, computed from them ([`Degradation::level`]); no other place assigns
//! either, and [`Freshness::from_causes`] is the only maker of a degradation.
//!
//! # The graph it travels
//!
//! Staleness travels along the *frames* of approvals (what each approval pinned), in
//! the order of the ledger's graph of frames, upstream first. It does not travel along the
//! links the shelf holds now: an approval stands on what it saw, and a link rewritten
//! since is told by [`Cause::LinksChanged`], not followed.
//!
//! ```text
//!            frame edges (what each approval pinned)
//!
//!      A ◄──── B ◄──── C            A changed:   B is UpstreamChanged (Stale)
//!      │       │       │                          C is Via B         (Stale)
//!      │       └───────┘ D          roots_of(C):  [C, B] UpstreamChanged
//!      └──────────────► D
//!
//!      cycle of frames:  X ◄──► Y   X and Y are InCycle (Broken), and no Via
//!                                   passes between them
//! ```
//!
//! # Which causes a document has
//!
//! Only an approved document has any. If its own file is gone, invalid, or cannot be
//! told about, the cause is that alone (and an expiry) and the frame is not looked
//! at: the document cannot be trusted to have one. If it is valid, the rules are, in
//! the order the causes are listed:
//!
//! - its own hash differs from the approved one;
//! - for each pinned upstream, the edge rules: an upstream that is gone, invalid or
//!   unknowable is that fact alone; otherwise every rule that applies is kept (the
//!   upstream's hash changed, its declared status is not an approval, its own
//!   freshness is stale or broken), so a hash change and a broken upstream are both
//!   reported;
//! - for each `Depends` link name, the set of what it names now against what the
//!   frame pinned ([`Cause::LinksChanged`]), leaving out what an edge rule already
//!   said about the same upstream (matched by key, not by count);
//! - for each pinned artifact still written under its link: when its adapter cannot
//!   tell about it now, nothing (it is a [`Notice::ArtifactUnknown`], and the
//!   freshness is unchanged); when it is there then and now and the link declares
//!   another [`Comparison`] than the one the pin was made with,
//!   [`Cause::ArtifactComparisonChanged`] and no comparison of hashes; otherwise rule
//!   A3: an artifact whose hash differs is [`Cause::ArtifactChanged`], one that went
//!   is [`Cause::ArtifactGone`], one that came where there was none is
//!   [`Cause::ArtifactArrived`], and nothing then and nothing now is nothing; a link
//!   that writes a name a pin was moved away from is [`Cause::ArtifactMoved`]; for
//!   each `Depends` link to artifacts, the names it writes now against those pinned,
//!   leaving out what a move accounts for ([`Cause::ArtifactLinksChanged`]);
//! - a cycle the document is in, among links or among frames;
//! - an expiry: `now >= expires`.
//!
//! A hash that differs is a change unless people declared the edit keeps the
//! meaning ([`SameMeaning`]): when the ledger joins the pinned hash to the hash now
//! ([`Ledger::doc_same_meaning`], [`Ledger::artifact_same_meaning`]), the
//! `SelfChanged`, `UpstreamChanged` or `ArtifactChanged` is not a cause, and a
//! [`Notice`] names the declarations instead. The same three comparisons decide
//! both, so an edit is either a cause or a notice, never both.
//!
//! [`SameMeaning`]: super::ledger::SameMeaning
//!
//! A pinned artifact is read by the name it is read as ([`ReadPin::read_as`]): when
//! people declared it moved after it was pinned ([`Moved`]), the new name, with the
//! hash and state it pinned, so an unchanged move is no cause, a move with an edit is
//! an `ArtifactChanged` at the new name, and a link that still writes a name the move
//! left (the old name, or one a chain passed) is one fact, [`Cause::ArtifactMoved`],
//! and no `ArtifactLinksChanged`. The reading is the ledger's ([`Ledger::read_pins`]); nothing
//! here follows a move. An artifact cause carries the [`ReadPin`]: the pin as it was
//! made, and the name it is read as.
//!
//! [`Moved`]: super::ledger::Moved
//!
//! Freshness is decided by hashes alone (rule A4): the [`ContentAddress`] a pin may
//! carry and the [`Revision`] of a record are never read here, so a project outside
//! any version control has the same freshness as one inside.
//!
//! [`ContentAddress`]: super::artifact::ContentAddress
//! [`Revision`]: super::artifact::Revision
//!
//! Every fact is said once. Notices ([`Notice`]) are what is worth knowing and does
//! not change a freshness: they come from `collect_notices`'s table, and the edits a
//! declaration kept from being causes from the comparisons above.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::{NonEmptyVec, TwoOrMore};

use super::artifact::{ArtifactHash, ArtifactKey, ArtifactState, Comparison, Untold};
use super::build::{DocFailure, FailureTag, InvalidDoc, Shelf};
use super::cover::{Decided, Reached, counts, covering_reach};
use super::doc::{Doc, DocHash, LinkState, UnresolvedLink, UnresolvedWhy};
use super::exemption::{Unneeded, by_subject, read_exemption};
use super::graph::{Cycle, CycleId};
use super::ledger::{
    DeclaredStatus, Exempt, FramePin, JudgedAt, JudgementId, Ledger, LedgerProblem, ReadPin,
};
use super::schema::{ArtifactPropagation, BookName, CoverSeverity, EntryName, Propagation};

/// The documents that take the place of a deprecated one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Successors {
    /// None does.
    None,
    /// These do.
    Some(NonEmptyVec<DocId>),
}

impl Successors {
    /// The successors of `id`.
    ///
    /// The one place a successor is decided: a document that [`Shelf::replacers_of`]
    /// names, and that is pending or approved. A denied, retired or deprecated one is
    /// not counted, and the chain is not followed (a successor that is itself
    /// deprecated has none of its own counted here). It depends on declared statuses
    /// only, so no order of travel and no cycle can change it.
    pub fn of(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Self {
        let candidates: Vec<DocId> = shelf
            .replacers_of(id)
            .into_iter()
            .filter(|c| {
                matches!(
                    ledger.status_of(*c),
                    DeclaredStatus::Pending | DeclaredStatus::Approved { .. }
                )
            })
            .collect();
        match NonEmptyVec::from_vec(candidates) {
            Ok(successors) => Self::Some(successors),
            Err(_) => Self::None,
        }
    }
}

/// How strongly a cause degrades an approval: `Expired < Stale < Broken`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The approval lapsed by the clock only.
    Expired,
    /// What the approval stood on has changed or is not settled: look again.
    Stale,
    /// What the approval stood on is gone, refused, or cannot be trusted.
    Broken,
}

/// The declared status of an upstream, when it is not an approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamStatus {
    /// No decision stands about it.
    Pending,
    /// It was denied.
    Denied,
    /// It was retired.
    Retired,
    /// It was deprecated, and these take its place.
    Deprecated {
        /// The successors, if any.
        successors: Successors,
    },
}

/// One fact that makes an approval not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// The document is not what was approved.
    SelfChanged {
        /// The hash that was approved.
        approved: DocHash,
        /// The hash it has now.
        now: DocHash,
    },
    /// The document itself is no valid document now: its files are invalid (with
    /// every problem of every file with its identity), no file has it, or it cannot
    /// be told whether one has.
    SelfNotValid(DocFailure),
    /// A pinned upstream is no valid document now, in the same three ways.
    UpstreamNotValid {
        /// The pin.
        pin: FramePin,
        /// Why it is not valid.
        failure: DocFailure,
    },
    /// A pinned upstream is not what was pinned.
    UpstreamChanged {
        /// The pin.
        pin: FramePin,
        /// The hash it has now.
        now: DocHash,
    },
    /// A pinned upstream is not approved.
    UpstreamDeclared {
        /// The pin.
        pin: FramePin,
        /// What stands about it.
        status: UpstreamStatus,
    },
    /// A pinned upstream is itself stale or broken.
    Via {
        /// The pin.
        pin: FramePin,
        /// How strong the upstream's level is, which this takes.
        severity: Severity,
    },
    /// What a `Depends` link names now is not what the approval pinned for it.
    LinksChanged {
        /// The link.
        link: EntryName,
        /// What it names now that was not pinned.
        added: Vec<DocId>,
        /// What was pinned that it no longer names (leaving out an upstream another
        /// cause already says is gone, invalid or unknown).
        removed: Vec<DocId>,
        /// Keys it writes now that cannot be followed (leaving out those of an
        /// upstream another cause already says is gone, invalid or unknown).
        unresolved: Vec<UnresolvedLink>,
    },
    /// A pinned artifact is there and is not what was pinned (rule A3).
    ArtifactChanged {
        /// The pin, with its space and its name.
        pin: ReadPin,
        /// The hash it has now.
        now: ArtifactHash,
    },
    /// A pinned artifact was there when it was pinned and is not now (rule A3).
    ArtifactGone {
        /// The pin.
        pin: ReadPin,
    },
    /// A pinned artifact was not there when it was pinned and is now: the evidence
    /// came, so look again (rule A3).
    ArtifactArrived {
        /// The pin.
        pin: ReadPin,
        /// The hash it has now.
        now: ArtifactHash,
    },
    /// The link a pinned artifact was pinned for now declares another comparison: the
    /// hash pinned and the hash now were made differently and are not compared. Look
    /// again, as for a change.
    ArtifactComparisonChanged {
        /// The pin, with the comparison it was made with.
        pin: ReadPin,
        /// The comparison the link declares now.
        now: Comparison,
    },
    /// The link a pin was made under writes a name people declared the artifact
    /// moved away from since (the name pinned, or one a chain of moves passed), and
    /// not only the name the pin is read as now: the link is to write that name.
    ArtifactMoved {
        /// The link.
        link: EntryName,
        /// The name the link writes, which the pin's way left.
        written: ArtifactKey,
        /// The pin, with the name it is read as and the moves it is read through.
        pin: ReadPin,
    },
    /// What a `Depends` link to artifacts names now is not what the approval pinned
    /// for it: names written since, and names pinned that it no longer writes. A name
    /// a pin was moved away from is not counted here ([`Cause::ArtifactMoved`]).
    ArtifactLinksChanged {
        /// The link.
        link: EntryName,
        /// Artifacts it names now that were not pinned.
        added: Vec<ArtifactKey>,
        /// Artifacts that were pinned and that it no longer names.
        removed: Vec<ArtifactKey>,
    },
    /// The document is in a cycle.
    InCycle {
        /// The cycle, which says whether it is among links or among frames.
        cycle: CycleId,
    },
    /// The approval lapsed by the clock.
    Expired {
        /// When it was to lapse.
        since: JudgedAt,
    },
}

impl Cause {
    /// Where this cause starts, held by the document `holder`: the upstream a pin
    /// names, the artifact an artifact pin names, the declaration for a comparison
    /// changed, the cycle for a cycle, else the holder itself (its own text, links
    /// or expiry). A `Via` starts nowhere of its own (it leads to its upstream's
    /// causes). The one place an origin is assigned.
    pub fn origin(&self, holder: DocId) -> Option<Origin> {
        match self {
            Self::Via { .. } => None,
            Self::SelfChanged { .. }
            | Self::SelfNotValid(_)
            | Self::LinksChanged { .. }
            | Self::ArtifactLinksChanged { .. }
            | Self::ArtifactMoved { .. }
            | Self::Expired { .. } => Some(Origin::Doc(holder)),
            Self::UpstreamNotValid { pin, .. }
            | Self::UpstreamChanged { pin, .. }
            | Self::UpstreamDeclared { pin, .. } => Some(Origin::Doc(pin.target())),
            // The name the pin is read as: an edit after a move is of the new name,
            // whichever name each approval pinned.
            Self::ArtifactChanged { pin, .. }
            | Self::ArtifactGone { pin }
            | Self::ArtifactArrived { pin, .. } => Some(Origin::Artifact(pin.read_as().clone())),
            Self::ArtifactComparisonChanged { .. } => Some(Origin::Declaration),
            Self::InCycle { cycle } => Some(Origin::Cycle(*cycle)),
        }
    }

    /// How strongly this cause degrades an approval. The one place a strength is
    /// assigned.
    ///
    /// | cause | strength |
    /// |---|---|
    /// | `SelfChanged`, `UpstreamChanged` | `Stale` |
    /// | `SelfNotValid` (invalid, gone or unknown) | `Broken` |
    /// | `UpstreamNotValid` (invalid, gone or unknown) | `Broken` |
    /// | `UpstreamDeclared` of `Pending`, or of `Deprecated` with successors | `Stale` |
    /// | `UpstreamDeclared` of `Denied`, `Retired`, or of `Deprecated` with none | `Broken` |
    /// | `Via` | the strength it carries (`Stale` or `Broken`; an upstream that is only `Expired` makes no `Via`) |
    /// | `LinksChanged` | `Broken` when some key cannot be followed, otherwise `Stale` |
    /// | `ArtifactChanged`, `ArtifactArrived`, `ArtifactComparisonChanged`, `ArtifactLinksChanged`, `ArtifactMoved` | `Stale` |
    /// | `ArtifactGone` | `Broken` |
    /// | `InCycle` | `Broken` |
    /// | `Expired` | `Expired` |
    ///
    /// A pinned artifact that was not there and still is not is no cause at all
    /// (rule A3), nor is one its adapter cannot tell about now (a notice), so they
    /// have no row.
    pub fn severity(&self) -> Severity {
        match self {
            Self::SelfChanged { .. }
            | Self::UpstreamChanged { .. }
            | Self::ArtifactChanged { .. }
            | Self::ArtifactArrived { .. }
            | Self::ArtifactComparisonChanged { .. }
            | Self::ArtifactLinksChanged { .. }
            | Self::ArtifactMoved { .. } => Severity::Stale,
            Self::SelfNotValid(_) | Self::UpstreamNotValid { .. } | Self::ArtifactGone { .. } => {
                Severity::Broken
            }
            Self::UpstreamDeclared { status, .. } => match status {
                UpstreamStatus::Pending
                | UpstreamStatus::Deprecated {
                    successors: Successors::Some(_),
                } => Severity::Stale,
                UpstreamStatus::Denied
                | UpstreamStatus::Retired
                | UpstreamStatus::Deprecated {
                    successors: Successors::None,
                } => Severity::Broken,
            },
            Self::Via { severity, .. } => *severity,
            Self::LinksChanged { unresolved, .. } => {
                if unresolved.is_empty() {
                    Severity::Stale
                } else {
                    Severity::Broken
                }
            }
            Self::InCycle { .. } => Severity::Broken,
            Self::Expired { .. } => Severity::Expired,
        }
    }
}

/// Why an approval does not hold: one or more causes, and the level they come to.
///
/// The causes are private and the level is computed from them, so a degradation
/// cannot claim a level its causes do not have. [`Freshness::from_causes`] is the
/// only way to make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    causes: NonEmptyVec<Cause>,
}

impl Degradation {
    /// The strongest [`Cause::severity`] among the causes.
    pub fn level(&self) -> Severity {
        let first = self.causes.first().severity();
        self.causes
            .iter()
            .map(Cause::severity)
            .fold(first, Severity::max)
    }

    /// The causes, in the order they were found.
    pub fn causes(&self) -> &NonEmptyVec<Cause> {
        &self.causes
    }
}

/// Whether an approval holds.
// `Degraded` holds its first `Cause` inline (the enum is as large as that), and `Fresh`
// holds nothing. There is one `Freshness` per approved document, kept in a map by
// `assess`: the cost is the size of the largest variant once per document, and boxing
// the causes would trade it for an allocation in each degraded approval and a deref at
// each access to its causes.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// It holds.
    Fresh,
    /// It does not, for these causes, at the level of the strongest.
    Degraded(Degradation),
}

impl Freshness {
    /// The freshness of an approval with these causes: `Fresh` when there are none,
    /// otherwise degraded at the strongest of them.
    ///
    /// The only place a [`Degradation`] is made.
    pub fn from_causes(causes: Vec<Cause>) -> Self {
        match NonEmptyVec::from_vec(causes) {
            Ok(causes) => Self::Degraded(Degradation { causes }),
            Err(_) => Self::Fresh,
        }
    }

    /// The level, with the causes left out.
    pub fn level(&self) -> FreshnessLevel {
        match self {
            Self::Fresh => FreshnessLevel::Fresh,
            Self::Degraded(degradation) => match degradation.level() {
                Severity::Expired => FreshnessLevel::Expired,
                Severity::Stale => FreshnessLevel::Stale,
                Severity::Broken => FreshnessLevel::Broken,
            },
        }
    }
}

/// A [`Freshness`] without its causes, for a display and for a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessLevel {
    /// It holds.
    Fresh,
    /// It lapsed by the clock only.
    Expired,
    /// What it stood on has changed or is not settled.
    Stale,
    /// What it stood on is gone, refused or cannot be trusted.
    Broken,
}

/// Where a document stands: what was decided, and for an approval whether it holds.
// As for `Freshness`, which `Approved` holds inline: one `Standing` per document, and
// boxing the approval would add an allocation to each one.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// No decision stands.
    Pending,
    /// Denied.
    Denied,
    /// Deprecated, with its successors.
    Deprecated {
        /// The documents that take its place.
        successors: Successors,
    },
    /// Retired.
    Retired,
    /// Approved, and whether it holds.
    Approved(Freshness),
}

impl Standing {
    /// Whether no decision went against it: pending, or approved at any freshness.
    /// Denied, deprecated and retired are decided against. It is
    /// [`counts`] of the standing, the one statement of it.
    pub fn stands(&self) -> bool {
        counts(Decided::from(self))
    }

    /// The standing without the causes and the successors.
    pub fn kind(&self) -> StandingKind {
        match self {
            Self::Pending => StandingKind::Pending,
            Self::Denied => StandingKind::Denied,
            Self::Deprecated { .. } => StandingKind::Deprecated,
            Self::Retired => StandingKind::Retired,
            Self::Approved(freshness) => StandingKind::Approved(freshness.level()),
        }
    }
}

impl From<&Standing> for Decided {
    fn from(standing: &Standing) -> Self {
        match standing {
            Standing::Pending => Self::Pending,
            Standing::Approved(_) => Self::Approved,
            Standing::Denied => Self::Denied,
            Standing::Deprecated { .. } => Self::Deprecated,
            Standing::Retired => Self::Retired,
        }
    }
}

/// A [`Standing`] without its causes, for a display and for a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandingKind {
    /// No decision stands.
    Pending,
    /// Denied.
    Denied,
    /// Deprecated.
    Deprecated,
    /// Retired.
    Retired,
    /// Approved, at this level.
    Approved(FreshnessLevel),
}

/// Something worth knowing about a document that changes no freshness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A document it informs of is not an approval that holds.
    InformsNotFresh {
        /// The link.
        link: EntryName,
        /// The document.
        target: DocId,
        /// Where it stands.
        standing: StandingKind,
    },
    /// An `Informs` link cannot be followed.
    InformsUnresolved {
        /// The link.
        link: EntryName,
        /// What was written and why it fails.
        unresolved: UnresolvedLink,
    },
    /// A pinned upstream's approval has lapsed by the clock.
    UpstreamExpired {
        /// The pin.
        pin: FramePin,
        /// When it was to lapse.
        since: JudgedAt,
    },
    /// A link of a book declares that it covers this document's book
    /// ([`LinkKind::cover`](super::schema::LinkKind::cover)), and no pending or
    /// approved document of that book reaches this one through it: a hole in the
    /// chain of reasons. The document's freshness is unchanged, whatever the
    /// strength. (A link of a book into itself is met only by another document of
    /// that book.) The notices of one document come in the order the declaration
    /// writes the books, and their links.
    NotCovered {
        /// The book whose link is expected to reach this document.
        book: BookName,
        /// The link.
        link: EntryName,
        /// How strongly the link expects to reach it.
        severity: CoverSeverity,
    },
    /// A covering link does not reach this document, and people declared it is not
    /// expected to ([`Exempt`]): no [`Notice::NotCovered`] for that link, whatever
    /// its strength. Given in the place the `NotCovered` would be.
    Exempt {
        /// The book whose link would be expected to reach it.
        book: BookName,
        /// The link.
        link: EntryName,
        /// How strongly the link expects to reach the documents of its book.
        severity: CoverSeverity,
        /// The exemption.
        exemption: JudgementId,
    },
    /// An exemption of this document is no longer needed ([`Unneeded`] says why):
    /// the link reaches it now, or no longer expects to. As Rust's `#[expect]` tells
    /// an expectation no longer met; withdraw the exemption.
    ExemptionUnneeded {
        /// The book the exemption names.
        book: BookName,
        /// The link it names.
        link: EntryName,
        /// The exemption.
        exemption: JudgementId,
        /// Why it is not needed.
        why: Unneeded,
    },
    /// Two or more exemptions that stand are of this document from the same link,
    /// as branches merged can leave them (`judge exempt` refuses a second): one is
    /// enough, so withdraw the others. Each is read and told as well.
    ExemptionRepeated {
        /// The book the exemptions name.
        book: BookName,
        /// The link they name.
        link: EntryName,
        /// The exemptions, in the order of identity.
        exemptions: TwoOrMore<JudgementId>,
    },
    /// A name a link to artifacts writes has nothing there now
    /// ([`Doc::absent_artifacts`]): the evidence is not written yet, or the name is
    /// wrong.
    ArtifactAbsent {
        /// The link.
        link: EntryName,
        /// The space and the name as written.
        artifact: ArtifactKey,
    },
    /// The adapter of the space cannot tell about a name a link to artifacts writes
    /// now ([`Doc::unknown_artifacts`]). Pinned or not, it changes no freshness; an
    /// approval cannot be made on it while it lasts.
    ArtifactUnknown {
        /// The link.
        link: EntryName,
        /// The space and the name as written.
        artifact: ArtifactKey,
        /// Why it cannot be told.
        why: Untold,
    },
    /// The document is not what was approved, and people declared that the edit
    /// keeps the meaning: no [`Cause::SelfChanged`].
    SelfSameMeaning {
        /// The hash that was approved.
        approved: DocHash,
        /// The hash it has now.
        now: DocHash,
        /// The declarations that join the two, in order.
        declared: NonEmptyVec<JudgementId>,
    },
    /// A pinned upstream is not what was pinned, and people declared that the edit
    /// keeps the meaning: no [`Cause::UpstreamChanged`].
    UpstreamSameMeaning {
        /// The pin.
        pin: FramePin,
        /// The hash it has now.
        now: DocHash,
        /// The declarations that join the two, in order.
        declared: NonEmptyVec<JudgementId>,
    },
    /// A pinned artifact is not what was pinned, and people declared that the edit
    /// keeps the meaning: no [`Cause::ArtifactChanged`].
    ArtifactSameMeaning {
        /// The pin.
        pin: ReadPin,
        /// The hash it has now.
        now: ArtifactHash,
        /// The declarations that join the two, in order.
        declared: NonEmptyVec<JudgementId>,
    },
}

/// The root of a degradation: where it starts, and the way from the document asked
/// about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootCause {
    /// The documents from the one asked about to the one the cause belongs to.
    pub path: Vec<DocId>,
    /// The cause, which is not a `Via`.
    pub cause: Cause,
}

/// Where an event that degrades approvals starts. It names the thing, not what
/// happened to it, so every approval one edit reaches is under one origin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// A document: edited, invalid or gone, declared about, or holding a link or an
    /// expiry of its own.
    Doc(DocId),
    /// An artifact a document pins, by its space and its name: edited, gone or
    /// arrived.
    Artifact(ArtifactKey),
    /// The declaration of the books: a link to artifacts that now declares another
    /// comparison than a pin was made with.
    Declaration,
    /// A cycle, one origin for all its members.
    Cycle(CycleId),
}

/// One origin, with the approvals it degrades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginGroup {
    /// Where it starts.
    pub origin: Origin,
    /// The degraded approvals one of whose roots starts there.
    pub degraded: BTreeSet<DocId>,
}

/// The standing of every document, and what is wrong around them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    standings: BTreeMap<DocId, Standing>,
    notices: BTreeMap<DocId, Vec<Notice>>,
    cycles: Vec<Cycle>,
    invalid: Vec<InvalidDoc>,
    ledger_problems: Vec<LedgerProblem>,
}

impl Assessment {
    /// The standing of every document the ledger or the shelf knows: the ledger's
    /// subjects, and the identities of the shelf's valid and readable invalid files.
    pub fn standings(&self) -> &BTreeMap<DocId, Standing> {
        &self.standings
    }

    /// Where one identity stands: the one answer for any identity, listed or not.
    ///
    /// The assessment lists every identity the ledger or the shelf knows. One it does
    /// not list has no file and no record, so no one decided anything about it: it is
    /// [`Standing::Pending`], as [`Ledger::status_of`] says of an identity with no
    /// decision.
    pub fn standing_of(&self, id: DocId) -> Standing {
        self.standings
            .get(&id)
            .cloned()
            .unwrap_or(Standing::Pending)
    }

    /// The notices about one identity; none when it has none or is not listed.
    pub fn notices_of(&self, id: DocId) -> &[Notice] {
        self.notices.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Every identity that has notices, with them, by identity. Among them may be an
    /// identity [`Assessment::standings`] does not list: one an exemption names that
    /// no file and no decision has, whose exemption is then unneeded.
    pub fn noticed(&self) -> impl Iterator<Item = (DocId, &[Notice])> {
        self.notices.iter().map(|(id, n)| (*id, n.as_slice()))
    }

    /// The cycles among links and among frames.
    pub fn cycles(&self) -> &[Cycle] {
        &self.cycles
    }

    /// Every invalid file, whether or not its identity could be read.
    pub fn invalid(&self) -> &[InvalidDoc] {
        &self.invalid
    }

    /// The contradictions the ledger set aside.
    pub fn ledger_problems(&self) -> &[LedgerProblem] {
        &self.ledger_problems
    }

    /// Where the degradation of `id` starts.
    ///
    /// Follows each `Via` to the upstream it came from, until a cause that is not a
    /// `Via`, and returns the way and that cause. Every cause of the document asked
    /// about is a root. Of an upstream only a cause that is `Stale` or stronger is
    /// (a `Via` is made only when the upstream is, so an expiry that does not
    /// propagate is not a root). A document is visited once, so a diamond of
    /// upstreams returns its root once and a large graph cannot make a large answer.
    ///
    /// One event is one root. An `UpstreamChanged` is the edit of the upstream seen
    /// from below; when the upstream's own causes hold the same edit (a `SelfChanged`
    /// from the hash this approval pinned to the hash it has now), the `Via` beside it
    /// leads to that edit and the `UpstreamChanged` is not a root of its own. When the
    /// upstream was approved again after the edit, it holds no such `SelfChanged`, and
    /// the edit is a root: the approval below saw the older text. Nor is there a
    /// `Via` across a cycle both are in, so there too the `UpstreamChanged` is a root.
    /// When two ways up reach one event (the same edit, from the same hash to the
    /// same hash, or the same cycle), it is named once.
    pub fn roots_of(&self, id: DocId) -> Vec<RootCause> {
        let mut roots = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_roots(id, vec![id], true, &mut visited, &mut roots);
        // Two ways up can reach one event: a member of a cycle that sees an edit of
        // another member from below, beside the edit reached directly; two members
        // of one cycle, each in it. Of an edit the upstream's own `SelfChanged` is
        // kept; of a cycle, the first found.
        let edits: BTreeSet<(DocId, DocHash, DocHash)> = roots
            .iter()
            .filter_map(|root| match &root.cause {
                Cause::SelfChanged { approved, now } => {
                    root.path.last().map(|holder| (*holder, *approved, *now))
                }
                _ => None,
            })
            .collect();
        let mut cycles = BTreeSet::new();
        roots.retain(|root| match &root.cause {
            Cause::UpstreamChanged { pin, now } => {
                !edits.contains(&(pin.target(), *pin.hash(), *now))
            }
            Cause::InCycle { cycle } => cycles.insert(*cycle),
            _ => true,
        });
        roots
    }

    /// Where the degradations of the approvals start, each origin with every
    /// degraded approval one of whose roots ([`Assessment::roots_of`]) starts there;
    /// the origin that degrades the most first, then by origin. An approval with
    /// roots at two origins is under both.
    ///
    /// The one place roots are grouped across documents: an edit of one file that
    /// several approvals pin, or of one document seen from several below, is one
    /// origin, read once.
    pub fn origins(&self) -> Vec<OriginGroup> {
        let mut by_origin: BTreeMap<Origin, BTreeSet<DocId>> = BTreeMap::new();
        for (id, standing) in &self.standings {
            if !matches!(standing, Standing::Approved(Freshness::Degraded(_))) {
                continue;
            }
            for root in self.roots_of(*id) {
                // A root's path runs from the document asked about to the one that
                // holds the cause, and is never empty.
                let Some(&holder) = root.path.last() else {
                    continue;
                };
                if let Some(origin) = root.cause.origin(holder) {
                    by_origin.entry(origin).or_default().insert(*id);
                }
            }
        }
        let mut groups: Vec<OriginGroup> = by_origin
            .into_iter()
            .map(|(origin, degraded)| OriginGroup { origin, degraded })
            .collect();
        groups.sort_by(|a, b| {
            b.degraded
                .len()
                .cmp(&a.degraded.len())
                .then_with(|| a.origin.cmp(&b.origin))
        });
        groups
    }

    fn collect_roots(
        &self,
        id: DocId,
        path: Vec<DocId>,
        is_start: bool,
        visited: &mut BTreeSet<DocId>,
        roots: &mut Vec<RootCause>,
    ) {
        if !visited.insert(id) {
            return;
        }
        let Some(Standing::Approved(Freshness::Degraded(degradation))) = self.standings.get(&id)
        else {
            return;
        };
        for cause in degradation.causes().iter() {
            if let Cause::Via { pin, .. } = cause {
                let mut onward = path.clone();
                onward.push(pin.target());
                self.collect_roots(pin.target(), onward, false, visited, roots);
            } else if let Cause::UpstreamChanged { pin, now } = cause
                && self.upstream_holds_edit(pin, now)
                && degradation.causes().iter().any(
                    |other| matches!(other, Cause::Via { pin: via, .. } if via.target() == pin.target()),
                )
            {
                // The same edit is a root further up, through the `Via` beside it.
                // Inside a cycle no `Via` passes, and the change is a root here.
            } else if is_start || cause.severity() >= Severity::Stale {
                roots.push(RootCause {
                    path: path.clone(),
                    cause: cause.clone(),
                });
            }
        }
    }

    /// Whether the upstream `pin` names counts, among its own causes, the edit from
    /// the pinned hash to `now`.
    fn upstream_holds_edit(&self, pin: &FramePin, now: &DocHash) -> bool {
        let Some(Standing::Approved(Freshness::Degraded(degradation))) =
            self.standings.get(&pin.target())
        else {
            return false;
        };
        degradation.causes().iter().any(|cause| {
            matches!(cause, Cause::SelfChanged { approved, now: then }
                if approved == pin.hash() && then == now)
        })
    }
}

/// What `assess` reads while it works.
struct Cx<'a> {
    shelf: &'a Shelf,
    ledger: &'a Ledger,
    now: JudgedAt,
    in_cycle: BTreeMap<DocId, Vec<CycleId>>,
    component: BTreeMap<DocId, usize>,
}

/// The standings, and the declared statuses they were made from, of every document.
struct Standings {
    standings: BTreeMap<DocId, Standing>,
    /// What each was read as, once: a notice asks it again of nobody.
    statuses: BTreeMap<DocId, DeclaredStatus>,
    /// The edits a declaration kept from being a cause, found while the causes were:
    /// told as notices, so each comparison is made once.
    same_meaning: BTreeMap<DocId, Vec<Notice>>,
}

/// The standing of every document, from the shelf and the ledger, at `now`.
///
/// The one entry of freshness: nothing is stored, so it is computed every time. See
/// the module page for the graph it travels and the causes it finds.
pub fn assess(shelf: &Shelf, ledger: &Ledger, now: JudgedAt) -> Assessment {
    let graph = ledger.frame_graph();
    let mut cycles: Vec<Cycle> = shelf.cycles().to_vec();
    cycles.extend(graph.cycles());
    let components = graph.components();
    let cx = Cx {
        shelf,
        ledger,
        now,
        in_cycle: cycle_ids_by_member(&cycles),
        component: component_of_each(&components),
    };

    let order = travel_order(shelf, ledger, components);
    let Standings {
        standings,
        statuses,
        same_meaning,
    } = stand_in_order(&cx, order);
    let notices = collect_notices(&cx, &standings, &statuses, same_meaning);
    Assessment {
        standings,
        notices,
        cycles,
        invalid: invalid_files(shelf),
        ledger_problems: ledger.problems().to_vec(),
    }
}

/// The cycles each document is a member of.
fn cycle_ids_by_member(cycles: &[Cycle]) -> BTreeMap<DocId, Vec<CycleId>> {
    let mut in_cycle: BTreeMap<DocId, Vec<CycleId>> = BTreeMap::new();
    for cycle in cycles {
        for member in cycle.ids() {
            in_cycle.entry(member).or_default().push(cycle.id());
        }
    }
    in_cycle
}

/// The place in the components of the frame graph of each document that is in it.
fn component_of_each(components: &[Vec<DocId>]) -> BTreeMap<DocId, usize> {
    components
        .iter()
        .enumerate()
        .flat_map(|(i, members)| members.iter().map(move |id| (*id, i)))
        .collect()
}

/// The order the documents are judged in: those of the frame graph upstream first,
/// then every other identity the ledger or the shelf knows.
fn travel_order(shelf: &Shelf, ledger: &Ledger, components: Vec<Vec<DocId>>) -> Vec<DocId> {
    let ids: BTreeSet<DocId> = ledger
        .subjects()
        .iter()
        .copied()
        .chain(shelf.ids())
        .collect();
    let mut order: Vec<DocId> = components
        .into_iter()
        .flatten()
        .filter(|id| ids.contains(id))
        .collect();
    let ordered: BTreeSet<DocId> = order.iter().copied().collect();
    order.extend(ids.iter().filter(|id| !ordered.contains(*id)).copied());
    order
}

/// The standing of each document in `order`, an approval after what it stands on.
fn stand_in_order(cx: &Cx<'_>, order: Vec<DocId>) -> Standings {
    let ledger = cx.ledger;
    let mut standings: BTreeMap<DocId, Standing> = BTreeMap::new();
    let mut statuses = BTreeMap::new();
    let mut same_meaning = BTreeMap::new();
    for id in order {
        let status = ledger.status_of(id);
        let mut kept = Vec::new();
        let standing = match &status {
            DeclaredStatus::Pending => Standing::Pending,
            DeclaredStatus::Denied { .. } => Standing::Denied,
            DeclaredStatus::Deprecated { .. } => Standing::Deprecated {
                successors: Successors::of(cx.shelf, ledger, id),
            },
            DeclaredStatus::Retired { .. } => Standing::Retired,
            DeclaredStatus::Approved {
                seen,
                frame,
                expires,
                ..
            } => Standing::Approved(freshness_of(
                cx,
                id,
                Pinned {
                    seen,
                    frame,
                    // The artifacts as the ledger reads them now, through moves.
                    artifacts: &ledger.read_pins(id),
                },
                *expires,
                &standings,
                &mut kept,
            )),
        };
        standings.insert(id, standing);
        statuses.insert(id, status);
        if !kept.is_empty() {
            same_meaning.insert(id, kept);
        }
    }
    Standings {
        standings,
        statuses,
        same_meaning,
    }
}

/// Every invalid file, whether or not its identity could be read.
fn invalid_files(shelf: &Shelf) -> Vec<InvalidDoc> {
    shelf
        .invalid()
        .values()
        .flat_map(|docs| docs.iter().cloned())
        .chain(shelf.unreadable().iter().cloned())
        .chain(shelf.anonymous().iter().cloned())
        .collect()
}

/// What an approval pinned: the hash it saw, the upstreams and the artifacts.
struct Pinned<'a> {
    seen: &'a DocHash,
    frame: &'a BTreeSet<FramePin>,
    artifacts: &'a [ReadPin],
}

fn freshness_of(
    cx: &Cx<'_>,
    id: DocId,
    pinned: Pinned<'_>,
    expires: Option<JudgedAt>,
    standings: &BTreeMap<DocId, Standing>,
    kept: &mut Vec<Notice>,
) -> Freshness {
    let mut causes = Vec::new();
    match cx.shelf.state_of(id).valid_or_failure() {
        Ok(doc) => {
            let found = Found {
                causes: &mut causes,
                kept,
            };
            valid_causes(cx, doc, pinned.seen, pinned.frame, standings, found);
            let found = Found {
                causes: &mut causes,
                kept,
            };
            artifact_causes(cx.ledger, doc, pinned.artifacts, found);
            if let Some(ids) = cx.in_cycle.get(&doc.id()) {
                causes.extend(ids.iter().map(|cycle| Cause::InCycle { cycle: *cycle }));
            }
        }
        Err(failure) => causes.push(Cause::SelfNotValid(failure)),
    }
    if let Some(since) = expires
        && cx.now >= since
    {
        causes.push(Cause::Expired { since });
    }
    Freshness::from_causes(causes)
}

/// Where the facts about one approval go as they are found: a cause, or an edit a
/// declaration kept from being one.
struct Found<'f> {
    causes: &'f mut Vec<Cause>,
    kept: &'f mut Vec<Notice>,
}

fn valid_causes(
    cx: &Cx<'_>,
    doc: &Doc,
    seen: &DocHash,
    frame: &BTreeSet<FramePin>,
    standings: &BTreeMap<DocId, Standing>,
    found: Found<'_>,
) {
    let Found { causes, kept } = found;
    if doc.hash() != seen {
        match cx.ledger.doc_same_meaning(doc.id(), seen, doc.hash()) {
            Some(declared) => kept.push(Notice::SelfSameMeaning {
                approved: *seen,
                now: *doc.hash(),
                declared,
            }),
            None => causes.push(Cause::SelfChanged {
                approved: *seen,
                now: *doc.hash(),
            }),
        }
    }

    let mut lost: Vec<(&FramePin, FailureTag)> = Vec::new();
    for pin in frame {
        let found = Found {
            causes: &mut *causes,
            kept: &mut *kept,
        };
        pin_causes(cx, doc, pin, standings, found, &mut lost);
    }

    causes.extend(link_changes(doc, frame, &lost));
}

/// The causes the pinned artifacts of a valid document add (rule A3), and the change
/// of what its `Depends` links to artifacts name.
///
/// Only the [`ArtifactState`] is compared: an address in a pin is never read (A4).
/// A pin is taken by the name it is read as ([`ReadPin::read_as`]): the name it
/// wrote, or the one a declared move took it to, with the hash and state it pinned.
/// An artifact still named under the link it was pinned for is compared with what
/// is there now, unless its adapter cannot tell now (no cause: the notice
/// [`Notice::ArtifactUnknown`] says so). A link that writes a name the pin's way left
/// ([`ReadPin::left_behind`]: the name pinned, or one a chain of moves passed) is one
/// fact, [`Cause::ArtifactMoved`]. What is left, a pinned artifact the link names in
/// no way, and a name the link writes that no pin accounts for, is an
/// [`Cause::ArtifactLinksChanged`] of that link.
fn artifact_causes(ledger: &Ledger, doc: &Doc, pinned: &[ReadPin], found: Found<'_>) {
    let Found { causes: out, kept } = found;
    let current: BTreeMap<(&EntryName, &ArtifactKey), &ArtifactState> = doc
        .artifact_links_of(ArtifactPropagation::Depends)
        .into_iter()
        .flat_map(|(link, artifacts)| {
            artifacts
                .iter()
                .map(move |artifact| ((link, artifact.key()), artifact.state()))
        })
        .collect();

    let mut causes = Vec::new();
    // Each pin with the names its way left that its link still writes.
    let mut behind_of: Vec<(&ReadPin, Vec<&ArtifactKey>)> = Vec::new();
    for read in pinned {
        let pin = read.pin();
        let behind: Vec<&ArtifactKey> = read
            .left_behind()
            .filter(|name| current.contains_key(&(pin.link(), *name)))
            .collect();
        causes.extend(behind.iter().map(|written| Cause::ArtifactMoved {
            link: pin.link().clone(),
            written: (*written).clone(),
            pin: read.clone(),
        }));
        behind_of.push((read, behind));
        let Some(now) = current.get(&(pin.link(), read.read_as())) else {
            continue;
        };
        let now = match now {
            ArtifactState::Unknown(_) => continue,
            ArtifactState::Present(hash) => ArtifactState::Present(*hash),
            ArtifactState::Absent => ArtifactState::Absent,
        };
        // Two hashes are compared only when they were made the same way: when an
        // artifact is there then and now and the link's declared comparison changed
        // since, that is the cause, and the hashes are not compared. An artifact gone,
        // or come, is told as such whatever the comparison: there is no pair of hashes.
        let declared = doc
            .artifact_links()
            .get(pin.link())
            .map(|link| &link.kind().comparison);
        let changed_comparison = declared.filter(|d| *d != pin.comparison());
        match (pin.seen().state(), now) {
            (ArtifactState::Present(_), ArtifactState::Present(_))
                if changed_comparison.is_some() =>
            {
                if let Some(declared) = changed_comparison {
                    causes.push(Cause::ArtifactComparisonChanged {
                        pin: read.clone(),
                        now: declared.clone(),
                    });
                }
            }
            (ArtifactState::Present(then), ArtifactState::Present(now)) if then != now => {
                match ledger.pin_same_meaning(read, &then, &now) {
                    Some(declared) => kept.push(Notice::ArtifactSameMeaning {
                        pin: read.clone(),
                        now,
                        declared,
                    }),
                    None => causes.push(Cause::ArtifactChanged {
                        pin: read.clone(),
                        now,
                    }),
                }
            }
            (ArtifactState::Present(_), ArtifactState::Absent) => {
                causes.push(Cause::ArtifactGone { pin: read.clone() });
            }
            (ArtifactState::Absent, ArtifactState::Present(now)) => {
                causes.push(Cause::ArtifactArrived {
                    pin: read.clone(),
                    now,
                });
            }
            // A pin is never of an unknown artifact (an approval on one is refused),
            // and one unknown now was passed over above.
            (ArtifactState::Present(_), ArtifactState::Present(_))
            | (ArtifactState::Absent, ArtifactState::Absent)
            | (ArtifactState::Unknown(_), _)
            | (_, ArtifactState::Unknown(_)) => {}
        }
    }

    let mut names: BTreeSet<&EntryName> = pinned.iter().map(|r| r.pin().link()).collect();
    names.extend(current.keys().map(|(link, _)| *link));
    for name in names {
        let is: BTreeSet<&ArtifactKey> = current
            .keys()
            .filter(|(link, _)| *link == name)
            .map(|(_, artifact)| *artifact)
            .collect();
        // What each pin of the link accounts for of what it writes now: the name it
        // is read as, and the names behind it (each an `ArtifactMoved` already). A pin
        // the link writes in no way is removed, as the name the link wrote then (the
        // name pinned), which is written now by no way of it.
        let mut accounted: BTreeSet<&ArtifactKey> = BTreeSet::new();
        let mut gone: BTreeSet<&ArtifactKey> = BTreeSet::new();
        for (read, behind) in behind_of.iter().filter(|(r, _)| r.pin().link() == name) {
            if is.contains(read.read_as()) {
                accounted.insert(read.read_as());
            } else if behind.is_empty() {
                gone.insert(read.pin().artifact());
            }
            accounted.extend(behind.iter().copied());
        }
        let added: Vec<ArtifactKey> = is.difference(&accounted).map(|a| (*a).clone()).collect();
        let removed: Vec<ArtifactKey> = gone.into_iter().cloned().collect();
        if !added.is_empty() || !removed.is_empty() {
            causes.push(Cause::ArtifactLinksChanged {
                link: name.clone(),
                added,
                removed,
            });
        }
    }
    out.extend(causes);
}

/// The causes one pin of the frame of a valid document adds: its upstream is no
/// valid document (then that is the cause alone, and the pin is `lost`), or every
/// rule that applies to the upstream that is one.
fn pin_causes<'p>(
    cx: &Cx<'_>,
    doc: &Doc,
    pin: &'p FramePin,
    standings: &BTreeMap<DocId, Standing>,
    found: Found<'_>,
    lost: &mut Vec<(&'p FramePin, FailureTag)>,
) {
    let Found { causes, kept } = found;
    let target = pin.target();
    match cx.shelf.state_of(target).valid_or_failure() {
        Err(failure) => {
            lost.push((pin, failure.tag()));
            causes.push(Cause::UpstreamNotValid {
                pin: pin.clone(),
                failure,
            });
        }
        Ok(upstream) => {
            if upstream.hash() != pin.hash() {
                match cx
                    .ledger
                    .doc_same_meaning(target, pin.hash(), upstream.hash())
                {
                    Some(declared) => kept.push(Notice::UpstreamSameMeaning {
                        pin: pin.clone(),
                        now: *upstream.hash(),
                        declared,
                    }),
                    None => causes.push(Cause::UpstreamChanged {
                        pin: pin.clone(),
                        now: *upstream.hash(),
                    }),
                }
            }
            let same_cycle = cx
                .component
                .get(&target)
                .is_some_and(|c| cx.component.get(&doc.id()) == Some(c));
            causes.extend(standing_cause(pin, standings.get(&target), same_cycle));
        }
    }
}

/// What the standing of a pinned upstream makes of an approval: its declared status
/// when that is not an approval, or a `Via` when the approval is itself stale or
/// broken (not across a cycle both are in, where no `Via` passes).
fn standing_cause(pin: &FramePin, upstream: Option<&Standing>, same_cycle: bool) -> Option<Cause> {
    let declared = |status| Cause::UpstreamDeclared {
        pin: pin.clone(),
        status,
    };
    match upstream {
        Some(Standing::Pending) => Some(declared(UpstreamStatus::Pending)),
        Some(Standing::Denied) => Some(declared(UpstreamStatus::Denied)),
        Some(Standing::Retired) => Some(declared(UpstreamStatus::Retired)),
        Some(Standing::Deprecated { successors }) => Some(declared(UpstreamStatus::Deprecated {
            successors: successors.clone(),
        })),
        Some(Standing::Approved(Freshness::Degraded(degradation)))
            if degradation.level() >= Severity::Stale && !same_cycle =>
        {
            Some(Cause::Via {
                pin: pin.clone(),
                severity: degradation.level(),
            })
        }
        Some(Standing::Approved(_)) | None => None,
    }
}

/// The `LinksChanged` causes of a document: for each `Depends` link name (those of
/// the frame and those the document holds now), what it names now against what the
/// frame pinned.
///
/// A pinned upstream an edge rule has already said is gone, invalid or unknown is not
/// said again as removed, and a key written now that is that upstream's (the key the
/// build read it as, matched with the key the pin kept) is not said again as
/// unresolved.
fn link_changes(
    doc: &Doc,
    frame: &BTreeSet<FramePin>,
    lost: &[(&FramePin, FailureTag)],
) -> Vec<Cause> {
    let current = doc.links_of(Propagation::Depends);
    let mut names: BTreeSet<&EntryName> = frame.iter().map(FramePin::link).collect();
    names.extend(current.iter().map(|(name, _)| *name));

    names
        .into_iter()
        .filter_map(|name| {
            let states: &[LinkState] = current
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(&[], |(_, states)| *states);
            link_change(name, states, frame, lost)
        })
        .collect()
}

/// The `LinksChanged` cause of one link name, when what `states` name now is not what
/// the frame pinned for it.
fn link_change(
    name: &EntryName,
    states: &[LinkState],
    frame: &BTreeSet<FramePin>,
    lost: &[(&FramePin, FailureTag)],
) -> Option<Cause> {
    let pinned: BTreeSet<DocId> = frame
        .iter()
        .filter(|p| p.link() == name)
        .map(FramePin::target)
        .collect();
    let resolved: BTreeSet<DocId> = states
        .iter()
        .filter_map(|s| match s {
            LinkState::Resolved(target) => Some(target.id()),
            LinkState::Unresolved(_) => None,
        })
        .collect();
    let lost_here: Vec<(&FramePin, FailureTag)> = lost
        .iter()
        .filter(|(p, _)| p.link() == name)
        .copied()
        .collect();

    let added: Vec<DocId> = resolved.difference(&pinned).copied().collect();
    let removed: Vec<DocId> = pinned
        .difference(&resolved)
        .filter(|t| !lost_here.iter().any(|(p, _)| p.target() == **t))
        .copied()
        .collect();
    let unresolved: Vec<UnresolvedLink> = states
        .iter()
        .filter_map(|s| match s {
            LinkState::Unresolved(u) => Some(u),
            LinkState::Resolved(_) => None,
        })
        .filter(|u| !already_said(u, &lost_here))
        .cloned()
        .collect();

    if added.is_empty() && removed.is_empty() && unresolved.is_empty() {
        return None;
    }
    Some(Cause::LinksChanged {
        link: name.clone(),
        added,
        removed,
        unresolved,
    })
}

/// Whether an edge rule has already said what an unresolved link says: the key the
/// build read the link as is a lost pin's key, or the document it names is a lost
/// pin that is invalid. Fields are compared; the written text is not read again.
fn already_said(unresolved: &UnresolvedLink, lost_here: &[(&FramePin, FailureTag)]) -> bool {
    let by_key = unresolved
        .key()
        .is_some_and(|key| lost_here.iter().any(|(p, _)| p.key() == key));
    if by_key {
        return true;
    }
    let invalid_target = |t: &DocId| {
        lost_here
            .iter()
            .any(|(p, how)| *how == FailureTag::Invalid && p.target() == *t)
    };
    match unresolved.why() {
        UnresolvedWhy::TargetInvalid(t) => invalid_target(t),
        UnresolvedWhy::Ambiguous(ts) => ts.iter().any(invalid_target),
        UnresolvedWhy::Malformed(_) | UnresolvedWhy::Missing => false,
    }
}

/// What the covering links of the books expect, and what they reach.
struct ExpectedReach<'s> {
    /// For each target book, the (book, link) pairs that declare they cover it, with
    /// the strength each declares.
    expectations: BTreeMap<&'s BookName, Vec<(&'s BookName, &'s EntryName, CoverSeverity)>>,
    /// Each document a covering link reaches from a document that counts, by the
    /// (book, link) it was reached through, with the documents it was reached from.
    reached: Reached,
}

/// Whether the document `id` counts for coverage: its standing
/// [stands](Standing::stands). What was denied, deprecated or retired neither
/// reaches nor is expected to be reached.
fn counted(standings: &BTreeMap<DocId, Standing>, id: DocId) -> bool {
    standings.get(&id).is_some_and(Standing::stands)
}

/// The expectations of the covering links (a [`LinkKind::cover`] of some strength),
/// and the documents they reach ([`covering_reach`]).
///
/// [`LinkKind::cover`]: super::schema::LinkKind::cover
fn expected_reach<'s>(
    shelf: &'s Shelf,
    standings: &BTreeMap<DocId, Standing>,
) -> ExpectedReach<'s> {
    let mut expectations: BTreeMap<&BookName, Vec<(&BookName, &EntryName, CoverSeverity)>> =
        BTreeMap::new();
    for (book, schema) in shelf.schema().books() {
        for (name, kind) in schema.links() {
            if let Some(severity) = kind.cover {
                expectations
                    .entry(&kind.target)
                    .or_default()
                    .push((book, name, severity));
            }
        }
    }
    ExpectedReach {
        expectations,
        reached: covering_reach(shelf, |id| counted(standings, id)),
    }
}

/// The notice of one exemption that stands, as
/// [`exemption_reading`](super::exemption::exemption_reading) reads it: `Exempt`, in
/// the place of a `NotCovered`, or `ExemptionUnneeded`.
fn exemption_notice(
    shelf: &Shelf,
    standings: &BTreeMap<DocId, Standing>,
    reached: &Reached,
    exempt: &Exempt,
) -> Notice {
    let of = exempt.of();
    match read_exemption(shelf, |id| counted(standings, id), reached, of) {
        Ok(severity) => Notice::Exempt {
            book: of.book().clone(),
            link: of.link().clone(),
            severity,
            exemption: exempt.id(),
        },
        Err(why) => Notice::ExemptionUnneeded {
            book: of.book().clone(),
            link: of.link().clone(),
            exemption: exempt.id(),
            why,
        },
    }
}

/// The notices of the valid documents, and those of the exemptions. They change no
/// freshness.
///
/// | case | notice |
/// |---|---|
/// | an `Informs` link names a document whose standing is not `Approved(Fresh)` | `InformsNotFresh` |
/// | an `Informs` link cannot be followed | `InformsUnresolved` |
/// | a pinned upstream's causes include `Expired` | `UpstreamExpired` |
/// | a `Replaces` link | none |
/// | a pending or approved document of a book a covering link points into, that no pending or approved document reaches through it, and that no exemption exempts from it | `NotCovered` |
/// | an exemption that stands, of a document a covering link does not reach | `Exempt` (see `exemption_notice`) |
/// | an exemption that stands and is not needed now | `ExemptionUnneeded` (see `exemption_notice`) |
/// | two or more exemptions that stand of one document from one link, after their own notices | `ExemptionRepeated` |
/// | a pending or approved document whose link to artifacts writes a name with nothing there | `ArtifactAbsent` |
/// | a pending or approved document whose link to artifacts writes a name its adapter cannot tell about now, pinned or not | `ArtifactUnknown` |
///
/// They are given whatever the document's own standing is, except `NotCovered`,
/// `ArtifactAbsent` and `ArtifactUnknown`: a document denied, deprecated or retired
/// is not expected to be reached, nor its evidence to be kept. To them are added, first,
/// the notices of an approval's edits that a declaration kept from being causes
/// (`SelfSameMeaning`, `UpstreamSameMeaning`, `ArtifactSameMeaning`): those are made
/// where the hashes are compared, so only an approved document has them.
fn collect_notices(
    cx: &Cx<'_>,
    standings: &BTreeMap<DocId, Standing>,
    statuses: &BTreeMap<DocId, DeclaredStatus>,
    mut same_meaning: BTreeMap<DocId, Vec<Notice>>,
) -> BTreeMap<DocId, Vec<Notice>> {
    let mut all: BTreeMap<DocId, Vec<Notice>> = BTreeMap::new();
    let expected = expected_reach(cx.shelf, standings);
    // Each exemption read once; those of a document with a valid file are told in
    // the place of the `NotCovered` they take, the rest after every document. Two
    // or more of one document and link are told as repeated, after their readings.
    let mut exemptions: BTreeMap<(DocId, BookName, EntryName), Vec<Notice>> = BTreeMap::new();
    for (of, group) in by_subject(cx.ledger) {
        let mut read: Vec<Notice> = group
            .iter()
            .map(|exempt| exemption_notice(cx.shelf, standings, &expected.reached, exempt))
            .collect();
        if let [first, second, rest @ ..] = group.as_slice() {
            read.push(Notice::ExemptionRepeated {
                book: of.book().clone(),
                link: of.link().clone(),
                exemptions: TwoOrMore::new(
                    first.id(),
                    second.id(),
                    rest.iter().map(|e| e.id()).collect(),
                ),
            });
        }
        exemptions.insert((of.doc(), of.book().clone(), of.link().clone()), read);
    }
    for doc in cx.shelf.docs().values() {
        let mut notices = same_meaning.remove(&doc.id()).unwrap_or_default();
        if counted(standings, doc.id()) {
            for (book, link, severity) in expected
                .expectations
                .get(&doc.key().book)
                .into_iter()
                .flatten()
            {
                let at = (doc.id(), (*book).clone(), (*link).clone());
                let read = exemptions.remove(&at).unwrap_or_default();
                let exempt = read.iter().any(|n| matches!(n, Notice::Exempt { .. }));
                if !exempt && !expected.reached.contains_key(&at) {
                    notices.push(Notice::NotCovered {
                        book: (*book).clone(),
                        link: (*link).clone(),
                        severity: *severity,
                    });
                }
                notices.extend(read);
            }
            for (link, artifact) in doc.absent_artifacts() {
                notices.push(Notice::ArtifactAbsent {
                    link: link.clone(),
                    artifact: artifact.clone(),
                });
            }
            for (link, artifact, why) in doc.unknown_artifacts() {
                notices.push(Notice::ArtifactUnknown {
                    link: link.clone(),
                    artifact: artifact.clone(),
                    why: why.clone(),
                });
            }
        }
        for (link, states) in doc.links_of(Propagation::Informs) {
            for state in states {
                match state {
                    LinkState::Resolved(target) => {
                        let standing = standings.get(&target.id()).map(Standing::kind);
                        if let Some(standing) = standing
                            && standing != StandingKind::Approved(FreshnessLevel::Fresh)
                        {
                            notices.push(Notice::InformsNotFresh {
                                link: link.clone(),
                                target: target.id(),
                                standing,
                            });
                        }
                    }
                    LinkState::Unresolved(unresolved) => notices.push(Notice::InformsUnresolved {
                        link: link.clone(),
                        unresolved: unresolved.clone(),
                    }),
                }
            }
        }
        if let Some(DeclaredStatus::Approved { frame, .. }) = statuses.get(&doc.id()) {
            for pin in frame {
                if let Some(Standing::Approved(Freshness::Degraded(degradation))) =
                    standings.get(&pin.target())
                {
                    notices.extend(degradation.causes().iter().filter_map(|cause| match cause {
                        Cause::Expired { since } => Some(Notice::UpstreamExpired {
                            pin: pin.clone(),
                            since: *since,
                        }),
                        _ => None,
                    }));
                }
            }
        }
        if !notices.is_empty() {
            all.insert(doc.id(), notices);
        }
    }
    // The exemptions of no expectation now: every one is unneeded, of a document
    // whose file is valid or not.
    for ((doc, _, _), read) in exemptions {
        all.entry(doc).or_default().extend(read);
    }
    all
}

#[cfg(test)]
mod absent_tests;
#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod exempt_tests;
#[cfg(test)]
mod moved_tests;
#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod origin_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod severity_tests;
#[cfg(test)]
mod successor_tests;
#[cfg(test)]
mod tests;
