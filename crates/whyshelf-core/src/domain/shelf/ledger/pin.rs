//! What an approval stood on, pin by pin: a [`FramePin`] for an upstream, an
//! [`ArtifactPin`] for an artifact (with the [`PinnedArtifact`] it saw), the [`Seen`]
//! of a retirement, and the one comparison of what two approvals stand on.

use std::collections::BTreeSet;
use std::fmt;

use crate::domain::id::DocId;

use super::super::artifact::{
    ArtifactHash, ArtifactKey, ArtifactName, ArtifactSeen, Comparison, ContentAddress, Space,
};
use super::super::build::{FailureTag, NowNamed};
use super::super::doc::{DocHash, DocKey};
use super::super::schema::EntryName;

/// What an approval stood on for one link: the upstream it named, the key it had
/// then and the hash it had then.
///
/// The key is kept so that an upstream that has since gone can be told from a link
/// that is written wrong now: both are a miss in the shelf, and only the key says
/// whether they are the same one. It is not part of any hash.
///
/// Its constructor is private to the ledger's modules: a pin is taken from a resolved
/// link by [`frame_approve`](super::frame_approve), or read back from a record by
/// [`Record::from_canon`](super::Record::from_canon), and by nothing else.
///
/// It may carry the [`ContentAddress`] of the upstream's file as it was (rule A5),
/// so that the content can be read back later; freshness never reads it (A4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FramePin {
    link: EntryName,
    pub(super) target: DocId,
    key: DocKey,
    hash: DocHash,
    pub(super) address: Option<ContentAddress>,
}

impl FramePin {
    pub(super) fn new(
        link: EntryName,
        target: DocId,
        key: DocKey,
        hash: DocHash,
        address: Option<ContentAddress>,
    ) -> Self {
        Self {
            link,
            target,
            key,
            hash,
            address,
        }
    }

    /// Where the upstream's content, as it was pinned, can be read back; none when
    /// the adapter could not say, or the record is of a version that kept none.
    pub fn address(&self) -> Option<&ContentAddress> {
        self.address.as_ref()
    }

    /// The link of the approved document the pin was taken for.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// The upstream.
    pub fn target(&self) -> DocId {
        self.target
    }

    /// The key the upstream had when it was pinned.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The hash the upstream had when it was pinned.
    pub fn hash(&self) -> &DocHash {
        &self.hash
    }

    /// What the pin stands on: the link, the upstream and its hash. The address is
    /// left out, as freshness never reads it (rule A4), and so is the key: it is the
    /// name the upstream had when it was pinned, which a command that renumbers the
    /// document changes and no approval stands on (P3), so two pins with the same
    /// value here stand on the same, and an approval is repeated by one that pins the
    /// upstream under its key now.
    fn stands_on(&self) -> (&EntryName, DocId, &DocHash) {
        (&self.link, self.target, &self.hash)
    }

    /// How an answer names the upstream of this pin, given what its identity comes
    /// to now: by the key it has now when it has one, and the key it had when it was
    /// pinned beside it when that is another.
    ///
    /// The one place a pin is named for an answer (P2): the key of a pin is the key
    /// of a time that is gone once the document is renumbered, so an answer that
    /// names a pin by it names a document that is not there under that key.
    pub fn named(&self, now: &NowNamed) -> PinName {
        let pinned = self.key.clone();
        match now {
            NowNamed::Valid(key) if *key == pinned => PinName::Same(pinned),
            NowNamed::Valid(key) => PinName::Renamed {
                now: key.clone(),
                pinned,
            },
            NowNamed::Invalid(key) => PinName::Invalid {
                now: key.clone(),
                pinned,
            },
            NowNamed::Gone => PinName::Gone(pinned),
            NowNamed::Unknown => PinName::Unknown(pinned),
        }
    }
}

/// How an answer names the upstream of a pin: what [`FramePin::named`] reads from
/// what the identity comes to now ([`NowNamed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinName {
    /// A valid document with the key it had when it was pinned.
    Same(DocKey),
    /// A valid document with another key now.
    Renamed {
        /// The key it has now.
        now: DocKey,
        /// The key it had when it was pinned.
        pinned: DocKey,
    },
    /// Invalid now: with the key its files write now when they write one that is
    /// told (they may write two), and the key it had when it was pinned.
    Invalid {
        /// The key it has now, when it can be told.
        now: Option<DocKey>,
        /// The key it had when it was pinned.
        pinned: DocKey,
    },
    /// No file carries it: the key it had when it was pinned.
    Gone(DocKey),
    /// Not knowable now: the key it had when it was pinned.
    Unknown(DocKey),
}

impl PinName {
    /// The key to name the document by: the one it has now when there is one, else the
    /// one it had when it was pinned.
    pub fn key(&self) -> &DocKey {
        match self {
            Self::Same(key) | Self::Gone(key) | Self::Unknown(key) => key,
            Self::Renamed { now, .. } => now,
            Self::Invalid { now, pinned } => now.as_ref().unwrap_or(pinned),
        }
    }

    /// The key it had when it was pinned, when that is not the key it has now.
    pub fn was(&self) -> Option<&DocKey> {
        match self {
            Self::Renamed { pinned, .. } => Some(pinned),
            Self::Invalid {
                now: Some(now),
                pinned,
            } if now != pinned => Some(pinned),
            Self::Same(_) | Self::Invalid { .. } | Self::Gone(_) | Self::Unknown(_) => None,
        }
    }
}

/// The name as a subject of a sentence: the key it has now, and `(pinned as K)` when
/// it was pinned under another.
impl fmt::Display for PinName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.was() {
            Some(pinned) => write!(f, "{} (pinned as {pinned})", self.key()),
            None => fmt::Display::fmt(self.key(), f),
        }
    }
}

/// What an approval saw of an artifact it stands on: a thing there with this hash
/// (and where its content can be read back, when that could be said), or nothing
/// there.
///
/// An absent artifact has no address by the type: there is nothing to read back. An
/// artifact that could not be told about is never pinned:
/// [`frame_approve`](super::frame_approve) refuses it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PinnedArtifact {
    /// A thing there, with the hash of its content.
    Present {
        /// The hash.
        hash: ArtifactHash,
        /// Where the content can be read back, when it could be said.
        address: Option<ContentAddress>,
    },
    /// Nothing was there (rule A2: an approval pins an absence too).
    Absent,
}

impl PinnedArtifact {
    /// What was there, without the address: what freshness compares (rule A4).
    pub fn state(&self) -> ArtifactSeen {
        match self {
            Self::Present { hash, .. } => ArtifactSeen::Present(*hash),
            Self::Absent => ArtifactSeen::Absent,
        }
    }

    /// Where the content can be read back.
    pub fn address(&self) -> Option<&ContentAddress> {
        match self {
            Self::Present { address, .. } => address.as_ref(),
            Self::Absent => None,
        }
    }
}

/// How a record wrote the artifact of a pin or of a declaration, kept so that a
/// record read back is written as it was stored, with the same record hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum NameForm {
    /// `space` and `name`: how a record is written.
    Spaced,
    /// `path` and no `space`: a record written before spaces existed. Its path is a
    /// name in the space [`PATH_ONLY_SPACE`], which is what it was.
    PathOnly,
}

/// The space a record written before spaces existed is read as naming its artifacts
/// in: every artifact then was a file, named by its path, so a pin or a declaration
/// with a `path` and no `space` is that path in this space. That is all it is: the
/// domain has no default space (a declaration names its spaces itself, and a record
/// made now writes its space), and the adapter that resolves the files takes its
/// space's name from here, so an old record always names a space that adapter
/// answers for.
pub const PATH_ONLY_SPACE: &str = "file";

/// What an approval stood on for one name of a `Depends` link to artifacts: the
/// link, the space and the name, and what was there (rule A2).
///
/// Its constructor is private to the ledger's modules, like a [`FramePin`]'s: it is
/// taken from an [`Artifact`](super::super::artifact::Artifact) of the document by
/// [`frame_approve`](super::frame_approve), or read back from a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactPin {
    pub(super) link: EntryName,
    pub(super) artifact: ArtifactKey,
    pub(super) seen: PinnedArtifact,
    /// How the hash in `seen` was made. A record written before comparisons existed
    /// has none, and is [`Comparison::Bytes`], as it was made.
    pub(super) comparison: Comparison,
    pub(super) form: NameForm,
}

impl ArtifactPin {
    /// How the hash of what was there was made.
    pub fn comparison(&self) -> &Comparison {
        &self.comparison
    }

    /// The link of the approved document the pin was taken for.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// The space and the name of the artifact, as the pin wrote them. The name it is
    /// read as now, after a declared move, is a [`ReadPin`](super::ReadPin)'s.
    pub fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    /// The space of the artifact.
    pub fn space(&self) -> &Space {
        self.artifact.space()
    }

    /// The name of the artifact.
    pub fn name(&self) -> &ArtifactName {
        self.artifact.name()
    }

    /// What was there when it was pinned.
    pub fn seen(&self) -> &PinnedArtifact {
        &self.seen
    }

    /// What the pin stands on: the link, the space and the name, the comparison and
    /// what was there (a hash, or nothing). The address is left out (rule A4), and so
    /// is how the record wrote the name: a record of before spaces names the same
    /// artifact in [`PATH_ONLY_SPACE`].
    fn stands_on(&self) -> (&EntryName, &ArtifactKey, &Comparison, ArtifactSeen) {
        (
            &self.link,
            &self.artifact,
            &self.comparison,
            self.seen.state(),
        )
    }
}

/// What an approval stands on, pin by pin, as values: the hash of the approved
/// document, every pin of an upstream and every pin of an artifact, each by
/// [`FramePin::stands_on`] and [`ArtifactPin::stands_on`]. The one place two
/// approvals are compared for what they stand on.
#[derive(PartialEq, Eq)]
pub(super) struct StandsOn<'a> {
    seen: &'a DocHash,
    frame: BTreeSet<(&'a EntryName, DocId, &'a DocHash)>,
    artifacts: BTreeSet<(&'a EntryName, &'a ArtifactKey, &'a Comparison, ArtifactSeen)>,
}

impl<'a> StandsOn<'a> {
    pub(super) fn of(
        seen: &'a DocHash,
        frame: &'a BTreeSet<FramePin>,
        artifacts: &'a BTreeSet<ArtifactPin>,
    ) -> Self {
        Self {
            seen,
            frame: frame.iter().map(FramePin::stands_on).collect(),
            artifacts: artifacts.iter().map(ArtifactPin::stands_on).collect(),
        }
    }
}

/// What a retirement saw of the document it retires.
///
/// A document can be retired when its file is gone, or broken, or cannot be told
/// about; then there is no hash to see, and the record says which of these it was
/// (the [`FailureTag`] of the [`DocFailure`](super::super::build::DocFailure), without
/// what it carries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// The document was valid, with this hash.
    At(DocHash),
    /// The document was not valid, in this way.
    Not(FailureTag),
}
