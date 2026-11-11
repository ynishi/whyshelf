//! The adapter of the space `file`: the files of a project, by their path under the
//! root, and where their content can be read back.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`FileSpace::resolve`] | reads each named file's bytes from the working tree and hashes them; names the blob its content is (or would be) in Git | `tokio::fs`, [`Git::blob_names`] |
//! | [`FileSpace::current`] | the bytes of a named file now | `tokio::fs` |
//! | [`FileSpace::read_back`] | the content of a blob, by its name | [`Git::read_blob`] |
//! | [`FileSpace::suggest`] | for each gone file, the files it may have moved to | [`Git::work_tree_files`], `tokio::fs`, [`Git::renames_since`] |
//! | [`presets`] | the preset `file`: the space, `compare = "bytes"`, `means = "depends"` | values |
//!
//! ```text
//!   name ──a path under the root?── no ──► not a name (why)
//!     │ yes
//!   path ──tokio::fs::read──► bytes, kept when no part of the path names an entry in another case only
//!                               ──ArtifactReading::of_bytes──► a hash per comparison asked (always)
//!                               └──git hash-object──► blob name ──► ContentAddress (in a repository)
//!        └─ no file there, or one named in another case only ──► absent
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
//! nothing there. The adapter always answers, and says "cannot tell" only of a name
//! reached through a symbolic link (below).
//!
//! # The case of a path
//!
//! A path names a file only as written: one that differs from the name of an entry
//! of its directory in ASCII case only names no file, on every system, as for Git (a
//! file system that does not tell case apart would open it). A name that differs in
//! a non-ASCII letter's case, or that the system stores in another form (macOS keeps
//! some non-ASCII names decomposed), is left to the system.
//!
//! # Suggesting moves
//!
//! For a file an approval pinned that is gone (`moves`), the adapter looks two
//! ways, and the domain orders what it finds:
//!
//! 1. **The same content.** Each file there now whose hash, under the comparison of
//!    a pin of the gone file, is the hash pinned (`same_content`). A pin that
//!    compares presence only has no content to look for. The files read are those
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
//!    its content only, when it is unchanged.
//!
//! What could not be looked at is a typed note of the name ([`MoveNote`]), never a
//! failure: a file or directory that cannot be read, or a file too large, passed
//! over and counted; a bound reached; no repository; no pin of the name that kept a
//! revision; a revision that is no commit of the repository (another clone, a
//! history rewritten); a history that cannot be read (a failing or slow Git, an
//! object a partial clone does not hold). How Git is run, and why nothing of the
//! repository chooses a program, is on the page of [`git`](super::super::git).
//!
//! # Symbolic links
//!
//! A link on the path of a name is followed by its canonical path while that stays
//! inside the canonical root: the name is the one written, and its content is the
//! target's (`CLAUDE.md -> AGENTS.md` is present, and an approval pins it). A link
//! that leads outside the root, or that is broken, is not followed: the name cannot
//! be told ([`FileSpace::resolve`]), has no bytes now ([`FileSpace::current`]), and
//! is never read or suggested by the search for moves. The rule is held in one place
//! for all three (`place_of`).
//!
//! # Outside Git
//!
//! The hash of a file never needs Git, so freshness works the same in any directory.
//! Outside a repository there is no address (see [`git`](super::super::git)).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use tokio::io::AsyncReadExt;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactHash, ArtifactName, ArtifactReading, Comparison, ContentAddress, Revision, Space,
    SpaceAsks, SpaceReadings,
};
use whyshelf_core::domain::shelf::ledger::{
    FoundBy, GoneName, MoveCandidate, MoveNote, NameSuggestions, PATH_ONLY_SPACE, PinnedThere,
    Similarity,
};
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::super::git::{Git, GitError, RenamesSince, first_line};

/// The name of the space this adapter resolves names in: the space a record written
/// before spaces existed is read as naming its files in
/// ([`PATH_ONLY_SPACE`]), so an old pin always names a space this adapter answers
/// for.
pub const FILE_SPACE: &str = PATH_ONLY_SPACE;

/// The name of the preset this adapter offers.
pub const FILE_PRESET: &str = "file";

/// The presets this adapter offers when it resolves the names of `space`: one,
/// [`FILE_PRESET`], for a link to files that stands on each byte of them
/// (`space` that space, `compare = "bytes"`, `means = "depends"`).
pub fn presets(space: &Space) -> BuiltinPresets {
    let values = PresetValues {
        space: Some(space.as_str().to_owned()),
        compare: Some("bytes".to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    match PresetName::new(FILE_PRESET) {
        Ok(name) => BuiltinPresets::from([(name, values)]),
        // Not a name: no preset. The test of this module holds it to be one.
        Err(_) => BuiltinPresets::new(),
    }
}

/// Why the files could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    /// A file is there and could not be read.
    #[error("cannot read {}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// Git gave no usable name for a file that is there.
    #[error(transparent)]
    Git(#[from] GitError),
    /// The domain refused the reason this adapter gave (a bug if it ever happens:
    /// the reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// The files of a project, and its Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct FileSpace {
    root: PathBuf,
    git: Git,
    limits: ScanLimits,
}

/// The bounds of one search for the same content ([`FileSpace::suggest`]): a search
/// that reaches one stops, or passes a file over, and a note says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// The most files read.
    pub files: usize,
    /// The largest file read, in bytes; a larger one is passed over.
    pub file_bytes: u64,
    /// The most bytes read in all.
    pub total_bytes: u64,
    /// The most bytes of Git's listing of the work tree held.
    pub listing_bytes: usize,
}

impl Default for ScanLimits {
    /// 20,000 files, 16 MiB a file, 512 MiB in all, and a listing of 16 MiB.
    ///
    /// Why these: what an approval pins is evidence a document stands on (source,
    /// tests, a small data file), and such files are far below 16 MiB, while a
    /// larger one (a build output, a data set, a media file) is rarely pinned and
    /// would cost the most to read; 512 MiB and 20,000 files bound the work of one
    /// answer in a work tree with many files Git does not ignore; a listing of
    /// 16 MiB holds that many paths many times over.
    fn default() -> Self {
        Self {
            files: 20_000,
            file_bytes: 16 * 1024 * 1024,
            total_bytes: 512 * 1024 * 1024,
            listing_bytes: 16 * 1024 * 1024,
        }
    }
}

/// What one search for the same content passed over or stopped at.
#[derive(Debug, Default)]
struct Tally {
    read: usize,
    bytes: u64,
    unreadable: usize,
    too_large: usize,
    truncated: bool,
}

impl Tally {
    /// Its notes, each when it has something to say.
    fn notes(&self, limits: &ScanLimits) -> Vec<MoveNote> {
        let mut notes = Vec::new();
        if self.truncated {
            notes.push(MoveNote::ScanTruncated { read: self.read });
        }
        if self.unreadable > 0 {
            notes.push(MoveNote::UnreadableSkipped {
                count: self.unreadable,
            });
        }
        if self.too_large > 0 {
            notes.push(MoveNote::TooLargeSkipped {
                count: self.too_large,
                limit: limits.file_bytes,
            });
        }
        notes
    }
}

/// The path a name of the space is: the one reader of the grammar of the names.
fn path_of(name: &ArtifactName) -> Result<RelPath, ValueError> {
    name.as_str().parse::<RelPath>()
}

impl FileSpace {
    /// The files under `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            git: Git::new(root.clone()),
            root,
            limits: ScanLimits::default(),
        }
    }

    /// The same, searching for the same content within `limits`.
    pub fn with_limits(mut self, limits: ScanLimits) -> Self {
        self.limits = limits;
        self
    }

    /// For each name: not a name (no path under the root), the hash of the file
    /// under each comparison asked and where its content can be read back, or no
    /// file. Every name asked about has an answer.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, ArtifactError> {
        let mut readings = SpaceReadings::new();
        let mut present = Vec::new();
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
            match bytes(&self.root, &path).await? {
                Read::Bytes(bytes) => {
                    readings.insert(
                        name.clone(),
                        ArtifactReading::of_bytes(&bytes, comparisons, None),
                    );
                    present.push((name, path));
                }
                Read::NoFile => {
                    readings.insert(name.clone(), ArtifactReading::absent());
                }
                Read::Outside => {
                    let why = NonBlank::new(
                        "a symbolic link on the path leads outside the root of the project, or is broken: nothing is read through it",
                    )
                    .map_err(ArtifactError::Reason)?;
                    readings.insert(name.clone(), ArtifactReading::cannot_tell(why));
                }
            }
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

    /// The bytes of the file a name is, as they are now, or none when it is no path
    /// or no file is there.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, ArtifactError> {
        match path_of(name) {
            Ok(path) => Ok(match bytes(&self.root, &path).await? {
                Read::Bytes(bytes) => Some(bytes),
                Read::NoFile | Read::Outside => None,
            }),
            Err(_) => Ok(None),
        }
    }

    /// The content a file had at `address`, or none when it cannot be read back.
    pub async fn read_back(
        &self,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, ArtifactError> {
        Ok(self.git.read_blob(address).await?)
    }

    /// For each gone name, the files it may have moved to (the rules on the module
    /// page, "Suggesting moves"): the files there now with the content pinned, then
    /// the renames Git detects since each approval's revision; with a note of what
    /// could not be looked at. Every name asked about has an entry.
    pub async fn suggest(
        &self,
        gone: &[GoneName],
    ) -> Result<BTreeMap<ArtifactName, NameSuggestions>, ArtifactError> {
        let mut found: BTreeMap<ArtifactName, Vec<MoveCandidate>> = gone
            .iter()
            .map(|g| (g.name().clone(), Vec::new()))
            .collect();
        let mut notes: BTreeMap<ArtifactName, Vec<MoveNote>> = BTreeMap::new();

        // 1. The same content, in the hash the pin used.
        let mut wanted: BTreeMap<&Comparison, BTreeMap<&ArtifactHash, BTreeSet<&ArtifactName>>> =
            BTreeMap::new();
        for name in gone {
            for (comparison, hash) in name.pins().iter().filter_map(PinnedThere::content) {
                wanted
                    .entry(comparison)
                    .or_default()
                    .entry(hash)
                    .or_default()
                    .insert(name.name());
            }
        }
        if !wanted.is_empty() {
            let mut tally = Tally::default();
            let files = match self.git.work_tree_files(self.limits.listing_bytes).await? {
                Some(listing) => {
                    tally.truncated |= listing.cut;
                    listing.files
                }
                None => walk(&self.root, &self.limits, &mut tally).await,
            };
            for path in &files {
                let Some(bytes) = scan_read(&self.root, path, &self.limits, &mut tally).await
                else {
                    if tally.truncated {
                        break;
                    }
                    continue;
                };
                let mut matched: BTreeSet<&ArtifactName> = BTreeSet::new();
                for (comparison, by_hash) in &wanted {
                    if let Some(names) = by_hash.get(&comparison.fingerprint(&bytes)) {
                        matched.extend(names.iter().copied());
                    }
                }
                if matched.is_empty() {
                    continue;
                }
                // The candidate is the name as written on disk, which a file system
                // that does not tell case apart may spell otherwise than the listing.
                let Some(at) = written_on_disk(&self.root, path)
                    .await
                    .and_then(|at| ArtifactName::new(at.as_str()).ok())
                else {
                    continue;
                };
                for name in matched {
                    if let Some(candidates) = found.get_mut(name) {
                        candidates.push(MoveCandidate::new(at.clone(), FoundBy::SameContent, true));
                    }
                }
            }
            let scanned = tally.notes(&self.limits);
            for name in gone {
                if name.pins().iter().any(|pin| pin.content().is_some()) {
                    notes
                        .entry(name.name().clone())
                        .or_default()
                        .extend(scanned.iter().cloned());
                }
            }
        }

        // 2. Git's rename detection, since the revision of each approval.
        let mut by_revision: BTreeMap<&Revision, BTreeMap<&ArtifactName, BTreeSet<String>>> =
            BTreeMap::new();
        for name in gone {
            let mut kept_one = false;
            for pin in name.pins().iter() {
                if let Some(revision) = pin.revision() {
                    kept_one = true;
                    // At the revision, the file had the name the pin wrote; a move
                    // declared since may have named it otherwise.
                    let olds = by_revision
                        .entry(revision)
                        .or_default()
                        .entry(name.name())
                        .or_default();
                    olds.insert(name.name().as_str().to_owned());
                    olds.insert(pin.written().name().as_str().to_owned());
                }
            }
            // Only when no pin of the name kept one: a pin that did is looked from.
            if !kept_one {
                notes
                    .entry(name.name().clone())
                    .or_default()
                    .push(MoveNote::NoRevision);
            }
        }
        let mut unknown: BTreeMap<&ArtifactName, BTreeSet<Revision>> = BTreeMap::new();
        for (revision, names) in by_revision {
            let found_since = match self.git.renames_since(revision).await {
                Ok(found_since) => found_since,
                // A history that cannot be read stops this way of looking, not the
                // answer.
                Err(error) => {
                    // One line: what Git said is cut to its first line.
                    let detail = NonBlank::new(first_line(&error.to_string()))
                        .map_err(ArtifactError::Reason)?;
                    for name in names.keys() {
                        notes.entry((*name).clone()).or_default().push(
                            MoveNote::HistoryUnreadable {
                                detail: detail.clone(),
                            },
                        );
                    }
                    continue;
                }
            };
            match found_since {
                RenamesSince::Found(renames) => {
                    for rename in renames {
                        let present = match place_of(&self.root, &rename.to).await {
                            Ok(Place::File { .. }) => {
                                named_as_written(&self.root, &rename.to).await
                            }
                            // A place that cannot be looked at is not said to have a
                            // file: `present` is what the adapter can say is there.
                            Ok(Place::NoFile | Place::Outside) | Err(_) => false,
                        };
                        let Ok(to) = ArtifactName::new(rename.to.as_str()) else {
                            continue;
                        };
                        let similarity = rename.similarity.and_then(Similarity::new);
                        for (name, olds) in &names {
                            if !olds.contains(rename.from.as_str()) {
                                continue;
                            }
                            if let Some(candidates) = found.get_mut(*name) {
                                candidates.push(MoveCandidate::new(
                                    to.clone(),
                                    FoundBy::Renamed { similarity },
                                    present,
                                ));
                            }
                        }
                    }
                }
                RenamesSince::NotARepository => {
                    for name in names.keys() {
                        notes
                            .entry((*name).clone())
                            .or_default()
                            .push(MoveNote::NoRepository);
                    }
                }
                RenamesSince::UnknownRevision => {
                    for name in names.keys() {
                        unknown.entry(name).or_default().insert(revision.clone());
                    }
                }
            }
        }
        for (name, revisions) in unknown {
            notes
                .entry(name.clone())
                .or_default()
                .push(MoveNote::UnknownRevision { revisions });
        }

        Ok(found
            .into_iter()
            .map(|(name, candidates)| {
                let mut notes = notes.remove(&name).unwrap_or_default();
                notes.sort_by_key(MoveNote::kind);
                notes.dedup_by_key(|n| n.kind());
                (name, NameSuggestions { candidates, notes })
            })
            .collect())
    }
}

/// The files under `root`, outside a repository: every regular file and symbolic
/// link, by its path, in no directory or file whose name starts with `.`, never
/// walking into a link (`place_of` follows one only inside the root when it is
/// read), up to `limits.files` of them (more marks the tally truncated). A
/// directory or an entry that cannot be read is passed over and counted.
async fn walk(root: &std::path::Path, limits: &ScanLimits, tally: &mut Tally) -> Vec<RelPath> {
    let mut files = Vec::new();
    let mut dirs: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = dirs.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) if is_absent(&e) => continue,
            Err(_) => {
                tally.unreadable += 1;
                continue;
            }
        };
        let mut found = Vec::new();
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => found.push(entry),
                Ok(None) => break,
                Err(_) => {
                    tally.unreadable += 1;
                    break;
                }
            }
        }
        found.sort_by_key(|entry| entry.file_name());
        for entry in found {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type().await else {
                tally.unreadable += 1;
                continue;
            };
            let at = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            if kind.is_dir() {
                dirs.push((entry.path(), at));
            // A link is listed and never walked into (no loop): reading it, `place_of`
            // follows it only inside the root.
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            } else if (kind.is_file() || kind.is_symlink())
                && let Ok(path) = at.parse::<RelPath>()
            {
                if files.len() == limits.files {
                    tally.truncated = true;
                    return files;
                }
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Where a path under the root leads: a regular file (with its length), no file
/// (nothing, a directory, a pipe or a device, a file on the way), or a place
/// outside the root (through a symbolic link) or behind a broken link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    File { len: u64 },
    NoFile,
    Outside,
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
async fn place_of(root: &std::path::Path, path: &RelPath) -> io::Result<Place> {
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

/// What reading a named file gave.
enum Read {
    /// Its bytes.
    Bytes(Vec<u8>),
    /// No file is there as written.
    NoFile,
    /// A symbolic link on the path leads outside the root, or is broken: nothing is
    /// read.
    Outside,
}

/// The bytes of a file under `root`: none when no file is there (nothing, a
/// directory, or a path that names an entry in another case only), and nothing read
/// when a link on the path leaves the root or is broken ([`place_of`]); through a
/// link inside the root, the target's bytes.
async fn bytes(root: &std::path::Path, path: &RelPath) -> Result<Read, ArtifactError> {
    let full = root.join(path.as_str());
    match place_of(root, path).await {
        Ok(Place::File { .. }) => {}
        Ok(Place::NoFile) => return Ok(Read::NoFile),
        Ok(Place::Outside) => return Ok(Read::Outside),
        Err(source) => return Err(ArtifactError::Io { path: full, source }),
    }
    let bytes = match tokio::fs::read(&full).await {
        Ok(bytes) => bytes,
        Err(e) if is_absent(&e) => return Ok(Read::NoFile),
        Err(source) => return Err(ArtifactError::Io { path: full, source }),
    };
    if named_as_written(root, path).await {
        Ok(Read::Bytes(bytes))
    } else {
        Ok(Read::NoFile)
    }
}

/// What reading a named file within a bound of bytes gave, for another space whose
/// names start with a path of this one (`rust-test`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Bounded {
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
/// of a path as written), when it has at most `max` bytes: an error only when the
/// file is there and cannot be read.
pub(super) async fn read_within(
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

/// The bytes of one file for the search for the same content, within `limits`:
/// none for no regular file, a link on the way, a file larger than one reading may
/// be, one that cannot be read (each of the last two counted), and once a bound of
/// the search is reached (the tally marked truncated).
async fn scan_read(
    root: &std::path::Path,
    path: &RelPath,
    limits: &ScanLimits,
    tally: &mut Tally,
) -> Option<Vec<u8>> {
    if tally.truncated {
        return None;
    }
    if tally.read == limits.files {
        tally.truncated = true;
        return None;
    }
    let len = match place_of(root, path).await {
        Ok(Place::File { len }) => len,
        Ok(Place::NoFile | Place::Outside) => return None,
        Err(_) => {
            tally.unreadable += 1;
            return None;
        }
    };
    if len > limits.file_bytes {
        tally.too_large += 1;
        return None;
    }
    if tally.bytes.saturating_add(len) > limits.total_bytes {
        tally.truncated = true;
        return None;
    }
    match tokio::fs::read(root.join(path.as_str())).await {
        Ok(bytes) => {
            tally.read += 1;
            tally.bytes = tally
                .bytes
                .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            Some(bytes)
        }
        Err(e) if is_absent(&e) => None,
        Err(_) => {
            tally.unreadable += 1;
            None
        }
    }
}

/// The path as its entries are written on disk, part by part: the entry named as
/// written, else the one entry that differs from it in ASCII case only (what a file
/// system that does not tell case apart opened), else the part as written. Kept only
/// when [`named_as_written`] holds it to name the file, so a candidate is always a
/// name `judge moved` takes; none otherwise.
async fn written_on_disk(root: &std::path::Path, path: &RelPath) -> Option<RelPath> {
    let mut dir = root.to_path_buf();
    let mut parts = Vec::new();
    for part in path.as_str().split('/') {
        let mut spelled = part.to_owned();
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            let mut others = Vec::new();
            let mut written = false;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                if name == part {
                    written = true;
                    break;
                }
                if let Some(name) = name.to_str()
                    && name.eq_ignore_ascii_case(part)
                {
                    others.push(name.to_owned());
                }
            }
            if !written && let [only] = others.as_slice() {
                spelled.clone_from(only);
            }
        }
        dir.push(&spelled);
        parts.push(spelled);
    }
    let on_disk: RelPath = parts.join("/").parse().ok()?;
    named_as_written(root, &on_disk).await.then_some(on_disk)
}

/// Whether a path that opened names the file as written (the rule of the case of a
/// path, on the module page): no part of it is the name of an entry of its directory
/// in another ASCII case only. A directory that cannot be listed says nothing against
/// the path, which opened.
async fn named_as_written(root: &std::path::Path, path: &RelPath) -> bool {
    let mut dir = root.to_path_buf();
    for part in path.as_str().split('/') {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            return true;
        };
        let mut names = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            names.push(entry.file_name());
        }
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
fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::IsADirectory
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::process::Stdio;

    use tokio::process::Command;
    use whyshelf_core::app::shelf::RevisionSource;
    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Comparison, LinePrefix, Untold};
    use whyshelf_core::domain::shelf::ledger::NoteKind;

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

    #[test]
    fn the_space_of_the_files_is_the_one_an_old_record_names() {
        assert_eq!(FILE_SPACE, PATH_ONLY_SPACE);
    }

    #[test]
    fn the_adapter_offers_the_preset_file_for_its_space() {
        let space = Space::new(FILE_SPACE).unwrap();
        let offered = presets(&space);
        let name = PresetName::new(FILE_PRESET).unwrap();
        assert_eq!(
            offered,
            BuiltinPresets::from([(
                name,
                PresetValues {
                    space: Some("file".to_owned()),
                    compare: Some("bytes".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                }
            )])
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

    #[test]
    fn a_part_is_held_against_the_names_of_its_directory() {
        use std::ffi::OsStr;
        let names = |all: &'static [&'static str]| all.iter().map(OsStr::new);
        assert_eq!(
            part_names_entry("Justfile", names(&["a", "Justfile"])),
            PartName::Written
        );
        assert_eq!(
            part_names_entry("justfile", names(&["a", "Justfile"])),
            PartName::OtherCase
        );
        // Both spellings there: the one written is a file of its own.
        assert_eq!(
            part_names_entry("justfile", names(&["Justfile", "justfile"])),
            PartName::Written
        );
        // A name stored decomposed (e + U+0301) against the composed one written.
        assert_eq!(
            part_names_entry("caf\u{e9}", names(&["cafe\u{301}"])),
            PartName::Unknown
        );
        assert_eq!(part_names_entry("x", names(&[])), PartName::Unknown);
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
        let mut app = ShelfApp::new(super::super::super::Project::new(
            root.to_path_buf(),
            Some("t".to_owned()),
            None,
        ));
        app.judge_all(vec![JudgeRequest {
            judgement: Judgement::Approve {
                until: None,
                reason: None,
                again: false,
            },
            reference: "N-1".parse().unwrap(),
            actor: None,
        }])
        .await
        .unwrap();
        dir
    }

    /// What `moves` answers for `src/a.rs`: each candidate's name and how it was
    /// found, and the kinds of its notes.
    async fn suggested(root: &std::path::Path) -> (Vec<(String, FoundBy)>, Vec<NoteKind>) {
        use whyshelf_core::app::shelf::ShelfApp;
        let app = ShelfApp::new(super::super::super::Project::new(
            root.to_path_buf(),
            Some("t".to_owned()),
            None,
        ));
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
        use whyshelf_core::app::shelf::LedgerRead;
        use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
        use whyshelf_core::domain::shelf::ledger::{Ledger, PinnedNames};
        let ledger = super::super::super::ledger::JsonLedger::new(
            root.join(".whyshelf/ledger"),
            ".whyshelf",
        );
        let pinned = PinnedNames::of(&Ledger::build(ledger.read().await.unwrap().records));
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
        use whyshelf_core::domain::shelf::snapshot::{ItemRole, Retrieval};
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
        let app = ShelfApp::new(super::super::super::Project::new(
            dir.path().to_path_buf(),
            Some("t".to_owned()),
            None,
        ));
        let approved = app.show_approved(&"N-1".parse().unwrap()).await.unwrap();
        let item = approved
            .items
            .iter()
            .find(|i| matches!(i.item.role(), ItemRole::Artifact { .. }))
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
}
