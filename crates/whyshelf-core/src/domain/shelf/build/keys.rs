//! Who writes a key, and who writes it in a link: the one reading of each, made once by
//! [`Shelf::build`] and asked of a built shelf.
//!
//! ```text
//!   candidates ──► KeyWriters ──► Writers (None, One, Many)        who writes key K
//!        │             │          Shelf::key_writers ──► KeyWritersRead (Complete, MaybeMore)
//!        │             └────────► DuplicateKey of each writer of a collision; a link's
//!        │                        state; find_key; next_key, claim_key, files_writing;
//!        │                        Shelf::collisions
//!        └──────────► LinkSites ──► Shelf::link_writers ──► LinkWriters (found, unread)
//!                                                           who writes K in a link
//! ```
//!
//! A file writes a key when its `key:` reads in the grammar of its book: whether it
//! is a valid document or not, whether its `id:` could be read or not. That one rule
//! was held in three places, two of them different (the files whose identity was read
//! for the links and the lookup, every file for the next key and the plan), so a key
//! the next key avoided could be a link that went to another document; [`KeyWriters`]
//! is the place it is held now, and every question about a key reads it through
//! [`Writers`].
//!
//! The reading is complete only for the files that could be read. A file whose front
//! matter could not be read at all ([`DocReading::Unparsed`], [`DocReading::Unheld`])
//! may write any key, and is no writer: [`Shelf::key_writers`] says so as
//! [`KeyWritersRead::MaybeMore`], with those files of the key's own book, so that a
//! caller that must know every writer (`rekey`) can refuse and name them, and one that
//! reads only what is known ([`Writers`]) is not made to.
//!
//! [`LinkWriters`] is the same for the links: the documents that write a key in a
//! link, from every file read, valid or not (a file that is invalid is still the file
//! that writes the link), and the places that cannot be read: a file that could not be
//! read at all in a book that declares a link into the key's book, and a link entry
//! that is no list.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonEmptyVec};
use crate::domain::shelf::doc::{DocKey, KeyCollision, KeyWriter};
use crate::domain::shelf::schema::{BookName, EntryName, Membership, Schema};

use super::{Candidate, DocReading, Shelf, read_link_key};

/// Who writes one key: no file, one file, or two or more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::domain::shelf) enum Writers {
    /// No file writes it.
    None,
    /// One file writes it.
    One(KeyWriter),
    /// Two or more files write it, valid or not.
    Many(KeyCollision),
}

impl Writers {
    /// Whether no file writes it.
    pub(in crate::domain::shelf) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// The files that write it, in the order of their paths.
    pub(in crate::domain::shelf) fn files(&self) -> Vec<RelPath> {
        match self {
            Self::None => Vec::new(),
            Self::One(writer) => vec![writer.file().clone()],
            Self::Many(collision) => collision.files().cloned().collect(),
        }
    }
}

/// Who writes a key, and whether that is every file that may: what
/// [`Shelf::key_writers`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::domain::shelf) enum KeyWritersRead {
    /// Every file of the key's book was read, so these are all its writers.
    Complete(Writers),
    /// Some file of the key's book could not be read at all and may write the key too.
    MaybeMore {
        /// The writers among the files that were read.
        writers: Writers,
        /// The files of the key's book that could not be read, in the order of their
        /// paths.
        unread: NonEmptyVec<RelPath>,
    },
}

impl KeyWritersRead {
    /// The writers among the files that were read, whether or not there may be more.
    pub(in crate::domain::shelf) fn writers(&self) -> &Writers {
        match self {
            Self::Complete(writers) | Self::MaybeMore { writers, .. } => writers,
        }
    }
}

/// Every key a file writes, with the files: the one reading of who writes a key.
///
/// Made once, by [`Shelf::build`], from every candidate that reads a key in its
/// book's grammar. The run of a plan adds the keys of its new documents to a copy of
/// it, so that a link to one of them resolves.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct KeyWriters {
    by_key: BTreeMap<DocKey, Vec<KeyWriter>>,
    /// The files that could not be read at all, in the order they were read.
    unread: Vec<RelPath>,
}

impl KeyWriters {
    /// The writers of the keys of `candidates`, and the files `unread`.
    pub(super) fn of(candidates: &[Candidate], unread: Vec<RelPath>) -> Self {
        let mut keys = Self {
            by_key: BTreeMap::new(),
            unread,
        };
        for candidate in candidates {
            if let Some(key) = &candidate.written_key {
                keys.add(
                    key.clone(),
                    KeyWriter::new(candidate.origin.clone(), candidate.identity.id()),
                );
            }
        }
        keys
    }

    /// `writer` writes `key` too. The writers of a key stay in the order of their
    /// files.
    pub(super) fn add(&mut self, key: DocKey, writer: KeyWriter) {
        let writers = self.by_key.entry(key).or_default();
        writers.push(writer);
        writers.sort_by(|a, b| a.file().cmp(b.file()));
    }

    /// Who writes `key`.
    pub(super) fn writers(&self, key: &DocKey) -> Writers {
        match Count::of(self.by_key.get(key).into_iter().flatten().cloned()) {
            Count::None => Writers::None,
            Count::One(writer) => Writers::One(writer),
            Count::Many(writers) => Writers::Many(KeyCollision::new(key.clone(), writers)),
        }
    }

    /// Every key some file writes, in order.
    pub(super) fn keys(&self) -> impl Iterator<Item = &DocKey> {
        self.by_key.keys()
    }

    /// The keys the files that carry the identity `id` write, each once: an invalid
    /// file has no key of its own, so this is where the key of one is read from.
    pub(super) fn keys_of(&self, id: DocId) -> BTreeSet<&DocKey> {
        self.by_key
            .iter()
            .filter(|(_, writers)| writers.iter().any(|writer| writer.id() == Some(id)))
            .map(|(key, _)| key)
            .collect()
    }

    /// The keys two or more files write, in the order of the keys.
    pub(super) fn collisions(&self) -> Vec<KeyCollision> {
        self.by_key
            .keys()
            .filter_map(|key| match self.writers(key) {
                Writers::Many(collision) => Some(collision),
                Writers::None | Writers::One(_) => None,
            })
            .collect()
    }

    /// The files that could not be read at all.
    pub(super) fn unread(&self) -> &[RelPath] {
        &self.unread
    }
}

/// The files of `readings` that could not be read at all: the front matter of one is no
/// mapping, or holds a value of a kind a front matter does not hold.
pub(super) fn unread_files(readings: &[DocReading]) -> Vec<RelPath> {
    readings
        .iter()
        .filter_map(|reading| match reading {
            DocReading::Unparsed { origin, .. } | DocReading::Unheld { origin, .. } => {
                Some(origin.clone())
            }
            DocReading::Parsed { .. } => None,
        })
        .collect()
}

/// A document that writes a key in one of its links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::domain::shelf) struct LinkWriter {
    file: RelPath,
    id: Option<DocId>,
    link: EntryName,
    written: String,
}

impl LinkWriter {
    /// The file that writes the link.
    pub(in crate::domain::shelf) fn file(&self) -> &RelPath {
        &self.file
    }

    /// The identity of the file, when it could be read.
    pub(in crate::domain::shelf) fn id(&self) -> Option<DocId> {
        self.id
    }

    /// The link of the file the key is written in.
    pub(in crate::domain::shelf) fn link(&self) -> &EntryName {
        &self.link
    }

    /// The key as the file wrote it.
    pub(in crate::domain::shelf) fn written(&self) -> &str {
        &self.written
    }
}

/// A place where a link may write a key and could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// A file that could not be read at all, in a book that declares a link into the
    /// key's book.
    File(RelPath),
    /// A link entry of a file that is no list, so that the keys it writes cannot be
    /// read.
    Link {
        /// The file.
        file: RelPath,
        /// The link.
        link: EntryName,
    },
}

/// The place as a sentence names it: the file, with the link when it is a link
/// entry of it.
impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(file) => fmt::Display::fmt(file, f),
            Self::Link { file, link } => write!(f, "{file} (the link `{link}`)"),
        }
    }
}

/// Who writes a key in a link, and the places that could not be read: what
/// [`Shelf::link_writers`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::domain::shelf) struct LinkWriters {
    found: Vec<LinkWriter>,
    unread: Vec<Unread>,
}

impl LinkWriters {
    /// The documents that write the key in a link, in the order of their files and
    /// links.
    pub(in crate::domain::shelf) fn found(&self) -> &[LinkWriter] {
        &self.found
    }

    /// The places that could not be read and may write the key in a link.
    pub(in crate::domain::shelf) fn unread(&self) -> &[Unread] {
        &self.unread
    }
}

/// Every key written in a link of any file read, and the places that could not be
/// read: what [`Shelf::link_writers`] reads. Made by [`Shelf::build`] before any file
/// is judged, so that an invalid file's links are kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct LinkSites {
    /// The links that write each key, read in the grammar of the book they point into
    /// ([`read_link_key`], which the resolution of a link uses too).
    found: BTreeMap<DocKey, Vec<LinkWriter>>,
    /// The files that could not be read at all.
    unread_files: Vec<RelPath>,
    /// The link entries that are no list, with the book they point into.
    unread_links: Vec<(BookName, RelPath, EntryName)>,
}

impl LinkSites {
    /// The links of every candidate, before the files are judged.
    pub(super) fn of(
        schema: &Schema,
        candidates: &[Candidate],
        unread_files: Vec<RelPath>,
    ) -> Self {
        let mut sites = Self {
            found: BTreeMap::new(),
            unread_files,
            unread_links: Vec::new(),
        };
        for candidate in candidates {
            let Membership::In(book) = schema.membership(&candidate.origin) else {
                continue;
            };
            for (link, written) in &candidate.links {
                for text in &written.keys {
                    // A text that is no key of the target's grammar names no key.
                    if let Ok(key) = read_link_key(&written.kind, text) {
                        sites.found.entry(key).or_default().push(LinkWriter {
                            file: candidate.origin.clone(),
                            id: candidate.identity.id(),
                            link: link.clone(),
                            written: text.clone(),
                        });
                    }
                }
            }
            for link in &candidate.unread_links {
                if let Some(kind) = book.links().get(link) {
                    sites.unread_links.push((
                        kind.target.clone(),
                        candidate.origin.clone(),
                        link.clone(),
                    ));
                }
            }
        }
        // The order of the files, whatever order the adapter read them in.
        for writers in sites.found.values_mut() {
            writers.sort_by(|a, b| {
                (&a.file, &a.link, &a.written).cmp(&(&b.file, &b.link, &b.written))
            });
        }
        sites.unread_links.sort();
        sites
    }
}

impl Shelf {
    /// Who writes `key`, and whether that is every file that may.
    ///
    /// The one place it is asked: a file writes a key when its `key:` reads in its
    /// book's grammar, valid or not, its identity read or not. The files of the key's
    /// book that could not be read at all may write it too, and are not among the
    /// writers: when there are any, the answer is [`KeyWritersRead::MaybeMore`] with
    /// them, and a caller that must know every writer refuses.
    pub(in crate::domain::shelf) fn key_writers(&self, key: &DocKey) -> KeyWritersRead {
        let writers = self.index.writers.writers(key);
        let unread: Vec<RelPath> = self
            .index
            .writers
            .unread()
            .iter()
            .filter(|file| {
                matches!(
                    self.schema.membership(file),
                    Membership::In(book) if *book.name() == key.book
                )
            })
            .cloned()
            .collect();
        match NonEmptyVec::from_vec(unread) {
            Ok(unread) => KeyWritersRead::MaybeMore { writers, unread },
            Err(_) => KeyWritersRead::Complete(writers),
        }
    }

    /// The keys that two or more files write, one collision for each, in the order of
    /// the keys: the files, and the identity of each that could be read. Each of the
    /// files is invalid ([`DocProblem::DuplicateKey`](super::DocProblem::DuplicateKey)).
    pub fn collisions(&self) -> Vec<KeyCollision> {
        self.index.writers.collisions()
    }

    /// The documents that write `key` in a link, and the places that could not be
    /// read.
    ///
    /// The one place it is asked: from every file read, invalid ones too, before any
    /// file was judged. The places that could not be read are the files that could not
    /// be read at all in a book that declares a link into the book of `key`, and the
    /// link entries that are no list and point into it.
    pub(in crate::domain::shelf) fn link_writers(&self, key: &DocKey) -> LinkWriters {
        let found = self.links.found.get(key).cloned().unwrap_or_default();
        let files = self.links.unread_files.iter().filter(|file| {
            matches!(
                self.schema.membership(file),
                Membership::In(book) if book.links().values().any(|kind| kind.target == key.book)
            )
        });
        let links = self
            .links
            .unread_links
            .iter()
            .filter(|(target, _, _)| *target == key.book);
        let unread = files
            .map(|file| Unread::File(file.clone()))
            .chain(links.map(|(_, file, link)| Unread::Link {
                file: file.clone(),
                link: link.clone(),
            }))
            .collect();
        LinkWriters { found, unread }
    }
}
