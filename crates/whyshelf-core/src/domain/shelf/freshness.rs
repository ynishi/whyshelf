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
//! [`Cause::severity`]'s table, and the level of a freshness is the strongest of its
//! causes ([`Freshness::from_causes`]); no other place assigns either.
//!
//! # The graph it travels
//!
//! Staleness travels along the *frames* of approvals (what each approval pinned), in
//! the order of [`Ledger::frame_graph`], upstream first. It does not travel along the
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
//! - a cycle the document is in, among links or among frames;
//! - an expiry: `now >= expires`.
//!
//! Every fact is said once. Notices ([`Notice`]) are what is worth knowing and does
//! not change a freshness: they come from `notices_of`'s table.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;

use super::build::{Cycle, CycleId, DocProblem, DocStateKind, InvalidDoc, Shelf};
use super::doc::{Doc, DocHash, DocKey, FramePin, LinkState, UnresolvedLink, UnresolvedWhy};
use super::ledger::{DeclaredStatus, JudgedAt, Ledger, LedgerProblem};
use super::schema::{BookName, EntryName, Propagation};

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
    /// The document's files are invalid: every problem of every file with its
    /// identity.
    SelfInvalid {
        /// The problems.
        problems: NonEmptyVec<DocProblem>,
    },
    /// No file has the document's identity.
    SelfGone,
    /// No file is known to have the identity, but some file's identity could not be
    /// read.
    SelfUnknown {
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
    },
    /// A pinned upstream has no file.
    UpstreamGone {
        /// The pin.
        pin: FramePin,
    },
    /// A pinned upstream is invalid.
    UpstreamInvalid {
        /// The pin.
        pin: FramePin,
    },
    /// It cannot be told whether a pinned upstream has a file.
    UpstreamUnknown {
        /// The pin.
        pin: FramePin,
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
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
    /// How strongly this cause degrades an approval. The one place a strength is
    /// assigned.
    ///
    /// | cause | strength |
    /// |---|---|
    /// | `SelfChanged`, `UpstreamChanged` | `Stale` |
    /// | `SelfInvalid`, `SelfGone`, `SelfUnknown` | `Broken` |
    /// | `UpstreamGone`, `UpstreamInvalid`, `UpstreamUnknown` | `Broken` |
    /// | `UpstreamDeclared` of `Pending`, or of `Deprecated` with successors | `Stale` |
    /// | `UpstreamDeclared` of `Denied`, `Retired`, or of `Deprecated` with none | `Broken` |
    /// | `Via` | the strength it carries (`Stale` or `Broken`; an upstream that is only `Expired` makes no `Via`) |
    /// | `LinksChanged` | `Broken` when some key cannot be followed, otherwise `Stale` |
    /// | `InCycle` | `Broken` |
    /// | `Expired` | `Expired` |
    pub fn severity(&self) -> Severity {
        match self {
            Self::SelfChanged { .. } | Self::UpstreamChanged { .. } => Severity::Stale,
            Self::SelfInvalid { .. } | Self::SelfGone | Self::SelfUnknown { .. } => {
                Severity::Broken
            }
            Self::UpstreamGone { .. }
            | Self::UpstreamInvalid { .. }
            | Self::UpstreamUnknown { .. } => Severity::Broken,
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

/// Whether an approval holds.
// `Fresh` is the common value and carries nothing; boxing the causes would put an
// allocation in every reading of a degraded one.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// It holds.
    Fresh,
    /// It does not, for these causes, at the level of the strongest.
    Degraded {
        /// The strongest severity among the causes.
        level: Severity,
        /// The causes.
        causes: NonEmptyVec<Cause>,
    },
}

impl Freshness {
    /// The freshness of an approval with these causes: `Fresh` when there are none,
    /// otherwise degraded at the strongest of them.
    ///
    /// The only place a level is made.
    pub fn from_causes(causes: Vec<Cause>) -> Self {
        let level = causes.iter().map(Cause::severity).max();
        match (level, NonEmptyVec::from_vec(causes)) {
            (Some(level), Ok(causes)) => Self::Degraded { level, causes },
            _ => Self::Fresh,
        }
    }

    /// The level, with the causes left out.
    pub fn level(&self) -> FreshnessLevel {
        match self {
            Self::Fresh => FreshnessLevel::Fresh,
            Self::Degraded { level, .. } => match level {
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

    /// The notices of each document that has any.
    pub fn notices(&self) -> &BTreeMap<DocId, Vec<Notice>> {
        &self.notices
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
    pub fn roots_of(&self, id: DocId) -> Vec<RootCause> {
        let mut roots = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_roots(id, vec![id], true, &mut visited, &mut roots);
        roots
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
        let Some(Standing::Approved(Freshness::Degraded { causes, .. })) = self.standings.get(&id)
        else {
            return;
        };
        for cause in causes.iter() {
            if let Cause::Via { pin, .. } = cause {
                let mut onward = path.clone();
                onward.push(pin.target());
                self.collect_roots(pin.target(), onward, false, visited, roots);
            } else if is_start || cause.severity() >= Severity::Stale {
                roots.push(RootCause {
                    path: path.clone(),
                    cause: cause.clone(),
                });
            }
        }
    }
}

/// Which way a pinned upstream was lost, for the exclusions of the link rule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lost {
    Gone,
    Invalid,
    Unknown,
}

/// What `assess` reads while it works.
struct Cx<'a> {
    shelf: &'a Shelf,
    ledger: &'a Ledger,
    now: JudgedAt,
    in_cycle: BTreeMap<DocId, Vec<CycleId>>,
    component: BTreeMap<DocId, usize>,
}

/// The standing of every document, from the shelf and the ledger, at `now`.
///
/// The one entry of freshness: nothing is stored, so it is computed every time. See
/// the module page for the graph it travels and the causes it finds.
pub fn assess(shelf: &Shelf, ledger: &Ledger, now: JudgedAt) -> Assessment {
    let graph = ledger.frame_graph();
    let mut cycles: Vec<Cycle> = shelf.cycles().to_vec();
    cycles.extend(graph.cycles());
    let mut in_cycle: BTreeMap<DocId, Vec<CycleId>> = BTreeMap::new();
    for cycle in &cycles {
        for member in cycle.ids() {
            in_cycle.entry(member).or_default().push(cycle.id());
        }
    }

    let components = graph.components();
    let component: BTreeMap<DocId, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(i, members)| members.iter().map(move |id| (*id, i)))
        .collect();
    let cx = Cx {
        shelf,
        ledger,
        now,
        in_cycle,
        component,
    };

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

    let mut standings: BTreeMap<DocId, Standing> = BTreeMap::new();
    for id in order {
        let standing = match ledger.status_of(id) {
            DeclaredStatus::Pending => Standing::Pending,
            DeclaredStatus::Denied { .. } => Standing::Denied,
            DeclaredStatus::Deprecated { .. } => Standing::Deprecated {
                successors: Successors::of(shelf, ledger, id),
            },
            DeclaredStatus::Retired { .. } => Standing::Retired,
            DeclaredStatus::Approved {
                seen,
                frame,
                expires,
                ..
            } => Standing::Approved(freshness_of(&cx, id, &seen, &frame, expires, &standings)),
        };
        standings.insert(id, standing);
    }

    let notices = notices_of(&cx, &standings);
    let invalid = shelf
        .invalid()
        .values()
        .flat_map(|docs| docs.iter().cloned())
        .chain(shelf.unreadable().iter().cloned())
        .collect();
    Assessment {
        standings,
        notices,
        cycles,
        invalid,
        ledger_problems: ledger.problems().to_vec(),
    }
}

fn freshness_of(
    cx: &Cx<'_>,
    id: DocId,
    seen: &DocHash,
    frame: &BTreeSet<FramePin>,
    expires: Option<JudgedAt>,
    standings: &BTreeMap<DocId, Standing>,
) -> Freshness {
    let mut causes = Vec::new();
    match cx.shelf.state_of(id).valid_or_kind() {
        Ok(doc) => valid_causes(cx, doc, seen, frame, standings, &mut causes),
        Err(DocStateKind::Gone) => causes.push(Cause::SelfGone),
        Err(DocStateKind::Invalid(problems)) => causes.push(Cause::SelfInvalid { problems }),
        Err(DocStateKind::Unknown(unreadable)) => causes.push(Cause::SelfUnknown { unreadable }),
    }
    if let Some(since) = expires
        && cx.now >= since
    {
        causes.push(Cause::Expired { since });
    }
    Freshness::from_causes(causes)
}

fn valid_causes(
    cx: &Cx<'_>,
    doc: &Doc,
    seen: &DocHash,
    frame: &BTreeSet<FramePin>,
    standings: &BTreeMap<DocId, Standing>,
    causes: &mut Vec<Cause>,
) {
    if doc.hash() != seen {
        causes.push(Cause::SelfChanged {
            approved: *seen,
            now: *doc.hash(),
        });
    }

    let mut lost: Vec<(&FramePin, Lost)> = Vec::new();
    for pin in frame {
        let target = pin.target();
        match cx.shelf.state_of(target).valid_or_kind() {
            Err(DocStateKind::Gone) => {
                causes.push(Cause::UpstreamGone { pin: pin.clone() });
                lost.push((pin, Lost::Gone));
            }
            Err(DocStateKind::Invalid(_)) => {
                causes.push(Cause::UpstreamInvalid { pin: pin.clone() });
                lost.push((pin, Lost::Invalid));
            }
            Err(DocStateKind::Unknown(unreadable)) => {
                causes.push(Cause::UpstreamUnknown {
                    pin: pin.clone(),
                    unreadable,
                });
                lost.push((pin, Lost::Unknown));
            }
            Ok(upstream) => {
                if upstream.hash() != pin.hash() {
                    causes.push(Cause::UpstreamChanged {
                        pin: pin.clone(),
                        now: *upstream.hash(),
                    });
                }
                let same_cycle = cx
                    .component
                    .get(&target)
                    .is_some_and(|c| cx.component.get(&doc.id()) == Some(c));
                match standings.get(&target) {
                    Some(Standing::Pending) => causes.push(declared(pin, UpstreamStatus::Pending)),
                    Some(Standing::Denied) => causes.push(declared(pin, UpstreamStatus::Denied)),
                    Some(Standing::Retired) => causes.push(declared(pin, UpstreamStatus::Retired)),
                    Some(Standing::Deprecated { successors }) => causes.push(declared(
                        pin,
                        UpstreamStatus::Deprecated {
                            successors: successors.clone(),
                        },
                    )),
                    Some(Standing::Approved(Freshness::Degraded { level, .. }))
                        if *level >= Severity::Stale && !same_cycle =>
                    {
                        causes.push(Cause::Via {
                            pin: pin.clone(),
                            severity: *level,
                        });
                    }
                    Some(Standing::Approved(_)) | None => {}
                }
            }
        }
    }

    causes.extend(link_changes(cx, doc, frame, &lost));

    if let Some(ids) = cx.in_cycle.get(&doc.id()) {
        causes.extend(ids.iter().map(|cycle| Cause::InCycle { cycle: *cycle }));
    }
}

fn declared(pin: &FramePin, status: UpstreamStatus) -> Cause {
    Cause::UpstreamDeclared {
        pin: pin.clone(),
        status,
    }
}

/// The `LinksChanged` causes of a document: for each `Depends` link name (those of
/// the frame and those of the schema now), what it names now against what the frame
/// pinned.
///
/// A pinned upstream an edge rule has already said is gone, invalid or unknown is not
/// said again as removed, and a key written now that is that upstream's (matched by
/// the key the pin kept) is not said again as unresolved.
fn link_changes(
    cx: &Cx<'_>,
    doc: &Doc,
    frame: &BTreeSet<FramePin>,
    lost: &[(&FramePin, Lost)],
) -> Vec<Cause> {
    let current = cx.shelf.links_of(doc, Propagation::Depends);
    let mut names: BTreeSet<&EntryName> = frame.iter().map(FramePin::link).collect();
    names.extend(current.iter().map(|(name, _)| *name));

    let mut changes = Vec::new();
    for name in names {
        let states: &[LinkState] = current
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(&[], |(_, states)| *states);
        let pinned: BTreeSet<DocId> = frame
            .iter()
            .filter(|p| p.link() == name)
            .map(FramePin::target)
            .collect();
        let resolved: BTreeSet<DocId> = states
            .iter()
            .filter_map(|s| match s {
                LinkState::Resolved { target } => Some(*target),
                LinkState::Unresolved(_) => None,
            })
            .collect();
        let lost_here: Vec<(&FramePin, Lost)> = lost
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
        let target_book = cx
            .shelf
            .schema()
            .book(&doc.key().book)
            .and_then(|book| book.links().get(name))
            .map(|kind| &kind.target);
        let unresolved: Vec<UnresolvedLink> = states
            .iter()
            .filter_map(|s| match s {
                LinkState::Unresolved(u) => Some(u),
                LinkState::Resolved { .. } => None,
            })
            .filter(|u| !already_said(cx, target_book, u, &lost_here))
            .cloned()
            .collect();

        if !(added.is_empty() && removed.is_empty() && unresolved.is_empty()) {
            changes.push(Cause::LinksChanged {
                link: name.clone(),
                added,
                removed,
                unresolved,
            });
        }
    }
    changes
}

/// Whether an edge rule has already said what an unresolved link says.
fn already_said(
    cx: &Cx<'_>,
    target_book: Option<&BookName>,
    unresolved: &UnresolvedLink,
    lost_here: &[(&FramePin, Lost)],
) -> bool {
    let by_key = target_book
        .and_then(|book| {
            let pattern = cx.shelf.schema().book(book)?.key();
            let form = pattern.parse(unresolved.written()).ok()?;
            Some(DocKey {
                book: book.clone(),
                form,
            })
        })
        .is_some_and(|key| lost_here.iter().any(|(p, _)| *p.key() == key));
    if by_key {
        return true;
    }
    let invalid_target = |t: &DocId| {
        lost_here
            .iter()
            .any(|(p, how)| *how == Lost::Invalid && p.target() == *t)
    };
    match unresolved.why() {
        UnresolvedWhy::TargetInvalid(t) => invalid_target(t),
        UnresolvedWhy::Ambiguous(ts) => ts.iter().any(invalid_target),
        UnresolvedWhy::Malformed(_) | UnresolvedWhy::Missing => false,
    }
}

/// The notices of the valid documents. They change no freshness.
///
/// | case | notice |
/// |---|---|
/// | an `Informs` link names a document whose standing is not `Approved(Fresh)` | `InformsNotFresh` |
/// | an `Informs` link cannot be followed | `InformsUnresolved` |
/// | a pinned upstream's causes include `Expired` | `UpstreamExpired` |
/// | a `Replaces` link | none |
///
/// They are given whatever the document's own standing is.
fn notices_of(cx: &Cx<'_>, standings: &BTreeMap<DocId, Standing>) -> BTreeMap<DocId, Vec<Notice>> {
    let mut all = BTreeMap::new();
    for doc in cx.shelf.docs().values() {
        let mut notices = Vec::new();
        for (link, states) in cx.shelf.links_of(doc, Propagation::Informs) {
            for state in states {
                match state {
                    LinkState::Resolved { target } => {
                        let standing = standings.get(target).map(Standing::kind);
                        if let Some(standing) = standing
                            && standing != StandingKind::Approved(FreshnessLevel::Fresh)
                        {
                            notices.push(Notice::InformsNotFresh {
                                link: link.clone(),
                                target: *target,
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
        if let DeclaredStatus::Approved { frame, .. } = cx.ledger.status_of(doc.id()) {
            for pin in &frame {
                if let Some(Standing::Approved(Freshness::Degraded { causes, .. })) =
                    standings.get(&pin.target())
                {
                    notices.extend(causes.iter().filter_map(|cause| match cause {
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
    all
}

#[cfg(test)]
mod tests;
