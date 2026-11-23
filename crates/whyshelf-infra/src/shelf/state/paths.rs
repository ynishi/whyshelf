//! The walk along the links of a name of a tree, and the case of a name as written: the
//! two rules every reader of a tree reads a name by, here once.
//!
//! A reader asks the state for a name ([`StateRead::read`], [`StateList::list`]); a part
//! of it that is a link comes back as where it leads ([`Leads`]), by the adapter's own
//! resolution. [`follow`] and [`follow_dir`] read the name again where the link leads,
//! inside the tree, and whether a link that leaves it is read through is the reader's
//! rule, given as a [`Leave`]:
//!
//! | the link leads | [`Leave::Allowed`] | [`Leave::Refused`] |
//! |---|---|---|
//! | inside the tree | read there | read there |
//! | out of the tree | read as the system resolves the name ([`StateRead::read_outside`], [`StateList::list_outside`]) | [`Followed::Left`] |
//! | to nothing (broken) | nothing there ([`AtSide::Absent`], [`NotADir::Absent`]) | [`Followed::Broken`] |
//! | nowhere the state can resolve (a loop, a file on the way, a place that may not be looked at) | read as the system resolves the name, so its own answer or error comes back | [`Followed::Broken`] |
//!
//! Past [`MOST_LINKS`] links read again, the walk stops: [`Followed::TooMany`].
//!
//! [`look_through`] walks a name the same way with [`Leave::Refused`], asking
//! [`StateRead::look`] and so opening nothing: what a reader asks to know a name is
//! there and within its bound. [`read_allowed`] and [`list_allowed`] are the one
//! reading, through every link as the system follows one, of a name and of a directory
//! of the project's tree ([`FileStates`]), whose answers the declaration, the documents
//! and the ledger each map to their own errors.
//!
//! | reader | its rule |
//! |---|---|
//! | the declaration, the documents, the ledger | [`Leave::Allowed`]: as the system reads a path |
//! | the spaces `file`, `rust-test`, `js-test`, and `rust-item` inside the project | [`Leave::Refused`]: nothing is read through a link that leaves the root |

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, Entries, Leads, NotADir, Side, StateList, StateRead,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::Untold;

use super::StateError;
use super::files::FileStates;

/// The most links one walk reads a name again through: past them it is a loop the
/// state did not tell, or a chain no reader follows.
pub(crate) const MOST_LINKS: usize = 40;

/// Whether a reader reads through a link that leaves the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leave {
    /// It does, as the system resolves the name.
    Allowed,
    /// It does not.
    Refused,
}

/// What a walk along the links of a name came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Followed<N, A> {
    /// The name, every link on it followed: where it was reached, with links resolved
    /// (none: the top of the tree; the name asked, when a link out of the tree was read
    /// through), and what is there.
    Reached { at: Option<N>, answer: A },
    /// A link leads out of the tree, and the reader does not read through it.
    Left,
    /// A link leads nowhere, and the reader does not read through it.
    Broken,
    /// Past [`MOST_LINKS`] links.
    TooMany,
}

/// A name of a tree, as parts: what the walk reads a name again from.
pub(crate) trait TreeName: Ord + Clone + Send + Sync {
    /// Its parts, from the top of the tree.
    fn parts(&self) -> Vec<&str>;

    /// The name of these parts, when they are one (none for no part at all).
    fn of_parts(parts: &[&str]) -> Option<Self>;
}

impl TreeName for RelPath {
    fn parts(&self) -> Vec<&str> {
        self.as_str().split('/').collect()
    }

    fn of_parts(parts: &[&str]) -> Option<Self> {
        if parts.is_empty() {
            return None;
        }
        parts.join("/").parse().ok()
    }
}

/// `name`, with its first parts, `at`, put where a link at `at` leads (`to`; none:
/// the top of the tree): none when nothing is left (the name is the top), and `name`
/// itself when `at` is not its start.
fn rebased<N: TreeName>(name: &N, at: &N, to: Option<&N>) -> Option<N> {
    let parts = name.parts();
    let head = at.parts();
    let rest = parts.get(head.len()..).unwrap_or_default();
    let mut joined: Vec<&str> = to.map(TreeName::parts).unwrap_or_default();
    joined.extend_from_slice(rest);
    N::of_parts(&joined)
}

/// One name read through `state` at `side`.
async fn read_one<N: TreeName, S: StateRead<N>>(
    state: &S,
    side: Side,
    name: &N,
    bound: ByteBound,
) -> Result<AtSide<N>, S::Error> {
    state
        .read(side, &BTreeSet::from([name.clone()]), bound)
        .await?
        .remove(name)
        .unwrap_or(Ok(AtSide::Untold(Untold::NoAnswer)))
}

/// One name read through `state` at `side` as the system resolves it.
async fn read_outside_one<N: TreeName, S: StateRead<N>>(
    state: &S,
    side: Side,
    name: &N,
    bound: ByteBound,
) -> Result<AtSide<N>, S::Error> {
    state
        .read_outside(side, &BTreeSet::from([name.clone()]), bound)
        .await?
        .remove(name)
        .unwrap_or(Ok(AtSide::Untold(Untold::NoAnswer)))
}

/// What `name` is at `side`, every link on it followed by the reader's rule `leave`
/// (the table on the page of the module), within `bound` bytes. Never
/// [`AtSide::Elsewhere`].
pub(crate) async fn follow<N: TreeName, S: StateRead<N>>(
    state: &S,
    side: Side,
    name: &N,
    leave: Leave,
    bound: ByteBound,
) -> Result<Followed<N, AtSide<N>>, S::Error> {
    let mut now = name.clone();
    for _ in 0..MOST_LINKS {
        let answer = read_one(state, side, &now, bound).await?;
        let AtSide::Elsewhere { at, leads, .. } = answer else {
            return Ok(Followed::Reached {
                at: Some(now),
                answer,
            });
        };
        match leads {
            Leads::Inside(to) => match rebased(&now, &at, Some(&to)) {
                Some(next) => now = next,
                None => return Ok(Followed::Broken),
            },
            Leads::Top => match rebased(&now, &at, None) {
                Some(next) => now = next,
                None => {
                    return Ok(Followed::Reached {
                        at: None,
                        answer: AtSide::Dir,
                    });
                }
            },
            Leads::Outside | Leads::Unresolved => {
                return Ok(match (leave, leads) {
                    // As the system resolves the name: its own answer, or its own error
                    // (a loop, a file on the way, a place that may not be looked at).
                    (Leave::Allowed, _) => Followed::Reached {
                        at: Some(name.clone()),
                        answer: read_outside_one(state, side, name, bound).await?,
                    },
                    (Leave::Refused, Leads::Outside) => Followed::Left,
                    (Leave::Refused, _) => Followed::Broken,
                });
            }
            Leads::Broken => {
                return Ok(match leave {
                    Leave::Allowed => Followed::Reached {
                        at: Some(now),
                        answer: AtSide::Absent,
                    },
                    Leave::Refused => Followed::Broken,
                });
            }
        }
    }
    Ok(Followed::TooMany)
}

/// What `name` is at `side`, every link on it followed while it stays inside the
/// tree ([`Leave::Refused`]), as [`follow`] walks it, with a file told by its length
/// and nothing opened ([`StateRead::look`]). Never [`AtSide::Elsewhere`].
pub(crate) async fn look_through<N: TreeName, S: StateRead<N>>(
    state: &S,
    side: Side,
    name: &N,
) -> Result<Followed<N, AtSide<N, u64>>, S::Error> {
    let mut now = name.clone();
    for _ in 0..MOST_LINKS {
        let answer = state
            .look(side, &BTreeSet::from([now.clone()]))
            .await?
            .remove(&now)
            .unwrap_or(Ok(AtSide::Untold(Untold::NoAnswer)))?;
        let AtSide::Elsewhere { at, leads, .. } = answer else {
            return Ok(Followed::Reached {
                at: Some(now),
                answer,
            });
        };
        match leads {
            Leads::Inside(to) => match rebased(&now, &at, Some(&to)) {
                Some(next) => now = next,
                None => return Ok(Followed::Broken),
            },
            Leads::Top => match rebased(&now, &at, None) {
                Some(next) => now = next,
                None => {
                    return Ok(Followed::Reached {
                        at: None,
                        answer: AtSide::Dir,
                    });
                }
            },
            Leads::Outside => return Ok(Followed::Left),
            Leads::Broken | Leads::Unresolved => return Ok(Followed::Broken),
        }
    }
    Ok(Followed::TooMany)
}

/// What is under `dir` at `side` (none: the top of the tree), every link on the way
/// followed by the reader's rule `leave`, as [`follow`] reads a name. Never
/// [`NotADir::Elsewhere`].
pub(crate) async fn follow_dir<N: TreeName, S: StateList<N>>(
    state: &S,
    side: Side,
    dir: Option<&N>,
    leave: Leave,
) -> Result<Followed<N, DirListing<N>>, S::Error> {
    let mut now = dir.cloned();
    for _ in 0..MOST_LINKS {
        let listing = state.list(side, now.as_ref()).await?;
        let DirListing::NotADir(NotADir::Elsewhere { at, leads, .. }) = listing else {
            return Ok(Followed::Reached {
                at: now,
                answer: listing,
            });
        };
        let Some(current) = now.as_ref() else {
            return Ok(Followed::Broken);
        };
        match leads {
            Leads::Inside(to) => now = rebased(current, &at, Some(&to)),
            Leads::Top => now = rebased(current, &at, None),
            Leads::Outside | Leads::Unresolved => {
                return Ok(match (leave, dir, leads) {
                    (Leave::Allowed, Some(asked), _) => Followed::Reached {
                        at: Some(asked.clone()),
                        answer: state.list_outside(side, asked).await?,
                    },
                    (_, _, Leads::Outside) => Followed::Left,
                    _ => Followed::Broken,
                });
            }
            Leads::Broken => {
                return Ok(match leave {
                    Leave::Allowed => Followed::Reached {
                        at: now,
                        answer: DirListing::NotADir(NotADir::Absent),
                    },
                    Leave::Refused => Followed::Broken,
                });
            }
        }
    }
    Ok(Followed::TooMany)
}

/// Why a reading of the project's tree that reads through every link failed: the
/// system's error of the path, or what else stops the state.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The system's error, as a reading of the path would give it.
    Io(io::Error),
    /// The state cannot be read at the side asked.
    State(StateError),
}

impl Failure {
    fn of(error: StateError) -> Self {
        match error {
            StateError::Io { source, .. } => Self::Io(source),
            other => Self::State(other),
        }
    }

    fn of_walk<N, A>(walk: &Followed<N, A>) -> Self {
        Self::Io(io::Error::other(match walk {
            Followed::TooMany => {
                "a symbolic link on the path cannot be resolved (a loop of links, or too many)"
            }
            _ => "a symbolic link on the path is not followed",
        }))
    }
}

/// What a name of the project's tree is, read through every link as the system
/// follows one ([`Leave::Allowed`]).
#[derive(Debug)]
pub(crate) enum Read {
    Bytes(Vec<u8>),
    Absent,
    Dir,
    /// A kind the state cannot tell, and why.
    Untold(String),
    Failed(Failure),
}

/// `name` of the project's tree at `side`, read whole through every link as the
/// system follows one: the one such reading of the declaration, a document's file and
/// a record.
pub(crate) async fn read_allowed(state: &FileStates, side: Side, name: &RelPath) -> Read {
    match follow(state, side, name, Leave::Allowed, ByteBound::WHOLE).await {
        Ok(Followed::Reached { answer, .. }) => Read::of(answer),
        Ok(walk) => Read::Failed(Failure::of_walk(&walk)),
        Err(error) => Read::Failed(Failure::of(error)),
    }
}

/// The place `at`, which the project's tree cannot name, read whole as the system
/// resolves it: the reading of an entry of the ledger whose name is no path.
pub(crate) async fn read_beyond_allowed(
    state: &FileStates,
    side: Side,
    at: &Beyond<RelPath>,
) -> Read {
    match state.read_beyond(side, at, ByteBound::WHOLE).await {
        Ok(answer) => Read::of(answer),
        Err(error) => Read::Failed(Failure::of(error)),
    }
}

impl Read {
    fn of(answer: AtSide<RelPath>) -> Self {
        match answer {
            AtSide::There(bytes) => Self::Bytes(bytes),
            AtSide::Absent => Self::Absent,
            AtSide::Dir => Self::Dir,
            AtSide::Untold(why) => Self::Untold(why.as_str().to_owned()),
            AtSide::Elsewhere { .. } => Self::Failed(Failure::of_walk(&Followed::<(), ()>::Left)),
        }
    }
}

impl Listed {
    fn of(listing: DirListing<RelPath>) -> Self {
        match listing {
            DirListing::All(entries) | DirListing::Cut { entries, .. } => Self::Entries(entries),
            DirListing::NotADir(NotADir::Absent) => Self::Absent,
            DirListing::NotADir(NotADir::File) => Self::File,
            DirListing::NotADir(NotADir::Untold(why)) => Self::Untold(why.as_str().to_owned()),
            DirListing::NotADir(NotADir::Elsewhere { .. }) => {
                Self::Failed(Failure::of_walk(&Followed::<(), ()>::Left))
            }
        }
    }
}

/// The entries under the place `at`, which the project's tree cannot name, listed as
/// the system resolves it: the walk of a directory of a range whose name is no path.
pub(crate) async fn list_beyond_allowed(
    state: &FileStates,
    side: Side,
    at: &Beyond<RelPath>,
) -> Listed {
    match state.list_beyond(side, at).await {
        Ok(listing) => Listed::of(listing),
        Err(error) => Listed::Failed(Failure::of(error)),
    }
}

/// What a directory of the project's tree is, listed through every link as the
/// system follows one ([`Leave::Allowed`]).
#[derive(Debug)]
pub(crate) enum Listed {
    Entries(Entries<RelPath>),
    Absent,
    /// The name is a file.
    File,
    /// A kind the state cannot tell, and why.
    Untold(String),
    Failed(Failure),
}

/// The entries of `dir` of the project's tree at `side`, listed through every link
/// as the system follows one: the one such listing of the ledger and of the
/// directories of a range.
pub(crate) async fn list_allowed(state: &FileStates, side: Side, dir: &RelPath) -> Listed {
    match follow_dir(state, side, Some(dir), Leave::Allowed).await {
        Ok(Followed::Reached { answer, .. }) => Listed::of(answer),
        Ok(walk) => Listed::Failed(Failure::of_walk(&walk)),
        Err(error) => Listed::Failed(Failure::of(error)),
    }
}

/// The last part of each entry of a directory, the entries whose name is no name of
/// the tree with them, as the state writes them.
pub(crate) fn entry_names<N: TreeName>(entries: &Entries<N>) -> impl Iterator<Item = &str> {
    entries
        .named
        .keys()
        .filter_map(|name| name.parts().last().copied())
        .chain(entries.unnamed.keys().map(String::as_str))
}

/// The names of the directories read while names are held to the rule of the case, by
/// directory (none: the top); none for one that cannot be listed. A listing of many
/// files reads each directory once through it.
pub(crate) type DirNames<N> = BTreeMap<Option<N>, Option<Vec<String>>>;

/// Whether a name that is there is written as the state writes it: no part of it is
/// the name of an entry of its directory in another ASCII case only. A directory that
/// cannot be listed (or that a link leads out of the tree from) says nothing against
/// the name.
pub(crate) async fn named_as_written<N: TreeName, S: StateList<N>>(
    state: &S,
    side: Side,
    name: &N,
) -> bool {
    named_as_written_in(state, side, name, &mut DirNames::new()).await
}

/// [`named_as_written`], reading each directory at most once into `dirs`.
pub(crate) async fn named_as_written_in<N: TreeName, S: StateList<N>>(
    state: &S,
    side: Side,
    name: &N,
    dirs: &mut DirNames<N>,
) -> bool {
    let parts = name.parts();
    let mut dir: Option<N> = None;
    for (i, part) in parts.iter().enumerate() {
        if !dirs.contains_key(&dir) {
            let names = match follow_dir(state, side, dir.as_ref(), Leave::Refused).await {
                Ok(Followed::Reached {
                    answer: DirListing::All(entries) | DirListing::Cut { entries, .. },
                    ..
                }) => Some(entry_names(&entries).map(str::to_owned).collect()),
                _ => None,
            };
            dirs.insert(dir.clone(), names);
        }
        let Some(Some(names)) = dirs.get(&dir) else {
            return true;
        };
        if part_names_entry(part, names.iter().map(String::as_str)) == PartName::OtherCase {
            return false;
        }
        dir = N::of_parts(&parts[..=i]);
    }
    true
}

/// What the entries of a directory say about one part of a name that is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartName {
    /// An entry has the name as written.
    Written,
    /// No entry has it, and one has it in another ASCII case only: a file system that
    /// does not tell case apart found it, and Git and one that does would not.
    OtherCase,
    /// Neither: a name the system stores in another form (macOS keeps some
    /// non-ASCII names decomposed), which it found and Git names as written.
    Unknown,
}

/// The one place a part of a name is held against the names of its directory.
fn part_names_entry<'a>(part: &str, names: impl Iterator<Item = &'a str>) -> PartName {
    let mut other_case = false;
    for name in names {
        if name == part {
            return PartName::Written;
        }
        if name.eq_ignore_ascii_case(part) {
            other_case = true;
        }
    }
    if other_case {
        PartName::OtherCase
    } else {
        PartName::Unknown
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
