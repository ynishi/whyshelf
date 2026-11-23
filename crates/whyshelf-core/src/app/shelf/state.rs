//! The state of a tree of names at either end of one change, read one way.
//!
//! Every reading of a project is of one state: the declaration, the documents and
//! the ledger read from the project's tree, and the spaces read from it or from a
//! tree of their own (the built docs of `rust-item`). A run reads with one adapter of
//! a tree, so that the state before a change and the state after it are read by the
//! same implementation, and what differs between them is the change and nothing of
//! how it was read.
//!
//! ```text
//!   a reader (declaration, documents, ledger, a space)
//!        |  side, names of the tree (N)
//!        v
//!   StateRead<N>   read, look, read_outside,           one adapter of the tree,
//!                  read_beyond, addresses               chosen by the composition root
//!   StateList<N>   list, list_outside, list_beyond, files
//!        |
//!        v  AtSide<N>, DirListing<N>, TreeFiles<N>
//! ```
//!
//! The ports are generic in the name of the tree (`N`): a path of the project, or a
//! path under the built docs of a space. Only the adapter turns a name into something
//! it reads; what a state is (a commit, a directory) is its own and the core never
//! sees it. The way a run reads is named once, by the composition root, as a
//! [`ReadWay`], which [`ShelfRead::way`](super::ShelfRead::way) returns and an answer
//! may name; the core does not read it.
//!
//! A space is not read through these ports by its own names: its adapter resolves a
//! name of the space to names of a tree, and reads the tree through them.
//!
//! The ports are the core's, beside the others the use cases read through: a use case
//! reads a side of a change (`load_at`) and hands that side to every port it reads, so
//! the side is a word of the application, and the adapters that read a tree are the
//! infrastructure's. The words of a tree here (a file, a directory, a link, a kind
//! that is none of them) are the kinds of place a tree has, whatever holds it.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::num::NonZeroU64;

use crate::domain::nonempty::{NonBlank, TextLiteral};
use crate::domain::shelf::artifact::{ContentAddress, Untold};

/// Which end of one change a reading is of. Every command but one that compares two
/// ends reads [`Side::After`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// The state before the change.
    Before,
    /// The state after the change: the project as it is, for a command that reads
    /// one end.
    After,
}

/// How a run reads, as the composition root names it (for a run of the files of the
/// project, `files: the project root`): returned by
/// [`ShelfRead::way`](super::ShelfRead::way), which an answer may name, and never read
/// by the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadWay(NonBlank);

impl ReadWay {
    /// The way, as the composition root names it.
    pub fn new(way: NonBlank) -> Self {
        Self(way)
    }

    /// The text of the way.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The most bytes a reader reads of one name: the reader's own bound, given so that
/// the adapter stops there.
///
/// An adapter reads at most one byte past the bound, so that a reader tells a name
/// whose content is past it by the length of what it got, and nothing more is read.
/// A reader that reads a whole file gives [`ByteBound::WHOLE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteBound(NonZeroU64);

impl ByteBound {
    /// No bound a file can reach: the whole content is read.
    pub const WHOLE: Self = Self(NonZeroU64::MAX);

    /// A bound of `most` bytes.
    pub fn new(most: NonZeroU64) -> Self {
        Self(most)
    }

    /// A bound of `most` bytes, where zero is read as one (no reader reads none).
    pub fn at_most(most: u64) -> Self {
        Self(NonZeroU64::new(most).unwrap_or(NonZeroU64::MIN))
    }

    /// The most bytes.
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// What a name of a tree is at one side: of a file, its content `C`, the bytes a
/// reading read ([`StateRead::read`]) or the length a look found ([`StateRead::look`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtSide<N, C = Vec<u8>> {
    /// A file, with its bytes (at most one byte past the bound asked), or its length.
    There(C),
    /// Nothing is at the name.
    Absent,
    /// The name is a directory: no bytes (each reader keeps its own answer for it).
    Dir,
    /// The first part of the name, from its start, that stands for another name (a
    /// link), with the text that part holds, as written, and where it leads by the
    /// adapter's own resolution. Whichever part of the name is the link, the first one
    /// is answered.
    Elsewhere {
        /// The part of the name, from its start, that is the link.
        at: N,
        /// The text the link holds, as written.
        target: String,
        /// Where it leads.
        leads: Leads<N>,
    },
    /// The adapter cannot tell, and why (a kind of file that is none of the above, a
    /// part of the tree it cannot read alike at both sides).
    Untold(Untold),
}

/// Where a link leads, as the adapter of the tree resolves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leads<N> {
    /// To this name of the tree, with every link on the way resolved.
    Inside(N),
    /// To the top of the tree itself, which is no name of it.
    Top,
    /// Out of the tree.
    Outside,
    /// To nothing: the link is broken (nothing is at what it names).
    Broken,
    /// Nowhere the adapter can resolve, for another reason than nothing there (a loop
    /// of links, a part on the way that is no directory, a place that may not be
    /// looked at). A reader that reads through every link reads the name as the system
    /// that holds the tree resolves it ([`StateRead::read_outside`]), and so gets that
    /// system's own answer; one that does not reads nothing through it.
    Unresolved,
}

/// The kind of an entry of a directory, as the state writes it: a link entry is
/// [`EntryKind::Link`], whatever it leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// A file.
    File,
    /// A directory.
    Dir,
    /// A link.
    Link,
    /// Another kind (a pipe, a socket, a device).
    Other,
}

/// A part of a place of a tree that is no part of a name of it (not text, or a part the
/// grammar of the tree refuses), as the adapter wrote it: opaque to the core, and read
/// back by the adapter alone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawPart(OsString);

impl RawPart {
    /// The part, as the adapter wrote it.
    pub fn new(part: OsString) -> Self {
        Self(part)
    }

    /// The part, for the adapter that wrote it.
    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }
}

/// A place of a tree that is no name of it: the directory it is under (none: the top),
/// and its parts below that directory as the adapter wrote them, the first of which is
/// no part of a name. What [`StateRead::read_beyond`] and [`StateList::list_beyond`]
/// read, so that what the tree cannot name is still read, never passed over.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Beyond<N> {
    /// The directory, a name of the tree (none: the top).
    pub dir: Option<N>,
    /// The parts below it.
    pub parts: Vec<RawPart>,
}

/// An entry whose name is no name of the tree, with its kind and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unnamed<N> {
    /// Its kind, as the state writes it.
    pub kind: EntryKind,
    /// Where it is.
    pub at: Beyond<N>,
}

/// The entries of a directory, each with its own kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entries<N: Ord> {
    /// The entries that are names of the tree, each by its whole name (the
    /// directory's name and the entry's).
    pub named: BTreeMap<N, EntryKind>,
    /// The entries whose name is no name of the tree (not UTF-8, or a part the
    /// grammar of the tree refuses), each by the text of its own part with what cannot
    /// be read replaced, and where it is.
    pub unnamed: BTreeMap<String, Unnamed<N>>,
}

impl<N: Ord> Default for Entries<N> {
    fn default() -> Self {
        Self {
            named: BTreeMap::new(),
            unnamed: BTreeMap::new(),
        }
    }
}

/// What is under a directory of a tree at one side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirListing<N: Ord> {
    /// Every entry.
    All(Entries<N>),
    /// Some of the entries: the listing stopped at a bound, with why. Never read as
    /// all of them.
    Cut {
        /// The entries listed.
        entries: Entries<N>,
        /// Why the listing stopped.
        why: NonBlank,
    },
    /// The name is no directory.
    NotADir(NotADir<N>),
}

/// Why a name listed is no directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotADir<N> {
    /// Nothing is at the name.
    Absent,
    /// The name is a file.
    File,
    /// A part of the name is a link, as [`AtSide::Elsewhere`] says it.
    Elsewhere {
        /// The part of the name, from its start, that is the link.
        at: N,
        /// The text the link holds, as written.
        target: String,
        /// Where it leads.
        leads: Leads<N>,
    },
    /// The adapter cannot tell, and why.
    Untold(Untold),
}

/// Every file of a tree, as the state lists the files of the project (what it leaves
/// out, such as files an ignore rule names, is the state's own rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeFiles<N: Ord> {
    /// Every file.
    All(BTreeSet<N>),
    /// Some of the files: the listing was cut at a bound, or left out files that are
    /// no names of the tree, with why. Never read as all of them.
    Cut {
        /// The files listed.
        names: BTreeSet<N>,
        /// What was left out.
        why: NonBlank,
    },
    /// The files cannot be listed, and why.
    CannotList(NonBlank),
}

/// For each name asked, its answer of content `C`, or the adapter's error of that name
/// alone.
pub type NameAnswers<N, C, E> = BTreeMap<N, Result<AtSide<N, C>, E>>;

/// What an adapter that reads no place the tree cannot name answers of one.
const UNNAMED: TextLiteral =
    TextLiteral::new("a place that is no name of the tree is not read by this adapter");

/// The content of the names of one tree at either side of one change, read the one
/// way of its adapter. The names are the tree's own type: only the adapter turns one
/// into something it reads.
///
/// Every name asked has an answer of its own, or an error of its own (what the system
/// that holds the tree says of that name): one name never stops the others. The
/// call's error is for what stops every name (a side the run does not read).
pub trait StateRead<N: Ord>: Send + Sync {
    /// The adapter's error.
    type Error: Error + Send + Sync + 'static;

    /// What each name is at `side`, within `bound` bytes of each: for every name, its
    /// answer, or the adapter's error of that name alone (a name the system cannot
    /// read). The call's own error is for what stops every name (a side the run does
    /// not read).
    fn read(
        &self,
        side: Side,
        names: &BTreeSet<N>,
        bound: ByteBound,
    ) -> impl Future<Output = Result<NameAnswers<N, Vec<u8>, Self::Error>, Self::Error>> + Send;

    /// What each name is at `side`, as [`read`](Self::read) answers it, with a file told
    /// by its length and nothing of it read or opened: what a reader asks to know that
    /// a name is there and within its bound, before or instead of reading it.
    fn look(
        &self,
        side: Side,
        names: &BTreeSet<N>,
    ) -> impl Future<Output = Result<NameAnswers<N, u64, Self::Error>, Self::Error>> + Send;

    /// Each name read as the system that holds the tree resolves it, every link on the
    /// way followed, out of the tree too: [`AtSide::There`], [`AtSide::Absent`],
    /// [`AtSide::Dir`] or [`AtSide::Untold`], never [`AtSide::Elsewhere`]. Only a run
    /// that reads one side of a tree where the system holds it can; every other
    /// answers [`AtSide::Untold`] for each name. Asked only by the walk along the
    /// links of a name, for a reader that reads through every link, when a link leaves
    /// the tree or the adapter cannot resolve it ([`Leads::Unresolved`]).
    fn read_outside(
        &self,
        side: Side,
        names: &BTreeSet<N>,
        bound: ByteBound,
    ) -> impl Future<Output = Result<NameAnswers<N, Vec<u8>, Self::Error>, Self::Error>> + Send;

    /// What the place `at`, which the tree cannot name, is at `side`, within `bound`
    /// bytes, read as [`read_outside`](Self::read_outside) reads a name (every link
    /// followed): what a reader asks of an entry it was listed and cannot name. Who
    /// can is as for `read_outside`; an adapter that cannot answers
    /// [`AtSide::Untold`], which every one that answers no such entry may.
    fn read_beyond(
        &self,
        side: Side,
        at: &Beyond<N>,
        bound: ByteBound,
    ) -> impl Future<Output = Result<AtSide<N>, Self::Error>> + Send {
        let _ = (side, at, bound);
        async { Ok(AtSide::Untold(Untold::Said(NonBlank::from(UNNAMED)))) }
    }

    /// Where the content of each name, as [`read`](Self::read) reads it at `side`, can
    /// be read back later, for those the state can say. Only a run that reads one
    /// side, at [`Side::After`], makes addresses; every other side and run answers
    /// none, which an address may always be, so an address and a hash are always of
    /// one reading.
    fn addresses(
        &self,
        side: Side,
        names: &BTreeSet<N>,
    ) -> impl Future<Output = Result<BTreeMap<N, ContentAddress>, Self::Error>> + Send;
}

/// The directories and the files of one tree at either side of one change, read the
/// one way of its adapter.
pub trait StateList<N: Ord>: StateRead<N> {
    /// The entries under `dir` at `side` (none: the top of the tree), each with its own
    /// kind. A part of `dir` that is a link is [`NotADir::Elsewhere`], as
    /// [`read`](StateRead::read) answers it.
    fn list(
        &self,
        side: Side,
        dir: Option<&N>,
    ) -> impl Future<Output = Result<DirListing<N>, Self::Error>> + Send;

    /// The entries under `dir` as the system resolves `dir` (links on the way to it
    /// followed, out of the tree too), each entry with its own kind. Who can is as for
    /// [`read_outside`](StateRead::read_outside); every other answers
    /// [`NotADir::Untold`].
    fn list_outside(
        &self,
        side: Side,
        dir: &N,
    ) -> impl Future<Output = Result<DirListing<N>, Self::Error>> + Send;

    /// The entries under the place `at`, which the tree cannot name, as
    /// [`list_outside`](Self::list_outside) lists a directory: every one of them is
    /// unnamed too, since the place it is under is. Who can is as for
    /// [`read_beyond`](StateRead::read_beyond); an adapter that cannot answers
    /// [`NotADir::Untold`].
    fn list_beyond(
        &self,
        side: Side,
        at: &Beyond<N>,
    ) -> impl Future<Output = Result<DirListing<N>, Self::Error>> + Send {
        let _ = (side, at);
        async {
            Ok(DirListing::NotADir(NotADir::Untold(Untold::Said(
                NonBlank::from(UNNAMED),
            ))))
        }
    }

    /// Every file of the tree at `side`, as the state lists the files of the project,
    /// within `bound` bytes of listing.
    fn files(
        &self,
        side: Side,
        bound: ByteBound,
    ) -> impl Future<Output = Result<TreeFiles<N>, Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_of_none_is_read_as_one_and_the_whole_is_past_any_file() {
        assert_eq!(ByteBound::at_most(0).get(), 1);
        assert_eq!(ByteBound::at_most(16).get(), 16);
        assert_eq!(ByteBound::WHOLE.get(), u64::MAX);
    }
}
