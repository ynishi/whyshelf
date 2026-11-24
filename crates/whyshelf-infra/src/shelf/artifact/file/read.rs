//! Reading the files of the space: what a name is at a side, the files a selection
//! takes, and the one reading of a path by the rules of the space (`read_within`: no
//! link out of the root followed, within a bound of bytes, the case of a path as
//! written).
//!
//! Every reading is of the tree of the project at the side asked
//! ([`ProjectTree`](super::super::super::state::ProjectTree)): a link on the path
//! is followed only while it stays inside the root ([`Leave::Refused`]), and a name is
//! held to be written as the state writes it
//! ([`named_as_written`](super::super::super::state::paths::named_as_written)).

use std::collections::BTreeSet;
use std::io;
use std::num::NonZeroUsize;

use whyshelf_core::app::shelf::{AtSide, ByteBound, Side, StateList, StateRead, TreeFiles};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, NameListing, Selection, SpaceAsks, SpaceReadings, Untold,
};

use super::super::super::state::paths::{
    DirNames, Followed, Leave, follow, look_through, look_through_links, named_as_written,
    named_as_written_in,
};
use super::super::super::state::{ProjectTree, StateError};
use super::{ArtifactError, FileSelection, FileSpace, ScanLimits, no_path_reason, path_of};

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;

impl FileSpace {
    /// For each name, as the project is ([`Side::After`]): not a name (no path under
    /// the root), the hash of the file under each comparison asked and where its
    /// content can be read back, no file, or cannot tell (a link out of the root, a
    /// file too large or that cannot be read, a kind of file that is neither a file, a
    /// directory nor a link). Every name asked about has an answer: an error is for
    /// Git, which is asked once for every file.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, ArtifactError> {
        self.resolve_at(Side::After, asks).await
    }

    /// The bytes of the file a name is, as the project is ([`Side::After`]): the bytes
    /// [`resolve`](Self::resolve) hashes, or none when it is no path, no file is there,
    /// or it cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, ArtifactError> {
        self.current_at(Side::After, name).await
    }

    /// The files a selection takes that are there, as the project is ([`Side::After`]),
    /// or why no listing can be made, by the rules on the module page ("Listing the
    /// names of a selection").
    pub async fn list(&self, selection: &Selection) -> Result<NameListing, ArtifactError> {
        self.list_at(Side::After, selection).await
    }

    /// [`resolve`](Self::resolve), at `side`.
    pub(super) async fn resolve_at(
        &self,
        side: Side,
        asks: &SpaceAsks,
    ) -> Result<SpaceReadings, ArtifactError> {
        self.state.reads(side)?;
        let mut readings = SpaceReadings::new();
        let mut present = Vec::new();
        let untold = |why: String| {
            NonBlank::new(why)
                .map(ArtifactReading::cannot_tell)
                .map_err(ArtifactError::Reason)
        };
        for (name, comparisons) in asks {
            let path = match path_of(name) {
                Ok(path) => path,
                Err(why) => {
                    let why = no_path_reason(&why)?;
                    readings.insert(name.clone(), ArtifactReading::not_a_name(why));
                    continue;
                }
            };
            let max_file = self.lister.max_file();
            let reading = match read_within(&self.state, side, &path, max_file).await {
                Ok(Bounded::Bytes(bytes)) => {
                    // A file may be as large as the bound: a large one is hashed on
                    // a blocking thread.
                    let asked = comparisons.clone();
                    match super::super::on_blocking(bytes.len(), move || {
                        ArtifactReading::of_bytes(&bytes, &asked, None)
                    })
                    .await
                    {
                        Ok(reading) => {
                            present.push((name, path));
                            reading
                        }
                        Err(why) => untold(format!("the hash of {path} stopped: {why}"))?,
                    }
                }
                Ok(Bounded::NoFile) => ArtifactReading::absent(),
                Ok(Bounded::Outside) => untold(OUTSIDE.to_owned())?,
                Ok(Bounded::TooLarge) => untold(format!(
                    "{path} is larger than {max_file} bytes, and is not read"
                ))?,
                Ok(Bounded::Untold(why)) => ArtifactReading::cannot_tell(why),
                Err(e) => untold(format!("{path} cannot be read: {e}"))?,
            };
            readings.insert(name.clone(), reading);
        }
        let paths: BTreeSet<RelPath> = present.iter().map(|(_, path)| path.clone()).collect();
        if !paths.is_empty() {
            let addresses = self.state.addresses(side, &paths).await?;
            for (name, path) in &present {
                if let (Some(reading), Some(address)) =
                    (readings.get_mut(*name), addresses.get(path))
                {
                    reading.set_address(address.clone());
                }
            }
        }
        Ok(readings)
    }

    /// [`current`](Self::current), at `side`.
    pub(super) async fn current_at(
        &self,
        side: Side,
        name: &ArtifactName,
    ) -> Result<Option<Vec<u8>>, ArtifactError> {
        self.state.reads(side)?;
        let Ok(path) = path_of(name) else {
            return Ok(None);
        };
        Ok(
            match read_within(&self.state, side, &path, self.lister.max_file()).await {
                Ok(Bounded::Bytes(bytes)) => Some(bytes),
                Ok(Bounded::NoFile | Bounded::Outside | Bounded::TooLarge | Bounded::Untold(_))
                | Err(_) => None,
            },
        )
    }

    /// The files a selection takes that are there at `side` (the rules on the module
    /// page, "Listing the names of a selection"), or why no listing can be made: a text
    /// of the selection that is no pattern, a project outside a Git repository, a Git
    /// that cannot list. Those are told as [`NameListing::CannotList`], never as an
    /// error; an error is for the reading of the state that stops the whole ask.
    pub(super) async fn list_at(
        &self,
        side: Side,
        selection: &Selection,
    ) -> Result<NameListing, ArtifactError> {
        let chosen = match FileSelection::read(selection) {
            Ok(chosen) => chosen,
            Err(not) => return Ok(NameListing::CannotList(not.why().clone())),
        };
        let takes = |path: &RelPath| {
            if !chosen.takes(path.as_str()) {
                return Take::Skip;
            }
            match ArtifactName::new(path.as_str()) {
                Ok(name) => Take::Named(name),
                Err(_) => Take::Unnameable,
            }
        };
        let most = Some(self.lister.limits().names);
        let (found, causes) = match self.lister.taken(side, takes, most).await? {
            Taken::CannotList(why) => return Ok(NameListing::CannotList(why)),
            Taken::All(found) => (found, Vec::new()),
            Taken::Some { found, short } => (
                found,
                short.into_vec().into_iter().map(Cause::from).collect(),
            ),
        };
        listing(found.into_iter().collect(), causes).map_err(ArtifactError::Reason)
    }
}

/// What the selection of an adapter makes of one file the state lists: not taken,
/// taken as this, or taken with a path that is no name of the space.
pub(in super::super) enum Take<T> {
    /// The selection does not take it.
    Skip,
    /// The selection takes it, as this.
    Named(T),
    /// The selection takes it, and its path is no name (too long for one).
    Unnameable,
}

/// The files a selection takes at a side, as a [`Lister`] tells them: all of them,
/// some of them and why not all, or no listing, with the state's why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Taken<T> {
    /// Every file the selection takes that is there, in the order the state lists.
    All(Vec<T>),
    /// Some of them, and each thing that kept the listing from being all of them.
    Some {
        found: Vec<T>,
        short: NonEmptyVec<Short>,
    },
    /// The state cannot list the files (outside a repository, a Git that cannot be
    /// run), with its why as it gives it.
    CannotList(NonBlank),
}

/// What kept a listing of files from being all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Short {
    /// More than `limit` files were taken, and the listing stopped there.
    Past { limit: usize },
    /// The state's listing of the files left some out, with its why (cut at its
    /// bound, or a file whose path is no path of the project).
    Left(NonBlank),
    /// So many files taken of which a reading cannot tell whether they are there.
    Untold(NonZeroUsize),
    /// So many files taken whose path is no name.
    Unnameable(NonZeroUsize),
}

/// The one choice of the files a selection takes at a side, for every adapter whose
/// names start with the path of a file (`file`, `rust-test`): the files the state of
/// the project lists, each kept where a reading would answer it as there
/// ([`Lister::taken`]). It holds the bounds of a listing ([`ScanLimits`]) and the
/// bound of bytes of a file, which the adapter that makes it gives: the same values
/// tell what is there and what a reading reads.
#[derive(Debug, Clone)]
pub(in super::super) struct Lister {
    /// The tree of the project of the run: which adapter of the state it is, is not
    /// looked at.
    state: ProjectTree,
    limits: ScanLimits,
    /// The most bytes of a file a reading reads.
    max_file: u64,
}

impl Lister {
    /// The lister of the files `state` lists, of the bounds by default
    /// ([`ScanLimits::default`]), a file being read up to `max_file` bytes.
    pub(in super::super) fn on(state: ProjectTree, max_file: u64) -> Self {
        Self {
            state,
            limits: ScanLimits::default(),
            max_file,
        }
    }

    /// The same, within `limits`.
    pub(in super::super) fn with_limits(mut self, limits: ScanLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The same, a file being read up to `max_file` bytes.
    #[cfg(test)]
    pub(in super::super) fn with_max_file(mut self, max_file: u64) -> Self {
        self.max_file = max_file;
        self
    }

    /// The bounds of a listing and of a search for moves.
    pub(in super::super) fn limits(&self) -> &ScanLimits {
        &self.limits
    }

    /// The most bytes of a file a reading reads.
    pub(in super::super) fn max_file(&self) -> u64 {
        self.max_file
    }

    /// The files the state lists at `side` that `takes` takes, each kept where a
    /// reading would answer it as there (`seen`), in the order of the state's
    /// listing: the rules on the page of the space `file` ("Listing the names of a
    /// selection"). A file `takes` names is counted against `most`, and once `most`
    /// of them are found the listing stops at the next; a file a reading cannot tell,
    /// and one whose path is no name, is counted and kept out, and neither counts
    /// against `most`; with `most` none, every file the state lists that `takes`
    /// takes is looked at (nothing is opened but a byte of it), and the bound of what
    /// is read is the caller's. The state's listing is held to
    /// [`ScanLimits::listing_bytes`].
    /// An error only for a reading of the state that stops the whole ask.
    pub(in super::super) async fn taken<T>(
        &self,
        side: Side,
        takes: impl Fn(&RelPath) -> Take<T>,
        most: Option<usize>,
    ) -> Result<Taken<T>, StateError> {
        let bound =
            ByteBound::at_most(u64::try_from(self.limits.listing_bytes).unwrap_or(u64::MAX));
        let (files, left_out) = match self.state.files(side, bound).await? {
            TreeFiles::All(files) => (files, None),
            TreeFiles::Cut { names, why } => (names, Some(why)),
            TreeFiles::CannotList(why) => return Ok(Taken::CannotList(why)),
        };
        let mut found = Vec::new();
        let mut past = None;
        // Files taken whose path is no name: there, but no document can name them.
        let mut unnameable = 0usize;
        // Files taken of which a reading could not tell whether they are there:
        // never left out as if they were not.
        let mut untold = 0usize;
        // Each directory is read once for the whole listing, not once per file under it.
        let mut dirs = DirNames::new();
        for path in &files {
            let take = takes(path);
            if matches!(take, Take::Skip) {
                continue;
            }
            match self.seen(side, path, &mut dirs).await {
                Seen::There => {}
                Seen::NotThere => continue,
                Seen::CannotTell => {
                    untold += 1;
                    continue;
                }
            }
            match take {
                Take::Skip => {}
                Take::Unnameable => unnameable += 1,
                Take::Named(it) => {
                    if let Some(limit) = most
                        && found.len() == limit
                    {
                        past = Some(limit);
                        break;
                    }
                    found.push(it);
                }
            }
        }
        let short: Vec<Short> = past
            .map(|limit| Short::Past { limit })
            .into_iter()
            .chain(left_out.map(Short::Left))
            .chain(NonZeroUsize::new(untold).map(Short::Untold))
            .chain(NonZeroUsize::new(unnameable).map(Short::Unnameable))
            .collect();
        Ok(match NonEmptyVec::from_vec(short) {
            Ok(short) => Taken::Some { found, short },
            Err(_) => Taken::All(found),
        })
    }

    /// What a listing takes the path to be, as a reading of the space `file` would
    /// answer it at `side`: there (a file of at most the bound of bytes, read through
    /// no link out of the root, named as written; the names of each directory read
    /// once, in `dirs`), not there (no file), or what a reading cannot tell (past the
    /// bound of bytes, a link that leads out of the root, an error reading the path, a
    /// kind of file that is no file, or a file there under a name that differs from
    /// the one the listing gives by case only), which is never taken for not there: a
    /// document can write such a name.
    async fn seen(&self, side: Side, path: &RelPath, dirs: &mut DirNames<RelPath>) -> Seen {
        // Looked at, nothing opened: whether it is a file within the bound.
        match look_through(&self.state, side, path).await {
            Ok(Followed::Reached {
                answer: AtSide::There(len),
                ..
            }) if len <= self.max_file => {}
            Ok(Followed::Reached {
                answer: AtSide::Absent | AtSide::Dir,
                ..
            }) => return Seen::NotThere,
            Err(StateError::Io { source, .. }) if is_absent(&source) => return Seen::NotThere,
            // A document can write the name, and a reading cannot tell it: never
            // left out as if it were not there.
            Ok(_) | Err(_) => return Seen::CannotTell,
        }
        // That it opens: a reading of one byte (and the one past it), not of the file.
        match follow(
            &self.state,
            side,
            path,
            Leave::Refused,
            ByteBound::at_most(1),
        )
        .await
        {
            Ok(Followed::Reached {
                answer: AtSide::There(_),
                ..
            }) => {}
            Ok(_) | Err(_) => return Seen::CannotTell,
        }
        if named_as_written_in(&self.state, side, path, dirs).await {
            Seen::There
        } else {
            // A file system that ignores case found a file under another case than
            // the one the listing gives (a rename by case only): a document can write
            // the name on disk, which this listing does not hold.
            Seen::CannotTell
        }
    }
}

/// What a listing takes a file it lists to be: what [`FileSpace::resolve`] would
/// answer of it, in the three ways a listing needs.
enum Seen {
    /// A reading would answer it as there.
    There,
    /// No file is there.
    NotThere,
    /// A reading cannot tell: past the bound of bytes, a link out of the root, an
    /// error reading the path, a kind of file that is no file, or a file there under
    /// another case than the name the listing gives.
    CannotTell,
}

/// What keeps a listing of names from being all of them, as an adapter that lists
/// gathers it: the one place its reason is written is [`listing`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Cause {
    /// More than `limit` names were taken, and the listing stopped there.
    PastNames { limit: usize },
    /// The state's listing of the files left some out, with its why.
    Left(NonBlank),
    /// So many things taken (files, or tests) whose name cannot be written.
    Unnameable { count: NonZeroUsize, of: Unit },
    /// So many files taken of which a reading cannot tell whether they are there.
    Untold(NonZeroUsize),
    /// The adapter's own reason (what may make a name it cannot see, a file it cannot
    /// read), which it writes whole.
    Own(NonBlank),
}

/// What a count of names that cannot be written is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Unit {
    /// Files, whose path is too long for a name.
    Files,
    /// Tests, whose name cannot be written as a name of the space.
    Tests,
}

impl From<Short> for Cause {
    /// The files a [`Lister`] took that it could not name are files.
    fn from(short: Short) -> Self {
        match short {
            Short::Past { limit } => Self::PastNames { limit },
            Short::Left(why) => Self::Left(why),
            Short::Untold(count) => Self::Untold(count),
            Short::Unnameable(count) => Self::Unnameable {
                count,
                of: Unit::Files,
            },
        }
    }
}

impl Cause {
    /// Where its words stand among the others of a reason.
    fn rank(&self) -> u8 {
        match self {
            Self::PastNames { .. } => 0,
            Self::Left(_) => 1,
            Self::Unnameable { .. } => 2,
            Self::Untold(_) => 3,
            Self::Own(_) => 4,
        }
    }

    /// Its words in a reason.
    fn said(&self) -> String {
        match self {
            Self::PastNames { limit } => {
                format!("the selection takes more than {limit} names, and the listing stops there")
            }
            Self::Left(why) | Self::Own(why) => why.as_str().to_owned(),
            Self::Unnameable {
                count,
                of: Unit::Files,
            } => format!("{count} file(s) the selection takes with a path too long for a name"),
            Self::Unnameable {
                count,
                of: Unit::Tests,
            } => format!("{count} test(s) whose name cannot be written as a name of the space"),
            Self::Untold(count) => format!(
                "{count} file(s) the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable)"
            ),
        }
    }
}

/// The listing of the names `found`, all of them when nothing kept it from being so
/// (`causes` is empty), and otherwise some of them with why: the one place the reason
/// of a listing that is not all is written, for every adapter whose names start with
/// the path of a file. A listing that stopped at its bound of names says that alone;
/// otherwise every cause is said, in one order (what the state left out, the names
/// that cannot be written, the files a reading cannot tell, the adapter's own), apart
/// by `; `. Nothing is passed over in silence.
pub(in super::super) fn listing(
    found: BTreeSet<ArtifactName>,
    mut causes: Vec<Cause>,
) -> Result<NameListing, ValueError> {
    if causes.is_empty() {
        return Ok(NameListing::All(found));
    }
    causes.sort_by_key(Cause::rank);
    let why = match causes.first() {
        Some(past @ Cause::PastNames { .. }) => past.said(),
        _ => causes
            .iter()
            .map(Cause::said)
            .collect::<Vec<_>>()
            .join("; "),
    };
    NonBlank::new(why).map(|why| NameListing::Some { found, why })
}

/// What reading a named file within a bound of bytes gave, for this space and for
/// another whose names start with a path of this one (`rust-test`, `js-test`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Bounded {
    /// Its bytes.
    Bytes(Vec<u8>),
    /// No file is there as written (nothing, a directory, a path through a file, or a
    /// path that names an entry in another case only).
    NoFile,
    /// A symbolic link on the path leads outside the root, or is broken: nothing is
    /// read.
    Outside,
    /// The file is larger than the bound: nothing more is read.
    TooLarge,
    /// The state cannot tell what is there (a kind of file that is neither a file, a
    /// directory nor a link), and why: nothing is read.
    Untold(NonBlank),
}

/// The bytes of a file at `side` by the rules of this space: none when no file is
/// there (nothing, a directory, a path through a file, or a path that names an entry
/// in another case only), and nothing read when a link on the path leaves the root or
/// is broken; through a link inside the root, the target's bytes; at most `max` of
/// them. Whether it is past the bound is told by its length, looked at before it is
/// opened, and by what is read, in case it grew since. An error only when the file is
/// there and cannot be read, which each caller tells as a name that cannot be told.
pub(in super::super) async fn read_within(
    state: &ProjectTree,
    side: Side,
    path: &RelPath,
    max: u64,
) -> io::Result<Bounded> {
    match look_through(state, side, path).await {
        Ok(Followed::Reached {
            answer: AtSide::There(len),
            ..
        }) if len > max => return Ok(Bounded::TooLarge),
        Ok(Followed::Reached {
            answer: AtSide::There(_),
            ..
        }) => {}
        Ok(Followed::Reached {
            answer: AtSide::Absent | AtSide::Dir,
            ..
        }) => return Ok(Bounded::NoFile),
        Ok(Followed::Reached {
            answer: AtSide::Untold(why),
            ..
        }) => return Ok(Bounded::Untold(untold_why(why))),
        Ok(
            Followed::Reached {
                answer: AtSide::Elsewhere { .. },
                ..
            }
            | Followed::Left
            | Followed::Broken
            | Followed::TooMany,
        ) => return Ok(Bounded::Outside),
        Err(StateError::Io { source, .. }) if is_absent(&source) => return Ok(Bounded::NoFile),
        Err(StateError::Io { source, .. }) => return Err(source),
        Err(other) => return Err(io::Error::other(other)),
    }
    let answer = match follow(state, side, path, Leave::Refused, ByteBound::at_most(max)).await {
        Ok(Followed::Reached { answer, .. }) => answer,
        Ok(Followed::Left | Followed::Broken | Followed::TooMany) => return Ok(Bounded::Outside),
        Err(StateError::Io { source, .. }) if is_absent(&source) => return Ok(Bounded::NoFile),
        Err(StateError::Io { source, .. }) => return Err(source),
        Err(other) => return Err(io::Error::other(other)),
    };
    let bytes = match answer {
        AtSide::There(bytes) => bytes,
        AtSide::Absent | AtSide::Dir => return Ok(Bounded::NoFile),
        AtSide::Untold(why) => return Ok(Bounded::Untold(untold_why(why))),
        AtSide::Elsewhere { .. } => return Ok(Bounded::Outside),
    };
    if u64::try_from(bytes.len()).map_or(true, |len| len > max) {
        return Ok(Bounded::TooLarge);
    }
    if named_as_written(state, side, path).await {
        Ok(Bounded::Bytes(bytes))
    } else {
        Ok(Bounded::NoFile)
    }
}

/// What `resolve` says of a name a link on whose path leaves the root or is broken.
pub(in super::super) const OUTSIDE: &str = "a symbolic link on the path leads outside the root of the project, or is broken: nothing is read through it";

/// Where the content of a path lives at `side`, by the rules [`read_within`] reads it
/// by, with nothing opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Placed {
    /// The files its content is read from: the path as written when no link is on it;
    /// when links inside the root are, each link entry on the way, as written, and
    /// the name they lead to (none of it when that is the top of the tree). `there`
    /// says whether a file is at the end.
    Files {
        files: NonEmptySet<RelPath>,
        there: bool,
    },
    /// Nothing is read through it, with the reason `resolve` gives: a link that leaves
    /// the root or is broken, a loop, a kind of file that cannot be told, or a path the
    /// state cannot look at.
    Unplaced(String),
}

/// Where the content of `path` lives at `side` ([`Placed`]), following its links as
/// [`read_within`] does ([`Leave::Refused`]: only inside the root), and looking at
/// what is at the end without opening it.
pub(in super::super) async fn placed_within(
    state: &ProjectTree,
    side: Side,
    path: &RelPath,
) -> Placed {
    let (followed, links) = match look_through_links(state, side, path).await {
        Ok(traced) => traced,
        Err(StateError::Io { source, .. }) if is_absent(&source) => {
            return Placed::Files {
                files: NonEmptySet::new(path.clone(), []),
                there: false,
            };
        }
        Err(e) => return Placed::Unplaced(format!("{path} cannot be read: {e}")),
    };
    let (at, there) = match followed {
        Followed::Reached {
            answer: AtSide::Untold(why),
            ..
        } => return Placed::Unplaced(untold_why(why).into_string()),
        Followed::Reached {
            answer: AtSide::Elsewhere { .. },
            ..
        }
        | Followed::Left
        | Followed::Broken
        | Followed::TooMany => return Placed::Unplaced(OUTSIDE.to_owned()),
        Followed::Reached { at, answer } => (at, matches!(answer, AtSide::There(_))),
    };
    let mut files: BTreeSet<RelPath> = links.into_iter().collect();
    files.extend(at);
    match NonEmptySet::from_set(files) {
        Ok(files) => Placed::Files { files, there },
        // No link and no name: the walk always has the one or the other.
        Err(_) => Placed::Files {
            files: NonEmptySet::new(path.clone(), []),
            there,
        },
    }
}

/// What is said of what the state could not tell, when it gave no reason of its own
/// (it always gives one).
const UNTOLD: TextLiteral = TextLiteral::new("the state cannot tell what is there");

/// The reason of what the state could not tell.
fn untold_why(why: Untold) -> NonBlank {
    match why {
        Untold::Said(why) | Untold::NotAName(why) => why,
        Untold::NoAnswer | Untold::NotAsked => NonBlank::from(UNTOLD),
    }
}

/// Whether a read failed because no file is at the path: nothing there, a directory
/// on the way that is a file, or a directory where the file would be.
pub(super) fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::IsADirectory
    )
}
