//! Landing: whether the change an item was approved with has reached the baseline.
//!
//! "Reached the baseline" used to mean one thing, that the approved commit is an
//! ancestor of the baseline, and a squash merge breaks it: the commit that was
//! approved is not in the history, only a commit with the same changes. So the
//! question is asked two ways, and the answer is *integrated* when either says yes.
//!
//! ```text
//!   approved revision --- ancestor of the baseline? ----------------+
//!                                                                   +--> Integrated
//!   hunks recorded at approval --- all in the baseline's history? --+
//!   (a ChangeFingerprint, in the CA attestation)
//! ```
//!
//! # What this module is
//!
//! The rule that turns facts into a verdict, and the value that is recorded to make
//! the second way possible. It does not run Git. The port that answers the two
//! questions ([`BaselineFacts`]) and the one that cuts a diff into hunks are
//! adapters' work; here the facts come in as values.
//!
//! # The verdict
//!
//! [`Integration`] is what a view shows for an item:
//!
//! | value | means |
//! |---|---|
//! | [`Unapproved`](Integration::Unapproved) | no CA approval stands for the item as it is |
//! | [`Pending`](Integration::Pending) | approved, and the change has not reached the baseline |
//! | [`Integrated`](Integration::Integrated) | it has, by [`IntegrationEvidence`] |
//! | [`Unknown`](Integration::Unknown) | approved, and this clone cannot tell, for an [`UnknownWhy`] |
//!
//! Each approval is judged on its own from its [`ApprovedChange`], and the item takes
//! the best of them: a CA approval that stands for the item as it is and has landed
//! makes the item Integrated, whatever the others say. Then Unknown (one of them may
//! have landed, and this clone cannot see it), then Pending.
//!
//! Judging one approval, in order:
//!
//! ```text
//!   adopted (taken in from an earlier decision)            -> Integrated (Adopted)
//!   the approved revision is an ancestor                   -> Integrated (Ancestor)
//!   every recorded hunk is in the baseline's history
//!     and the change has a strong hunk                     -> Integrated (HunksContained)
//!   the ancestry is known to be no, and
//!     the hunks are known to be no, or too weak to count
//!     (whatever is said of them)                           -> Pending
//!   the fingerprint is of a scheme this build does not have -> Unknown (FingerprintVersion)
//!   anything else                                          -> Unknown
//! ```
//!
//! The verdict is monotonic in the facts: a fact that turns from "no" or "cannot
//! tell" into "yes" never lowers it, and a further approval never lowers it. It is
//! also monotonic in time, which is a choice: once the hunks are in the history of
//! the baseline they stay there, so a later revert of the code does not take the
//! landing back. That is a known limitation of the model, not of the code.
//!
//! # Hunks
//!
//! A change is cut into hunks with no lines of context, so that lines the baseline
//! changed beside a hunk, or between two of them, do not alter it. A hunk is known
//! by its [`HunkId`]: the path, the lines taken out and the lines
//! put in, with white space collapsed and no line number. Two items of one pull
//! request that touch one file are told apart by their hunks.
//!
//! A hunk of a few characters (a closing brace, a short `use`) can match a change
//! that has nothing to do with the item. A hunk whose lines hold fewer than
//! [`WEAK_HUNK_BELOW`] characters other than white space is *weak*, and a change
//! with no strong hunk (`strong` is false) never lands by its hunks alone: it needs
//! the ancestry. A weak match is no evidence at all, so what the port says of weak
//! hunks is read as a no and the ancestry alone decides.
//!
//! How the hunks are cut and normalized is a [`HunkScheme`], recorded in the
//! fingerprint. A scheme is never changed in place: a new version is a new variant,
//! and a fingerprint is read under the scheme it was made with. A fingerprint made
//! under a scheme this build has not got is kept as it was read
//! ([`HunkScheme::Unknown`], the name only), so that the file that holds it is not
//! refused and the identifier of the approval stays what it was; its landing is
//! [`UnknownWhy::FingerprintVersion`] unless the ancestry settles it.
//!
//! # Known limitations
//!
//! A hunk id is a plain hash of the path and of the changed lines, with white space
//! collapsed. It is made from code that lives in the repository's history, and it
//! says nothing the history does not say already: whoever can read the history can
//! compute the id of any change in it, and that is the point, since it is how the
//! change is found again. For the same reason it is not a way to hide anything. It is
//! not used for the text of private documents: their lines are not in the
//! repository's history, and a hash of them in a public ledger would be a hash of
//! text that is meant to stay out of it, which anyone could confirm by guessing the
//! lines. Hunks are only ever cut from code under the repository's version control.
//!
//! The verdict is also monotonic in time (see above): a revert of the code after it
//! landed does not take the landing back.
//!
//! # Without a fingerprint
//!
//! An approval written before fingerprints existed has none. It is not wrong, it
//! cannot be matched by its hunks, and so it lands by its ancestry or it is Unknown
//! ([`UnknownWhy::NoFingerprint`]) and never Pending: the hunks could have landed.
//! The same holds for an approval made where the change could not be cut
//! ([`CaChange::NotFingerprinted`]); the reason the approver's clone gave
//! ([`FingerprintUnavailable`]) is shown with it and is not read into the verdict. An
//! approval of a change that had nothing to fingerprint ([`CaChange::NothingToFingerprint`])
//! is different: no hunk could have landed, so the ancestry alone decides, and a no
//! there is Pending. The table is on [`ApprovedChange::checked`].

use std::collections::BTreeSet;
use std::fmt;

use super::approval::CaChange;
use super::boundary::{Crosses, CrossingText};
use super::canon::{Canon, ToCanon};
use super::error::{FingerprintError, ValueError};
use super::hash::HunkId;
use super::knowable::Knowable;
use super::location::RelPath;
use super::nonempty::NonEmptySet;
use super::revision::Revision;
use super::vocabulary::closed_vocabulary;

/// Fewer characters than this, other than white space, in the lines of a hunk make
/// it weak.
pub const WEAK_HUNK_BELOW: usize = 16;

/// How a change is cut into hunks and how a hunk is read: the version of the rule.
///
/// Recorded in every [`ChangeFingerprint`]. The rule of a version is never edited;
/// a rule that changes is a new variant, and the old one stays so that what was
/// recorded under it can still be judged. A version never adds a `kind` key to the
/// map of a fingerprint: that key is how [`CaChange`] tells the other cases from it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HunkScheme {
    /// Diff with no lines of context; the lines of a hunk with white space
    /// collapsed and empty lines dropped; weak below [`WEAK_HUNK_BELOW`]
    /// characters.
    V1,
    /// A scheme this build does not have, kept by the name it was recorded under.
    ///
    /// A fingerprint made by a newer build is still a fingerprint: it is read back
    /// whole, so the identifier of the approval that holds it (a hash of the
    /// canonical form, in which the scheme is only its name) stays the same, and the
    /// event file is not refused. It cannot be matched here, so a verdict that
    /// turns on it is [`UnknownWhy::FingerprintVersion`]. Built by
    /// [`from_name`](Self::from_name), which gives [`V1`](Self::V1) for the name
    /// that is its own.
    Unknown(SchemeName),
}

impl HunkScheme {
    /// The scheme new fingerprints are made under.
    pub const CURRENT: Self = Self::V1;

    /// The name recorded in a fingerprint.
    pub fn name(&self) -> &str {
        match self {
            Self::V1 => "hunks-v1",
            Self::Unknown(name) => name.as_str(),
        }
    }

    /// The scheme with this name: a known one, or [`Unknown`](Self::Unknown) for a
    /// name this build does not know, if the name has the shape of one.
    pub fn from_name(name: &str) -> Result<Self, ValueError> {
        if name == Self::V1.name() {
            Ok(Self::V1)
        } else {
            SchemeName::new(name).map(Self::Unknown)
        }
    }

    /// Whether this build can cut a change into hunks under the scheme and read the
    /// hunks of a fingerprint made under it.
    pub fn is_known(&self) -> bool {
        match self {
            Self::V1 => true,
            Self::Unknown(_) => false,
        }
    }
}

impl ToCanon for HunkScheme {
    fn to_canon(&self) -> Canon {
        Canon::str(self.name())
    }
}

/// The name of a hunk scheme: 1 to 64 characters of lowercase letters, digits and
/// `. _ -`, so that it can say nothing but a name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemeName(String);

impl SchemeName {
    /// The longest a name is.
    pub const MAX_LEN: usize = 64;

    /// A name from its text.
    pub fn new(text: &str) -> Result<Self, ValueError> {
        let shaped = !text.is_empty()
            && text.len() <= Self::MAX_LEN
            && text.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            });
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(ValueError::SchemeNameBadShape)
        }
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One hunk of a change, as the adapter that cut the diff reports it.
///
/// The lines are as they stand in the diff, without their line breaks. The values
/// that matter ([`id`](Self::id), [`is_weak`](Self::is_weak)) are decided here,
/// so that the approval side and the baseline side, which both build hunks, cannot
/// disagree on them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    path: RelPath,
    removed: Vec<String>,
    added: Vec<String>,
}

impl Hunk {
    /// The hunk of `path` that takes out `removed` and puts in `added`.
    pub fn new(path: RelPath, removed: Vec<String>, added: Vec<String>) -> Self {
        Self {
            path,
            removed,
            added,
        }
    }

    /// Its identity under `scheme`; `None` under a scheme this build does not have.
    pub fn id(&self, scheme: &HunkScheme) -> Option<HunkId> {
        match scheme {
            HunkScheme::V1 => {
                let removed: Vec<&str> = self.removed.iter().map(String::as_str).collect();
                let added: Vec<&str> = self.added.iter().map(String::as_str).collect();
                Some(HunkId::of(&self.path, &removed, &added))
            }
            HunkScheme::Unknown(_) => None,
        }
    }

    /// Whether the hunk is too short to be told from a change that has nothing to do
    /// with it (see the module documentation); `None` under a scheme this build does
    /// not have.
    pub fn is_weak(&self, scheme: &HunkScheme) -> Option<bool> {
        match scheme {
            HunkScheme::V1 => {
                let weight: usize = self
                    .removed
                    .iter()
                    .chain(&self.added)
                    .map(|line| line.chars().filter(|c| !c.is_whitespace()).count())
                    .sum();
                Some(weight < WEAK_HUNK_BELOW)
            }
            HunkScheme::Unknown(_) => None,
        }
    }
}

/// What a CA approval records of the change it approved, so that the change can be
/// found again in the baseline after the approved commit is gone.
///
/// It is part of the attestation of a CA approval, as [`CaChange::Fingerprinted`]. An
/// approval that has none (one written before this existed, of a change with no hunk,
/// or made where the change could not be cut) is judged by the ancestry alone.
///
/// # The keys of its map
///
/// The canonical map of a fingerprint is told from the other cases of [`CaChange`] by
/// having no `kind` key, which the others have. A key named `kind` must therefore
/// never be added to it, and a new field takes another name; a scheme that needs a
/// tag is a new [`HunkScheme`], which is written under the `scheme` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFingerprint(Box<FingerprintParts>);

/// The parts of a [`ChangeFingerprint`]; boxed there so that the attestation that
/// holds an optional one stays small.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FingerprintParts {
    base: Revision,
    baseline_at_approval: Revision,
    hunks: NonEmptySet<HunkId>,
    strong: bool,
    scheme: HunkScheme,
}

impl ChangeFingerprint {
    /// A fingerprint from its parts, as they were recorded.
    ///
    /// `base` is the nearest ancestor of the approved revision that is on the
    /// baseline's first-parent line, where the change branched off.
    /// `baseline_at_approval` is the tip of the baseline's first-parent line when the
    /// approval was made; together they bound where the baseline's history is looked
    /// at. `strong` says that at least one hunk is not weak.
    pub fn new(
        base: Revision,
        baseline_at_approval: Revision,
        hunks: NonEmptySet<HunkId>,
        strong: bool,
        scheme: HunkScheme,
    ) -> Self {
        Self(Box::new(FingerprintParts {
            base,
            baseline_at_approval,
            hunks,
            strong,
            scheme,
        }))
    }

    /// The fingerprint of a change cut into `hunks`, under `scheme`; `strong` is
    /// decided here by the weak-hunk rule.
    pub fn of_hunks(
        base: Revision,
        baseline_at_approval: Revision,
        hunks: &[Hunk],
        scheme: HunkScheme,
    ) -> Result<Self, FingerprintError> {
        let unknown = || FingerprintError::UnknownScheme {
            scheme: scheme.name().to_owned(),
        };
        let mut ids = BTreeSet::new();
        let mut strong = false;
        for hunk in hunks {
            ids.insert(hunk.id(&scheme).ok_or_else(unknown)?);
            strong |= !hunk.is_weak(&scheme).ok_or_else(unknown)?;
        }
        let hunks_set = NonEmptySet::from_set(ids).map_err(|_| FingerprintError::NoHunks)?;
        Ok(Self::new(
            base,
            baseline_at_approval,
            hunks_set,
            strong,
            scheme,
        ))
    }

    /// Where the change branched off the baseline's first-parent line.
    pub fn base(&self) -> &Revision {
        &self.0.base
    }

    /// The tip of the baseline's first-parent line when the approval was made.
    pub fn baseline_at_approval(&self) -> &Revision {
        &self.0.baseline_at_approval
    }

    /// The hunks of the change.
    pub fn hunks(&self) -> &NonEmptySet<HunkId> {
        &self.0.hunks
    }

    /// Whether at least one hunk is not weak.
    pub fn is_strong(&self) -> bool {
        self.0.strong
    }

    /// The scheme the hunks were cut and read under.
    pub fn scheme(&self) -> &HunkScheme {
        &self.0.scheme
    }
}

impl Crosses for ChangeFingerprint {
    /// The two revisions it names cross, like the revision of the approval; the hunks
    /// are hashes and the scheme a name of a fixed shape (1 to 64 characters of
    /// `a-z 0-9 . _ -`, [`SchemeName`]), and say nothing a reader could use.
    fn cross_into(&self, out: &mut CrossingText) {
        let FingerprintParts {
            base,
            baseline_at_approval,
            hunks: _,
            strong: _,
            scheme: _,
        } = &*self.0;
        base.cross_into(out);
        baseline_at_approval.cross_into(out);
    }
}

impl ToCanon for ChangeFingerprint {
    fn to_canon(&self) -> Canon {
        let FingerprintParts {
            base,
            baseline_at_approval,
            hunks,
            strong,
            scheme,
        } = &*self.0;
        Canon::map([
            ("base", base.to_canon()),
            ("baseline", baseline_at_approval.to_canon()),
            ("hunks", hunks.to_canon()),
            ("strong", Canon::Bool(*strong)),
            ("scheme", scheme.to_canon()),
        ])
    }
}

/// What the version-control side can say of one approval against the baseline.
///
/// Each fact is [`Knowable`]: a clone that lacks the baseline or the commit
/// (shallow, single-branch) cannot say "no", and calling it "no" would show work as
/// pending that may well have landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineFacts {
    ancestor: Knowable<bool>,
    hunks_contained: Knowable<bool>,
}

impl BaselineFacts {
    /// The two facts.
    ///
    /// `ancestor`: the approved revision is an ancestor of the baseline.
    /// `hunks_contained`: every hunk of the recorded fingerprint is in the diff of
    /// some commit of the baseline's history, which is what "landed" means for a
    /// change that was squashed or rebased.
    pub fn new(ancestor: Knowable<bool>, hunks_contained: Knowable<bool>) -> Self {
        Self {
            ancestor,
            hunks_contained,
        }
    }

    /// The ancestry fact.
    pub fn ancestor(&self) -> Knowable<bool> {
        self.ancestor
    }

    /// The hunks fact.
    pub fn hunks_contained(&self) -> Knowable<bool> {
        self.hunks_contained
    }
}

/// Why this clone cannot tell whether a change landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnknownWhy {
    /// The clone is shallow: the history needed is cut off.
    Shallow,
    /// The commits to look through are more than the window allows, and the change
    /// was not found in the part that was looked at.
    OutsideWindow,
    /// The point the fingerprint records (where the change branched off, or the
    /// baseline's tip at approval) is not on the baseline's line in this clone.
    BaseMissing,
    /// The baseline ref is not in this clone.
    BaselineUnresolved,
    /// The fingerprint was made under a scheme this build does not have.
    FingerprintVersion,
    /// The approval records no fingerprint, and the ancestry does not settle it.
    NoFingerprint,
}

impl fmt::Display for UnknownWhy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shallow => "the clone is shallow",
            Self::OutsideWindow => "the change was not found within the commits looked at",
            Self::BaseMissing => "where the change branched off is not in this clone",
            Self::BaselineUnresolved => "the baseline is not in this clone",
            Self::FingerprintVersion => "the fingerprint was made by a newer version",
            Self::NoFingerprint => "the approval records no fingerprint of its change",
        })
    }
}

/// Why the clone an approval was made in could not cut its change into hunks.
///
/// A fact about the approver's clone when the approval was made, recorded beside the
/// approval ([`CaChange::NotFingerprinted`]). It is **not** an [`UnknownWhy`]: that is
/// a fact about the clone that reads the landing now, and the two clones are not the
/// same. The recorded reason is shown and never copied into a verdict; how the
/// approval is read is [`CaChange`]'s table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FingerprintUnavailable {
    /// The clone was shallow: the history needed to find where the change branched
    /// off was cut.
    Shallow,
    /// Where the change branched off is not on the baseline's line in the clone.
    BaseMissing,
    /// The baseline ref was not in the clone.
    BaselineUnresolved,
    /// The code was not under version control.
    NotVersioned,
    /// The approved revision is a patch over a commit: the change is not in history
    /// yet, so there is no commit of it to cut against the baseline.
    NotCommitted,
}

closed_vocabulary!(FingerprintUnavailable, "reason a change has no fingerprint", {
    Shallow => "shallow",
    BaseMissing => "base-missing",
    BaselineUnresolved => "baseline-unresolved",
    NotVersioned => "not-versioned",
    NotCommitted => "not-committed",
});

impl fmt::Display for FingerprintUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shallow => "the clone was shallow",
            Self::BaseMissing => "where the change branched off was not in the clone",
            Self::BaselineUnresolved => "the baseline was not in the clone",
            Self::NotVersioned => "the code was not under version control",
            Self::NotCommitted => "the change was a patch, not in history yet",
        })
    }
}

/// What the version-control side says of the fingerprint of a change, for the CA
/// check to record.
///
/// Three answers, because three are different facts: a change was cut into hunks,
/// there was nothing to cut, or the clone could not. The check turns each into the
/// [`CaChange`] it records, once ([`gate::ca`](super::gate::ca)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fingerprinting {
    /// The change, cut into hunks.
    Of(ChangeFingerprint),
    /// The revision is the baseline, or the change has no hunk.
    NothingToFingerprint,
    /// The clone cannot cut the change into hunks, for this reason. It is an answer,
    /// not an error: an approval is a person's judgement, and the fingerprint is a hint
    /// to the landing that the ancestry can do without.
    Unavailable(FingerprintUnavailable),
}

/// How an item's approved change has landed, for an item as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integration {
    /// No CA approval stands for the item as it is.
    Unapproved,
    /// Approved, and the change has not reached the baseline.
    Pending {
        /// The approved revision (the base of a patch).
        approved: Revision,
    },
    /// The change has reached the baseline.
    Integrated {
        /// How it is known.
        evidence: IntegrationEvidence,
    },
    /// Approved, and whether the change reached the baseline cannot be told here.
    ///
    /// Not a failure: the rest of the state stands, and a clone that has the
    /// history says more.
    Unknown {
        /// The approved revision (the base of a patch).
        approved: Revision,
        /// Why it cannot be told.
        why: UnknownWhy,
    },
}

impl Integration {
    /// Whether the change has reached the baseline.
    pub fn is_integrated(&self) -> bool {
        matches!(self, Self::Integrated { .. })
    }

    /// The verdict over the CA approvals that stand for the item as it is.
    ///
    /// Each approval is judged on its own ([`ApprovedChange`]); the item takes the
    /// best verdict, as the module documentation says. The result does not depend on
    /// the order of `changes`.
    pub fn decide(changes: impl IntoIterator<Item = ApprovedChange>) -> Self {
        let mut landed: Option<IntegrationEvidence> = None;
        let mut unknown: Option<(Revision, UnknownWhy)> = None;
        let mut pending: Option<Revision> = None;
        for change in changes {
            match change.landing() {
                Landing::Integrated(evidence) => {
                    landed = Some(match landed {
                        Some(known) => known.min(evidence),
                        None => evidence,
                    });
                }
                Landing::Unknown(revision, why) => {
                    unknown = Some(match unknown {
                        Some(known) => known.min((revision, why)),
                        None => (revision, why),
                    });
                }
                Landing::Pending(revision) => {
                    pending = Some(match pending {
                        Some(known) => known.min(revision),
                        None => revision,
                    });
                }
            }
        }
        match (landed, unknown, pending) {
            (Some(evidence), _, _) => Self::Integrated { evidence },
            (None, Some((approved, why)), _) => Self::Unknown { approved, why },
            (None, None, Some(approved)) => Self::Pending { approved },
            (None, None, None) => Self::Unapproved,
        }
    }
}

/// How an integrated change is known to be.
///
/// The variants are in the order the verdict prefers when several approvals have
/// landed: the ancestry first, which needs no matching, then the hunks, then an
/// earlier decision taken in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrationEvidence {
    /// The approved revision is an ancestor of the baseline.
    Ancestor(Revision),
    /// Every hunk of the approved change is in the baseline's history, and the
    /// change has a strong hunk.
    HunksContained,
    /// The approval was taken in from an earlier decision, which landed before the
    /// tool was in use.
    Adopted,
}

/// One CA approval that stands for an item as it is, with what is known of it
/// against the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedChange {
    source: ChangeSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangeSource {
    Checked {
        approved: Revision,
        change: CaChange,
        facts: BaselineFacts,
        cannot_tell: UnknownWhy,
    },
    Adopted,
}

impl ApprovedChange {
    /// An approval made through the gate, at `approved`.
    ///
    /// `change` is what the attestation records of the change. `facts` are the port's
    /// answers, and `cannot_tell` the port's reason for whichever of them it could not
    /// give; it is used only when the verdict turns on a fact that is unknown.
    ///
    /// How each [`CaChange`] is read is decided here and nowhere else:
    ///
    /// | `change` | the hunks are | so the landing is |
    /// |---|---|---|
    /// | `Fingerprinted(fp)` | what the port says, or a known no when `fp` has no strong hunk, or unknown under a scheme this build lacks | by hunks or ancestry, as the module says |
    /// | `NothingToFingerprint` | a known no: there is nothing to find | by ancestry; Pending when it is a no |
    /// | `NotFingerprinted(_)` | unknown, for [`UnknownWhy::NoFingerprint`]; the reason the approver's clone gave is for showing | by ancestry; else Unknown |
    /// | `NotRecorded` | as `NotFingerprinted` | as `NotFingerprinted` |
    pub fn checked(
        approved: Revision,
        change: CaChange,
        facts: BaselineFacts,
        cannot_tell: UnknownWhy,
    ) -> Self {
        Self {
            source: ChangeSource::Checked {
                approved,
                change,
                facts,
                cannot_tell,
            },
        }
    }

    /// An approval taken in from an earlier decision, which landed before the tool
    /// was in use: it needs no fact about the baseline.
    pub fn adopted() -> Self {
        Self {
            source: ChangeSource::Adopted,
        }
    }

    fn landing(&self) -> Landing {
        match &self.source {
            ChangeSource::Adopted => Landing::Integrated(IntegrationEvidence::Adopted),
            ChangeSource::Checked {
                approved,
                change,
                facts,
                cannot_tell,
            } => {
                // A change with nothing to fingerprint has no hunk to find, so the
                // hunks are a known no whatever the port says, and the ancestry alone
                // decides. The cases that have no fingerprint for another reason read
                // as no fingerprint, below.
                let nothing = matches!(change, CaChange::NothingToFingerprint);
                let fingerprint = change.fingerprint();
                // Without a fingerprint nothing can be matched, whatever the port
                // says, and that is its own reason when it is what leaves it open.
                // The same holds for a fingerprint under a scheme this build does
                // not have: its hunks cannot be read, so nothing is believed of
                // them, and the reason is the version.
                // A fingerprint of weak hunks alone can never land by them, so what
                // the port says of them is a known no: reading it as anything else
                // would let a yes about them lower an Unknown to a Pending.
                let scheme_unknown =
                    matches!(fingerprint, Some(fingerprint) if !fingerprint.scheme().is_known());
                let hunks = match fingerprint {
                    Some(_) if scheme_unknown => Knowable::Unknown,
                    Some(fingerprint) if fingerprint.is_strong() => facts.hunks_contained,
                    Some(_) => Knowable::Known(false),
                    None if nothing => Knowable::Known(false),
                    None => Knowable::Unknown,
                };
                let hunks_unknown_why = match fingerprint {
                    None => UnknownWhy::NoFingerprint,
                    Some(_) if scheme_unknown => UnknownWhy::FingerprintVersion,
                    Some(_) => *cannot_tell,
                };
                match (facts.ancestor, hunks) {
                    (Knowable::Known(true), _) => {
                        Landing::Integrated(IntegrationEvidence::Ancestor(approved.clone()))
                    }
                    (_, Knowable::Known(true)) => {
                        Landing::Integrated(IntegrationEvidence::HunksContained)
                    }
                    // The ancestry is a known no, and the hunks are a known no or
                    // too weak to count.
                    (Knowable::Known(false), Knowable::Known(false)) => {
                        Landing::Pending(approved.clone())
                    }
                    // The ancestry is a known no and it is the hunks that cannot be
                    // told: for the reason they give.
                    (Knowable::Known(false), Knowable::Unknown) => {
                        Landing::Unknown(approved.clone(), hunks_unknown_why)
                    }
                    // The ancestry cannot be told: the port's reason, which is the
                    // first thing missing, unless the fingerprint is of a scheme
                    // this build does not have: that no deeper clone mends, and
                    // it is what is said.
                    (Knowable::Unknown, _) => Landing::Unknown(
                        approved.clone(),
                        if scheme_unknown {
                            UnknownWhy::FingerprintVersion
                        } else {
                            *cannot_tell
                        },
                    ),
                }
            }
        }
    }
}

enum Landing {
    Integrated(IntegrationEvidence),
    Pending(Revision),
    Unknown(Revision, UnknownWhy),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::revision::RevisionId;

    fn rev(text: &str) -> Revision {
        Revision::Commit(RevisionId::from_validated(text.into()).unwrap())
    }

    fn path(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    fn hunk(removed: &[&str], added: &[&str]) -> Hunk {
        Hunk::new(
            path("src/a.rs"),
            removed.iter().map(|l| (*l).to_owned()).collect(),
            added.iter().map(|l| (*l).to_owned()).collect(),
        )
    }

    const LONG: &str = "let timeout = Duration::from_secs(30);";

    fn strong_fp() -> ChangeFingerprint {
        ChangeFingerprint::of_hunks(
            rev("base"),
            rev("tip"),
            &[hunk(&[], &[LONG])],
            HunkScheme::CURRENT,
        )
        .unwrap()
    }

    fn weak_fp() -> ChangeFingerprint {
        ChangeFingerprint::of_hunks(
            rev("base"),
            rev("tip"),
            &[hunk(&[], &["}"])],
            HunkScheme::CURRENT,
        )
        .unwrap()
    }

    const YES: Knowable<bool> = Knowable::Known(true);
    const NO: Knowable<bool> = Knowable::Known(false);
    const UNKNOWN: Knowable<bool> = Knowable::Unknown;

    fn facts(ancestor: Knowable<bool>, hunks: Knowable<bool>) -> BaselineFacts {
        BaselineFacts::new(ancestor, hunks)
    }

    fn change(
        fp: Option<ChangeFingerprint>,
        ancestor: Knowable<bool>,
        hunks: Knowable<bool>,
    ) -> ApprovedChange {
        ApprovedChange::checked(
            rev("a1"),
            fp.map_or(CaChange::NotRecorded, CaChange::Fingerprinted),
            facts(ancestor, hunks),
            UnknownWhy::Shallow,
        )
    }

    fn decide_with(
        change: CaChange,
        ancestor: Knowable<bool>,
        hunks: Knowable<bool>,
    ) -> Integration {
        Integration::decide([ApprovedChange::checked(
            rev("a1"),
            change,
            facts(ancestor, hunks),
            UnknownWhy::Shallow,
        )])
    }

    fn decide_one(
        fp: Option<ChangeFingerprint>,
        ancestor: Knowable<bool>,
        hunks: Knowable<bool>,
    ) -> Integration {
        Integration::decide([change(fp, ancestor, hunks)])
    }

    // --- the hunks and the weak rule --------------------------------------------

    #[test]
    fn a_hunk_is_weak_below_sixteen_characters_other_than_white_space() {
        let weight = |n: usize| hunk(&[], &[&"x".repeat(n)]);
        let v1 = &HunkScheme::V1;
        assert_eq!(weight(15).is_weak(v1), Some(true));
        assert_eq!(weight(16).is_weak(v1), Some(false));
        // The two runs are summed, and white space does not count.
        assert_eq!(hunk(&["aaaaaaaa"], &["bbbbbbbb"]).is_weak(v1), Some(false));
        assert_eq!(
            hunk(&["aaaaaaaa"], &["   bbbbbbb   "]).is_weak(v1),
            Some(true)
        );
        assert_eq!(hunk(&[], &["  }  ", "\t"]).is_weak(v1), Some(true));
    }

    #[test]
    fn a_fingerprint_is_strong_when_one_hunk_is_not_weak() {
        let both = ChangeFingerprint::of_hunks(
            rev("b"),
            rev("t"),
            &[hunk(&[], &["}"]), hunk(&[], &[LONG])],
            HunkScheme::CURRENT,
        )
        .unwrap();
        assert!(both.is_strong());
        assert!(!weak_fp().is_strong());
        assert_eq!(strong_fp().hunks().len(), 1);
    }

    #[test]
    fn a_change_with_no_hunk_has_no_fingerprint() {
        assert_eq!(
            ChangeFingerprint::of_hunks(rev("b"), rev("t"), &[], HunkScheme::CURRENT),
            Err(FingerprintError::NoHunks)
        );
    }

    #[test]
    fn the_hunks_of_a_fingerprint_are_a_set() {
        let twice = ChangeFingerprint::of_hunks(
            rev("b"),
            rev("t"),
            &[hunk(&[], &[LONG]), hunk(&[], &[LONG])],
            HunkScheme::CURRENT,
        )
        .unwrap();
        assert_eq!(twice.hunks().len(), 1);
    }

    #[test]
    fn a_hunk_keeps_its_identity_when_it_moves_or_is_respaced() {
        // The line numbers are not in the hunk at all, so moving it cannot show;
        // spacing is read away.
        let v1 = &HunkScheme::V1;
        assert_eq!(
            hunk(&["a  =  1;"], &["a = 2;"]).id(v1),
            hunk(&["  a = 1;"], &["a =\t2;  "]).id(v1)
        );
        assert_ne!(
            hunk(&["a = 1;"], &["a = 2;"]).id(v1),
            hunk(&["a = 1;"], &["a = 3;"]).id(v1)
        );
    }

    #[test]
    fn the_scheme_is_written_by_name_and_read_back() {
        assert_eq!(HunkScheme::from_name("hunks-v1"), Ok(HunkScheme::V1));
        assert_eq!(
            HunkScheme::from_name(HunkScheme::CURRENT.name()),
            Ok(HunkScheme::CURRENT)
        );
        // A name this build does not know is kept, as it was read.
        let future = HunkScheme::from_name("hunks-v9").unwrap();
        assert_eq!(future.name(), "hunks-v9");
        assert!(!future.is_known());
        assert!(HunkScheme::V1.is_known());
        // But it has to be the shape of a name.
        for bad in ["", "Hunks V9", "hunks/v9", &"x".repeat(65)] {
            assert_eq!(
                HunkScheme::from_name(bad),
                Err(ValueError::SchemeNameBadShape),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn an_unknown_scheme_is_the_same_canonical_text_and_cannot_cut_hunks() {
        let future = HunkScheme::from_name("hunks-v9").unwrap();
        assert_eq!(future.to_canon(), Canon::str("hunks-v9"));
        assert_eq!(HunkScheme::V1.to_canon(), Canon::str("hunks-v1"));
        assert_eq!(hunk(&[], &[LONG]).id(&future), None);
        assert_eq!(hunk(&[], &[LONG]).is_weak(&future), None);
        assert_eq!(
            ChangeFingerprint::of_hunks(rev("b"), rev("t"), &[hunk(&[], &[LONG])], future),
            Err(FingerprintError::UnknownScheme {
                scheme: "hunks-v9".into()
            })
        );
    }

    fn future_fp() -> ChangeFingerprint {
        let known = strong_fp();
        ChangeFingerprint::new(
            known.base().clone(),
            known.baseline_at_approval().clone(),
            known.hunks().clone(),
            true,
            HunkScheme::from_name("hunks-v9").unwrap(),
        )
    }

    #[test]
    fn a_fingerprint_of_a_scheme_this_build_has_not_got_is_unknown_unless_the_ancestry_settles_it()
    {
        let unknown = Integration::Unknown {
            approved: rev("a1"),
            why: UnknownWhy::FingerprintVersion,
        };
        // Whatever the port says of the hunks, they are not believed.
        for hunks in [YES, NO, UNKNOWN] {
            for ancestor in [NO, UNKNOWN] {
                assert_eq!(
                    decide_one(Some(future_fp()), ancestor, hunks),
                    unknown,
                    "{ancestor:?} {hunks:?}"
                );
            }
            assert!(decide_one(Some(future_fp()), YES, hunks).is_integrated());
        }
    }

    #[test]
    fn the_canonical_form_of_a_fingerprint_tells_its_parts_apart() {
        let a = strong_fp();
        assert_eq!(a.to_canon(), strong_fp().to_canon());
        let other_base = ChangeFingerprint::new(
            rev("other"),
            rev("tip"),
            a.hunks().clone(),
            true,
            HunkScheme::V1,
        );
        let weaker = ChangeFingerprint::new(
            rev("base"),
            rev("tip"),
            a.hunks().clone(),
            false,
            HunkScheme::V1,
        );
        assert_ne!(a.to_canon(), other_base.to_canon());
        assert_ne!(a.to_canon(), weaker.to_canon());
        assert_ne!(a.to_canon(), weak_fp().to_canon());
    }

    // --- the verdict of one approval --------------------------------------------

    #[test]
    fn an_ancestor_has_landed_whatever_the_hunks_say() {
        for hunks in [YES, NO, UNKNOWN] {
            for fp in [Some(strong_fp()), Some(weak_fp()), None] {
                assert_eq!(
                    decide_one(fp, YES, hunks),
                    Integration::Integrated {
                        evidence: IntegrationEvidence::Ancestor(rev("a1"))
                    }
                );
            }
        }
    }

    #[test]
    fn a_squash_is_integrated_by_its_hunks() {
        // The approved commit is not in the history; its changes are.
        for ancestor in [NO, UNKNOWN] {
            assert_eq!(
                decide_one(Some(strong_fp()), ancestor, YES),
                Integration::Integrated {
                    evidence: IntegrationEvidence::HunksContained
                }
            );
        }
    }

    #[test]
    fn a_change_of_weak_hunks_alone_does_not_land_without_its_ancestry() {
        assert_eq!(
            decide_one(Some(weak_fp()), NO, YES),
            Integration::Pending {
                approved: rev("a1")
            },
            "the ancestry is a known no, and a weak match is not evidence"
        );
        assert_eq!(
            decide_one(Some(weak_fp()), UNKNOWN, YES),
            Integration::Unknown {
                approved: rev("a1"),
                why: UnknownWhy::Shallow
            }
        );
        assert_eq!(
            decide_one(Some(weak_fp()), YES, YES),
            Integration::Integrated {
                evidence: IntegrationEvidence::Ancestor(rev("a1"))
            }
        );
    }

    #[test]
    fn neither_way_is_pending() {
        for fp in [strong_fp(), weak_fp()] {
            assert_eq!(
                decide_one(Some(fp), NO, NO),
                Integration::Pending {
                    approved: rev("a1")
                }
            );
        }
    }

    #[test]
    fn what_cannot_be_told_is_unknown_with_the_reason_the_port_gave() {
        for (ancestor, hunks) in [(UNKNOWN, UNKNOWN), (UNKNOWN, NO), (NO, UNKNOWN)] {
            for why in [
                UnknownWhy::Shallow,
                UnknownWhy::OutsideWindow,
                UnknownWhy::BaseMissing,
                UnknownWhy::BaselineUnresolved,
                UnknownWhy::FingerprintVersion,
            ] {
                let got = Integration::decide([ApprovedChange::checked(
                    rev("a1"),
                    CaChange::Fingerprinted(strong_fp()),
                    facts(ancestor, hunks),
                    why,
                )]);
                assert_eq!(
                    got,
                    Integration::Unknown {
                        approved: rev("a1"),
                        why
                    }
                );
            }
        }
    }

    #[test]
    fn an_approval_without_a_fingerprint_is_unknown_unless_the_ancestry_settles_it() {
        assert_eq!(
            decide_one(None, NO, UNKNOWN),
            Integration::Unknown {
                approved: rev("a1"),
                why: UnknownWhy::NoFingerprint
            }
        );
        // The port cannot have matched hunks that were never recorded; if it says
        // so anyway, it is not believed.
        assert_eq!(
            decide_one(None, NO, YES),
            Integration::Unknown {
                approved: rev("a1"),
                why: UnknownWhy::NoFingerprint
            }
        );
        assert!(decide_one(None, YES, UNKNOWN).is_integrated());
        // An ancestry that cannot be told keeps the port's reason, which is the
        // first thing missing.
        assert_eq!(
            decide_one(None, UNKNOWN, UNKNOWN),
            Integration::Unknown {
                approved: rev("a1"),
                why: UnknownWhy::Shallow
            }
        );
    }

    #[test]
    fn an_approval_of_a_change_with_nothing_to_fingerprint_is_judged_by_the_ancestry_alone() {
        // The ancestry settles it either way: no hunk could have landed, so a no there
        // is a Pending and not an Unknown.
        for hunks in [YES, NO, UNKNOWN] {
            assert_eq!(
                decide_with(CaChange::NothingToFingerprint, YES, hunks),
                Integration::Integrated {
                    evidence: IntegrationEvidence::Ancestor(rev("a1"))
                },
                "what the port says of hunks changes nothing"
            );
            assert_eq!(
                decide_with(CaChange::NothingToFingerprint, NO, hunks),
                Integration::Pending {
                    approved: rev("a1")
                }
            );
        }
        // An ancestry that cannot be told keeps the port's reason.
        assert_eq!(
            decide_with(CaChange::NothingToFingerprint, UNKNOWN, NO),
            Integration::Unknown {
                approved: rev("a1"),
                why: UnknownWhy::Shallow
            }
        );
    }

    #[test]
    fn an_approval_made_where_the_change_could_not_be_cut_reads_as_one_with_no_fingerprint() {
        // Whatever reason the approver's clone gave, the reading is that of a record
        // that has none: ancestry, else Unknown for the missing fingerprint.
        for why in FingerprintUnavailable::ALL {
            let recorded = CaChange::NotFingerprinted(why);
            assert_eq!(
                decide_with(recorded.clone(), NO, UNKNOWN),
                Integration::Unknown {
                    approved: rev("a1"),
                    why: UnknownWhy::NoFingerprint
                },
                "{why:?}"
            );
            assert_eq!(
                decide_with(recorded.clone(), NO, YES),
                Integration::Unknown {
                    approved: rev("a1"),
                    why: UnknownWhy::NoFingerprint
                },
                "{why:?}: hunks cannot be matched without a fingerprint"
            );
            assert!(decide_with(recorded.clone(), YES, UNKNOWN).is_integrated());
            for (ancestor, hunks) in [(NO, NO), (UNKNOWN, UNKNOWN), (YES, NO), (NO, UNKNOWN)] {
                assert_eq!(
                    decide_with(recorded.clone(), ancestor, hunks),
                    decide_with(CaChange::NotRecorded, ancestor, hunks),
                    "{why:?} reads as NotRecorded"
                );
            }
        }
    }

    #[test]
    fn the_reasons_for_no_fingerprint_have_names_that_read_back_and_words_to_show() {
        for why in FingerprintUnavailable::ALL {
            assert_eq!(why.name().parse::<FingerprintUnavailable>(), Ok(why));
            assert!(!why.to_string().is_empty());
        }
        assert!("elsewhere".parse::<FingerprintUnavailable>().is_err());
    }

    #[test]
    fn an_adopted_approval_has_landed() {
        assert_eq!(
            Integration::decide([ApprovedChange::adopted()]),
            Integration::Integrated {
                evidence: IntegrationEvidence::Adopted
            }
        );
    }

    // --- several approvals --------------------------------------------------------

    #[test]
    fn no_approval_is_unapproved() {
        assert_eq!(Integration::decide([]), Integration::Unapproved);
    }

    #[test]
    fn one_approval_that_landed_makes_the_item_integrated() {
        let pending = ApprovedChange::checked(
            rev("old"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, NO),
            UnknownWhy::Shallow,
        );
        let unknown = ApprovedChange::checked(
            rev("older"),
            CaChange::Fingerprinted(strong_fp()),
            facts(UNKNOWN, UNKNOWN),
            UnknownWhy::Shallow,
        );
        let landed = ApprovedChange::checked(
            rev("new"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, YES),
            UnknownWhy::Shallow,
        );
        let want = Integration::Integrated {
            evidence: IntegrationEvidence::HunksContained,
        };
        for order in [
            vec![pending.clone(), unknown.clone(), landed.clone()],
            vec![landed.clone(), unknown.clone(), pending.clone()],
            vec![unknown.clone(), landed.clone(), pending.clone()],
        ] {
            assert_eq!(Integration::decide(order), want);
        }
    }

    #[test]
    fn an_unknown_approval_keeps_the_item_open_beside_a_pending_one() {
        let pending = ApprovedChange::checked(
            rev("p"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, NO),
            UnknownWhy::Shallow,
        );
        let unknown = ApprovedChange::checked(
            rev("u"),
            CaChange::Fingerprinted(strong_fp()),
            facts(UNKNOWN, UNKNOWN),
            UnknownWhy::BaseMissing,
        );
        let want = Integration::Unknown {
            approved: rev("u"),
            why: UnknownWhy::BaseMissing,
        };
        assert_eq!(
            Integration::decide([pending.clone(), unknown.clone()]),
            want
        );
        assert_eq!(Integration::decide([unknown, pending]), want);
    }

    #[test]
    fn the_verdict_of_several_pending_approvals_does_not_depend_on_their_order() {
        let a = ApprovedChange::checked(
            rev("a"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, NO),
            UnknownWhy::Shallow,
        );
        let b = ApprovedChange::checked(
            rev("b"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, NO),
            UnknownWhy::Shallow,
        );
        assert_eq!(
            Integration::decide([a.clone(), b.clone()]),
            Integration::decide([b, a])
        );
    }

    #[test]
    fn the_ancestry_is_preferred_as_evidence_when_both_approvals_landed() {
        let by_hunks = ApprovedChange::checked(
            rev("h"),
            CaChange::Fingerprinted(strong_fp()),
            facts(NO, YES),
            UnknownWhy::Shallow,
        );
        let by_ancestry = ApprovedChange::checked(
            rev("z"),
            CaChange::Fingerprinted(strong_fp()),
            facts(YES, NO),
            UnknownWhy::Shallow,
        );
        let adopted = ApprovedChange::adopted();
        let want = Integration::Integrated {
            evidence: IntegrationEvidence::Ancestor(rev("z")),
        };
        assert_eq!(
            Integration::decide([adopted.clone(), by_hunks.clone(), by_ancestry.clone()]),
            want
        );
        assert_eq!(Integration::decide([by_ancestry, by_hunks, adopted]), want);
    }

    // --- monotonic ------------------------------------------------------------------

    fn rank(integration: &Integration) -> u8 {
        match integration {
            Integration::Unapproved | Integration::Pending { .. } => 0,
            Integration::Unknown { .. } => 1,
            Integration::Integrated { .. } => 2,
        }
    }

    /// A fact that gets better never lowers the verdict: when a fact that was a no or
    /// a cannot-tell becomes a yes, an integrated item stays integrated and an
    /// unknown one does not become pending.
    #[test]
    fn a_fact_that_turns_to_yes_never_lowers_the_verdict() {
        let facts_all = [YES, NO, UNKNOWN];
        for fp in [Some(strong_fp()), Some(weak_fp()), None] {
            for ancestor in facts_all {
                for hunks in facts_all {
                    let before = decide_one(fp.clone(), ancestor, hunks);
                    for (better_ancestor, better_hunks) in
                        [(YES, hunks), (ancestor, YES), (YES, YES)]
                    {
                        let after = decide_one(fp.clone(), better_ancestor, better_hunks);
                        assert!(
                            rank(&after) >= rank(&before),
                            "{fp:?} {ancestor:?} {hunks:?} -> {better_ancestor:?} {better_hunks:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn what_the_port_cannot_tell_of_weak_hunks_does_not_leave_the_item_open() {
        // Weak hunks can never land the change, so a known no on the ancestry is
        // Pending whatever the port says of them.
        for hunks in [YES, NO, UNKNOWN] {
            assert_eq!(
                decide_one(Some(weak_fp()), NO, hunks),
                Integration::Pending {
                    approved: rev("a1")
                },
                "{hunks:?}"
            );
        }
    }

    /// Another approval never lowers the verdict either.
    #[test]
    fn a_further_approval_never_lowers_the_verdict() {
        let facts_all = [YES, NO, UNKNOWN];
        let mut approvals = Vec::new();
        for fp in [Some(strong_fp()), Some(weak_fp()), None] {
            for ancestor in facts_all {
                for hunks in facts_all {
                    approvals.push(change(fp.clone(), ancestor, hunks));
                }
            }
        }
        for first in &approvals {
            let alone = Integration::decide([first.clone()]);
            for second in &approvals {
                let both = Integration::decide([first.clone(), second.clone()]);
                assert!(rank(&both) >= rank(&alone));
            }
        }
    }
}
