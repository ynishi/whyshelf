//! The tree of a project in a directory of the file system: [`FileStates`].
//!
//! A side is a directory. A run that reads one side ([`FileStates::new`], every
//! command but one that compares two ends) reads the project's root as
//! [`Side::After`], and has no side before. A run that reads two
//! ([`FileStates::between`]) reads one directory for each.
//!
//! | ask | how it is read |
//! |---|---|
//! | `read` | each part of the name looked at without following a link (`symlink_metadata`): the first that is a link is [`AtSide::Elsewhere`], with where it leads; at the end a file is its bytes (within the bound and one byte more), a directory is [`AtSide::Dir`], another kind (a pipe, a socket, a device) cannot be told, and is not opened |
//! | where a link leads | its canonical path: under the canonical directory of the side, the name there ([`Leads::Inside`], or [`Leads::Top`] for the directory itself); elsewhere, or a place inside that is no name of the tree, [`Leads::Outside`]; a link to nothing [`Leads::Broken`]; one the system cannot resolve otherwise (a loop, a file on the way) [`Leads::Unresolved`] |
//! | `look` | what `read` answers, with a file told by the length its metadata gives: nothing is opened |
//! | `list` | the entries of the directory as written, each with its own kind (a link entry is a link, whatever it leads to) |
//! | `read_outside`, `list_outside` | the name as the system resolves it, every link followed; one side only: a run of two answers that it cannot tell ([`LEAVES`]) |
//! | `read_beyond`, `list_beyond` | a place whose name is no path of the project (not UTF-8, a part a path refuses), by the parts the listing wrote, as the system resolves it; one side only |
//!
//! Each name of `read`, `look` and `read_outside` has its own answer or its own error:
//! the system's error of one name (a file on its way, a place that may not be looked
//! at) is that name's, and every other name is answered.
//! | `addresses` | the blob Git would store for each file in the work tree (`git hash-object`); one side only, at [`Side::After`]: a run of two, and the side before, make none |
//! | `files` | the files Git lists of the work tree of the side's directory (tracked, and not tracked and not ignored); none outside a repository |
//!
//! A part of a name on the way that is no directory is the system's own error, as it
//! is for a reading of the path: a reader tells it as it tells that error. Nothing is
//! written, and no directory is made.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, Entries, EntryKind, Leads, NameAnswers, NotADir,
    RawPart, Side, StateList, StateRead, TreeFiles, Unnamed,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::super::git::{Git, first_line};
use super::StateError;

/// Why a run of two sides cannot read through a link that leaves the tree: the place
/// outside is not of either side, so the two sides cannot be read alike.
pub const LEAVES: TextLiteral =
    TextLiteral::new("a link leaves the project, so the two sides cannot be read alike");

/// The tree of a project in a directory: at one side (the root of the project, read
/// as [`Side::After`]) or at two (a directory for each). A clone is the same state,
/// shared: every reader of a run holds one.
#[derive(Debug, Clone)]
pub struct FileStates(Arc<Sides>);

/// The directories a [`FileStates`] reads.
#[derive(Debug)]
struct Sides {
    after: SideDir,
    before: Option<SideDir>,
}

/// The directory of one side, and Git asked of it.
#[derive(Debug, Clone)]
struct SideDir {
    dir: PathBuf,
    git: Git,
}

impl SideDir {
    fn at(dir: PathBuf) -> Self {
        Self {
            git: Git::new(dir.clone()),
            dir,
        }
    }
}

impl FileStates {
    /// The project at `root`, at one side: the side after is the root, and there is
    /// no side before.
    pub fn new(root: PathBuf) -> Self {
        Self(Arc::new(Sides {
            after: SideDir::at(root),
            before: None,
        }))
    }

    /// The project at two sides: `before` the directory of the side before a change,
    /// `after` that of the side after it.
    pub fn between(before: PathBuf, after: PathBuf) -> Self {
        Self(Arc::new(Sides {
            after: SideDir::at(after),
            before: Some(SideDir::at(before)),
        }))
    }

    /// The same state, with each call of Git bounded by `timeout` (for a test of the
    /// bound).
    pub fn with_timeout(self, timeout: Duration) -> Self {
        let at = |side: &SideDir| SideDir {
            dir: side.dir.clone(),
            git: Git::new(side.dir.clone()).with_timeout(timeout),
        };
        Self(Arc::new(Sides {
            after: at(&self.0.after),
            before: self.0.before.as_ref().map(at),
        }))
    }

    /// The directory of the side after: the root of the project.
    pub fn root(&self) -> &Path {
        &self.0.after.dir
    }

    /// Whether the run reads one side only.
    pub(crate) fn one_side(&self) -> bool {
        self.0.before.is_none()
    }

    fn side(&self, side: Side) -> Result<&SideDir, StateError> {
        match side {
            Side::After => Ok(&self.0.after),
            Side::Before => self.0.before.as_ref().ok_or(StateError::NoBefore),
        }
    }

    /// The directory of `side`: an error for a side the run does not read.
    pub fn dir(&self, side: Side) -> Result<&Path, StateError> {
        self.side(side).map(|side| side.dir.as_path())
    }

    /// Where `name` is at `side`, as a path of the file system: what a reader names in
    /// what it says.
    pub fn path_of(&self, side: Side, name: &RelPath) -> Result<PathBuf, StateError> {
        Ok(self.dir(side)?.join(name.as_str()))
    }

    /// What `name` is under `dir`.
    async fn read_one(
        &self,
        dir: &Path,
        name: &RelPath,
        bound: ByteBound,
    ) -> Result<AtSide<RelPath>, StateError> {
        Ok(match walk(dir, name).await? {
            Walked::Absent => AtSide::Absent,
            Walked::Link { at, target, leads } => AtSide::Elsewhere { at, target, leads },
            Walked::End { path, kind, .. } => match kind {
                EntryKind::File => match read_bounded(&path, bound).await? {
                    Some(bytes) => AtSide::There(bytes),
                    None => AtSide::Absent,
                },
                EntryKind::Dir => AtSide::Dir,
                EntryKind::Link | EntryKind::Other => AtSide::Untold(other_kind(name)),
            },
        })
    }

    /// The path of a place of the tree that is no name of it, at `side`.
    pub(crate) fn beyond_path(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
    ) -> Result<PathBuf, StateError> {
        let mut path = match &at.dir {
            Some(dir) => self.path_of(side, dir)?,
            None => self.dir(side)?.to_path_buf(),
        };
        for part in &at.parts {
            path.push(part.as_os_str());
        }
        Ok(path)
    }
}

/// What is at `path` as the system resolves it, every link followed: `what` is how a
/// reader names it in a reason.
pub(crate) async fn read_path(
    path: &Path,
    bound: ByteBound,
    what: &str,
) -> Result<AtSide<RelPath>, StateError> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(AtSide::Absent),
        Err(source) => {
            return Err(StateError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
    Ok(if meta.is_file() {
        match read_bounded(path, bound).await? {
            Some(bytes) => AtSide::There(bytes),
            None => AtSide::Absent,
        }
    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
    } else if meta.is_dir() {
        AtSide::Dir
    } else {
        AtSide::Untold(other_kind_of(what))
    })
}

/// Why a run of two sides reads no place that is no name of the tree.
const UNNAMED_TWO: TextLiteral =
    TextLiteral::new("a place that is no name of the project cannot be read alike at two sides");

/// What is said of a name of another kind, when what names it cannot be made (it
/// never cannot: a name is not blank).
const OTHER_KIND: TextLiteral =
    TextLiteral::new("a name is neither a file, a directory nor a link, and is not read");

/// What is said of a listing of the files that is not all of them, when what says
/// why cannot be made (it never cannot: each text is not blank).
const UNLISTED: TextLiteral = TextLiteral::new("the files of the project cannot be listed");

/// Why a name of another kind than a file, a directory or a link is not read.
fn other_kind(name: &RelPath) -> Untold {
    other_kind_of(name.as_str())
}

/// Why a place, named by a reader as `what`, of another kind than a file, a directory
/// or a link is not read.
pub(crate) fn other_kind_of(what: &str) -> Untold {
    Untold::Said(
        NonBlank::new(format!(
            "{what} is neither a file, a directory nor a link, and is not read"
        ))
        .unwrap_or_else(|_| NonBlank::from(OTHER_KIND)),
    )
}

/// What a run of two sides answers of a reading through a link that leaves the tree.
fn leaves() -> Untold {
    Untold::Said(NonBlank::from(LEAVES))
}

impl StateRead<RelPath> for FileStates {
    type Error = StateError;

    async fn read(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        let dir = self.dir(side)?;
        let mut read = BTreeMap::new();
        for name in names {
            read.insert(name.clone(), self.read_one(dir, name, bound).await);
        }
        Ok(read)
    }

    /// The parts of each name looked at as `read` looks at them, and a file told by
    /// the length its metadata gives: nothing is opened.
    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<NameAnswers<RelPath, u64, StateError>, StateError> {
        let dir = self.dir(side)?;
        let mut looked = BTreeMap::new();
        for name in names {
            let answer = walk(dir, name).await.map(|walked| match walked {
                Walked::Absent => AtSide::Absent,
                Walked::Link { at, target, leads } => AtSide::Elsewhere { at, target, leads },
                Walked::End { kind, len, .. } => match kind {
                    EntryKind::File => AtSide::There(len),
                    EntryKind::Dir => AtSide::Dir,
                    EntryKind::Link | EntryKind::Other => AtSide::Untold(other_kind(name)),
                },
            });
            looked.insert(name.clone(), answer);
        }
        Ok(looked)
    }

    async fn read_outside(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        let dir = self.dir(side)?;
        let mut read = BTreeMap::new();
        for name in names {
            let answer = if self.one_side() {
                read_path(&dir.join(name.as_str()), bound, name.as_str()).await
            } else {
                Ok(AtSide::Untold(leaves()))
            };
            read.insert(name.clone(), answer);
        }
        Ok(read)
    }

    /// The place as the system resolves its path, every link followed; in a run of
    /// two sides, it cannot be told.
    async fn read_beyond(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
        bound: ByteBound,
    ) -> Result<AtSide<RelPath>, StateError> {
        let path = self.beyond_path(side, at)?;
        if !self.one_side() {
            return Ok(AtSide::Untold(Untold::Said(NonBlank::from(UNNAMED_TWO))));
        }
        let what = path.display().to_string();
        read_path(&path, bound, &what).await
    }

    async fn addresses(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, StateError> {
        let at = self.side(side)?;
        if !self.one_side() || side != Side::After {
            return Ok(BTreeMap::new());
        }
        let paths: Vec<&RelPath> = names.iter().collect();
        let Some(addresses) = at.git.blob_names(&paths).await? else {
            return Ok(BTreeMap::new());
        };
        Ok(names.iter().cloned().zip(addresses).collect())
    }
}

impl StateList<RelPath> for FileStates {
    async fn list(
        &self,
        side: Side,
        dir: Option<&RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        let top = self.dir(side)?;
        let Some(name) = dir else {
            return Ok(DirListing::All(entries(top, None).await?));
        };
        Ok(match walk(top, name).await? {
            Walked::Absent => DirListing::NotADir(NotADir::Absent),
            Walked::Link { at, target, leads } => {
                DirListing::NotADir(NotADir::Elsewhere { at, target, leads })
            }
            Walked::End { path, kind, .. } => match kind {
                EntryKind::Dir => DirListing::All(entries(&path, Some(name)).await?),
                EntryKind::File => DirListing::NotADir(NotADir::File),
                EntryKind::Link | EntryKind::Other => {
                    DirListing::NotADir(NotADir::Untold(other_kind(name)))
                }
            },
        })
    }

    async fn list_outside(
        &self,
        side: Side,
        dir: &RelPath,
    ) -> Result<DirListing<RelPath>, StateError> {
        let top = self.dir(side)?;
        if !self.one_side() {
            return Ok(DirListing::NotADir(NotADir::Untold(leaves())));
        }
        let path = top.join(dir.as_str());
        list_path(&path, dir.as_str(), Under::Named(dir)).await
    }

    /// The entries under the place as the system resolves its path, every link
    /// followed, each unnamed; in a run of two sides, it cannot be told.
    async fn list_beyond(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        let path = self.beyond_path(side, at)?;
        if !self.one_side() {
            return Ok(DirListing::NotADir(NotADir::Untold(Untold::Said(
                NonBlank::from(UNNAMED_TWO),
            ))));
        }
        let what = path.display().to_string();
        list_path(&path, &what, Under::Beyond(at)).await
    }

    async fn files(&self, side: Side, bound: ByteBound) -> Result<TreeFiles<RelPath>, StateError> {
        let at = self.side(side)?;
        let cap = usize::try_from(bound.get()).unwrap_or(usize::MAX);
        let said = |why: String| NonBlank::new(why).unwrap_or_else(|_| NonBlank::from(UNLISTED));
        let listing = match at.git.work_tree_files(cap).await {
            Ok(Some(listing)) => listing,
            Ok(None) => {
                return Ok(TreeFiles::CannotList(said(
                    "the files of the project cannot be listed: it is not in a Git repository, or no Git can be run"
                        .to_owned(),
                )));
            }
            Err(error) => {
                return Ok(TreeFiles::CannotList(said(format!(
                    "the files of the project cannot be listed: {}",
                    first_line(&error.to_string())
                ))));
            }
        };
        let names: BTreeSet<RelPath> = listing.files.into_iter().collect();
        Ok(if listing.cut {
            TreeFiles::Cut {
                names,
                why: said(format!("Git's listing of the files was cut at {cap} bytes")),
            }
        } else if listing.unnamed > 0 {
            TreeFiles::Cut {
                names,
                why: said(format!(
                    "{} file(s) Git lists with a path that is not UTF-8 or no path of the project",
                    listing.unnamed
                )),
            }
        } else {
            TreeFiles::All(names)
        })
    }
}

/// What a walk along the parts of a name, links not followed, came to.
enum Walked {
    /// Nothing is at a part.
    Absent,
    /// A part is a link.
    Link {
        at: RelPath,
        target: String,
        leads: Leads<RelPath>,
    },
    /// The whole name, of this kind (never a link), and its length.
    End {
        path: PathBuf,
        kind: EntryKind,
        len: u64,
    },
}

/// The parts of `name` under `top`, looked at one by one without following a link.
async fn walk(top: &Path, name: &RelPath) -> Result<Walked, StateError> {
    let parts: Vec<&str> = name.as_str().split('/').collect();
    let mut path = top.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        path.push(part);
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Walked::Absent),
            Err(source) => return Err(StateError::Io { path, source }),
        };
        let of = meta.file_type();
        // arch-lint: allow(no-sync-io) reason="`of` is a FileType fetched with `.await`: pure accessors, no I/O"
        let kind = kind_of(of.is_symlink(), of.is_dir(), of.is_file());
        if kind == EntryKind::Link {
            let target = tokio::fs::read_link(&path)
                .await
                .map_err(|source| StateError::Io {
                    path: path.clone(),
                    source,
                })?
                .to_string_lossy()
                .into_owned();
            let leads = leads(top, &path).await?;
            let at = parts[..=i].join("/").parse().map_err(|_| StateError::Io {
                path: path.clone(),
                source: io::Error::other("a part of a name is no name"),
            })?;
            return Ok(Walked::Link { at, target, leads });
        }
        if i + 1 == parts.len() {
            return Ok(Walked::End {
                path,
                kind,
                len: meta.len(),
            });
        }
    }
    Ok(Walked::Absent)
}

/// The kind of an entry, its links not followed, from what its type says it is.
fn kind_of(link: bool, dir: bool, file: bool) -> EntryKind {
    if link {
        EntryKind::Link
    } else if dir {
        EntryKind::Dir
    } else if file {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Where the link at `link` leads, by its canonical path against the canonical `top`:
/// broken when the system finds nothing at what it names, and unresolved when it
/// cannot resolve it otherwise (a loop of links, a file on the way).
async fn leads(top: &Path, link: &Path) -> Result<Leads<RelPath>, StateError> {
    let real = match tokio::fs::canonicalize(link).await {
        Ok(real) => real,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Leads::Broken),
        Err(_) => return Ok(Leads::Unresolved),
    };
    let top_real = tokio::fs::canonicalize(top)
        .await
        .map_err(|source| StateError::Io {
            path: top.to_path_buf(),
            source,
        })?;
    let Ok(rest) = real.strip_prefix(&top_real) else {
        return Ok(Leads::Outside);
    };
    if rest.as_os_str().is_empty() {
        return Ok(Leads::Top);
    }
    Ok(name_of(rest).map_or(Leads::Outside, Leads::Inside))
}

/// The name of the tree a relative path of the file system is, when it is one.
pub(crate) fn name_of(path: &Path) -> Option<RelPath> {
    let parts = path
        .components()
        .map(|part| match part {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<&str>>>()?;
    parts.join("/").parse().ok()
}

/// The entries of the directory at `path`, the name `name` of the tree (none: its
/// top), each with its own kind.
async fn entries(path: &Path, name: Option<&RelPath>) -> Result<Entries<RelPath>, StateError> {
    let mut listed = Entries::default();
    for (file_name, kind) in read_entries(path).await? {
        let named = file_name.to_str().and_then(|part| {
            match name {
                Some(dir) => format!("{dir}/{part}"),
                None => part.to_owned(),
            }
            .parse::<RelPath>()
            .ok()
        });
        match named {
            Some(named) => {
                listed.named.insert(named, kind);
            }
            None => {
                let at = Beyond {
                    dir: name.cloned(),
                    parts: vec![RawPart::new(file_name.clone())],
                };
                listed.unnamed.insert(
                    file_name.to_string_lossy().into_owned(),
                    Unnamed { kind, at },
                );
            }
        }
    }
    Ok(listed)
}

/// The entries of the directory at `path`, the place `at` that is no name of the tree:
/// each unnamed, below it.
async fn entries_beyond(path: &Path, at: &Beyond<RelPath>) -> Result<Entries<RelPath>, StateError> {
    let mut listed = Entries::default();
    for (file_name, kind) in read_entries(path).await? {
        let mut parts = at.parts.clone();
        parts.push(RawPart::new(file_name.clone()));
        listed.unnamed.insert(
            file_name.to_string_lossy().into_owned(),
            Unnamed {
                kind,
                at: Beyond {
                    dir: at.dir.clone(),
                    parts,
                },
            },
        );
    }
    Ok(listed)
}

/// The entries of the directory at `path` as written, each with its own kind, its
/// links not followed.
pub(crate) async fn read_entries(path: &Path) -> Result<Vec<(OsString, EntryKind)>, StateError> {
    let io = |source| StateError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut found = Vec::new();
    let mut read = tokio::fs::read_dir(path).await.map_err(io)?;
    while let Some(entry) = read.next_entry().await.map_err(io)? {
        let of = entry.file_type().await.map_err(io)?;
        // arch-lint: allow(no-sync-io) reason="`of` is a FileType fetched with `.await`: pure accessors, no I/O"
        let kind = kind_of(of.is_symlink(), of.is_dir(), of.is_file());
        found.push((entry.file_name(), kind));
    }
    Ok(found)
}

/// What the entries of a directory listed are named under.
enum Under<'a> {
    /// The name of the directory.
    Named(&'a RelPath),
    /// A place that is no name of the tree.
    Beyond(&'a Beyond<RelPath>),
}

/// What is under the directory at `path` as the system resolves it, every link
/// followed, its entries named under `under`; `what` names it in a reason.
async fn list_path(
    path: &Path,
    what: &str,
    under: Under<'_>,
) -> Result<DirListing<RelPath>, StateError> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(DirListing::NotADir(NotADir::Absent));
        }
        Err(source) => {
            return Err(StateError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
    Ok(if meta.is_dir() {
        DirListing::All(match under {
            Under::Named(dir) => entries(path, Some(dir)).await?,
            Under::Beyond(at) => entries_beyond(path, at).await?,
        })
    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
    } else if meta.is_file() {
        DirListing::NotADir(NotADir::File)
    } else {
        DirListing::NotADir(NotADir::Untold(other_kind_of(what)))
    })
}

/// The bytes of the file at `path`, at most one byte past `bound`; none when it is no
/// longer there.
async fn read_bounded(path: &Path, bound: ByteBound) -> Result<Option<Vec<u8>>, StateError> {
    let io = |source| StateError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io(source)),
    };
    let mut bytes = Vec::new();
    file.take(bound.get().saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(io)?;
    Ok(Some(bytes))
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
