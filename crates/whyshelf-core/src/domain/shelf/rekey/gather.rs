//! The proof that every writer of a key and every link that writes it was read: a
//! [`Gathered`], which only [`gather`] can make.
//!
//! What `rekey` decides by (who writes the key, who links to it) is read from two
//! places, the one reading of each ([`Shelf::key_writers`], [`Shelf::link_writers`]),
//! and each says whether it is the whole. [`gather`] asks both and refuses, naming
//! what could not be read, unless both are whole. Its result is a value of a type
//! whose fields are private to this module, so that the reading of a document
//! ([`meaning`](super::meaning)) takes the proof as its argument and cannot be called
//! on a partial reading: that a key has no other writer is not a promise about the
//! order of calls.

use crate::domain::shelf::build::{KeyWritersRead, LinkWriter, LinkWriters, Writers};

use super::RekeyProblem;

/// Every writer of a key and every link that writes it, with nothing left unread.
///
/// Made by `gather` and by nothing else (its fields are private to its module), so a
/// value of it says that the writers of the key and the links that write it were
/// read whole; [`meaning`](super::meaning) takes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gathered {
    writers: Writers,
    links: Vec<LinkWriter>,
}

impl Gathered {
    /// The files that write the key, and their identities.
    pub(super) fn writers(&self) -> &Writers {
        &self.writers
    }

    /// The documents that write the key in a link, a document once for each link.
    pub(super) fn links(&self) -> &[LinkWriter] {
        &self.links
    }
}

/// `keys` and `links` as the proof that they are whole: the writers of the key and the
/// links that write it, or [`RekeyProblem::NotAllRead`] with every place that could
/// not be read.
///
/// The places are those of [`KeyWritersRead::MaybeMore`] (the files of the key's own
/// book whose front matter could not be read, which may write the key) and of
/// [`LinkWriters::unread`] (the files that could not be read in a book that declares a
/// link into the key's book, and the link entries that are no list): files outside
/// every book are neither, since they write no key and no link.
pub(super) fn gather(keys: KeyWritersRead, links: LinkWriters) -> Result<Gathered, RekeyProblem> {
    let (writers, unread_keys) = match keys {
        KeyWritersRead::Complete(writers) => (writers, Vec::new()),
        KeyWritersRead::MaybeMore { writers, unread } => (writers, unread.into_vec()),
    };
    let unread_links = links.unread().to_vec();
    if !unread_keys.is_empty() || !unread_links.is_empty() {
        return Err(RekeyProblem::NotAllRead {
            keys: unread_keys,
            links: unread_links,
        });
    }
    Ok(Gathered {
        writers,
        links: links.found().to_vec(),
    })
}
