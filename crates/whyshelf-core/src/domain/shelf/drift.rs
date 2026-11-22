//! What a pinned content comes to now: the one place the hash an approval pinned and
//! what is there now are told the same, edited, or edited and kept in its meaning, and
//! for an artifact, gone, arrived, still absent, not told now, or remade by another
//! comparison.
//!
//! There are two kinds of content, a document's and an artifact's, and one place for
//! each: [`DocDrift`] for a document (the approved one, or an upstream, alike) and
//! [`ArtifactChange`] for an artifact. Which role a content has in an approval, and the
//! words of what it came to, are its readers': freshness makes causes and notices of
//! it, and `diff` shows it.
//!
//! ```text
//!   the hash pinned ─┐
//!   what is there now ┼─► Edit: Same | Edited | KeptMeaning (a declaration joins them)
//!   the declarations ─┘      │
//!                            ├─► DocDrift       (a document, or why it is no valid document)
//!                            └─► ArtifactChange (Content | Gone | Arrived | StillAbsent | Untold)
//! ```
//!
//! It asks nothing: what is there now, and which declarations join two hashes, are
//! handed in. The evidence that a declaration joins them is the caller's `D`, and a
//! caller that asks no declaration (`diff`) says so with
//! [`Infallible`](std::convert::Infallible), so that it can hold no
//! [`Edit::KeptMeaning`]. Why a document is not valid now is the caller's `F`, carried
//! as it is.

use super::artifact::{ArtifactHash, ArtifactSeen, Comparison, Untold};
use super::doc::DocHash;

/// The hash a content was pinned with, beside its hash now: the same, edited, or
/// edited and kept in its meaning by the declarations `declared` (of type `D`).
///
/// Two hashes that differ are an edit unless a declaration joins them; two that are
/// the same are the same, and no declaration is asked of them. The one place that rule
/// is applied, for both kinds of content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit<H, D> {
    /// The hash is the one pinned.
    Same {
        /// The hash, then and now.
        then: H,
    },
    /// The hash differs, and no declaration joins the two.
    Edited {
        /// The hash pinned.
        then: H,
        /// The hash now.
        now: H,
    },
    /// The hash differs, and declarations join the two: the edit keeps the meaning.
    KeptMeaning {
        /// The hash pinned.
        then: H,
        /// The hash now.
        now: H,
        /// The declarations that join them.
        declared: D,
    },
}

impl<H: Copy, D> Edit<H, D> {
    /// The hash pinned.
    pub fn then(&self) -> H {
        match self {
            Self::Same { then } | Self::Edited { then, .. } | Self::KeptMeaning { then, .. } => {
                *then
            }
        }
    }

    /// The hash now.
    pub fn now(&self) -> H {
        match self {
            Self::Same { then: now } | Self::Edited { now, .. } | Self::KeptMeaning { now, .. } => {
                *now
            }
        }
    }
}

/// The [`Edit`] from `then` to `now`: `join` is asked whether declarations join the two
/// only when they differ.
pub(crate) fn edit_of<H: Eq + Copy, D>(
    then: H,
    now: H,
    join: impl FnOnce(&H, &H) -> Option<D>,
) -> Edit<H, D> {
    if then == now {
        return Edit::Same { then };
    }
    match join(&then, &now) {
        Some(declared) => Edit::KeptMeaning {
            then,
            now,
            declared,
        },
        None => Edit::Edited { then, now },
    }
}

/// What a pinned document comes to now: an [`Edit`] of its hash when it is a valid
/// document, else why it is not (`F`, as the caller carries it).
pub type DocDrift<D, F> = Result<Edit<DocHash, D>, F>;

/// The [`DocDrift`] of a document pinned with `then`, whose hash is `now` when it is a
/// valid document.
pub(crate) fn doc_drift<D, F>(
    then: DocHash,
    now: Result<DocHash, F>,
    join: impl FnOnce(&DocHash, &DocHash) -> Option<D>,
) -> DocDrift<D, F> {
    now.map(|now| edit_of(then, now, join))
}

/// What a pinned artifact comes to now, its hashes made by the comparison it was
/// pinned with (rule A3): there then and now ([`Edit`]), gone, arrived where there was
/// nothing, nothing then and nothing now, or not told now, with why. A name that cannot
/// be told now is not read as absent (DEC-45).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactChange<D> {
    /// There then and there now.
    Content(Edit<ArtifactHash, D>),
    /// There then, nothing there now.
    Gone {
        /// The hash pinned.
        then: ArtifactHash,
    },
    /// Nothing there then, there now.
    Arrived {
        /// The hash now.
        now: ArtifactHash,
    },
    /// Nothing there then, nothing there now.
    StillAbsent,
    /// What is there cannot be told now.
    Untold(Untold),
}

impl<D> ArtifactChange<D> {
    /// What an artifact pinned as `seen` comes to, given what is there now (`now`, or
    /// why it cannot be told); `join` is asked only of two hashes there that differ.
    pub(crate) fn of(
        seen: ArtifactSeen,
        now: Result<ArtifactSeen, &Untold>,
        join: impl FnOnce(&ArtifactHash, &ArtifactHash) -> Option<D>,
    ) -> Self {
        match now {
            Err(why) => Self::Untold(why.clone()),
            Ok(now) => match (seen, now) {
                (ArtifactSeen::Present(then), ArtifactSeen::Present(now)) => {
                    Self::Content(edit_of(then, now, join))
                }
                (ArtifactSeen::Present(then), ArtifactSeen::Absent) => Self::Gone { then },
                (ArtifactSeen::Absent, ArtifactSeen::Present(now)) => Self::Arrived { now },
                (ArtifactSeen::Absent, ArtifactSeen::Absent) => Self::StillAbsent,
            },
        }
    }
}

/// What a pinned artifact comes to under the comparison its link declares now: remade
/// by another comparison than the one it was pinned with, when it is there then and
/// now (two hashes made differently are not compared), else its [`ArtifactChange`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArtifactDrift<D> {
    /// There then and now, and the link declares another comparison than the pin's.
    Remade {
        /// The comparison the link declares now.
        now: Comparison,
    },
    /// What it came to, its hashes made the same way.
    Change(ArtifactChange<D>),
}

impl<D> ArtifactDrift<D> {
    /// What an artifact pinned as `seen` by the comparison `pinned` comes to, given
    /// what is there now under the comparison its link `declared`: a name that cannot
    /// be told now is not told whatever the comparison, and one gone or come has no two
    /// hashes to compare, so only one there then and now is remade.
    pub(crate) fn of(
        seen: ArtifactSeen,
        pinned: &Comparison,
        now: Result<ArtifactSeen, &Untold>,
        declared: &Comparison,
        join: impl FnOnce(&ArtifactHash, &ArtifactHash) -> Option<D>,
    ) -> Self {
        match (seen, now) {
            (ArtifactSeen::Present(_), Ok(ArtifactSeen::Present(_))) if pinned != declared => {
                Self::Remade {
                    now: declared.clone(),
                }
            }
            (seen, now) => Self::Change(ArtifactChange::of(seen, now, join)),
        }
    }
}

#[cfg(test)]
mod tests;
