//! Reading the files of the space: what a name is now, the files a selection takes,
//! and the one place a path meets the file system (`place_of`, the case of a path as
//! written).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use tokio::io::AsyncReadExt;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, NameListing, Selection, SpaceAsks, SpaceReadings,
};

use super::super::super::git::first_line;
use super::{ArtifactError, FileSelection, FileSpace, path_of};

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;

impl FileSpace {
    /// For each name: not a name (no path under the root), the hash of the file
    /// under each comparison asked and where its content can be read back, no file,
    /// or cannot tell (a link out of the root, a file too large or that cannot be
    /// read). Every name asked about has an answer: an error is for Git, which is
    /// asked once for every file.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, ArtifactError> {
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
            let reading = match read_within(&self.root, &path, self.max_file).await {
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
                Err(e) => untold(format!("{path} cannot be read: {e}"))?,
            };
            readings.insert(name.clone(), reading);
        }
        let paths: Vec<&RelPath> = present.iter().map(|(_, path)| path).collect();
        if let Some(addresses) = self.git.blob_names(&paths).await? {
            for ((name, _), address) in present.iter().zip(addresses) {
                if let Some(reading) = readings.get_mut(*name) {
                    reading.set_address(address);
                }
            }
        }
        Ok(readings)
    }

    /// The bytes of the file a name is, as they are now (the bytes
    /// [`resolve`](Self::resolve) hashes), or none when it is no path, no file is
    /// there, or it cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, ArtifactError> {
        let Ok(path) = path_of(name) else {
            return Ok(None);
        };
        Ok(match read_within(&self.root, &path, self.max_file).await {
            Ok(Bounded::Bytes(bytes)) => Some(bytes),
            Ok(Bounded::NoFile | Bounded::Outside | Bounded::TooLarge) | Err(_) => None,
        })
    }

    /// The files a selection takes that are there now (the rules on the module page,
    /// "Listing the names of a selection"), or why no listing can be made: a text of
    /// the selection that is no pattern, a project outside a Git repository, a Git
    /// that cannot list. Those are told as [`NameListing::CannotList`], never as an
    /// error; an error is for the reading of the filesystem that stops the whole ask.
    pub async fn list(&self, selection: &Selection) -> Result<NameListing, ArtifactError> {
        let cannot = |why: String| {
            NonBlank::new(why)
                .map(NameListing::CannotList)
                .map_err(ArtifactError::Reason)
        };
        let chosen = match FileSelection::read(selection) {
            Ok(chosen) => chosen,
            Err(not) => return Ok(NameListing::CannotList(not.why().clone())),
        };
        let listing = match self.git.work_tree_files(self.limits.listing_bytes).await {
            Ok(Some(listing)) => listing,
            Ok(None) => {
                return cannot(
                    "the files of the project cannot be listed: it is not in a Git repository, or no Git can be run"
                        .to_owned(),
                );
            }
            Err(error) => {
                return cannot(format!(
                    "the files of the project cannot be listed: {}",
                    first_line(&error.to_string())
                ));
            }
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
        for path in &listing.files {
            if !chosen.takes(path.as_str()) {
                continue;
            }
            match self.seen(path, &mut dirs).await {
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
        if listing.cut {
            let why = format!(
                "Git's listing of the files was cut at {} bytes",
                self.limits.listing_bytes
            );
            return NonBlank::new(why)
                .map(|why| NameListing::Some { found, why })
                .map_err(ArtifactError::Reason);
        }
        // Nothing is passed over in silence: a file Git lists whose path cannot be
        // read (whether the selection takes it cannot be told), one the selection
        // takes whose path is no name, or one a reading cannot tell, keeps the
        // listing from being all of them. Only the causes that apply are said.
        let causes: Vec<String> = [
            (
                listing.unnamed,
                "Git lists with a path that is not UTF-8 or no path of the project",
            ),
            (unnameable, "the selection takes with a path too long for a name"),
            (
                untold,
                "the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable)",
            ),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| format!("{count} file(s) {what}"))
        .collect();
        if !causes.is_empty() {
            return NonBlank::new(causes.join("; "))
                .map(|why| NameListing::Some { found, why })
                .map_err(ArtifactError::Reason);
        }
        Ok(NameListing::All(found))
    }

    /// What a listing takes the path to be, as [`resolve`](Self::resolve) would
    /// answer it: there (a regular file ([`place_of`]) of at most the bound of bytes,
    /// that opens, named as written; the names of each directory read once, in
    /// `dirs`), not there (no file), or what a reading cannot tell (past the bound
    /// of bytes, a link that leads out of the root, an error looking at the path, a
    /// file that does not open, or a file there under a name that differs from the
    /// one Git lists by case only), which is never taken for not there: a document
    /// can write such a name.
    async fn seen(&self, path: &RelPath, dirs: &mut DirNames) -> Seen {
        match place_of(&self.root, path).await {
            Ok(Place::File { len }) if len <= self.max_file => {}
            Ok(Place::NoFile) => return Seen::NotThere,
            // A document can write the name, and a reading cannot tell it: never
            // left out as if it were not there.
            Ok(Place::File { .. } | Place::Outside) | Err(_) => return Seen::CannotTell,
        }
        if tokio::fs::File::open(self.root.join(path.as_str()))
            .await
            .is_err()
        {
            return Seen::CannotTell;
        }
        if named_as_written_in(&self.root, path, dirs).await {
            Seen::There
        } else {
            // A file system that ignores case found a file under another case than
            // the one Git lists (a rename by case only): a document can write the
            // name on disk, which this listing does not hold.
            Seen::CannotTell
        }
    }
}

/// Where a path under the root leads: a regular file (with its length), no file
/// (nothing, a directory, a pipe or a device, a file on the way), or a place
/// outside the root (through a symbolic link) or behind a broken link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    File { len: u64 },
    NoFile,
    Outside,
}

/// What a listing takes a file it lists to be: what [`FileSpace::resolve`] would
/// answer of it, in the three ways a listing needs.
enum Seen {
    /// A reading would answer it as there.
    There,
    /// No file is there.
    NotThere,
    /// A reading cannot tell: past the bound of bytes, a link out of the root, an
    /// error looking at the path, a file that does not open, or a file there under
    /// another case than the name Git lists.
    CannotTell,
}

/// The one place a path of the space meets the file system, for every reading of a
/// file ([`FileSpace::resolve`], [`FileSpace::current`], the search for moves),
/// looked at part by part. A part that is a symbolic link is followed by its
/// canonical path: one inside the canonical root is read on from there, so a name
/// whose path holds a link inside the project (`CLAUDE.md -> AGENTS.md`) is the
/// target's content under the name written; one that leaves the root, or a link
/// that is broken (or a loop of them), is [`Place::Outside`], and nothing is read.
/// A [`RelPath`] has no `..`, `.` or empty part and is not absolute, so with every
/// link held inside, the path stays inside. The root itself may be reached through
/// a link, which is the person's own choice of where the project is. A part replaced
/// by a link between this look and the read is not seen (a race the adapter does
/// not close).
pub(super) async fn place_of(root: &std::path::Path, path: &RelPath) -> io::Result<Place> {
    let mut at = root.to_path_buf();
    let mut inside: Option<PathBuf> = None;
    let parts: Vec<&str> = path.as_str().split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        at.push(part);
        let mut meta = match tokio::fs::symlink_metadata(&at).await {
            Ok(meta) => meta,
            Err(e) if is_absent(&e) => return Ok(Place::NoFile),
            Err(e) => return Err(e),
        };
        if meta.file_type().is_symlink() {
            // A link that cannot be resolved (broken, a loop) cannot be told.
            let Ok(real) = tokio::fs::canonicalize(&at).await else {
                return Ok(Place::Outside);
            };
            let root_real = match &inside {
                Some(root_real) => root_real.clone(),
                None => {
                    let root_real = tokio::fs::canonicalize(root).await?;
                    inside = Some(root_real.clone());
                    root_real
                }
            };
            if !real.starts_with(&root_real) {
                return Ok(Place::Outside);
            }
            meta = match tokio::fs::metadata(&real).await {
                Ok(meta) => meta,
                Err(e) if is_absent(&e) => return Ok(Place::Outside),
                Err(e) => return Err(e),
            };
            at = real;
        }
        if i + 1 == parts.len() {
            // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
            return Ok(if meta.is_file() {
                Place::File { len: meta.len() }
            } else {
                Place::NoFile
            });
        }
        // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
        if !meta.is_dir() {
            return Ok(Place::NoFile);
        }
    }
    Ok(Place::NoFile)
}

/// What reading a named file within a bound of bytes gave, for this space and for
/// another whose names start with a path of this one (`rust-test`, `js-test`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) enum Bounded {
    /// Its bytes.
    Bytes(Vec<u8>),
    /// No file is there as written.
    NoFile,
    /// A symbolic link on the path leads outside the root, or is broken: nothing is
    /// read.
    Outside,
    /// The file is larger than the bound: nothing is read.
    TooLarge,
}

/// The bytes of a file under `root` by the rules of this space (`place_of`, the case
/// of a path as written), when it has at most `max` bytes: none when no file is there
/// (nothing, a directory, or a path that names an entry in another case only), and
/// nothing read when a link on the path leaves the root or is broken; through a link
/// inside the root, the target's bytes. An error only when the file is there and
/// cannot be read, which each caller tells as a name that cannot be told.
pub(in super::super) async fn read_within(
    root: &std::path::Path,
    path: &RelPath,
    max: u64,
) -> io::Result<Bounded> {
    match place_of(root, path).await? {
        Place::File { len } if len > max => return Ok(Bounded::TooLarge),
        Place::File { .. } => {}
        Place::NoFile => return Ok(Bounded::NoFile),
        Place::Outside => return Ok(Bounded::Outside),
    }
    let file = match tokio::fs::File::open(root.join(path.as_str())).await {
        Ok(file) => file,
        Err(e) if is_absent(&e) => return Ok(Bounded::NoFile),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .await?;
    // It may have grown since it was looked at.
    if u64::try_from(bytes.len()).map_or(true, |len| len > max) {
        return Ok(Bounded::TooLarge);
    }
    if named_as_written(root, path).await {
        Ok(Bounded::Bytes(bytes))
    } else {
        Ok(Bounded::NoFile)
    }
}

/// Whether a path that opened names the file as written (the rule of the case of a
/// path, on the module page): no part of it is the name of an entry of its directory
/// in another ASCII case only. A directory that cannot be listed says nothing against
/// the path, which opened.
pub(super) async fn named_as_written(root: &std::path::Path, path: &RelPath) -> bool {
    named_as_written_in(root, path, &mut DirNames::new()).await
}

/// The names of the directories read while paths are held to the rule of the case,
/// by directory; none for one that cannot be listed. A listing of many files reads
/// each directory once through it.
type DirNames = BTreeMap<PathBuf, Option<Vec<std::ffi::OsString>>>;

/// [`named_as_written`], reading each directory at most once into `dirs`.
async fn named_as_written_in(root: &std::path::Path, path: &RelPath, dirs: &mut DirNames) -> bool {
    let mut dir = root.to_path_buf();
    for part in path.as_str().split('/') {
        if !dirs.contains_key(&dir) {
            let names = match tokio::fs::read_dir(&dir).await {
                Ok(mut entries) => {
                    let mut names = Vec::new();
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        names.push(entry.file_name());
                    }
                    Some(names)
                }
                Err(_) => None,
            };
            dirs.insert(dir.clone(), names);
        }
        let Some(Some(names)) = dirs.get(&dir) else {
            return true;
        };
        if part_names_entry(part, names.iter().map(|n| n.as_os_str())) == PartName::OtherCase {
            return false;
        }
        dir.push(part);
    }
    true
}

/// What the entries of a directory say about one part of a path that opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartName {
    /// An entry has the name as written.
    Written,
    /// No entry has it, and one has it in another ASCII case only: a file system that
    /// does not tell case apart opened it, and Git and one that does would not.
    OtherCase,
    /// Neither: a name the system stores in another form (macOS keeps some
    /// non-ASCII names decomposed), which it opened and Git names as written.
    Unknown,
}

/// The one place a part of a path is held against the names of its directory.
fn part_names_entry<'a>(part: &str, names: impl Iterator<Item = &'a std::ffi::OsStr>) -> PartName {
    let mut other_case = false;
    for name in names {
        if name == part {
            return PartName::Written;
        }
        if name
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(part))
        {
            other_case = true;
        }
    }
    if other_case {
        PartName::OtherCase
    } else {
        PartName::Unknown
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
