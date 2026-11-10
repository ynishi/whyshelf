//! A file of a project that is no document: code, a test, data.
//!
//! A document may stand on such a file (`depends`) or point at it (`informs`), as it
//! does on another document. What an approval wants to keep of it is the same as of
//! an upstream document: *what the file was when the approval was made*, so that a
//! later change makes the approval stale. The model holds four values for that, none
//! of which knows a version-control system:
//!
//! | value | what it is | who makes it |
//! |---|---|---|
//! | [`Artifact`] | a path under the root, and whether a file is there now with which [`ArtifactHash`] | the build of a shelf, from an [`ArtifactReading`] (rule A1) |
//! | [`ArtifactHash`] | the hash of the file's bytes: the only thing freshness compares (A3, A4) | [`ArtifactHash::of_content`], over the bytes the adapter chose to read |
//! | [`ContentAddress`] | where the content can be read back later; opaque, only compared | an adapter, after its own check ([`ContentAddress::from_validated`], A7) |
//! | [`Revision`] | one point of the project's history, kept as a hint; opaque | the same ([`Revision::from_validated`]) |
//!
//! ```text
//!   adapter ──read──► ArtifactReading (hash, address?)  or  Absent
//!                        │
//!   Shelf::build ────────┴──► Artifact { path, ArtifactState }   (in a Doc's artifact links)
//!   frame_approve ──────────► ArtifactPin { link, path, PinnedArtifact }   (ledger)
//!   assess ─────────────────► compares ArtifactHash only; addresses never count
//! ```
//!
//! # Opaque values
//!
//! A [`ContentAddress`] and a [`Revision`] have no form the domain knows: an adapter
//! checks the text it read against its own rules and only then makes
//! one with `from_validated`, which checks the weakest rules only (not empty, no
//! white space, no control character). The domain compares and shows them and never
//! reads through them; reading content back is a port of the application.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::location::RelPath;

/// The hash of the content of an artifact: what an approval pins and what freshness
/// compares.
///
/// It covers what the adapter hands to [`ArtifactHash::of_content`] (the whole file,
/// as the adapter of this project reads it); which part of a file counts is the
/// adapter's choice, so it can change without touching the domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactHash(ContentHash);

impl ArtifactHash {
    /// The hash of these bytes, under its own [`HashKind`], so it never equals the
    /// hash of a document for the same bytes.
    pub fn of_content(bytes: &[u8]) -> Self {
        Self(ContentHash::of_bytes(HashKind::ShelfArtifact, bytes))
    }

    /// The hash a record of the ledger kept: only where a record is read back.
    pub(super) fn restore(hash: ContentHash) -> Self {
        Self(hash)
    }

    /// The digest.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

impl fmt::Display for ArtifactHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Why a text is not an opaque value ([`ContentAddress`], [`Revision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OpaqueProblem {
    /// The text is empty.
    #[error("an opaque value is not empty")]
    Empty,
    /// The text holds white space or a control character.
    #[error("an opaque value holds no white space and no control character")]
    BadChar,
}

fn check_opaque(raw: &str) -> Result<(), OpaqueProblem> {
    if raw.is_empty() {
        return Err(OpaqueProblem::Empty);
    }
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(OpaqueProblem::BadChar);
    }
    Ok(())
}

macro_rules! opaque {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// A value an adapter has already checked against its own form (rule
            /// A7). Only the weakest rules are checked here: not empty, no white
            /// space, no control character.
            pub fn from_validated(raw: String) -> Result<Self, OpaqueProblem> {
                check_opaque(&raw)?;
                Ok(Self(raw))
            }

            /// The value as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque!(
    /// Where the content of a file, as it was, can be read back later.
    ///
    /// Opaque: the domain only compares it and keeps it in a record (rule A5), and
    /// never decides freshness with it (A4). What it is, is the adapter's.
    ContentAddress
);

opaque!(
    /// One point of the history of the project, kept by an approval as a hint of
    /// where it was made.
    ///
    /// Opaque, like [`ContentAddress`], and never part of a freshness (rule A4).
    Revision
);

/// What is at the path of an artifact now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactState {
    /// A file, with the hash of its content.
    Present(ArtifactHash),
    /// No file. A link to a path with no file is still an artifact (rule A1): the
    /// evidence may not be written yet.
    Absent,
}

impl ArtifactState {
    /// The hash, when a file is there.
    pub fn hash(&self) -> Option<&ArtifactHash> {
        match self {
            Self::Present(hash) => Some(hash),
            Self::Absent => None,
        }
    }
}

/// What an adapter found at one path: the hash of the file and where its content can
/// be read back, or no file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactReading {
    /// A file is there.
    Present {
        /// The hash of what the adapter read of it.
        hash: ArtifactHash,
        /// Where its content can be read back, when the adapter can say.
        address: Option<ContentAddress>,
    },
    /// No file is there.
    Absent,
}

impl ArtifactReading {
    /// What the shelf keeps of it: the hash or its absence, without the address,
    /// which no judgement of freshness reads (rule A4).
    pub fn state(&self) -> ArtifactState {
        match self {
            Self::Present { hash, .. } => ArtifactState::Present(*hash),
            Self::Absent => ArtifactState::Absent,
        }
    }

    /// Where the content can be read back, when a file is there and the adapter
    /// could say.
    pub fn address(&self) -> Option<&ContentAddress> {
        match self {
            Self::Present { address, .. } => address.as_ref(),
            Self::Absent => None,
        }
    }
}

/// What an adapter found at each path it was asked about.
///
/// A path it was not asked about, or did not answer for, is no file here: the build
/// asks for every path the documents write, so a hole is a file that is not there.
pub type ArtifactReadings = BTreeMap<RelPath, ArtifactReading>;

/// A file of the project a document links to: its path, and what is there now.
///
/// Made by the build only, from what the adapter read, so what it says is what the
/// project has: an approval pins it as it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Artifact {
    path: RelPath,
    state: ArtifactState,
}

impl Artifact {
    pub(super) fn new(path: RelPath, state: ArtifactState) -> Self {
        Self { path, state }
    }

    /// The path, under the root of the project.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// What is there now.
    pub fn state(&self) -> &ArtifactState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_value_is_not_empty_and_has_no_space() {
        assert_eq!(
            ContentAddress::from_validated(String::new()),
            Err(OpaqueProblem::Empty)
        );
        assert_eq!(
            Revision::from_validated("a b".to_owned()),
            Err(OpaqueProblem::BadChar)
        );
        assert_eq!(
            ContentAddress::from_validated("e69de29".to_owned())
                .unwrap()
                .as_str(),
            "e69de29"
        );
    }

    #[test]
    fn the_hash_of_an_artifact_is_kept_apart_from_other_hashes_of_the_same_bytes() {
        let bytes = b"fn main() {}\n";
        let artifact = ArtifactHash::of_content(bytes);
        assert_eq!(artifact, ArtifactHash::of_content(bytes));
        assert_ne!(
            *artifact.hash(),
            ContentHash::of_bytes(HashKind::ShelfDoc, bytes)
        );
        assert_ne!(artifact, ArtifactHash::of_content(b"fn main() { }\n"));
    }

    #[test]
    fn a_reading_keeps_its_address_apart_from_its_state() {
        let hash = ArtifactHash::of_content(b"x");
        let address = ContentAddress::from_validated("abc".to_owned()).unwrap();
        let reading = ArtifactReading::Present {
            hash,
            address: Some(address.clone()),
        };
        assert_eq!(reading.state(), ArtifactState::Present(hash));
        assert_eq!(reading.address(), Some(&address));
        assert_eq!(ArtifactReading::Absent.state(), ArtifactState::Absent);
        assert_eq!(ArtifactReading::Absent.address(), None);
    }
}
