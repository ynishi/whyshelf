//! The tree of a project in Git: [`GitStates`], a commit before a change and a commit,
//! or the index, after it. Always a run of two sides.
//!
//! | ask | how it is read |
//! |---|---|
//! | the tree of a side | listed once in the run, when first asked (`ls-tree -r -t -z --full-tree <commit>`, or `ls-files -s -z` for the index, its directories made from the paths), within a bound of bytes ([`LIST_BOUND`]): past it, every name of that side cannot be told |
//! | `read` | each part of the name looked up in that listing: a link (mode `120000`) is [`AtSide::Elsewhere`], a submodule (`160000`) and a path the index holds a conflict at cannot be told, a directory is [`AtSide::Dir`]; a file is its content converted as a checkout writes it, by the attributes of the side (`cat-file --filters` for each file, filter drivers shut out), at most one byte past the bound held |
//! | `look` | as `read`, a file told by the length of that content: the whole blob is read and counted, and none of it held (a pipe the reading waits on, which a look of the file system keeps from opening, is no part of a commit, and the cost is the bytes of the files looked at) |
//! | where a link leads | its text resolved in the tree, from the directory of the link: an absolute text, and one that goes above the top, lead [`Leads::Outside`]; a link on the way is resolved again, up to the walk's bound of links (past it, [`Leads::Unresolved`]); a file or a submodule on the way, and a link whose text Git does not give whole, are [`Leads::Unresolved`]; nothing there [`Leads::Broken`]; the top itself [`Leads::Top`] |
//! | `list` | the entries of the directory in the listing, each with its kind: a tree a directory, a blob a file, a link a link, a submodule another kind |
//! | `files` | every blob, link and submodule of the listing |
//! | `read_outside`, `list_outside`, `read_beyond`, `list_beyond` | nothing: a commit holds no place outside the tree, and the system's resolution of one is of neither side ([`LEAVES`](super::files::LEAVES)) |
//! | `addresses` | none |
//!
//! What it reads differs from what a directory holds: an untracked file is not there,
//! a submodule cannot be told (a directory holds its files), a file a filter driver
//! names is read as a checkout that runs no driver writes it (for Git LFS, the
//! pointer), and a link that holds an absolute path leads out of the tree wherever it
//! points. Nothing is written: no object, no index, no ref.
//!
//! Known differences from a checkout of the same commit:
//!
//! - Only the attributes the side holds (its `.gitattributes`, and the repository's
//!   `info/attributes`) convert a file. The attributes of the person's settings
//!   (`core.attributesFile`, and the file Git reads by default) and of the system
//!   are not read, as no call of Git reads them (the page of [`git`](super::super::git)):
//!   nothing outside the repository chooses how a file is converted, so a checkout by
//!   a person who has such attributes may convert a file otherwise.
//! - A path the index holds by intent to add (`git add -N`) is read at the index side
//!   as an empty file: `ls-files -s` writes it as a staged empty file, and nothing the
//!   reading asks tells the two apart without reading the work tree.
//! - Each converted file costs one process of Git (`cat-file --filters`), as a batch
//!   cannot be read apart when its files convert: a reading of many files, the
//!   listing of a selection's names among them, starts one process for each file it
//!   reads or looks at.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, Entries, EntryKind, Leads, NameAnswers, NotADir,
    RawPart, Side, StateList, StateRead, TreeFiles, Unnamed,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::super::OpenError;
use super::super::git::{Blob, BlobAsk, Git, TreeCall, TreeEntry, TreeListing};
use super::files::{leaves, list_beyond_by, list_outside_by, read_beyond_by, read_outside_by};
use super::paths::MOST_LINKS;
use super::{GitAfter, StateError, SystemRead};

/// The most bytes of the listing of the tree of one side held: the bound of Git's
/// answers elsewhere (the renames, the files of the work tree).
pub const LIST_BOUND: usize = 16 * 1024 * 1024;

/// The most bytes of the text of a link read.
const LINK_BYTES: u64 = 4096;

/// What is said of a submodule.
const SUBMODULE: TextLiteral = TextLiteral::new("a submodule");

/// What is said of a path the index holds a conflict at.
const CONFLICT: TextLiteral = TextLiteral::new("the index holds a conflict at this path");

/// What is said of a file when a filter driver could not be shut out.
const UNSHUT: TextLiteral = TextLiteral::new(
    "a filter driver the settings define cannot be shut out, so no content is converted",
);

/// What is said of a file whose content Git does not give (an object it does not
/// hold, in a partial clone).
const MISSING: TextLiteral = TextLiteral::new("Git gives no content for this file");

/// The tree of the project in Git, at two sides: a commit before, and a commit or the
/// index after. A clone is the same state, shared: every reader of a run holds one.
#[derive(Debug, Clone)]
pub struct GitStates(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    git: Git,
    /// Where the root is from the top of the work tree (`""`, or `dir/`).
    prefix: Vec<u8>,
    before: SideTree,
    after: SideTree,
    list_bound: usize,
}

/// One side: what it is, and its listing once read.
#[derive(Debug)]
struct SideTree {
    at: At,
    listing: OnceLock<Arc<Tree>>,
}

/// What a side is.
#[derive(Debug, Clone)]
enum At {
    /// A commit, by its object name and the revision as written.
    Commit { oid: String, written: String },
    /// The index, its attributes read from `HEAD`, by its object name.
    Index { head: String },
}

impl At {
    /// The commit whose attributes convert the blobs of the side.
    fn attributes(&self) -> &str {
        match self {
            Self::Commit { oid, .. } => oid,
            Self::Index { head } => head,
        }
    }
}

/// The listing of one side.
#[derive(Debug)]
enum Tree {
    /// Every entry, by its path from the root, as bytes.
    Listed(BTreeMap<Vec<u8>, Entry>),
    /// The listing cannot be read whole, and why.
    Untold(Untold),
}

/// An entry of the listing.
#[derive(Debug, Clone)]
struct Entry {
    kind: Kind,
    oid: String,
    /// Its path from the top of the work tree: what its attributes are read for.
    full: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    /// A link, with its text.
    Link(Vec<u8>),
    Submodule,
    Conflict,
}

impl Kind {
    fn entry_kind(&self) -> EntryKind {
        match self {
            Self::Dir => EntryKind::Dir,
            Self::File => EntryKind::File,
            Self::Link(_) => EntryKind::Link,
            Self::Submodule | Self::Conflict => EntryKind::Other,
        }
    }
}

fn said(text: TextLiteral) -> Untold {
    Untold::Said(NonBlank::from(text))
}

impl GitStates {
    /// The project at `root` between the commit `since` names and `until`: the commit
    /// it names, or the index. Opening asks Git that the root is in a repository, that
    /// each revision names a commit, that this Git knows every form the tree is read
    /// with (each tried once, [`TreeCall`]), and, for the index, that its
    /// `.gitattributes` are `HEAD`'s; otherwise it cannot be opened, and says why.
    pub async fn open(root: PathBuf, since: &str, until: &GitAfter) -> Result<Self, OpenError> {
        Self::open_on(Git::new(root.clone()), root, since, until).await
    }

    /// [`open`](Self::open), running `program` in place of `git`: what a test stands a
    /// Git that knows less in with.
    #[cfg(test)]
    pub(crate) async fn open_with_program(
        root: PathBuf,
        since: &str,
        until: &GitAfter,
        program: PathBuf,
    ) -> Result<Self, OpenError> {
        Self::open_on(
            Git::new(root.clone()).with_program(program),
            root,
            since,
            until,
        )
        .await
    }

    async fn open_on(
        git: Git,
        root: PathBuf,
        since: &str,
        until: &GitAfter,
    ) -> Result<Self, OpenError> {
        if !git.runs().await? {
            return Err(OpenError::NoGit);
        }
        let Some(prefix) = git.prefix().await? else {
            return Err(OpenError::NotARepository { root });
        };
        let commit = |rev: &str| {
            let git = git.clone();
            let rev = rev.to_owned();
            async move { git.commit(&rev).await?.ok_or(OpenError::NotACommit { rev }) }
        };
        let before = At::Commit {
            oid: commit(since).await?,
            written: since.to_owned(),
        };
        let (after, calls) = match until {
            GitAfter::Commit(rev) => (
                At::Commit {
                    oid: commit(rev).await?,
                    written: rev.clone(),
                },
                TreeCall::OF_COMMITS.to_vec(),
            ),
            GitAfter::Index => {
                let mut calls = TreeCall::OF_COMMITS.to_vec();
                calls.push(TreeCall::IndexListing);
                (
                    At::Index {
                        head: commit("HEAD").await?,
                    },
                    calls,
                )
            }
        };
        for call in calls {
            match git.knows(call, before.attributes()).await? {
                Some(true) => {}
                Some(false) => {
                    return Err(OpenError::UnknownOption {
                        option: call.written(),
                    });
                }
                None => return Err(OpenError::NoGit),
            }
        }
        if matches!(after, At::Index { .. }) && !git.index_attributes_as_head().await? {
            return Err(OpenError::IndexAttributes);
        }
        Ok(Self(Arc::new(Inner {
            git,
            prefix,
            before: SideTree {
                at: before,
                listing: OnceLock::new(),
            },
            after: SideTree {
                at: after,
                listing: OnceLock::new(),
            },
            list_bound: LIST_BOUND,
        })))
    }

    /// The same, holding at most `bytes` of the listing of each side (for a test of
    /// the bound): past it, every name of that side cannot be told.
    #[cfg(test)]
    pub(crate) fn with_list_bound(self, bytes: usize) -> Self {
        let fresh = |side: &SideTree| SideTree {
            at: side.at.clone(),
            listing: OnceLock::new(),
        };
        Self(Arc::new(Inner {
            git: self.0.git.clone(),
            prefix: self.0.prefix.clone(),
            before: fresh(&self.0.before),
            after: fresh(&self.0.after),
            list_bound: bytes,
        }))
    }

    fn side(&self, side: Side) -> &SideTree {
        match side {
            Side::Before => &self.0.before,
            Side::After => &self.0.after,
        }
    }

    /// How `name` at `side` is written in what a reader says: `<rev>:<path>` of a
    /// commit, `:<path>` of the index.
    pub(crate) fn path_of(&self, side: Side, name: &RelPath) -> PathBuf {
        PathBuf::from(format!("{}{name}", self.written(side)))
    }

    /// How the place `at` is written, as [`path_of`](Self::path_of) writes a name, its
    /// parts that are no text replaced.
    pub(crate) fn beyond_path(&self, side: Side, at: &Beyond<RelPath>) -> PathBuf {
        let mut parts: Vec<String> = at.dir.iter().map(|dir| dir.as_str().to_owned()).collect();
        parts.extend(
            at.parts
                .iter()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        );
        PathBuf::from(format!("{}{}", self.written(side), parts.join("/")))
    }

    /// The start of a name as Git writes a path of a side: from the top of the work
    /// tree, so the prefix of the root is part of it.
    fn written(&self, side: Side) -> String {
        let prefix = String::from_utf8_lossy(&self.0.prefix);
        match &self.side(side).at {
            At::Commit { written, .. } => format!("{written}:{prefix}"),
            At::Index { .. } => format!(":{prefix}"),
        }
    }

    /// A tree of Git holds no place the system resolves: neither side reads one.
    pub(crate) fn system_read(&self) -> SystemRead<'static> {
        SystemRead::Untold(leaves())
    }

    /// The listing of `side`, read once.
    async fn tree(&self, side: Side) -> Result<Arc<Tree>, StateError> {
        let of = self.side(side);
        if let Some(tree) = of.listing.get() {
            return Ok(Arc::clone(tree));
        }
        let tree = Arc::new(self.list_side(&of.at).await?);
        Ok(Arc::clone(of.listing.get_or_init(|| tree)))
    }

    async fn list_side(&self, at: &At) -> Result<Tree, StateError> {
        let git = &self.0.git;
        let bound = self.0.list_bound;
        let listing = match at {
            At::Commit { oid, .. } => git.tree_entries(oid, bound).await?,
            At::Index { .. } => git.index_entries(bound).await?,
        };
        let TreeListing::Entries(listed) = listing else {
            // The side is the reader's to name: the same state at both sides says the
            // same, so it is no change.
            let why = format!("the listing of the tree of this side is past {bound} bytes");
            return Ok(Tree::Untold(Untold::Said(
                NonBlank::new(why).unwrap_or_else(|_| NonBlank::from(MISSING)),
            )));
        };
        let index = matches!(at, At::Index { .. });
        let prefix = &self.0.prefix;
        let mut entries: BTreeMap<Vec<u8>, Entry> = BTreeMap::new();
        let mut links = Vec::new();
        for TreeEntry {
            mode,
            oid,
            stage,
            path,
        } in listed
        {
            // A commit lists from the top of the work tree; the index from the root.
            let (rel, full) = if index {
                let mut full = prefix.clone();
                full.extend_from_slice(&path);
                (path, full)
            } else {
                match path.strip_prefix(prefix.as_slice()) {
                    Some(rel) if !rel.is_empty() => (rel.to_vec(), path.clone()),
                    _ => continue,
                }
            };
            let kind = match (stage, mode) {
                (1.., _) => Kind::Conflict,
                (_, 0o040000) => Kind::Dir,
                (_, 0o120000) => Kind::Link(Vec::new()),
                (_, 0o160000) => Kind::Submodule,
                _ => Kind::File,
            };
            if index {
                let mut parent = rel.as_slice();
                while let Some(slash) = parent.iter().rposition(|b| *b == b'/') {
                    parent = &parent[..slash];
                    entries.entry(parent.to_vec()).or_insert_with(|| Entry {
                        kind: Kind::Dir,
                        oid: String::new(),
                        full: Vec::new(),
                    });
                }
            }
            if kind == Kind::Link(Vec::new()) {
                links.push((rel.clone(), oid.clone()));
            }
            // A conflict keeps its stages apart; the path is one of a conflict.
            if let Some(there) = entries.get(&rel)
                && there.kind == Kind::Conflict
            {
                continue;
            }
            entries.insert(rel, Entry { kind, oid, full });
        }
        if !links.is_empty() {
            let asks: Vec<BlobAsk> = links
                .iter()
                .map(|(_, oid)| BlobAsk {
                    oid: oid.clone(),
                    path: Vec::new(),
                })
                .collect();
            let texts = git
                .blobs(None, &asks, LINK_BYTES)
                .await?
                .unwrap_or_default();
            // A text Git does not give whole (missing, or past the bound) stays empty,
            // which no link holds: where it leads cannot be resolved (`leads`).
            for ((rel, _), text) in links.iter().zip(texts) {
                if let (Some(entry), Blob::There { len, bytes }) = (entries.get_mut(rel), text)
                    && u64::try_from(bytes.len()).is_ok_and(|held| held == len)
                {
                    entry.kind = Kind::Link(bytes);
                }
            }
        }
        Ok(Tree::Listed(entries))
    }

    /// The contents of the files `asks` names at `side`, converted, at most `keep`
    /// bytes of each held.
    async fn contents(
        &self,
        side: Side,
        asks: &[BlobAsk],
        keep: u64,
    ) -> Result<Vec<AtSide<RelPath, (u64, Vec<u8>)>>, StateError> {
        let source = self.side(side).at.attributes().to_owned();
        Ok(match self.0.git.blobs(Some(&source), asks, keep).await? {
            None => asks.iter().map(|_| AtSide::Untold(said(UNSHUT))).collect(),
            Some(blobs) => blobs
                .into_iter()
                .map(|blob| match blob {
                    Blob::There { len, bytes } => AtSide::There((len, bytes)),
                    Blob::Missing => AtSide::Untold(said(MISSING)),
                })
                .collect(),
        })
    }

    /// Each name of `names` at `side`, its files' contents read with at most `keep`
    /// bytes held.
    async fn answers(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        keep: u64,
    ) -> Result<NameAnswers<RelPath, (u64, Vec<u8>), StateError>, StateError> {
        let tree = self.tree(side).await?;
        let mut answers = BTreeMap::new();
        let mut files = Vec::new();
        for name in names {
            let answer = match tree.as_ref() {
                Tree::Untold(why) => Ok(AtSide::Untold(why.clone())),
                Tree::Listed(entries) => match walk(entries, name) {
                    Walk::Absent => Ok(AtSide::Absent),
                    Walk::ThroughFile => Err(StateError::Io {
                        path: self.path_of(side, name),
                        source: io::ErrorKind::NotADirectory.into(),
                    }),
                    Walk::Link { at, target } => Ok(AtSide::Elsewhere {
                        leads: leads(entries, &at, &target),
                        at,
                        target: String::from_utf8_lossy(&target).into_owned(),
                    }),
                    Walk::End(entry) => match &entry.kind {
                        Kind::Dir => Ok(AtSide::Dir),
                        Kind::Submodule => Ok(AtSide::Untold(said(SUBMODULE))),
                        Kind::Conflict => Ok(AtSide::Untold(said(CONFLICT))),
                        Kind::File | Kind::Link(_) => {
                            files.push((
                                name.clone(),
                                BlobAsk {
                                    oid: entry.oid.clone(),
                                    path: entry.full.clone(),
                                },
                            ));
                            continue;
                        }
                    },
                },
            };
            answers.insert(name.clone(), answer);
        }
        let asks: Vec<BlobAsk> = files.iter().map(|(_, ask)| ask.clone()).collect();
        let read = self.contents(side, &asks, keep).await?;
        for ((name, _), answer) in files.into_iter().zip(read) {
            answers.insert(name, Ok(answer));
        }
        Ok(answers)
    }
}

/// What a walk along the parts of a name in a listing came to.
enum Walk<'a> {
    Absent,
    /// A part on the way is a file.
    ThroughFile,
    /// A part is a link: the name of that part, and its text.
    Link {
        at: RelPath,
        target: Vec<u8>,
    },
    End(&'a Entry),
}

fn walk<'a>(entries: &'a BTreeMap<Vec<u8>, Entry>, name: &RelPath) -> Walk<'a> {
    let parts: Vec<&str> = name.as_str().split('/').collect();
    for i in 0..parts.len() {
        let at = parts[..=i].join("/");
        let Some(entry) = entries.get(at.as_bytes()) else {
            return Walk::Absent;
        };
        let last = i + 1 == parts.len();
        match &entry.kind {
            Kind::Link(target) => {
                return match at.parse() {
                    Ok(at) => Walk::Link {
                        at,
                        target: target.clone(),
                    },
                    Err(_) => Walk::Absent,
                };
            }
            Kind::Dir if !last => {}
            _ if last => return Walk::End(entry),
            Kind::Submodule | Kind::Conflict => return Walk::End(entry),
            _ => return Walk::ThroughFile,
        }
    }
    Walk::Absent
}

/// Where the link at `at`, holding `target`, leads in the listing.
fn leads(entries: &BTreeMap<Vec<u8>, Entry>, at: &RelPath, target: &[u8]) -> Leads<RelPath> {
    // An empty text is one Git did not give whole: it names no place.
    if target.is_empty() {
        return Leads::Unresolved;
    }
    let mut parts: Vec<String> = at.as_str().split('/').map(str::to_owned).collect();
    parts.pop();
    let mut pending: VecDeque<String> = VecDeque::new();
    let Some(()) = queue(&mut pending, target) else {
        return Leads::Outside;
    };
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        match part.as_str() {
            "" | "." => continue,
            ".." => {
                if parts.pop().is_none() {
                    return Leads::Outside;
                }
                continue;
            }
            _ => parts.push(part),
        }
        let here = parts.join("/");
        let Some(entry) = entries.get(here.as_bytes()) else {
            return Leads::Broken;
        };
        match &entry.kind {
            Kind::Dir => {}
            Kind::Link(text) => {
                hops += 1;
                if hops > MOST_LINKS || text.is_empty() {
                    return Leads::Unresolved;
                }
                parts.pop();
                if queue(&mut pending, text).is_none() {
                    return Leads::Outside;
                }
            }
            _ if pending.iter().all(|p| p.is_empty() || p == ".") => {}
            _ => return Leads::Unresolved,
        }
    }
    if parts.is_empty() {
        return Leads::Top;
    }
    parts
        .join("/")
        .parse()
        .map_or(Leads::Outside, Leads::Inside)
}

/// The parts of a link's text put first in `pending`; none for a text that leaves the
/// tree whatever it names (absolute, or not UTF-8, which no name of the tree is).
fn queue(pending: &mut VecDeque<String>, text: &[u8]) -> Option<()> {
    let text = std::str::from_utf8(text).ok()?;
    if text.starts_with('/') {
        return None;
    }
    for part in text.split('/').rev() {
        pending.push_front(part.to_owned());
    }
    Some(())
}

/// The direct entries of the directory `dir` (none: the top) in the listing.
fn children(entries: &BTreeMap<Vec<u8>, Entry>, dir: Option<&RelPath>) -> Entries<RelPath> {
    let start: Vec<u8> = dir.map_or_else(Vec::new, |dir| format!("{dir}/").into_bytes());
    let mut listed = Entries::default();
    for (path, entry) in entries.range(start.clone()..) {
        let Some(rest) = path.strip_prefix(start.as_slice()) else {
            break;
        };
        if rest.is_empty() || rest.contains(&b'/') {
            continue;
        }
        let kind = entry.kind.entry_kind();
        let named = std::str::from_utf8(path)
            .ok()
            .and_then(|text| text.parse::<RelPath>().ok());
        match named {
            Some(name) => {
                listed.named.insert(name, kind);
            }
            None => {
                let text = String::from_utf8_lossy(rest).into_owned();
                listed.unnamed.insert(
                    text.clone(),
                    Unnamed {
                        kind,
                        at: Beyond {
                            dir: dir.cloned(),
                            parts: vec![RawPart::new(OsString::from(text))],
                        },
                    },
                );
            }
        }
    }
    listed
}

impl StateRead<RelPath> for GitStates {
    type Error = StateError;

    async fn read(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        let keep = bound.get().saturating_add(1);
        Ok(self
            .answers(side, names, keep)
            .await?
            .into_iter()
            .map(|(name, answer)| (name, answer.map(|at| content(at, |(_, bytes)| bytes))))
            .collect())
    }

    /// As `read`, a file told by the length of its converted content: read whole and
    /// counted, none of it held.
    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<NameAnswers<RelPath, u64, StateError>, StateError> {
        Ok(self
            .answers(side, names, 0)
            .await?
            .into_iter()
            .map(|(name, answer)| (name, answer.map(|at| content(at, |(len, _)| len))))
            .collect())
    }

    /// As the run's answer of whether it reads what the system resolves says
    /// (`system_read`).
    async fn read_outside(
        &self,
        _: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        Ok(read_outside_by(&self.system_read(), names, bound).await)
    }

    /// As [`read_outside`](StateRead::read_outside).
    async fn read_beyond(
        &self,
        _: Side,
        at: &Beyond<RelPath>,
        bound: ByteBound,
    ) -> Result<AtSide<RelPath>, StateError> {
        read_beyond_by(&self.system_read(), at, bound).await
    }

    async fn addresses(
        &self,
        _: Side,
        _: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, StateError> {
        Ok(BTreeMap::new())
    }
}

/// An answer with the content of a file mapped.
fn content<C, D>(at: AtSide<RelPath, C>, map: impl FnOnce(C) -> D) -> AtSide<RelPath, D> {
    match at {
        AtSide::There(content) => AtSide::There(map(content)),
        AtSide::Absent => AtSide::Absent,
        AtSide::Dir => AtSide::Dir,
        AtSide::Elsewhere { at, target, leads } => AtSide::Elsewhere { at, target, leads },
        AtSide::Untold(why) => AtSide::Untold(why),
    }
}

impl StateList<RelPath> for GitStates {
    async fn list(
        &self,
        side: Side,
        dir: Option<&RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        let tree = self.tree(side).await?;
        let entries = match tree.as_ref() {
            Tree::Untold(why) => return Ok(DirListing::NotADir(NotADir::Untold(why.clone()))),
            Tree::Listed(entries) => entries,
        };
        let Some(name) = dir else {
            return Ok(DirListing::All(children(entries, None)));
        };
        Ok(match walk(entries, name) {
            Walk::Absent => DirListing::NotADir(NotADir::Absent),
            Walk::ThroughFile => {
                return Err(StateError::Io {
                    path: self.path_of(side, name),
                    source: io::ErrorKind::NotADirectory.into(),
                });
            }
            Walk::Link { at, target } => DirListing::NotADir(NotADir::Elsewhere {
                leads: leads(entries, &at, &target),
                at,
                target: String::from_utf8_lossy(&target).into_owned(),
            }),
            Walk::End(entry) => match entry.kind {
                Kind::Dir => DirListing::All(children(entries, Some(name))),
                Kind::File | Kind::Link(_) => DirListing::NotADir(NotADir::File),
                Kind::Submodule => DirListing::NotADir(NotADir::Untold(said(SUBMODULE))),
                Kind::Conflict => DirListing::NotADir(NotADir::Untold(said(CONFLICT))),
            },
        })
    }

    /// As [`read_outside`](StateRead::read_outside).
    async fn list_outside(
        &self,
        _: Side,
        dir: &RelPath,
    ) -> Result<DirListing<RelPath>, StateError> {
        list_outside_by(&self.system_read(), dir).await
    }

    /// As [`read_outside`](StateRead::read_outside).
    async fn list_beyond(
        &self,
        _: Side,
        at: &Beyond<RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        list_beyond_by(&self.system_read(), at).await
    }

    /// Every blob, link and submodule of the listing of `side`, which its own bound
    /// holds to: `bound` asks no more of it.
    async fn files(&self, side: Side, _: ByteBound) -> Result<TreeFiles<RelPath>, StateError> {
        let tree = self.tree(side).await?;
        let entries = match tree.as_ref() {
            Tree::Untold(why) => {
                return Ok(TreeFiles::CannotList(
                    NonBlank::new(why.as_str()).unwrap_or_else(|_| NonBlank::from(MISSING)),
                ));
            }
            Tree::Listed(entries) => entries,
        };
        let mut names = BTreeSet::new();
        let mut unnamed = 0usize;
        for (path, entry) in entries {
            if entry.kind == Kind::Dir {
                continue;
            }
            match std::str::from_utf8(path)
                .ok()
                .and_then(|text| text.parse::<RelPath>().ok())
            {
                Some(name) => {
                    names.insert(name);
                }
                None => unnamed += 1,
            }
        }
        if unnamed == 0 {
            return Ok(TreeFiles::All(names));
        }
        let why = format!(
            "{unnamed} file(s) Git lists with a path that is not UTF-8 or no path of the project"
        );
        Ok(TreeFiles::Cut {
            names,
            why: NonBlank::new(why).unwrap_or_else(|_| NonBlank::from(MISSING)),
        })
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
