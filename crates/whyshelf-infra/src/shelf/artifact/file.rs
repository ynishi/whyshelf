//! The adapter of the space `file`: the files of a project, by their path under the
//! root, and where their content can be read back.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`FileSpace::resolve`] | reads each named file's bytes at a side, up to [`MAX_FILE_BYTES`], and hashes them; names the blob its content is (or would be) in Git | [`FileStates`] (its reading and its addresses) |
//! | [`FileSpace::current`] | the bytes of a named file at a side, the ones `resolve` hashes | [`FileStates`] |
//! | [`FileSpace::read_back`] | the content of a blob, by its name, up to [`MAX_FILE_BYTES`] | [`Git::read_blob_within`] |
//! | [`FileSpace::suggest`] | for each gone file, the files it may have moved to | [`Git::work_tree_files`], `tokio::fs`, [`Git::renames_since`], [`FileStates`] |
//! | [`FileSpace::list`] | the files a selection takes that are there at a side (what `resolve` answers as there), or why none can be listed | [`FileStates`] (its files, as Git lists them), [`PathGlob`] |
//! | [`NamePlaces`] for [`FileSpace`] | where a name lives: the files its content is read from at a side (its path, or each link entry inside the root and where it leads), or why none | [`FileStates`] (`placed_within`, which opens nothing) |
//! | [`presets`] | the preset `file`: the space, `compare = "bytes"`, `means = "depends"`; and the preset `code`: the space, `compare = "presence"`, `means = "depends"` | values |
//! | [`FileWrite`] for [`FileSpace`] | a new file at a path, where nothing is, for `init` | `tokio::fs` (`create_new`) |
//!
//! ```text
//!   name ──a path under the root?── no ──► not a name (why)
//!     │ yes
//!   path ──the state of the project, at most MAX_FILE_BYTES──► bytes, kept when no part of the path names an entry in another case only
//!                               ──ArtifactReading::of_bytes──► a hash per comparison asked (always)
//!                               └──git hash-object──► blob name ──► ContentAddress (in a repository)
//!        ├─ no file there, or one named in another case only ──► absent
//!        └─ a link out of the root, too large, cannot be read ──► cannot tell
//!   ContentAddress ──git cat-file blob──► bytes, or not readable
//! ```
//!
//! # The names of the space
//!
//! A name of the space `file` is a path relative to the project root, as
//! [`RelPath`] reads it: not empty, not absolute (no leading `/`), no backslash and
//! no control character, and no part that is empty, `.` or `..` (so `a//b`, `./a`
//! and `a/../b` are none). Any other text is not a name of the space, and the adapter
//! says so with why ([`ArtifactReading::not_a_name`]); the document that writes it is
//! invalid. A file that is there is the only thing a name has there: a directory is
//! nothing there.
//!
//! # Positions
//!
//! A name holds no position in a file: it names the file whole.
//!
//! # Equal names
//!
//! A path is compared as written, case included (below): no two spellings name one
//! file, and none is rewritten into another. A name whose path holds a symbolic link
//! inside the root is its own name, with the content of the file the link leads to
//! (below).
//!
//! # One thing for a name
//!
//! A path leads to one entry of the file system, or to none: no name matches two
//! things.
//!
//! # The answers
//!
//! | answer | when |
//! |---|---|
//! | there | a regular file is at the path as written, with its bytes and, in a repository, the address of its blob |
//! | absent | nothing is there, a directory is, or an entry is there in another ASCII case only |
//! | cannot tell | a symbolic link on the path leads outside the root or is broken; the file is larger than [`MAX_FILE_BYTES`]; or it is there and cannot be read (its permissions, a read that fails) |
//! | not a name | the text is no path under the root, as above |
//!
//! A file that cannot be read is told as such alone: every other name asked with it
//! is answered. Only Git, asked once for every file, can fail the whole reading.
//!
//! # What is compared
//!
//! The content of a name is every byte of the file, read up to [`MAX_FILE_BYTES`];
//! what of them counts is the link's comparison. The address is the name of the blob
//! Git would store for the file, its line endings converted as Git converts them and
//! no filter driver run ([`Git::blob_names`]); it is never compared. It is read back,
//! up to the same bound, only while Git holds that blob: a file never added has an
//! address and reads nothing back.
//!
//! # Where it reads
//!
//! The files under the root of the project, through a symbolic link only while it
//! stays inside the root (below); Git, run in the root. It declares no build
//! directory.
//!
//! # The case of a path
//!
//! A path names a file only as written: one that differs from the name of an entry
//! of its directory in ASCII case only names no file, on every system, as for Git (a
//! file system that does not tell case apart would open it). A name that differs in
//! a non-ASCII letter's case, or that the system stores in another form (macOS keeps
//! some non-ASCII names decomposed), is left to the system.
//!
//! # Prefixes of the names
//!
//! `judge moved --prefix` moves every name under a prefix at once, and this adapter
//! says which names are under it and what each becomes
//! ([`NamePrefixes`]).
//! A prefix is a path under the root, read as a name's path is, into its parts (what
//! lies between two `/`); one `/` at its end is dropped, as a shell's completion of a
//! directory writes it (`src/a/` is `src/a`, `src/a//` is no path). A name is under
//! it when its parts start with all of the prefix's, so a part is never cut (`src/a`
//! is no prefix of `src/ab.rs`, and is one of `src/a` itself and of `src/a/x.rs`); the
//! same name under `to` is the parts of `to` as written and the rest of the name's. A
//! `from` that is no path (`src//a`, `../a`) is no prefix of the space's names, and a
//! `to` that is none (or a name built that is too long) makes each name under `from`
//! unbuildable, with why. The adapter looks for no text in a name: it compares the
//! parts it read.
//!
//! # Suggesting moves
//!
//! For a file an approval pinned that is gone (`moves`), the adapter looks two
//! ways, and the domain orders what it finds:
//!
//! 1. **The same content.** Each file there now whose hash, under the comparison of
//!    a pin of the gone file, is the hash pinned (`same_content`). A pin that
//!    compares presence only has no content to look for: a name pinned by such
//!    pins only is not searched, and has the note `NoContent`. The files read are those
//!    Git lists (tracked, and untracked ones no ignore rule leaves out); outside a
//!    repository, the regular files under the root, in no directory or file whose
//!    name starts with `.`. The two differ on purpose: in a repository the ignore
//!    rules bound the listing, and a dot file Git lists (`.github/...`) is a file of
//!    the project; outside one nothing bounds it, and dot directories are where tools
//!    keep caches and environments (`.venv`, `.cargo`, a nested `.git`). Within
//!    [`ScanLimits`]: only regular files (through a link only inside the root, as
//!    below), of at most `file_bytes` each, at most
//!    `files` of them and `total_bytes` in all, and `listing_bytes` of Git's
//!    listing. A candidate is named as its entries are written on disk, so a move in
//!    case only (`a.rs` to `A.rs` on a file system that does not tell case apart) is
//!    one, held to the rule of the case of a path (above).
//! 2. **A rename.** The pairs Git's rename detection gives between the commit each
//!    approval was made at and the index, whose old path is the gone name or the
//!    name the pin wrote (`renamed`, with Git's similarity). The index holds blobs,
//!    so no filter of the repository runs: a file moved with `git mv`, or moved and
//!    added, is seen with its content as added; one moved and not added is found by
//!    its content only, when it is unchanged. At most `listing_bytes` of Git's answer
//!    are read: a longer one is a history that cannot be read (below), never the
//!    renames of a part of it.
//!
//! What could not be looked at is a typed note of the name ([`MoveNote`]), never a
//! failure: a file or directory that cannot be read, or a file too large, passed
//! over and counted; a bound reached; no repository; no pin of the name that kept a
//! revision; a revision that is no commit of the repository (another clone, a
//! history rewritten); a history that cannot be read (a failing or slow Git, an
//! object a partial clone does not hold). How Git is run, and why nothing of the
//! repository chooses a program, is on the page of [`git`](super::super::git).
//!
//! # Listing the names of a selection
//!
//! A selection of this space is patterns over the paths under the root, each text
//! read by the one grammar of a segment glob ([`PathGlob`]: `**`, `*` and `?`; the
//! same one the range of a book is read by, without the book's rule of `.md`): a path
//! is taken when some `include` matches it and no `exclude` does. A text that is no
//! pattern makes the selection no selection of the space, with why, and its listing
//! [`NameListing::CannotList`] with the same why.
//!
//! The listing is the files Git lists (`git ls-files --cached --others
//! --exclude-standard`, as the search for moves reads them), kept where `resolve`
//! would answer *there*: a file (`read_within`, through a link only inside the root)
//! of at most [`MAX_FILE_BYTES`], that reads, named as written (the rule of the case
//! of a path). A file tracked and deleted, a directory and a submodule are not
//! in it, so no name of it is absent under `resolve`. The same content gives the
//! same names.
//!
//! Nothing is passed over in silence: a listing is [`NameListing::Some`] with why,
//! and never all of them, when it takes more than [`ScanLimits::names`] names, when
//! Git's listing is cut at [`ScanLimits::listing_bytes`], when Git lists a file whose
//! path is not UTF-8 or no path of the project, when the selection takes a path too
//! long for a name, or when it takes a file `resolve` cannot tell (past
//! [`MAX_FILE_BYTES`], a link out of the root, one the file system will not read, or
//! one there under another case than the name Git lists, on a file system that
//! ignores case): a document can write such a name, so it is never left out as if
//! it were not there. Outside a Git repository, or when Git cannot be run or fails,
//! no listing can be made: [`NameListing::CannotList`] with why, and no error.
//!
//! # Where a name lives
//!
//! A name lives, whole ([`Placement::Only`]), in the files its content is read from
//! at the side asked, read through the state by the rule of symbolic links below
//! (`placed_within`, which opens nothing): its own path, as written, when no link is
//! on it, there or not (a name with nothing there lives where it would be); each
//! link entry on its way, as written, and the name they lead to, when links inside
//! the root are, since its content depends on each. A link that leaves the root, is
//! broken or loops, a kind of file that cannot be told, and a text that is no path
//! under the root (by the grammar alone, with nothing read) place it nowhere
//! ([`Placement::Unplaced`]), with the reason `resolve` gives.
//!
//! # Symbolic links
//!
//! A link on the path of a name is followed by its canonical path while that stays
//! inside the canonical root: the name is the one written, and its content is the
//! target's (`CLAUDE.md -> AGENTS.md` is present, and an approval pins it). A link
//! that leads outside the root, or that is broken, is not followed: the name cannot
//! be told ([`FileSpace::resolve`]), has no bytes now ([`FileSpace::current`]), and
//! is never read or suggested by the search for moves. Where a link leads is the
//! state's ([`FileStates`]), and that it is not followed out of the root is this
//! space's rule, held in one place for all three (`read_within`, which walks the
//! links of a name refusing to leave the root). A kind of file
//! that is neither a file, a directory nor a link (a pipe) cannot be told, and is not
//! opened.
//!
//! # Outside Git
//!
//! The hash of a file never needs Git, so freshness works the same in any directory.
//! Outside a repository there is no address (see [`git`](super::super::git)).
//!
//! # Where each part is
//!
//! What a name is at a side and the files a selection takes are read in
//! `file/read.rs`, with the one reading of a path by the rules of the space
//! (`read_within`, through the state of the project, with the case of a path as
//! written); the search for moves is `file/moves.rs`; the reading back of a
//! content by its address `file/read_back.rs`; the new file of `init`
//! ([`FileWrite`]) `file/write.rs`.
//!
//! [`FileWrite`]: whyshelf_core::app::shelf::FileWrite
//! [`MoveNote`]: whyshelf_core::domain::shelf::ledger::MoveNote
//! [`ArtifactReading::not_a_name`]: whyshelf_core::domain::shelf::artifact::ArtifactReading::not_a_name
//! [`NamePlaces`]: whyshelf_core::app::shelf::NamePlaces
//! [`Placement::Only`]: whyshelf_core::domain::shelf::placement::Placement::Only
//! [`Placement::Unplaced`]: whyshelf_core::domain::shelf::placement::Placement::Unplaced

mod moves;
mod read;
mod read_back;
mod write;

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use whyshelf_core::app::shelf::{
    Answers, NameList, NamePlaces, NamePrefixes, Side, SpaceAdapter, SpaceReadBack, SpaceSuggest,
};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::{PathGlob, RelPath};
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ContentAddress, NameListing, NamePrefix, NotASelection, PrefixMove, Renamed,
    Selection, SelectionList, SpaceAsks, SpacePrefixes, SpaceReadings, SpaceRenamed,
};
use whyshelf_core::domain::shelf::ledger::{GoneName, NameSuggestions, PATH_ONLY_SPACE};
use whyshelf_core::domain::shelf::placement::Placement;
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::super::git::{Git, GitError};
use super::super::state::StateError;
use super::super::state::files::FileStates;
#[cfg(test)]
pub(super) use read::OUTSIDE;
pub(super) use read::{Bounded, Placed, placed_within, read_within};

/// The name of the space this adapter resolves names in: the space a record written
/// before spaces existed is read as naming its files in
/// ([`PATH_ONLY_SPACE`]), so an old pin always names a space this adapter answers
/// for.
pub const FILE_SPACE: &str = PATH_ONLY_SPACE;

/// The most bytes of a file a reading reads: the largest file the search for moves
/// reads ([`ScanLimits::file_bytes`]); a larger one cannot be told.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The name of the preset of a link to files that stands on each byte of them.
pub const FILE_PRESET: &str = "file";

/// The name of the preset of a link to files that stands on their being there.
pub const CODE_PRESET: &str = "code";

/// The presets this adapter offers, each `space` [`FILE_SPACE`] and
/// `means = "depends"`:
///
/// | preset | `compare` | an edit of a file | a file removed or renamed |
/// |---|---|---|---|
/// | [`FILE_PRESET`] | `bytes` | makes the approvals that pin it stale | breaks them |
/// | [`CODE_PRESET`] | `presence` | changes nothing | breaks them |
///
/// `code` is for a decision's link to the code that carries it out: the code is
/// edited all the time without the decision changing, so only that the file is
/// there is pinned, and a file that goes away, or moves, breaks them. `judge
/// moved` follows a move; `moves` can suggest one only from Git's renames (a
/// rename staged in the index, or committed), since a presence pin has no content
/// to search for (its note `no_content`).
pub fn presets() -> BuiltinPresets {
    let values = |compare: &str| PresetValues {
        space: Some(FILE_SPACE.to_owned()),
        compare: Some(compare.to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    [(FILE_PRESET, "bytes"), (CODE_PRESET, "presence")]
        .into_iter()
        // Not a name: no preset. The test of this module holds each to be one.
        .filter_map(|(name, compare)| {
            PresetName::new(name)
                .ok()
                .map(|name| (name, values(compare)))
        })
        .collect()
}

/// Why a file could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// What is at the path, or at a parent of it, could not be looked at.
    #[error("cannot look at {}", .path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// A step of writing the file failed: what it was, with the path and what
    /// became of the file it made.
    #[error("cannot {step}")]
    Write {
        /// The step, with its path.
        step: String,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// The space [`FILE_SPACE`]: it answers a move of a prefix, suggests where a file
/// moved, reads content back by the name of its blob, lists the files a selection
/// takes, and says where a name lives (the file it writes).
impl SpaceAdapter for FileSpace {
    type Error = ArtifactError;
    type Prefixes = Answers;
    type Suggest = Answers;
    type ReadBack = Answers;
    type List = Answers;
    type Places = Answers;

    fn space(&self) -> &str {
        FILE_SPACE
    }

    fn presets(&self) -> BuiltinPresets {
        presets()
    }

    async fn resolve(&self, side: Side, asks: &SpaceAsks) -> Result<SpaceReadings, ArtifactError> {
        self.resolve_at(side, asks).await
    }

    async fn current(
        &self,
        side: Side,
        name: &ArtifactName,
    ) -> Result<Option<Vec<u8>>, ArtifactError> {
        self.current_at(side, name).await
    }
}

impl NamePrefixes for FileSpace {
    fn renamed(
        &self,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), ArtifactError> {
        renamed(names, of).map_err(ArtifactError::Reason)
    }
}

impl SpaceSuggest for FileSpace {
    async fn suggest(
        &self,
        gone: &[GoneName],
    ) -> Result<BTreeMap<ArtifactName, NameSuggestions>, ArtifactError> {
        FileSpace::suggest(self, gone).await
    }
}

impl SpaceReadBack for FileSpace {
    async fn read_back(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, ArtifactError> {
        FileSpace::read_back(self, address).await
    }
}

impl NameList for FileSpace {
    fn select(&self, selection: &Selection) -> Result<(), NotASelection> {
        FileSelection::read(selection).map(|_| ())
    }

    async fn list(&self, side: Side, selection: &Selection) -> Result<NameListing, ArtifactError> {
        self.list_at(side, selection).await
    }
}

impl NamePlaces for FileSpace {
    /// A name lives in the files its content is read from, whole: the module page,
    /// "Where a name lives".
    async fn places(
        &self,
        side: Side,
        names: &BTreeSet<ArtifactName>,
    ) -> Result<BTreeMap<ArtifactName, Placement>, ArtifactError> {
        self.state.dir(side)?;
        let mut placed = BTreeMap::new();
        for name in names {
            let placement = match path_of(name) {
                Ok(path) => placed_only(&self.state, side, &path)
                    .await
                    .map_err(ArtifactError::Reason)?,
                Err(why) => Placement::Unplaced(no_path_reason(&why)?),
            };
            placed.insert(name.clone(), placement);
        }
        Ok(placed)
    }
}

/// Where the file `path` names lives at `side`, by the rules this space reads it by:
/// [`Placement::Only`] of the files its content is read from (the path as written,
/// and each link entry on its way with where they lead inside the root), there or
/// not; or unplaced, with the reason `resolve` gives, when nothing is read through it.
/// The one reading of it, for this space and for those whose names start with a path
/// of it (`rust-test`, `js-test`).
pub(super) async fn placed_only(
    state: &FileStates,
    side: Side,
    path: &RelPath,
) -> Result<Placement, ValueError> {
    Ok(match placed_within(state, side, path).await {
        Placed::Files { files, .. } => Placement::Only(files),
        Placed::Unplaced(why) => Placement::Unplaced(NonBlank::new(why)?),
    })
}

/// What is said of a text of a selection that is no pattern, when what says which
/// cannot be made (it never cannot: it is not blank).
const NO_PATTERN: TextLiteral = TextLiteral::new("a text of the selection is no pattern of files");

/// A [`Selection`] of the space `file`, read: each text is a [`PathGlob`], the one
/// grammar of a segment glob, matched against a path under the root.
struct FileSelection {
    include: Vec<PathGlob>,
    exclude: Vec<PathGlob>,
}

impl FileSelection {
    /// The selection read, or the first text that is no pattern of the grammar, with
    /// why. No I/O.
    fn read(selection: &Selection) -> Result<Self, NotASelection> {
        let globs = |list: SelectionList, texts: &[String]| {
            texts
                .iter()
                .map(|text| {
                    PathGlob::new(text).map_err(|why| {
                        // What is said starts with a quoted text: it is never blank.
                        let said = NonBlank::new(format!(
                            "`{text}` of the {list} of a selection of files {why}"
                        ));
                        NotASelection::new(said.unwrap_or_else(|_| NonBlank::from(NO_PATTERN)))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self {
            include: globs(SelectionList::Include, selection.include())?,
            exclude: globs(SelectionList::Exclude, selection.exclude())?,
        })
    }

    /// Whether the selection takes the path: some `include` matches it and no
    /// `exclude` does.
    fn takes(&self, path: &str) -> bool {
        self.include.iter().any(|glob| glob.matches(path))
            && !self.exclude.iter().any(|glob| glob.matches(path))
    }
}

/// Why the files could not be read at all: a file that is there and cannot be read
/// is no error, but a name that cannot be told.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    /// Git gave no usable name for a file that is there.
    #[error(transparent)]
    Git(#[from] GitError),
    /// The domain refused the reason this adapter gave (a bug if it ever happens:
    /// the reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
    /// The state of the project could not be read at the side asked.
    #[error(transparent)]
    State(StateError),
}

/// Git asked through the state is Git asked by the space: one error for it.
impl From<StateError> for ArtifactError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::Git(git) => Self::Git(git),
            other => Self::State(other),
        }
    }
}

/// The files of a project, and its Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct FileSpace {
    root: PathBuf,
    git: Git,
    /// The state of the project the files are read from.
    state: FileStates,
    limits: ScanLimits,
    /// The most bytes of a file a reading reads.
    max_file: u64,
}

/// The bounds of one search for the same content ([`FileSpace::suggest`]) and of one
/// listing of the files a selection takes ([`FileSpace::list`]): a search that reaches
/// one stops, or passes a file over, and a note says so; a listing that reaches one is
/// told as some of the names, never as all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// The most files read.
    pub files: usize,
    /// The largest file read, in bytes; a larger one is passed over.
    pub file_bytes: u64,
    /// The most bytes read in all.
    pub total_bytes: u64,
    /// The most bytes of Git's listing of the work tree held, and of its answer of
    /// the renames since a revision.
    pub listing_bytes: usize,
    /// The most names a listing of a selection holds: more than this is some of the
    /// names ([`NameListing::Some`]). Unlike `files`, nothing of them is read.
    pub names: usize,
}

impl Default for ScanLimits {
    /// 20,000 files, 16 MiB a file, 512 MiB in all, a listing of 16 MiB, and 20,000
    /// names a listing.
    ///
    /// Why these: what an approval pins is evidence a document stands on (source,
    /// tests, a small data file), and such files are far below 16 MiB, while a
    /// larger one (a build output, a data set, a media file) is rarely pinned and
    /// would cost the most to read; 512 MiB and 20,000 files bound the work of one
    /// answer in a work tree with many files Git does not ignore; a listing of
    /// 16 MiB holds that many paths many times over; a selection that takes more
    /// than 20,000 names is one a person cannot hold a gate to, and it is told as
    /// such (some of the names) rather than cut silently.
    fn default() -> Self {
        Self {
            files: 20_000,
            file_bytes: MAX_FILE_BYTES,
            total_bytes: 512 * 1024 * 1024,
            listing_bytes: 16 * 1024 * 1024,
            names: 20_000,
        }
    }
}

/// The path a name of the space is: the one reader of the grammar of the names.
fn path_of(name: &ArtifactName) -> Result<RelPath, ValueError> {
    name.as_str().parse::<RelPath>()
}

/// Why a text is no name of the space, from why it is no path ([`path_of`]): the one
/// place the reason is written, which `resolve` and where a name lives both give.
fn no_path_reason(why: &ValueError) -> Result<NonBlank, ArtifactError> {
    NonBlank::new(format!("no path under the root of the project: {why}"))
        .map_err(ArtifactError::Reason)
}

/// One part of a path: what lies between two `/` of it. Not empty, not `.` or `..`
/// ([`RelPath`] reads a path into them), so a prefix ends only between two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Part(String);

impl Part {
    /// The part as written.
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The parts of a path, in order: the one reader of where a part ends, for the names
/// of this space and for every adapter whose names start with the path of a file.
pub(super) fn parts_of(path: &RelPath) -> Vec<Part> {
    path.as_str()
        .split('/')
        .map(|part| Part(part.to_owned()))
        .collect()
}

/// The path of these parts, in the form the names of the space write it, or why they
/// are none (no part, or a part [`RelPath`] refuses).
pub(super) fn path_from(parts: &[Part]) -> Result<RelPath, ValueError> {
    parts
        .iter()
        .map(Part::as_str)
        .collect::<Vec<_>>()
        .join("/")
        .parse()
}

/// A prefix of the names of the space, read: the parts of a path under the root, as
/// a name's path is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FilePrefix(Vec<Part>);

impl FilePrefix {
    /// The prefix a text writes: a path under the root, read as a name's is, after one
    /// `/` at its end is dropped (a directory as a shell completes it: `src/a/` is
    /// `src/a`). The rule of a name's path is not changed: a name never ends in `/`.
    pub(super) fn parse(text: &str) -> Result<Self, ValueError> {
        let path: RelPath = text.strip_suffix('/').unwrap_or(text).parse()?;
        Ok(Self(parts_of(&path)))
    }

    /// The parts of a path after this prefix, when the parts of the path start with
    /// all of its parts: a part is never cut (`src/a` is no prefix of `src/ab.rs`).
    pub(super) fn rest<'a>(&self, path: &'a [Part]) -> Option<&'a [Part]> {
        path.strip_prefix(self.0.as_slice())
    }

    /// Its parts.
    pub(super) fn parts(&self) -> &[Part] {
        &self.0
    }
}

/// What this adapter says of a move of a prefix, for the `names` of its space asked
/// about: `from` is read as a path under the root (else it is no prefix of the
/// space's names), each name is read as a path, and one whose parts start with the
/// parts of `from` is the same path with those parts replaced by the parts of `to`
/// (a `to` that is no path makes each such name unbuildable). A name that is no path
/// is not under it. Nothing is looked for in the text of a name.
pub(super) fn renamed(
    names: &BTreeSet<ArtifactName>,
    of: &PrefixMove,
) -> Result<(SpacePrefixes, SpaceRenamed), ValueError> {
    let no_path = |prefix: &NamePrefix, why: ValueError| {
        format!("`{prefix}` is no path under the root of the project: {why}")
    };
    let from = match FilePrefix::parse(of.from().as_str()) {
        Ok(from) => from,
        Err(why) => {
            let why = NonBlank::new(no_path(of.from(), why))?;
            return Ok((SpacePrefixes::NotUnder(why), SpaceRenamed::new()));
        }
    };
    let to = FilePrefix::parse(of.to().as_str());
    let mut answers = SpaceRenamed::new();
    for name in names {
        let answer = match path_of(name).ok().map(|path| parts_of(&path)) {
            None => Renamed::Outside,
            Some(parts) => match (from.rest(&parts), &to) {
                (None, _) => Renamed::Outside,
                (Some(_), Err(why)) => super::unbuildable(no_path(of.to(), why.clone()))?,
                (Some(rest), Ok(to)) => {
                    let joined: Vec<Part> = to.parts().iter().chain(rest).cloned().collect();
                    match path_from(&joined) {
                        Ok(path) => super::built(path.as_str())?,
                        Err(why) => super::unbuildable(no_path(of.to(), why))?,
                    }
                }
            },
        };
        answers.insert(name.clone(), answer);
    }
    Ok((SpacePrefixes::Answered, answers))
}

impl FileSpace {
    /// The files under `root`, read at one side (the root as it is).
    pub fn new(root: PathBuf) -> Self {
        Self::on(FileStates::new(root))
    }

    /// The files of the project `state` reads; a new file is written, and moves are
    /// searched for, under the root of the side after.
    pub fn on(state: FileStates) -> Self {
        let root = state.root().to_path_buf();
        Self {
            git: Git::new(root.clone()),
            root,
            state,
            limits: ScanLimits::default(),
            max_file: MAX_FILE_BYTES,
        }
    }

    /// The same, searching for the same content within `limits`.
    pub fn with_limits(mut self, limits: ScanLimits) -> Self {
        self.limits = limits;
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::process::Stdio;

    use tokio::process::Command;
    use whyshelf_core::app::shelf::{Creation, FileWrite, RevisionSource};
    use whyshelf_core::domain::shelf::artifact::{
        ArtifactReading, ArtifactState, Comparison, LinePrefix, Space, Untold,
    };
    use whyshelf_core::domain::shelf::ledger::{FoundBy, MoveNote, NoteKind};

    use whyshelf_core::domain::nonempty::NonEmptySet;

    use super::super::super::git::RenamesSince;
    use super::super::conformance;
    use super::*;

    /// Whether `git` can be run here; the tests that need it pass without it.
    async fn git_installed() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success())
    }

    async fn run(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn p(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    /// Each name, asked for its bytes only.
    fn bytes_of(paths: &[&str]) -> SpaceAsks {
        paths
            .iter()
            .map(|t| (p(t), BTreeSet::from([Comparison::Bytes])))
            .collect()
    }

    fn lines(ignore: &[&str]) -> Comparison {
        Comparison::Lines {
            ignore: ignore.iter().map(|i| LinePrefix::new(i).unwrap()).collect(),
        }
    }

    async fn write(dir: &std::path::Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(full, text).await.unwrap();
    }

    /// The bound of bytes of the files of the conformance test.
    const MAX_TEST_FILE: u64 = 64;

    /// The hook of the conformance test: files in a repository (when Git is here, so
    /// that each has an address), one that cannot be read, one past the bound, a link
    /// out of the root and a link inside it.
    struct Files;

    impl conformance::Hook for Files {
        fn space(&self) -> &'static str {
            FILE_SPACE
        }

        fn source(&self) -> &'static str {
            include_str!("file.rs")
        }

        fn bounds(&self) -> Vec<&'static str> {
            vec!["a file"]
        }

        fn unread(&self) -> conformance::Unread {
            conformance::Unread::CannotTell
        }

        fn walks_nesting(&self) -> bool {
            false
        }

        fn lists(&self) -> bool {
            true
        }

        fn places(&self) -> bool {
            true
        }

        fn adapter_at(&self, dir: &std::path::Path) -> super::super::Adapter {
            super::super::Adapter::File(FileSpace {
                max_file: MAX_TEST_FILE,
                ..FileSpace::new(dir.to_path_buf())
            })
        }

        async fn tree(&self, dir: &std::path::Path) -> conformance::Tree {
            let root = dir.join("root");
            write(&root, "a.txt", "one\n").await;
            write(&root, "dir/b.txt", "two\n").await;
            write(&root, "locked.txt", "locked\n").await;
            let over = usize::try_from(MAX_TEST_FILE).unwrap() + 1;
            write(&root, "big.txt", &"x".repeat(over)).await;
            if git_installed().await {
                run(&root, &["init", "-q"]).await;
                run(&root, &["add", "-A"]).await;
            }
            // Written after the files were added: Git names its blob, and holds none.
            write(&root, "new.txt", "never added\n").await;
            let mut tree = conformance::Tree::of(self.adapter_at(&root));
            tree.present = vec![p("a.txt"), p("dir/b.txt")];
            tree.unstored = vec![p("new.txt")];
            tree.absent = vec![p("missing.txt"), p("A.txt"), p("dir")];
            tree.not_names = vec![p("/a.txt"), p("a//b.txt"), p("../a.txt")];
            // A name lives in the file it writes, there or not.
            let only = |text: &str| {
                Placement::Only(NonEmptySet::new(text.parse::<RelPath>().unwrap(), []))
            };
            tree.placed = ["a.txt", "dir/b.txt", "missing.txt", "dir"]
                .into_iter()
                .map(|text| (p(text), only(text)))
                .collect();
            tree.over_bytes = vec![conformance::OverBytes::Name {
                bound: "a file",
                name: p("big.txt"),
            }];
            tree.prefix = Some((
                PrefixMove::new(
                    NamePrefix::new("dir").unwrap(),
                    NamePrefix::new("moved").unwrap(),
                )
                .unwrap(),
                p("dir/b.txt"),
                p("moved/b.txt"),
            ));
            // Files are listed through Git: with none here, no listing is made.
            if git_installed().await {
                let one_name = ScanLimits {
                    names: 1,
                    ..ScanLimits::default()
                };
                tree.listing = Some(conformance::ListCase {
                    // The file that cannot be read and the one past the bound make a
                    // listing `Some`; the whole is listed of the others.
                    selection: Selection::new(
                        vec!["**/*.txt".to_owned()],
                        vec!["locked.txt".to_owned(), "big.txt".to_owned()],
                    )
                    .unwrap(),
                    bad: Selection::new(vec!["/a.txt".to_owned()], vec![]).unwrap(),
                    bounded: super::super::Adapter::File(
                        FileSpace {
                            max_file: MAX_TEST_FILE,
                            ..FileSpace::new(root.clone())
                        }
                        .with_limits(one_name),
                    ),
                });
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                write(dir, "outside/x.txt", "out\n").await;
                tokio::fs::symlink(dir.join("outside"), root.join("out"))
                    .await
                    .unwrap();
                tree.outside = Some(p("out/x.txt"));
                tokio::fs::symlink("a.txt", root.join("alias.txt"))
                    .await
                    .unwrap();
                tree.spellings = Some((p("a.txt"), p("alias.txt")));
                // Read through the link: the link and what it leads to.
                let both = NonEmptySet::new(
                    "alias.txt".parse::<RelPath>().unwrap(),
                    ["a.txt".parse::<RelPath>().unwrap()],
                );
                tree.placed.push((p("alias.txt"), Placement::Only(both)));
                tree.linked = Some(p("alias.txt"));
                let locked = root.join("locked.txt");
                tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
                    .await
                    .unwrap();
                // Run as a user every file opens to (root), it is no name to test.
                if tokio::fs::File::open(&locked).await.is_err() {
                    tree.unreadable = Some(p("locked.txt"));
                }
            }
            tree
        }
    }

    #[tokio::test]
    async fn the_adapter_keeps_the_contract_of_an_adapter() {
        conformance::check(&Files).await;
    }

    /// The bound of time is the whole reading's (Git is asked once for every file):
    /// past it the reading is an error, and no name is answered.
    #[tokio::test]
    async fn a_git_past_its_bound_of_time_fails_the_whole_reading() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "one\n").await;
        run(dir.path(), &["init", "-q"]).await;
        // Under a bound of zero, Git is stopped before it can answer: it is started
        // within the first poll, and cannot have exited by its end. The addresses are
        // the state's, so the bound is set on the Git the state asks.
        let files = FileSpace::on(
            FileStates::new(dir.path().to_path_buf()).with_timeout(std::time::Duration::ZERO),
        );
        let found = files.resolve(&bytes_of(&["a.txt", "missing.txt"])).await;
        assert!(
            matches!(found, Err(ArtifactError::Git(GitError::TimedOut { .. }))),
            "{found:?}"
        );
    }

    #[test]
    fn the_space_of_the_files_is_the_one_an_old_record_names() {
        assert_eq!(FILE_SPACE, PATH_ONLY_SPACE);
    }

    #[test]
    fn the_adapter_offers_the_presets_file_and_code_for_its_space() {
        let offered = presets();
        let values = |compare: &str| PresetValues {
            space: Some("file".to_owned()),
            compare: Some(compare.to_owned()),
            ignore: None,
            means: Some(Propagation::Depends),
        };
        assert_eq!(
            offered,
            BuiltinPresets::from([
                (PresetName::new(FILE_PRESET).unwrap(), values("bytes")),
                (PresetName::new(CODE_PRESET).unwrap(), values("presence")),
            ])
        );
    }

    #[test]
    fn the_preset_code_expands_a_link_to_files_pinned_by_their_being_there() {
        use whyshelf_core::domain::shelf::schema::Schema;
        let space = Space::new(FILE_SPACE).unwrap();
        let decls = super::super::super::books::parse(
            "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
             [[book.link]]\nname='code'\ntarget='artifact'\npreset='code'\n",
        )
        .unwrap();
        let schema = Schema::declared(
            decls,
            &presets(),
            &BTreeSet::from([space]),
            &Default::default(),
        )
        .unwrap();
        let book = schema.book("a").unwrap();
        let kind = book.artifact_links().get("code").unwrap();
        assert_eq!(kind.space.as_str(), "file");
        assert_eq!(kind.comparison.kind_word(), "presence");
        assert_eq!(kind.propagation.propagation(), Propagation::Depends);
        assert!(
            book.preset_of("code")
                .is_some_and(|p| p.name().as_str() == CODE_PRESET && !p.is_default())
        );
    }

    /// A project whose note links `src/a.rs` through the preset `code`, approved.
    async fn approved_by_code(root: &std::path::Path) -> ShelfAppOf {
        use whyshelf_core::app::shelf::{JudgeRequest, Judgement};
        let books = BOOKS.replace(
            "space = \"file\"\nmeans = \"depends\"\n",
            "preset = \"code\"\n",
        );
        write(root, ".whyshelf/books.toml", &books).await;
        write(root, "notes/N-1.md", NOTE).await;
        write(root, "src/a.rs", CODE).await;
        let mut app = whyshelf_core::app::shelf::ShelfApp::new(
            super::super::super::Project::new(root.to_path_buf(), Some("t".to_owned()), None, None)
                .unwrap(),
        );
        app.judge_all(vec![JudgeRequest {
            judgement: Judgement::Approve {
                until: None,
                reason: None,
                again: false,
            },
            reference: "N-1".parse().unwrap(),
            by: Default::default(),
        }])
        .await
        .unwrap();
        app
    }

    type ShelfAppOf = whyshelf_core::app::shelf::ShelfApp<super::super::super::Project>;

    /// Where N-1 stands now.
    async fn standing_of_n1(
        app: &ShelfAppOf,
    ) -> whyshelf_core::domain::shelf::freshness::StandingKind {
        let status = app.status().await.unwrap();
        assert_eq!(status.rows.len(), 1);
        status.rows[0].standing.kind()
    }

    #[tokio::test]
    async fn a_link_of_the_preset_code_stays_fresh_on_an_edit_and_breaks_on_a_removal() {
        use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};
        let dir = tempfile::tempdir().unwrap();
        let app = approved_by_code(dir.path()).await;
        let fresh = StandingKind::Approved(FreshnessLevel::Fresh);
        assert_eq!(standing_of_n1(&app).await, fresh);
        // An edit of the code changes nothing.
        write(dir.path(), "src/a.rs", "fn one() { two() }\n").await;
        assert_eq!(standing_of_n1(&app).await, fresh);
        // The file gone breaks the approval, and `moves` names it.
        tokio::fs::remove_file(dir.path().join("src/a.rs"))
            .await
            .unwrap();
        assert_eq!(
            standing_of_n1(&app).await,
            StandingKind::Approved(FreshnessLevel::Broken)
        );
        // `moves` cannot search it by content, and says so.
        let moves = app.moves().await.unwrap();
        assert_eq!(moves.suggestions.absent.len(), 1);
        let gone = &moves.suggestions.absent[0];
        assert_eq!(gone.gone().name().as_str(), "src/a.rs");
        assert!(gone.candidates().is_empty());
        assert!(
            gone.notes().iter().any(|n| n.kind() == NoteKind::NoContent),
            "{:?}",
            gone.notes()
        );
    }

    #[tokio::test]
    async fn a_file_of_the_preset_code_renamed_holds_again_once_judge_moved_follows_it() {
        use whyshelf_core::app::shelf::MovedRequest;
        use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};
        let dir = tempfile::tempdir().unwrap();
        let mut app = approved_by_code(dir.path()).await;
        // A plain rename, and the link written anew: broken until the move is
        // declared, as the pin still names the old file.
        tokio::fs::rename(dir.path().join("src/a.rs"), dir.path().join("src/b.rs"))
            .await
            .unwrap();
        write(
            dir.path(),
            "notes/N-1.md",
            &NOTE.replace("src/a.rs", "src/b.rs"),
        )
        .await;
        assert_ne!(
            standing_of_n1(&app).await,
            StandingKind::Approved(FreshnessLevel::Fresh)
        );
        app.moved(vec![MovedRequest {
            from: p("src/a.rs"),
            to: p("src/b.rs"),
            reason: NonBlank::new("renamed").unwrap(),
            by: Default::default(),
        }])
        .await
        .unwrap();
        assert_eq!(
            standing_of_n1(&app).await,
            StandingKind::Approved(FreshnessLevel::Fresh)
        );
    }

    #[tokio::test]
    async fn a_new_file_is_written_where_nothing_is_and_never_over_one() {
        let dir = tempfile::tempdir().unwrap();
        let files = FileSpace::new(dir.path().to_path_buf());
        let at: RelPath = "data/t.csv".parse().unwrap();
        assert_eq!(FileWrite::in_the_way(&files, &at).await.unwrap(), None);
        assert_eq!(
            files.create(&at, b"a,b\n").await.unwrap(),
            Creation::Created
        );
        assert_eq!(
            FileWrite::in_the_way(&files, &at).await.unwrap(),
            Some(at.clone())
        );
        assert_eq!(
            files.create(&at, b"other\n").await.unwrap(),
            Creation::AlreadyThere
        );
        assert_eq!(
            tokio::fs::read(dir.path().join("data/t.csv"))
                .await
                .unwrap(),
            b"a,b\n"
        );
    }

    #[tokio::test]
    async fn outside_a_repository_a_file_has_a_hash_and_no_address() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source
            .resolve(&bytes_of(&["tests/a.rs", "tests/none.rs"]))
            .await
            .unwrap();
        assert_eq!(
            found.get(&p("tests/a.rs")),
            Some(&ArtifactReading::of_bytes(b"one\n", &BTreeSet::new(), None))
        );
        assert_eq!(found[&p("tests/a.rs")].address(), None);
        assert_eq!(
            found.get(&p("tests/none.rs")),
            Some(&ArtifactReading::absent())
        );
        assert_eq!(
            source.current(&p("tests/a.rs")).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
        assert_eq!(source.current(&p("tests/none.rs")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_text_that_is_no_path_under_the_root_is_not_a_name() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let wrongs = ["../a.rs", "/a.rs", "a//b", "./a.rs", "a/../a.rs", "a\\b"];
        let mut all = wrongs.to_vec();
        all.push("a.rs");
        let found = source.resolve(&bytes_of(&all)).await.unwrap();
        for wrong in wrongs {
            let why = found[&p(wrong)].not_a_name_why();
            assert!(
                why.is_some_and(|w| w.as_str().starts_with("no path under the root")),
                "{wrong}: {why:?}"
            );
            assert_eq!(source.current(&p(wrong)).await.unwrap(), None);
        }
        assert!(found[&p("a.rs")].is_present());
        assert_eq!(found[&p("a.rs")].not_a_name_why(), None);
    }

    #[tokio::test]
    async fn a_path_that_differs_from_the_name_only_in_case_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Justfile", "check:\n").await;
        write(dir.path(), "Tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source
            .resolve(&bytes_of(&[
                "justfile",
                "tests/a.rs",
                "Tests/A.rs",
                "Justfile",
            ]))
            .await
            .unwrap();
        for wrong in ["justfile", "tests/a.rs", "Tests/A.rs"] {
            assert_eq!(
                found.get(&p(wrong)),
                Some(&ArtifactReading::absent()),
                "{wrong}"
            );
        }
        assert!(
            found
                .get(&p("Justfile"))
                .is_some_and(ArtifactReading::is_present)
        );
        assert_eq!(source.current(&p("justfile")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_directory_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source.resolve(&bytes_of(&["tests"])).await.unwrap();
        assert_eq!(found.get(&p("tests")), Some(&ArtifactReading::absent()));
    }

    #[tokio::test]
    async fn a_file_is_hashed_under_each_comparison_asked_and_its_bytes_always() {
        let dir = tempfile::tempdir().unwrap();
        let text = "// a comment\nfn  main() {}\n";
        write(dir.path(), "src/a.rs", text).await;
        write(dir.path(), "src/b.rs", text).await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let slashes = lines(&["//"]);
        let asks = SpaceAsks::from([
            (p("src/a.rs"), BTreeSet::from([slashes.clone(), lines(&[])])),
            (p("src/b.rs"), BTreeSet::new()),
        ]);
        let found = source.resolve(&asks).await.unwrap();

        let a = &found[&p("src/a.rs")];
        assert!(a.is_present());
        for comparison in [Comparison::Bytes, slashes.clone(), lines(&[])] {
            assert_eq!(
                a.state(&comparison),
                ArtifactState::Present(comparison.fingerprint(text.as_bytes())),
                "{comparison:?}"
            );
        }
        // The whole reading is the one the domain makes of those bytes.
        assert_eq!(
            a,
            &ArtifactReading::of_bytes(text.as_bytes(), &asks[&p("src/a.rs")], None)
        );
        // A comparison not asked cannot be told.
        assert_eq!(
            a.state(&lines(&["#"])),
            ArtifactState::Unknown(Untold::NotAsked)
        );
        // Asked for nothing, a file still has the hash of its bytes, and no other.
        let b = &found[&p("src/b.rs")];
        assert_eq!(
            b.state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(text.as_bytes()))
        );
        assert_eq!(b.state(&slashes), ArtifactState::Unknown(Untold::NotAsked));
    }

    #[tokio::test]
    async fn in_a_repository_a_file_has_its_blob_name_and_reads_back_once_committed() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]).await;
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let git = Git::new(dir.path().to_path_buf());

        // Before the first commit: an address, no revision, and nothing to read back.
        let found = source.resolve(&bytes_of(&["tests/a.rs"])).await.unwrap();
        let Some(address) = found
            .get(&p("tests/a.rs"))
            .and_then(ArtifactReading::address)
        else {
            panic!("a file in a repository has an address: {found:?}");
        };
        let expected = run(dir.path(), &["hash-object", "tests/a.rs"]).await;
        assert_eq!(address.as_str(), expected);
        assert_eq!(git.revision().await.unwrap(), None);
        assert_eq!(source.read_back(address).await.unwrap(), None);

        run(dir.path(), &["add", "."]).await;
        run(dir.path(), &["commit", "-q", "-m", "start"]).await;
        let head = run(dir.path(), &["rev-parse", "HEAD"]).await;
        assert_eq!(
            git.revision().await.unwrap().map(|r| r.as_str().to_owned()),
            Some(head)
        );

        // Edited after: the old content still reads back by its name.
        write(dir.path(), "tests/a.rs", "two\n").await;
        assert_eq!(
            source.read_back(address).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
    }

    // Suggesting moves, through `moves` on a project of one note that stands on
    // `a.rs`.

    /// Git run on `dir` with the person's own settings shut out: no system file, an
    /// empty global file, a home of its own, and no `GIT_*` variable of the caller.
    async fn isolated_git(dir: &std::path::Path, args: &[&str]) {
        let home = dir.join(".home");
        tokio::fs::create_dir_all(&home).await.unwrap();
        let global = home.join("gitconfig");
        tokio::fs::write(&global, "").await.unwrap();
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(&name);
            }
        }
        let out = command
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", &global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    const BOOKS: &str = "[[book]]\nname = \"note\"\ndir = \"notes\"\nkey = { prefix = \"N\" }\n[[book.field]]\nname = \"title\"\ntype = \"text\"\nrequired = true\n[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n";

    const NOTE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nfiles: [\"src/a.rs\"]\n---\nThe body.\n";

    /// Twelve lines, so that an edit of one keeps the file alike to Git.
    const CODE: &str = "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\nfn six() {}\nfn seven() {}\nfn eight() {}\nfn nine() {}\nfn ten() {}\nfn eleven() {}\nfn twelve() {}\n";

    /// A project whose note stands on `src/a.rs`, in a Git repository when `git`,
    /// committed, and approved (at that commit, in a repository).
    async fn approved(git: bool) -> tempfile::TempDir {
        approved_with(git, &[]).await
    }

    /// As [`approved`], with these files written (over the others) before the
    /// commit.
    async fn approved_with(git: bool, files: &[(&str, &str)]) -> tempfile::TempDir {
        use whyshelf_core::app::shelf::{JudgeRequest, Judgement, ShelfApp};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".whyshelf/books.toml", BOOKS).await;
        write(root, "notes/N-1.md", NOTE).await;
        write(root, "src/a.rs", CODE).await;
        for (at, text) in files {
            write(root, at, text).await;
        }
        if git {
            isolated_git(root, &["init", "-q"]).await;
            isolated_git(root, &["add", "."]).await;
            isolated_git(root, &["commit", "-q", "-m", "start"]).await;
        }
        let mut app = ShelfApp::new(
            super::super::super::Project::new(root.to_path_buf(), Some("t".to_owned()), None, None)
                .unwrap(),
        );
        app.judge_all(vec![JudgeRequest {
            judgement: Judgement::Approve {
                until: None,
                reason: None,
                again: false,
            },
            reference: "N-1".parse().unwrap(),
            by: Default::default(),
        }])
        .await
        .unwrap();
        dir
    }

    /// What `moves` answers for `src/a.rs`: each candidate's name and how it was
    /// found, and the kinds of its notes.
    async fn suggested(root: &std::path::Path) -> (Vec<(String, FoundBy)>, Vec<NoteKind>) {
        use whyshelf_core::app::shelf::ShelfApp;
        let app = ShelfApp::new(
            super::super::super::Project::new(root.to_path_buf(), Some("t".to_owned()), None, None)
                .unwrap(),
        );
        let report = app.moves().await.unwrap();
        let absent = &report.suggestions.absent;
        assert_eq!(absent.len(), 1, "{absent:?}");
        assert_eq!(absent[0].gone().name().as_str(), "src/a.rs");
        let candidates = absent[0]
            .candidates()
            .iter()
            .inspect(|c| assert!(c.present(), "{c:?}"))
            .map(|c| (c.name().as_str().to_owned(), c.found()))
            .collect();
        (
            candidates,
            absent[0].notes().iter().map(MoveNote::kind).collect(),
        )
    }

    /// The gone names of the project at `root`, read as the use case reads them:
    /// the ledger's pinned names, resolved by this adapter.
    async fn gone_at(root: &std::path::Path) -> Vec<GoneName> {
        use whyshelf_core::app::shelf::{LedgerRead, Side};
        use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
        use whyshelf_core::domain::shelf::ledger::{Admit, Ledger, PinnedNames};
        let ledger =
            super::super::super::ledger::JsonLedger::new(root, ".whyshelf/ledger".parse().unwrap());
        let pinned = PinnedNames::of(&Ledger::build(
            ledger.read(Side::After).await.unwrap().records,
            &Admit::unruled(),
        ));
        let source = FileSpace::new(root.to_path_buf());
        let mut readings = ArtifactReadings::new();
        for (space, names) in pinned.asks().spaces() {
            readings.extend_space(space.clone(), source.resolve(names).await.unwrap());
        }
        pinned
            .gone(&readings)
            .spaces()
            .flat_map(|(_, names)| names)
            .collect()
    }

    async fn rename(root: &std::path::Path, from: &str, to: &str) {
        let to = root.join(to);
        tokio::fs::create_dir_all(to.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::rename(root.join(from), to).await.unwrap();
    }

    #[tokio::test]
    async fn a_file_moved_unchanged_is_found_by_its_content() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        // Moved with the system, not with Git: Git does not see the new file as a
        // rename, and its content is what was pinned.
        rename(dir.path(), "src/a.rs", "lib/b.rs").await;
        let (candidates, notes) = suggested(dir.path()).await;
        assert_eq!(candidates, [("lib/b.rs".to_owned(), FoundBy::SameContent)]);
        assert_eq!(notes, []);
    }

    #[tokio::test]
    async fn a_file_moved_with_an_edit_is_found_by_gits_rename_detection() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        isolated_git(dir.path(), &["mv", "src/a.rs", "src/b.rs"]).await;
        write(dir.path(), "src/b.rs", &CODE.replace("seven", "seventh")).await;
        // The rename is read from the index, which holds the edit once it is added.
        isolated_git(dir.path(), &["add", "src/b.rs"]).await;
        let index = tokio::fs::read(dir.path().join(".git/index"))
            .await
            .unwrap();
        let (candidates, note) = suggested(dir.path()).await;
        // Git's reading wrote nothing: the index is as it was.
        assert_eq!(
            tokio::fs::read(dir.path().join(".git/index"))
                .await
                .unwrap(),
            index
        );
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0].0, "src/b.rs");
        let FoundBy::Renamed {
            similarity: Some(similarity),
        } = candidates[0].1
        else {
            panic!("a rename with a score: {candidates:?}");
        };
        assert!((50..100).contains(&similarity.percent()), "{similarity:?}");
        assert_eq!(note, []);
    }

    #[tokio::test]
    async fn outside_a_repository_the_content_is_still_found_and_renames_are_not_looked_for() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        tokio::fs::remove_dir_all(dir.path().join(".git"))
            .await
            .unwrap();
        rename(dir.path(), "src/a.rs", "lib/b.rs").await;
        let (candidates, notes) = suggested(dir.path()).await;
        assert_eq!(candidates, [("lib/b.rs".to_owned(), FoundBy::SameContent)]);
        assert_eq!(notes, [NoteKind::NoRepository]);

        // Approved outside Git: the approval kept no revision.
        let dir = approved(false).await;
        rename(dir.path(), "src/a.rs", "lib/b.rs").await;
        let (candidates, notes) = suggested(dir.path()).await;
        assert_eq!(candidates, [("lib/b.rs".to_owned(), FoundBy::SameContent)]);
        assert_eq!(notes, [NoteKind::NoRevision]);
    }

    #[tokio::test]
    async fn a_revision_that_is_no_commit_of_the_repository_is_a_note() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        // Another history: the commit the approval was made at is not in it.
        tokio::fs::remove_dir_all(dir.path().join(".git"))
            .await
            .unwrap();
        write(dir.path(), "other.txt", "another history\n").await;
        isolated_git(dir.path(), &["init", "-q"]).await;
        isolated_git(dir.path(), &["add", "."]).await;
        isolated_git(dir.path(), &["commit", "-q", "-m", "another"]).await;
        isolated_git(dir.path(), &["mv", "src/a.rs", "src/b.rs"]).await;
        let (candidates, notes) = suggested(dir.path()).await;
        assert_eq!(candidates, [("src/b.rs".to_owned(), FoundBy::SameContent)]);
        assert_eq!(notes, [NoteKind::UnknownRevision]);
    }

    #[tokio::test]
    async fn no_filter_the_repository_names_is_run_and_the_answer_is_given() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        isolated_git(dir.path(), &["mv", "src/a.rs", "src/b.rs"]).await;
        write(dir.path(), "src/b.rs", &CODE.replace("seven", "seventh")).await;
        isolated_git(dir.path(), &["add", "src/b.rs"]).await;
        // From now on the repository names, for every file, a clean filter that leaves
        // a mark when it runs, a required one, one that fails, and a process filter.
        let marker = dir.path().join("filter-ran");
        let mark = format!("touch '{}'; cat", marker.display());
        isolated_git(dir.path(), &["config", "filter.mark.clean", &mark]).await;
        isolated_git(dir.path(), &["config", "filter.mark.smudge", &mark]).await;
        isolated_git(dir.path(), &["config", "filter.mark.required", "true"]).await;
        isolated_git(dir.path(), &["config", "filter.fail.clean", "false"]).await;
        isolated_git(dir.path(), &["config", "filter.proc.process", &mark]).await;
        isolated_git(dir.path(), &["config", "filter.proc.required", "true"]).await;
        write(
            dir.path(),
            ".gitattributes",
            "* filter=mark\n*.rs filter=fail\n*.md filter=proc\n",
        )
        .await;

        let (candidates, notes) = suggested(dir.path()).await;
        // A file there is named, its drivers shut out: an empty command is no
        // filter, and `required=false` lets the call go on.
        let git = Git::new(dir.path().to_path_buf());
        let named: [RelPath; 2] = ["notes/N-1.md".parse().unwrap(), "src/b.rs".parse().unwrap()];
        let blobs = git
            .blob_names(&[&named[0], &named[1]])
            .await
            .unwrap()
            .unwrap();
        for (path, blob) in named.iter().zip(&blobs) {
            let expected = run(dir.path(), &["hash-object", "--no-filters", path.as_str()]).await;
            assert_eq!(blob.as_str(), expected, "{path:?}");
        }
        assert!(tokio::fs::metadata(&marker).await.is_err(), "a filter ran");
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0].0, "src/b.rs");
        assert!(matches!(candidates[0].1, FoundBy::Renamed { .. }));
        assert_eq!(notes, []);
        // The filter is real: a `hash-object` that does not shut it out runs it.
        run(dir.path(), &["hash-object", ".gitattributes"]).await;
        assert!(
            tokio::fs::metadata(&marker).await.is_ok(),
            "the filter never runs"
        );
    }

    #[tokio::test]
    async fn a_driver_whose_name_is_not_utf8_runs_nothing_and_no_address_is_given() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        let marker = dir.path().join("filter-ran");
        let mut config = tokio::fs::read(dir.path().join(".git/config"))
            .await
            .unwrap();
        config.extend_from_slice(b"[filter \"e\xffv\"]\n\tclean = touch '");
        config.extend_from_slice(marker.display().to_string().as_bytes());
        config.extend_from_slice(b"'; cat\n");
        tokio::fs::write(dir.path().join(".git/config"), config)
            .await
            .unwrap();
        tokio::fs::write(dir.path().join(".gitattributes"), b"* filter=e\xffv\n")
            .await
            .unwrap();

        let found = FileSpace::new(dir.path().to_path_buf())
            .resolve(&bytes_of(&["src/a.rs"]))
            .await
            .unwrap();
        assert!(found[&p("src/a.rs")].is_present());
        assert_eq!(found[&p("src/a.rs")].address(), None);
        let named: RelPath = "src/a.rs".parse().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        assert_eq!(git.blob_names(&[&named]).await.unwrap(), None);
        assert!(tokio::fs::metadata(&marker).await.is_err(), "a filter ran");
        // The filter is real: a `hash-object` that does not shut it out runs it.
        run(dir.path(), &["hash-object", "src/a.rs"]).await;
        assert!(
            tokio::fs::metadata(&marker).await.is_ok(),
            "the filter never runs"
        );
    }

    #[tokio::test]
    async fn a_converted_text_file_is_named_as_git_stores_it_and_reads_back() {
        use whyshelf_core::app::shelf::ShelfApp;
        use whyshelf_core::domain::shelf::snapshot::{ItemKind, Retrieval};
        if !git_installed().await {
            return;
        }
        let crlf = CODE.replace('\n', "\r\n");
        let dir = approved_with(
            true,
            &[(".gitattributes", "* text=auto\n"), ("src/a.rs", &crlf)],
        )
        .await;
        // The address is the blob of the content converted, as `git hash-object`
        // names it, and not that of the bytes as they are.
        let found = FileSpace::new(dir.path().to_path_buf())
            .resolve(&bytes_of(&["src/a.rs"]))
            .await
            .unwrap();
        let address = found[&p("src/a.rs")].address().unwrap().clone();
        assert_eq!(
            address.as_str(),
            run(dir.path(), &["hash-object", "src/a.rs"]).await
        );
        assert_ne!(
            address.as_str(),
            run(dir.path(), &["hash-object", "--no-filters", "src/a.rs"]).await
        );
        // `get --approved` reads it back: the content Git stores.
        let app = ShelfApp::new(
            super::super::super::Project::new(
                dir.path().to_path_buf(),
                Some("t".to_owned()),
                None,
                None,
            )
            .unwrap(),
        );
        let approved = app.show_approved(&"N-1".parse().unwrap()).await.unwrap();
        let item = approved
            .items
            .iter()
            .find(|i| matches!(i.item.kind(), ItemKind::Artifact(_)))
            .unwrap();
        assert_eq!(item.retrieval, Retrieval::Read(CODE.as_bytes().to_vec()));
    }

    #[tokio::test]
    async fn the_search_for_the_same_content_stops_at_its_bounds_and_says_so() {
        let dir = approved(false).await;
        rename(dir.path(), "src/a.rs", "z/b.rs").await;
        // Files before it in the order of the walk, one too large to read.
        for at in ["a/1.rs", "a/2.rs", "a/3.rs"] {
            write(dir.path(), at, "other\n").await;
        }
        write(dir.path(), "a/big.rs", &"x".repeat(4096)).await;
        let gone = gone_at(dir.path()).await;
        assert_eq!(gone.len(), 1);
        let kinds = |found: &BTreeMap<ArtifactName, NameSuggestions>| {
            found[&p("src/a.rs")]
                .notes
                .iter()
                .map(MoveNote::kind)
                .collect::<Vec<_>>()
        };
        let bounded =
            |limits: ScanLimits| FileSpace::new(dir.path().to_path_buf()).with_limits(limits);

        // Within the bounds: found, with the large file passed over and said so.
        let roomy = ScanLimits {
            file_bytes: 1024,
            ..ScanLimits::default()
        };
        let found = bounded(roomy).suggest(&gone).await.unwrap();
        assert_eq!(found[&p("src/a.rs")].candidates.len(), 1);
        assert_eq!(
            kinds(&found),
            [NoteKind::NoRevision, NoteKind::TooLargeSkipped]
        );
        assert!(
            found[&p("src/a.rs")]
                .notes
                .contains(&MoveNote::TooLargeSkipped {
                    count: 1,
                    limit: 1024
                })
        );

        // Two files at most: the walk stops before `z/b.rs`, and says so.
        let found = bounded(ScanLimits { files: 2, ..roomy })
            .suggest(&gone)
            .await
            .unwrap();
        assert!(found[&p("src/a.rs")].candidates.is_empty());
        assert!(kinds(&found).contains(&NoteKind::ScanTruncated));

        // A few bytes in all: it stops too.
        let found = bounded(ScanLimits {
            total_bytes: 10,
            ..roomy
        })
        .suggest(&gone)
        .await
        .unwrap();
        assert!(found[&p("src/a.rs")].candidates.is_empty());
        assert!(
            found[&p("src/a.rs")]
                .notes
                .iter()
                .any(|n| matches!(n, MoveNote::ScanTruncated { read } if *read < 4))
        );
    }

    #[tokio::test]
    async fn a_listing_of_git_cut_at_its_bound_says_so() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        rename(dir.path(), "src/a.rs", "z/b.rs").await;
        let gone = gone_at(dir.path()).await;
        let found = FileSpace::new(dir.path().to_path_buf())
            .with_limits(ScanLimits {
                listing_bytes: 8,
                ..ScanLimits::default()
            })
            .suggest(&gone)
            .await
            .unwrap();
        assert!(
            found[&p("src/a.rs")]
                .notes
                .iter()
                .any(|n| n.kind() == NoteKind::ScanTruncated),
            "{found:?}"
        );
    }

    /// Git's answer of renames past its bound is cut, never read in part: the search
    /// for moves notes a history that cannot be read, and gives no rename.
    #[tokio::test]
    async fn an_answer_of_renames_past_its_bound_is_a_note_and_no_rename() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        isolated_git(dir.path(), &["mv", "src/a.rs", "src/b.rs"]).await;
        write(dir.path(), "src/b.rs", &CODE.replace("seven", "seventh")).await;
        isolated_git(dir.path(), &["add", "src/b.rs"]).await;
        let git = Git::new(dir.path().to_path_buf());
        let revision = git.revision().await.unwrap().unwrap();
        assert!(matches!(
            git.renames_since(&revision, 8).await,
            Err(GitError::Cut {
                command: "diff-index",
                cap: 8
            })
        ));
        assert!(matches!(
            git.renames_since(&revision, 1 << 20).await,
            Ok(RenamesSince::Found(renames)) if !renames.is_empty()
        ));
        let gone = gone_at(dir.path()).await;
        let found = FileSpace::new(dir.path().to_path_buf())
            .with_limits(ScanLimits {
                listing_bytes: 8,
                ..ScanLimits::default()
            })
            .suggest(&gone)
            .await
            .unwrap();
        let suggested = &found[&p("src/a.rs")];
        assert!(
            suggested
                .candidates
                .iter()
                .all(|c| !matches!(c.found(), FoundBy::Renamed { .. })),
            "{suggested:?}"
        );
        assert!(
            suggested.notes.iter().any(|n| matches!(
                n,
                MoveNote::HistoryUnreadable { detail } if detail.as_str().contains("more than 8 bytes")
            )),
            "{suggested:?}"
        );
    }

    /// A blob is read back only within the bound of a file: one past it reads back
    /// none, as it cannot be read back whole.
    #[tokio::test]
    async fn a_blob_past_the_bound_of_a_file_reads_back_none() {
        if !git_installed().await {
            return;
        }
        let dir = approved(true).await;
        let path: RelPath = "src/a.rs".parse().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        let address = git.blob_names(&[&path]).await.unwrap().unwrap().remove(0);
        let len = CODE.len();
        assert_eq!(
            git.read_blob_within(&address, len).await.unwrap(),
            Some(CODE.as_bytes().to_vec())
        );
        assert_eq!(git.read_blob_within(&address, len - 1).await.unwrap(), None);
        let small = FileSpace {
            max_file: u64::try_from(len - 1).unwrap(),
            ..FileSpace::new(dir.path().to_path_buf())
        };
        assert_eq!(small.read_back(&address).await.unwrap(), None);
        assert!(
            FileSpace::new(dir.path().to_path_buf())
                .read_back(&address)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreadable_directory_is_passed_over_and_counted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = approved(false).await;
        rename(dir.path(), "src/a.rs", "z/b.rs").await;
        write(dir.path(), "locked/x.rs", "x\n").await;
        let locked = dir.path().join("locked");
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // Run as a user every directory opens to (root), there is nothing to test.
        let opens = tokio::fs::read_dir(&locked).await.is_ok();
        let gone = gone_at(dir.path()).await;
        let found = FileSpace::new(dir.path().to_path_buf())
            .suggest(&gone)
            .await;
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        let found = found.unwrap();
        assert_eq!(found[&p("src/a.rs")].candidates.len(), 1);
        if !opens {
            assert!(
                found[&p("src/a.rs")]
                    .notes
                    .contains(&MoveNote::UnreadableSkipped { count: 1 })
            );
        }
    }

    /// Whether the file system at `dir` does not tell names apart by case.
    async fn folds_case(dir: &std::path::Path) -> bool {
        write(dir, "case-probe", "").await;
        let folds = tokio::fs::metadata(dir.join("CASE-PROBE")).await.is_ok();
        tokio::fs::remove_file(dir.join("case-probe"))
            .await
            .unwrap();
        folds
    }

    #[tokio::test]
    async fn a_move_in_case_only_is_found_with_the_name_as_written_on_disk() {
        let dir = approved(git_installed().await).await;
        if !folds_case(dir.path()).await {
            // A file system that tells case apart has two files, and the test of
            // the content above covers it.
            return;
        }
        rename(dir.path(), "src/a.rs", "src/A.rs").await;
        let (candidates, _) = suggested(dir.path()).await;
        assert_eq!(candidates, [("src/A.rs".to_owned(), FoundBy::SameContent)]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_inside_the_root_is_followed_and_a_broken_one_cannot_be_told() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "AGENTS.md", "the rules\n").await;
        write(root, "docs/real/guide.md", "a guide\n").await;
        tokio::fs::symlink("AGENTS.md", root.join("CLAUDE.md"))
            .await
            .unwrap();
        tokio::fs::symlink("real", root.join("docs/here"))
            .await
            .unwrap();
        tokio::fs::symlink("nowhere.md", root.join("broken.md"))
            .await
            .unwrap();
        let source = FileSpace::new(root.to_path_buf());
        let found = source
            .resolve(&bytes_of(&["CLAUDE.md", "docs/here/guide.md", "broken.md"]))
            .await
            .unwrap();
        // Present under the name written, with the target's content: an approval
        // pins it as it pins any file there.
        assert_eq!(
            found[&p("CLAUDE.md")].state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(b"the rules\n"))
        );
        assert_eq!(
            found[&p("docs/here/guide.md")].state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(b"a guide\n"))
        );
        assert_eq!(
            source.current(&p("CLAUDE.md")).await.unwrap().as_deref(),
            Some(&b"the rules\n"[..])
        );
        // A broken link is not absent: it cannot be told.
        assert!(matches!(
            found[&p("broken.md")].state(&Comparison::Bytes),
            ArtifactState::Unknown(Untold::Said(_))
        ));
        assert_eq!(source.current(&p("broken.md")).await.unwrap(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_moved_file_reached_through_a_link_inside_the_root_is_a_candidate() {
        let dir = approved(false).await;
        rename(dir.path(), "src/a.rs", "lib/real/b.rs").await;
        tokio::fs::symlink("real", dir.path().join("lib/here"))
            .await
            .unwrap();
        let gone = gone_at(dir.path()).await;
        let found = FileSpace::new(dir.path().to_path_buf())
            .suggest(&gone)
            .await
            .unwrap();
        // The walk lists the link and does not walk into it; the file is found once,
        // by its own path.
        let names: Vec<&str> = found[&p("src/a.rs")]
            .candidates
            .iter()
            .map(|c| c.name().as_str())
            .collect();
        assert_eq!(names, ["lib/real/b.rs"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn no_file_is_read_through_a_symbolic_link_on_the_way() {
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "a.rs", CODE).await;
        let dir = approved(false).await;
        // The directory of the pinned file replaced by a link to outside the root,
        // and a moved copy reached through another.
        tokio::fs::remove_dir_all(dir.path().join("src"))
            .await
            .unwrap();
        tokio::fs::symlink(outside.path(), dir.path().join("src"))
            .await
            .unwrap();
        tokio::fs::symlink(outside.path(), dir.path().join("lib"))
            .await
            .unwrap();
        let source = FileSpace::new(dir.path().to_path_buf());
        // Resolved, the pinned name cannot be told: nothing is read through the link.
        let found = source.resolve(&bytes_of(&["src/a.rs"])).await.unwrap();
        assert!(matches!(
            found[&p("src/a.rs")].state(&Comparison::Bytes),
            ArtifactState::Unknown(Untold::Said(why)) if why.as_str().contains("symbolic link")
        ));
        assert_eq!(source.current(&p("src/a.rs")).await.unwrap(), None);
        // Gone by a plain removal, the copy beyond the link is no candidate.
        tokio::fs::remove_file(dir.path().join("src"))
            .await
            .unwrap();
        let gone = gone_at(dir.path()).await;
        assert_eq!(gone.len(), 1);
        let found = source.suggest(&gone).await.unwrap();
        assert!(found[&p("src/a.rs")].candidates.is_empty(), "{found:?}");
    }

    // Listing the names of a selection.

    fn selecting(include: &[&str], exclude: &[&str]) -> Selection {
        let texts = |texts: &[&str]| texts.iter().map(|t| (*t).to_owned()).collect();
        Selection::new(texts(include), texts(exclude)).unwrap()
    }

    fn names(listing: NameListing) -> BTreeSet<String> {
        match listing {
            NameListing::All(found) | NameListing::Some { found, .. } => {
                found.iter().map(|n| n.as_str().to_owned()).collect()
            }
            NameListing::CannotList(why) => panic!("no listing: {why}"),
        }
    }

    #[test]
    fn a_selection_is_read_by_the_grammar_of_a_segment_glob() {
        let space = FileSpace::new(PathBuf::from("."));
        assert_eq!(
            space.select(&selecting(&["src/**/*.rs", "?.md"], &["src/gen/**"])),
            Ok(())
        );
        for text in [
            "", "/a.rs", "../a.rs", "a//b", "!a", "a\\b", "{a,b}", "sub**",
        ] {
            // The empty text is no text of a selection at all.
            let Ok(selection) = Selection::new(vec![text.to_owned()], vec![]) else {
                continue;
            };
            let Err(not) = space.select(&selection) else {
                panic!("`{text}` is read as a pattern");
            };
            assert!(not.why().as_str().contains(text), "{}", not.why());
        }
        // A text of `exclude` is read as one of `include`.
        assert!(
            space
                .select(&selecting(&["a.rs"], &["/b.rs"]))
                .unwrap_err()
                .why()
                .as_str()
                .contains("exclude")
        );
    }

    #[tokio::test]
    async fn the_listing_is_the_files_a_selection_takes_that_resolve_answers_as_there() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for at in [
            "a.rs",
            "src/b.rs",
            "src/gen/c.rs",
            "gone.rs",
            "notes.md",
            "src/big.rs",
        ] {
            write(root, at, "fn x() {}\n").await;
        }
        write(root, "src/big.rs", &"x".repeat(2000)).await;
        run(root, &["init", "-q"]).await;
        run(root, &["add", "-A"]).await;
        // A gitlink (a submodule), registered with no repository behind it.
        run(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},sub.rs", "a".repeat(40)),
            ],
        )
        .await;
        // Tracked and deleted, and untracked.
        tokio::fs::remove_file(root.join("gone.rs")).await.unwrap();
        write(root, "src/new.rs", "fn n() {}\n").await;
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            write(outside.path(), "x.rs", "fn o() {}\n").await;
            tokio::fs::symlink(outside.path().join("x.rs"), root.join("out.rs"))
                .await
                .unwrap();
            tokio::fs::symlink("a.rs", root.join("alias.rs"))
                .await
                .unwrap();
        }
        let space = FileSpace {
            max_file: 1000,
            ..FileSpace::new(root.to_path_buf())
        };
        // A file past the bound of bytes, and a link out of the root, are names a
        // document can write that a reading cannot tell: the listing that takes them
        // is never all of them.
        let taking = space
            .list(&selecting(&["**/*.rs"], &["src/gen/**"]))
            .await
            .unwrap();
        let NameListing::Some { found, why } = &taking else {
            panic!("a listing that takes a file a reading cannot tell is not all: {taking:?}");
        };
        assert!(why.as_str().contains("cannot tell"), "{why}");
        let leaving_out = ["src/gen/**", "src/big.rs", "out.rs"];
        let listing = space
            .list(&selecting(&["**/*.rs"], &leaving_out))
            .await
            .unwrap();
        assert!(matches!(listing, NameListing::All(_)), "{listing:?}");
        let listed = names(listing);
        assert_eq!(
            found
                .iter()
                .map(|n| n.as_str().to_owned())
                .collect::<BTreeSet<_>>(),
            listed
        );
        // Not the ones a pattern leaves out, one deleted, a gitlink, or a file that
        // is no `.rs`.
        let mut expected = BTreeSet::from(["a.rs".to_owned(), "src/b.rs".to_owned()]);
        expected.insert("src/new.rs".to_owned());
        #[cfg(unix)]
        expected.insert("alias.rs".to_owned());
        assert_eq!(listed, expected);
        // Each is there, as `resolve` reads it.
        let asks: SpaceAsks = listed
            .iter()
            .map(|n| (p(n), BTreeSet::from([Comparison::Bytes])))
            .collect();
        for (name, reading) in space.resolve(&asks).await.unwrap() {
            assert!(reading.is_present(), "{name}");
        }
        // The same files, listed again.
        assert_eq!(
            names(
                space
                    .list(&selecting(&["**/*.rs"], &leaving_out))
                    .await
                    .unwrap()
            ),
            listed
        );
    }

    #[tokio::test]
    async fn a_file_renamed_by_case_only_keeps_the_listing_from_being_all() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.rs", "fn a() {}\n").await;
        write(root, "C.rs", "fn c() {}\n").await;
        // Only a file system that ignores case finds `C.rs` as `c.rs`; elsewhere
        // there is nothing to check.
        if tokio::fs::metadata(root.join("c.rs")).await.is_err() {
            return;
        }
        run(root, &["init", "-q"]).await;
        run(root, &["add", "-A"]).await;
        run(root, &["commit", "-q", "-m", "x"]).await;
        tokio::fs::rename(root.join("C.rs"), root.join("c.rs"))
            .await
            .unwrap();
        let listing = FileSpace::new(root.to_path_buf())
            .list(&selecting(&["*.rs"], &[]))
            .await
            .unwrap();
        assert!(
            matches!(listing, NameListing::Some { .. }),
            "a file there under another case than Git lists is not passed over: {listing:?}"
        );
    }

    #[tokio::test]
    async fn a_listing_that_cannot_be_made_or_is_cut_is_never_all_of_them() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for at in ["a.rs", "b.rs", "c.rs"] {
            write(root, at, "fn x() {}\n").await;
        }
        let selection = selecting(&["*.rs"], &[]);
        // Outside a repository there is nothing to list from: no listing, no error.
        let space = FileSpace::new(root.to_path_buf());
        assert!(
            matches!(space.list(&selection).await, Ok(NameListing::CannotList(_))),
            "{:?}",
            space.list(&selection).await
        );
        // A text that is no pattern: no listing, with the reason of `select`.
        let bad = selecting(&["/a.rs"], &[]);
        assert_eq!(
            space.list(&bad).await.unwrap(),
            NameListing::CannotList(space.select(&bad).unwrap_err().why().clone())
        );

        run(root, &["init", "-q"]).await;
        run(root, &["add", "-A"]).await;
        assert_eq!(names(space.list(&selection).await.unwrap()).len(), 3);
        // Three names where two are held: two of them, and a reason.
        let two = space.clone().with_limits(ScanLimits {
            names: 2,
            ..ScanLimits::default()
        });
        let listing = two.list(&selection).await.unwrap();
        assert!(
            matches!(&listing, NameListing::Some { found, .. } if found.len() == 2),
            "{listing:?}"
        );
        // Exactly as many as held is all of them.
        let three = space.clone().with_limits(ScanLimits {
            names: 3,
            ..ScanLimits::default()
        });
        assert!(matches!(
            three.list(&selection).await.unwrap(),
            NameListing::All(found) if found.len() == 3
        ));
        // A listing of Git cut at its bound.
        let cut = space.with_limits(ScanLimits {
            listing_bytes: 8,
            ..ScanLimits::default()
        });
        assert!(matches!(
            cut.list(&selection).await.unwrap(),
            NameListing::Some { .. }
        ));
    }
}
