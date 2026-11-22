//! Giving one document a new key, and what the links that name its old key come to.
//!
//! Two branches that each took the next key of a book leave two documents with one key
//! after they merge. One of them is to take another key, and then each link that wrote
//! the old key has to be read: does it mean the document that took the new key, the
//! other, or both? A link writes a key as a text, so once the branches are merged
//! nothing in the file says which of the two it meant; what does is the approval of the
//! document that wrote it, which pinned the identity it stood on (`FramePin`), and a
//! person who knows. [`plan`] reads each of these documents by that evidence and by the
//! words the caller gave, and gives the plan when every document is decided, or refuses
//! with every one that is not: it decides nothing by a default, since a default for a
//! link that is not known is the mistake of taking the other branch's link.
//!
//! ```text
//!   RekeyAsk { target, key?, words }
//!     │
//!     ├─ the target: one file, with an id and a key read (R2)
//!     ├─ Shelf::key_writers + Shelf::link_writers ──► gather ──► Gathered
//!     │        every writer of the key and every link that writes it was read,
//!     │        or the plan is refused naming what was not (R3)
//!     ├─ the new key: Shelf::claim_key when given, else Shelf::next_key; its file
//!     ├─ each document that writes the key in a link: meaning(&Gathered, ..) ──► Reading
//!     │        Decided | Undecided | Contradicted
//!     └─ any Undecided or Contradicted: every one is listed and nothing is planned (R4)
//!        else RekeyPlan: the target's new key, each document's meaning and the items
//!        it writes, and the documents whose body names the old key
//!           ├─ files(), entries_of(..): the files it writes and what each is to hold
//!           └─ confirm(..): the files, as the adapter says they read, built by
//!              Shelf::build and held against the plan (R7), before anything is written
//! ```
//!
//! # What a link means
//!
//! The evidence, in this order (M1, M2):
//!
//! 1. The old key has no other writer ([`Evidence::OnlyWriter`]): every link that
//!    writes it means the target. [`Gathered`] says so, since it is made only from
//!    writers that were all read.
//! 2. The approval that stands about the document that writes the link
//!    (`Ledger::standing_decision`; one that is not an approval is no evidence)
//!    pinned, for that link, a document that writes the old key: the target
//!    ([`Evidence::Pinned`] of [`Side::Follow`]), another writer ([`Side::Stay`]), or
//!    both ([`Side::Both`]).
//!
//! The caller's words ([`Side`], for a document) are laid beside the evidence and not
//! dropped when there is some (M3): a word that agrees with it, or `--both` (which
//! contains whichever side a pin is on), decides; a word that disagrees with it, and
//! evidence that points at both sides with no word, is a [`Contradiction`]. With no
//! evidence the word decides, and with neither the document is
//! [`Undecided`](Reading::Undecided).
//!
//! The unit is a document. A document that wrote the old key in two links is read once.
//! One older than both sides may have added the same item on both, and so a single
//! entry may point at two documents; that is what `--both` is for (M4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::{
    ClaimKeyProblem, DocState, NewDocProblem, NewKey, NextKeyProblem, NowNamed, Shelf, Unread,
    Writers,
};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::ledger::{DecisionVerb, Ledger};
use crate::domain::shelf::schema::EntryName;
use crate::domain::words::{Join, Mark, listed};

mod files;
mod gather;

pub use files::{Confirmed, Difference, Files, PlanDiffers};
pub use gather::Gathered;
use gather::gather;

/// What a caller says of a document that writes the old key in a link: which document
/// its link means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// The link means the document that takes the new key: it is rewritten to the new
    /// key.
    Follow,
    /// The link means the other writer of the old key: it is left as it is.
    Stay,
    /// The link means both (an entry may point at two documents, M4): it keeps the old
    /// key and gains the new one.
    Both,
}

/// What `rekey` is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyAsk {
    /// The document that takes the new key.
    pub target: DocId,
    /// The new key, as written, when the caller gives one; else the next of the book.
    pub key: Option<String>,
    /// What the caller says of each document that writes the old key in a link.
    pub words: BTreeMap<DocId, Side>,
}

/// Why a document's link follows the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowWhy {
    /// Nobody else writes the old key: every link to it means the target.
    OnlyWriter,
    /// The approval of the document pinned the target for the link.
    Pinned,
    /// The caller said so.
    Asked,
}

/// Why a document's link stays.
///
/// There is no `OnlyWriter`: when nobody else writes the old key, a link has nothing
/// to stay with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayWhy {
    /// The approval of the document pinned another writer of the old key for the link.
    Pinned,
    /// The caller said so.
    Asked,
}

/// What the links of one document that write the old key mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meaning {
    /// They mean the target: they are rewritten to the new key.
    Follows(FollowWhy),
    /// They mean another writer: they are left as they are.
    Stays(StayWhy),
    /// They mean both: they keep the old key and gain the new one. Only a caller can
    /// say it (the evidence of a pin names one side or, when it names both, is
    /// contradicted until the caller says this).
    Both,
}

/// What the evidence about one document says, when it says something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Nobody else writes the old key.
    OnlyWriter,
    /// The approval of the document pinned this side for its links to the old key.
    Pinned(Side),
}

/// A document the evidence and the caller's word do not agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contradiction {
    /// The evidence.
    pub evidence: Evidence,
    /// The caller's word; none when the evidence contradicts itself (it names both
    /// sides) and the caller said nothing.
    pub asked: Option<Side>,
}

/// What there is to go on for a document that is [`Undecided`](Reading::Undecided):
/// why there is no evidence, so that the caller knows what to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hints {
    /// No approval stands about the document (it is pending, or decided against), so
    /// there is no pin to read.
    NoApproval,
    /// An approval stands, and pinned no writer of the old key for the link.
    NoPin,
    /// The file carries no identity that can be read, so no record and no word can
    /// name it.
    NoIdentity,
}

/// What one document that writes the old key in a link comes to: [`meaning`]'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// The meaning is known, from the evidence or from the caller.
    Decided(Meaning),
    /// There is no evidence and no word.
    Undecided(Hints),
    /// The evidence and the word, or the evidence with itself, disagree.
    Contradicted(Contradiction),
}

/// Why `rekey` gives no plan.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RekeyProblem {
    /// No file carries the document.
    #[error("no file in the books' ranges carries the document")]
    Gone,
    /// No file is known to carry the document, but some file's identity could not be
    /// read.
    #[error(
        "whether a file carries the document cannot be told; unreadable: {}",
        listed(unreadable.iter(), Mark::Plain, Join::Semicolon)
    )]
    Unknown {
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
    },
    /// The identity is in more than one file, so the document is not one file to
    /// give a key to.
    #[error(
        "the identity is in {} files: {}",
        files.len(),
        listed(files.iter(), Mark::Plain, Join::And)
    )]
    InSeveralFiles {
        /// The files.
        files: NonEmptyVec<RelPath>,
    },
    /// The file of the document writes no key that can be read as one, or its files
    /// write two.
    #[error(
        "the key of the document cannot be told from {}",
        listed(files.iter(), Mark::Plain, Join::And)
    )]
    NoKey {
        /// The files of the document.
        files: NonEmptyVec<RelPath>,
    },
    /// The new key was given, and cannot be taken.
    #[error(transparent)]
    ClaimKey(ClaimKeyProblem),
    /// No next key can be chosen.
    #[error(transparent)]
    NextKey(NextKeyProblem),
    /// The file of the new key is no file.
    #[error(transparent)]
    File(Box<NewDocProblem>),
    /// A place that may write the old key, or write it in a link, could not be read, so
    /// that the writers and the links are not known whole.
    #[error(
        "not every file that may write the key, or link to it, could be read: {}",
        listed(keys.iter().map(ToString::to_string).chain(links.iter().map(ToString::to_string)), Mark::Plain, Join::Semicolon)
    )]
    NotAllRead {
        /// The files of the key's book that could not be read at all and may write the
        /// key.
        keys: Vec<RelPath>,
        /// The places that may write the key in a link and could not be read.
        links: Vec<Unread>,
    },
    /// A word names a document that does not write the old key in a link.
    #[error("{doc} does not write the key in a link, so nothing is said of it")]
    NotAWriter {
        /// The document.
        doc: DocId,
    },
    /// Some documents that write the old key in a link are not decided, or are
    /// contradicted; every one is listed and nothing is planned.
    #[error(
        "{} document(s) are not decided and {} contradicted",
        undecided.len(),
        contradicted.len()
    )]
    Unsettled {
        /// The documents that have no evidence and no word.
        undecided: Vec<Undecided>,
        /// The documents whose evidence and word disagree.
        contradicted: Vec<Contradicted>,
    },
}

/// A document that has no evidence and no word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undecided {
    /// The file that writes the old key.
    pub file: RelPath,
    /// Its identity, when it could be read.
    pub id: Option<DocId>,
    /// The links of it that write the old key.
    pub links: Vec<EntryName>,
    /// Why there is nothing to go on.
    pub hints: Hints,
}

/// A document whose evidence and word disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contradicted {
    /// The file that writes the old key.
    pub file: RelPath,
    /// Its identity, when it could be read.
    pub id: Option<DocId>,
    /// The links of it that write the old key.
    pub links: Vec<EntryName>,
    /// What disagrees.
    pub contradiction: Contradiction,
}

/// One item of a link that writes the old key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The link of the document.
    pub link: EntryName,
    /// The key as the document wrote it.
    pub written: String,
}

/// What one document that writes the old key in a link comes to, decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewriting {
    file: RelPath,
    id: Option<DocId>,
    meaning: Meaning,
    items: Vec<Item>,
}

impl Rewriting {
    /// The file of the document.
    pub fn file(&self) -> &RelPath {
        &self.file
    }

    /// Its identity, when it could be read.
    pub fn id(&self) -> Option<DocId> {
        self.id
    }

    /// What its links that write the old key mean.
    pub fn meaning(&self) -> Meaning {
        self.meaning
    }

    /// The items of its links that write the old key, in the order of the links.
    pub fn items(&self) -> &[Item] {
        &self.items
    }
}

/// The document that takes the new key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retargeted {
    /// The identity.
    pub id: DocId,
    /// The key it has.
    pub key: DocKey,
    /// The file it is in.
    pub file: RelPath,
}

/// What `rekey` is to do: the target's new key and file, what each document that
/// writes the old key in a link comes to, and the documents whose body names the old
/// key (which are not changed: the body is covered by the hash).
///
/// Made by [`plan`] only. Nothing is written by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyPlan {
    target: Retargeted,
    new_key: NewKey,
    new_file: RelPath,
    writers: Vec<Rewriting>,
    mentions: Vec<DocId>,
}

impl RekeyPlan {
    /// The document that takes the new key, as it is.
    pub fn target(&self) -> &Retargeted {
        &self.target
    }

    /// The new key.
    pub fn new_key(&self) -> &NewKey {
        &self.new_key
    }

    /// The file the target is to be written as: `<book dir>/<new key>.md`.
    pub fn new_file(&self) -> &RelPath {
        &self.new_file
    }

    /// Every document that writes the old key in a link, each decided, in the order of
    /// their files.
    pub fn writers(&self) -> &[Rewriting] {
        &self.writers
    }

    /// The documents whose body names the old key, in the order of identity. Their body
    /// is not changed.
    ///
    /// The valid documents are read by [`plan`]; the target's own is read by
    /// [`RekeyPlan::mention_own_body`], as the target is invalid while its key is written
    /// twice, and the shelf has no body of an invalid document.
    pub fn mentions(&self) -> &[DocId] {
        &self.mentions
    }

    /// Add the target to the mentions when `body`, the body of its file (the reading the
    /// caller has of it where the file is rewritten), names the old key: the same
    /// reading of "names" as for the other documents, and no document twice.
    pub fn mention_own_body(&mut self, body: &str) {
        let id = self.target.id;
        if !self.mentions.contains(&id) && names(body, &self.target.key.form.to_string()) {
            self.mentions.push(id);
            self.mentions.sort();
        }
    }
}

/// The meaning of the links of the document in `file` that write the old key.
///
/// It takes the proof that the writers of the key and the links that write it were all
/// read ([`Gathered`]), so that "nobody else writes the key" is a fact of its argument
/// and not a promise about the order calls are made in. `target` is the document that
/// takes the new key, and `asked` the caller's word about the document.
///
/// 1. The evidence: nobody but `target` writes the key
///    ([`Evidence::OnlyWriter`]); else the approval that stands about the document
///    (`Ledger::standing_decision`), when it is an approval, read for the links of the
///    document that write the key, and only the pins whose upstream writes the key.
///    A document with no identity has none.
/// 2. The word is laid beside it (`Reading`'s page): the evidence is not dropped for
///    the word.
pub fn meaning(
    gathered: &Gathered,
    ledger: &Ledger,
    target: DocId,
    file: &RelPath,
    asked: Option<Side>,
) -> Reading {
    let links: Vec<&EntryName> = gathered
        .links()
        .iter()
        .filter(|writer| writer.file() == file)
        .map(|writer| writer.link())
        .collect();
    let id = gathered
        .links()
        .iter()
        .find(|writer| writer.file() == file)
        .and_then(|writer| writer.id());
    match evidence(gathered, ledger, target, id, &links) {
        Found::Evidence(evidence) => laid_beside(evidence, asked),
        Found::Nothing(hints) => match asked {
            Some(Side::Follow) => Reading::Decided(Meaning::Follows(FollowWhy::Asked)),
            Some(Side::Stay) => Reading::Decided(Meaning::Stays(StayWhy::Asked)),
            Some(Side::Both) => Reading::Decided(Meaning::Both),
            None => Reading::Undecided(hints),
        },
    }
}

/// What the evidence about one document comes to.
enum Found {
    Evidence(Evidence),
    Nothing(Hints),
}

/// The ids of the files that write the key.
fn writer_ids(writers: &Writers) -> BTreeSet<DocId> {
    match writers {
        Writers::None => BTreeSet::new(),
        Writers::One(writer) => writer.id().into_iter().collect(),
        Writers::Many(collision) => collision.ids(),
    }
}

/// The evidence about the document `id` for its `links` that write the key.
fn evidence(
    gathered: &Gathered,
    ledger: &Ledger,
    target: DocId,
    id: Option<DocId>,
    links: &[&EntryName],
) -> Found {
    let writers = writer_ids(gathered.writers());
    if writers == BTreeSet::from([target]) && matches!(gathered.writers(), Writers::One(_)) {
        return Found::Evidence(Evidence::OnlyWriter);
    }
    let Some(id) = id else {
        return Found::Nothing(Hints::NoIdentity);
    };
    let Some(decision) = ledger.standing_decision(id) else {
        return Found::Nothing(Hints::NoApproval);
    };
    let DecisionVerb::Approve { frame, .. } = decision.verb() else {
        return Found::Nothing(Hints::NoApproval);
    };
    let pinned: Vec<DocId> = frame
        .iter()
        .filter(|pin| links.contains(&pin.link()) && writers.contains(&pin.target()))
        .map(|pin| pin.target())
        .collect();
    let follows = pinned.contains(&target);
    let stays = pinned.iter().any(|pinned| *pinned != target);
    match (follows, stays) {
        (true, false) => Found::Evidence(Evidence::Pinned(Side::Follow)),
        (false, true) => Found::Evidence(Evidence::Pinned(Side::Stay)),
        (true, true) => Found::Evidence(Evidence::Pinned(Side::Both)),
        (false, false) => Found::Nothing(Hints::NoPin),
    }
}

/// The evidence and the word, laid together (M3).
fn laid_beside(evidence: Evidence, asked: Option<Side>) -> Reading {
    let disagree = Reading::Contradicted(Contradiction { evidence, asked });
    match (evidence, asked) {
        (Evidence::OnlyWriter, None | Some(Side::Follow)) => {
            Reading::Decided(Meaning::Follows(FollowWhy::OnlyWriter))
        }
        // Nobody else writes the key: there is nothing to stay with or to keep.
        (Evidence::OnlyWriter, Some(Side::Stay | Side::Both)) => disagree,
        (Evidence::Pinned(Side::Follow), None | Some(Side::Follow)) => {
            Reading::Decided(Meaning::Follows(FollowWhy::Pinned))
        }
        (Evidence::Pinned(Side::Stay), None | Some(Side::Stay)) => {
            Reading::Decided(Meaning::Stays(StayWhy::Pinned))
        }
        // `--both` holds whichever side the pins are on.
        (Evidence::Pinned(_), Some(Side::Both)) => Reading::Decided(Meaning::Both),
        // The pins name both sides and the caller did not say `--both`; or a word
        // against the side of the pins.
        (Evidence::Pinned(Side::Both), None | Some(Side::Follow | Side::Stay))
        | (Evidence::Pinned(Side::Follow), Some(Side::Stay))
        | (Evidence::Pinned(Side::Stay), Some(Side::Follow)) => disagree,
    }
}

/// The key and the file of the document that takes the new key: one file, with an
/// identity and a key that were read (R2). An invalid document is a target too (two
/// files that write one key are both invalid, and one of them is the one to renumber).
///
/// It matches the four arms of the state itself, and does not read the files the
/// state lists (`DocState::files`): it needs the files of an invalid document as a
/// [`NonEmptyVec`] (more than one is [`RekeyProblem::InSeveralFiles`]), and says why
/// a document that is gone or cannot be told is no target. It is not a yes or no on
/// which file carries the identity.
fn target_of(shelf: &Shelf, id: DocId) -> Result<Retargeted, RekeyProblem> {
    match shelf.state_of(id) {
        DocState::Valid(doc) => Ok(Retargeted {
            id,
            key: doc.key().clone(),
            file: doc.origin().clone(),
        }),
        DocState::Invalid(docs) => {
            let files = docs.map(|doc| doc.origin().clone());
            if files.len() > 1 {
                return Err(RekeyProblem::InSeveralFiles { files });
            }
            match shelf.now_named(id) {
                NowNamed::Invalid(Some(key)) => Ok(Retargeted {
                    id,
                    key,
                    file: files.first().clone(),
                }),
                NowNamed::Valid(_)
                | NowNamed::Invalid(None)
                | NowNamed::Gone
                | NowNamed::Unknown => Err(RekeyProblem::NoKey { files }),
            }
        }
        DocState::Gone => Err(RekeyProblem::Gone),
        DocState::Unknown { unreadable } => Err(RekeyProblem::Unknown { unreadable }),
    }
}

/// Whether `body` names `key`: the text, not a part of a longer name (`FR-1` is not in
/// `FR-10` or `XFR-1`).
fn names(body: &str, key: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    body.match_indices(key).any(|(at, found)| {
        let before = body[..at].chars().next_back();
        let after = body[at + found.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(part)
    })
}

/// What to do to give the document `ask.target` a new key, or why not.
///
/// The one entry of `rekey` in the domain. It reads, and decides nothing by a default:
///
/// 1. The target is one file, with an identity and a key that were read (R2).
/// 2. The writers of the old key and the links that write it were all read
///    (`gather`, R3).
/// 3. The new key is the one given ([`Shelf::claim_key`]) or the next of the book
///    ([`Shelf::next_key`]), and its file is where `new` would write it (R1).
/// 4. Every document that writes the old key in a link is read by [`meaning`]; when
///    any is undecided or contradicted, every one is listed and nothing is planned
///    (R4).
///
/// The plan changes the target's `key:` and the items of the links that follow or keep
/// both; no body and no field (R5), so no hash changes.
// The problems are the answer, read once per request; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn plan(shelf: &Shelf, ledger: &Ledger, ask: &RekeyAsk) -> Result<RekeyPlan, RekeyProblem> {
    let target = target_of(shelf, ask.target)?;
    let gathered = gather(
        shelf.key_writers(&target.key),
        shelf.link_writers(&target.key),
    )?;
    let new_key = match &ask.key {
        Some(written) => shelf
            .claim_key(&target.key.book, written)
            .map_err(RekeyProblem::ClaimKey)?,
        None => shelf
            .next_key(&target.key.book)
            .map_err(RekeyProblem::NextKey)?,
    };
    let new_file = shelf
        .file_for(new_key.key())
        .map_err(|problem| RekeyProblem::File(Box::new(problem)))?;
    if let Some(doc) = ask
        .words
        .keys()
        .find(|doc| !gathered.links().iter().any(|w| w.id() == Some(**doc)))
    {
        return Err(RekeyProblem::NotAWriter { doc: *doc });
    }

    let files: BTreeSet<&RelPath> = gathered.links().iter().map(|w| w.file()).collect();
    let mut writers = Vec::new();
    let mut undecided = Vec::new();
    let mut contradicted = Vec::new();
    for file in files {
        let own: Vec<_> = gathered
            .links()
            .iter()
            .filter(|writer| writer.file() == file)
            .collect();
        let id = own.first().and_then(|writer| writer.id());
        let links: Vec<EntryName> = own.iter().map(|writer| writer.link().clone()).collect();
        let asked = id.and_then(|id| ask.words.get(&id)).copied();
        match meaning(&gathered, ledger, target.id, file, asked) {
            Reading::Decided(meaning) => writers.push(Rewriting {
                file: file.clone(),
                id,
                meaning,
                items: own
                    .iter()
                    .map(|writer| Item {
                        link: writer.link().clone(),
                        written: writer.written().to_owned(),
                    })
                    .collect(),
            }),
            Reading::Undecided(hints) => undecided.push(Undecided {
                file: file.clone(),
                id,
                links,
                hints,
            }),
            Reading::Contradicted(contradiction) => contradicted.push(Contradicted {
                file: file.clone(),
                id,
                links,
                contradiction,
            }),
        }
    }
    if !undecided.is_empty() || !contradicted.is_empty() {
        return Err(RekeyProblem::Unsettled {
            undecided,
            contradicted,
        });
    }

    // The valid documents only: the shelf has no body of an invalid one, and the target
    // is invalid while its key is written twice (`RekeyPlan::mention_own_body`).
    let old = target.key.form.to_string();
    let mentions = shelf
        .docs()
        .values()
        .filter(|doc| names(doc.body(), &old))
        .map(|doc| doc.id())
        .collect();
    Ok(RekeyPlan {
        target,
        new_key,
        new_file,
        writers,
        mentions,
    })
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Follow => "follow",
            Self::Stay => "stay",
            Self::Both => "both",
        })
    }
}

#[cfg(test)]
mod rekey_tests;
