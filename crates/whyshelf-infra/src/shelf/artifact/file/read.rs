//! Reading the files of the space: what a name is at a side, the files a selection
//! takes, and the one reading of a path by the rules of the space (`read_within`: no
//! link out of the root followed, within a bound of bytes, the case of a path as
//! written).
//!
//! Every reading is of the state of the project at the side asked
//! ([`FileStates`](super::super::super::state::files::FileStates)): a link on the path
//! is followed only while it stays inside the root ([`Leave::Refused`]), and a name is
//! held to be written as the state writes it
//! ([`named_as_written`](super::super::super::state::paths::named_as_written)).

use std::collections::BTreeSet;
use std::io;

use whyshelf_core::app::shelf::{AtSide, ByteBound, Side, StateList, StateRead, TreeFiles};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, NameListing, Selection, SpaceAsks, SpaceReadings, Untold,
};

use super::super::super::state::StateError;
use super::super::super::state::files::FileStates;
use super::super::super::state::paths::{
    DirNames, Followed, Leave, follow, look_through, named_as_written, named_as_written_in,
};
use super::{ArtifactError, FileSelection, FileSpace, path_of};

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
        self.state.dir(side)?;
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
                    let why =
                        NonBlank::new(format!("no path under the root of the project: {why}"))
                            .map_err(ArtifactError::Reason)?;
                    readings.insert(name.clone(), ArtifactReading::not_a_name(why));
                    continue;
                }
            };
            let reading = match read_within(&self.state, side, &path, self.max_file).await {
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
                Ok(Bounded::Outside) => untold(
                    "a symbolic link on the path leads outside the root of the project, or is broken: nothing is read through it"
                        .to_owned(),
                )?,
                Ok(Bounded::TooLarge) => untold(format!(
                    "{path} is larger than {} bytes, and is not read",
                    self.max_file
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
        self.state.dir(side)?;
        let Ok(path) = path_of(name) else {
            return Ok(None);
        };
        Ok(
            match read_within(&self.state, side, &path, self.max_file).await {
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
        let bound =
            ByteBound::at_most(u64::try_from(self.limits.listing_bytes).unwrap_or(u64::MAX));
        let (files, left_out) = match self.state.files(side, bound).await? {
            TreeFiles::All(files) => (files, None),
            TreeFiles::Cut { names, why } => (names, Some(why)),
            TreeFiles::CannotList(why) => return Ok(NameListing::CannotList(why)),
        };
        let mut found = BTreeSet::new();
        let mut past_names = false;
        // Files the selection takes whose path is too long for a name: there, but no
        // document can name them, so the listing is not all of them.
        let mut unnameable = 0usize;
        // Files the selection takes of which a reading could not tell whether they
        // are there: never left out as if they were not.
        let mut untold = 0usize;
        // Each directory is read once for the whole listing, not once per file under it.
        let mut dirs = DirNames::new();
        for path in &files {
            if !chosen.takes(path.as_str()) {
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
            let Ok(name) = ArtifactName::new(path.as_str()) else {
                unnameable += 1;
                continue;
            };
            if found.len() == self.limits.names {
                past_names = true;
                break;
            }
            found.insert(name);
        }
        if past_names {
            let why = format!(
                "the selection takes more than {} names, and the listing stops there",
                self.limits.names
            );
            return NonBlank::new(why)
                .map(|why| NameListing::Some { found, why })
                .map_err(ArtifactError::Reason);
        }
        // Nothing is passed over in silence: what the state's listing of the files
        // left out (cut at its bound, or files whose path is no path of the project),
        // a file the selection takes whose path is no name, or one a reading cannot
        // tell, keeps the listing from being all of them. Only the causes that apply
        // are said.
        let causes: Vec<String> = left_out
            .map(|why| why.as_str().to_owned())
            .into_iter()
            .chain(
                [
                    (unnameable, "the selection takes with a path too long for a name"),
                    (
                        untold,
                        "the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable)",
                    ),
                ]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, what)| format!("{count} file(s) {what}")),
            )
            .collect();
        if !causes.is_empty() {
            return NonBlank::new(causes.join("; "))
                .map(|why| NameListing::Some { found, why })
                .map_err(ArtifactError::Reason);
        }
        Ok(NameListing::All(found))
    }

    /// What a listing takes the path to be, as [`resolve`](Self::resolve) would
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
    state: &FileStates,
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
