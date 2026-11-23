//! The built docs where they are outside the root of the project
//! ([`DocsAt::Outside`](super::docs_tree::DocsAt::Outside)): [`OutsideDocs`].
//!
//! They are read on the file system, every link followed as the system follows one,
//! and the rule of what is read is the one the space keeps for its docs: a page or the
//! directory of a module only when its canonical path is under the root of the project
//! or the build directory (the parent of the docs), and a re-export only when the
//! canonical path of its link is under the docs.
//!
//! A run that reads two sides of a change reads nothing here: the place outside the
//! project is of neither side, so the two sides cannot be read alike ([`OUTSIDE`]).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, Entries, NameAnswers, NotADir, RawPart, Side, StateList,
    StateRead, TreeFiles, Unnamed,
};
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::super::super::state::StateError;
use super::super::super::state::files::{other_kind_of, read_entries, read_path};
use super::docs_path::DocsPath;
use super::docs_tree::BuiltDocs;

/// Why a run of two sides reads nothing of built docs outside the project.
pub(crate) const OUTSIDE: TextLiteral = TextLiteral::new(
    "the built docs are outside the project, so the two sides cannot be read alike",
);

/// How a reason names a place of the built docs of another kind than a file, a
/// directory or a link.
const A_PAGE: &str = "a page";

/// The built docs in a directory outside the root of the project.
#[derive(Debug, Clone)]
pub(crate) struct OutsideDocs {
    docs: PathBuf,
    /// The docs with every symbolic link resolved, when they can be.
    real_docs: Option<PathBuf>,
    /// The root of the project and the build directory (the parent of the docs),
    /// each with every symbolic link resolved, when it can be: no page and no
    /// directory of a module is read outside both.
    real_places: Vec<PathBuf>,
    /// Whether the run reads one side: one of two reads nothing here.
    one_side: bool,
}

impl OutsideDocs {
    /// The docs at `docs`, of the project at `root`, for a run of one side or of two.
    pub(crate) async fn new(root: &Path, docs: PathBuf, one_side: bool) -> Self {
        let mut real_places = Vec::new();
        for place in std::iter::once(root).chain(docs.parent()) {
            if let Ok(real) = tokio::fs::canonicalize(place).await {
                real_places.push(real);
            }
        }
        Self {
            real_docs: tokio::fs::canonicalize(&docs).await.ok(),
            real_places,
            docs,
            one_side,
        }
    }

    fn untold() -> Untold {
        Untold::Said(NonBlank::from(OUTSIDE))
    }

    /// What `p` is, every link followed, read as the project's tree reads a place
    /// through the system ([`read_path`]).
    async fn read_one(
        &self,
        p: &DocsPath,
        bound: ByteBound,
    ) -> Result<AtSide<DocsPath>, StateError> {
        Ok(
            match read_path(&p.under(&self.docs), bound, A_PAGE).await? {
                AtSide::There(bytes) => AtSide::There(bytes),
                AtSide::Dir => AtSide::Dir,
                AtSide::Untold(why) => AtSide::Untold(why),
                // Every link is followed, so nothing is told elsewhere.
                AtSide::Absent | AtSide::Elsewhere { .. } => AtSide::Absent,
            },
        )
    }

    /// The entries of `dir` (none: the docs), each with its own kind, links not
    /// followed: nothing there is [`NotADir::Absent`] and a file [`NotADir::File`], as
    /// the port says; an error when it cannot be listed.
    async fn list_one(&self, dir: Option<&DocsPath>) -> Result<DirListing<DocsPath>, StateError> {
        let at = dir.cloned().unwrap_or_default();
        let path = at.under(&self.docs);
        let found = match read_entries(&path).await {
            Ok(found) => found,
            Err(StateError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(DirListing::NotADir(NotADir::Absent));
            }
            Err(StateError::Io { source, .. }) if source.kind() == io::ErrorKind::NotADirectory => {
                return Ok(DirListing::NotADir(NotADir::File));
            }
            Err(error) => return Err(error),
        };
        let mut entries = Entries::default();
        for (name, kind) in found {
            match name.to_str().and_then(|text| at.join(text)) {
                Some(named) => {
                    entries.named.insert(named, kind);
                }
                None => {
                    let unnamed = Unnamed {
                        kind,
                        at: Beyond {
                            dir: Some(at.clone()),
                            parts: vec![RawPart::new(name.clone())],
                        },
                    };
                    entries
                        .unnamed
                        .insert(name.to_string_lossy().into_owned(), unnamed);
                }
            }
        }
        Ok(DirListing::All(entries))
    }
}

impl StateRead<DocsPath> for OutsideDocs {
    type Error = StateError;

    async fn read(
        &self,
        _: Side,
        names: &BTreeSet<DocsPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, StateError>, StateError> {
        let mut read = BTreeMap::new();
        for name in names {
            let answer = if self.one_side {
                self.read_one(name, bound).await
            } else {
                Ok(AtSide::Untold(Self::untold()))
            };
            read.insert(name.clone(), answer);
        }
        Ok(read)
    }

    /// Each place by its metadata, every link followed: a file by its length, and
    /// nothing opened.
    async fn look(
        &self,
        _: Side,
        names: &BTreeSet<DocsPath>,
    ) -> Result<NameAnswers<DocsPath, u64, StateError>, StateError> {
        let mut looked = BTreeMap::new();
        for name in names {
            let answer = if self.one_side {
                let path = name.under(&self.docs);
                match tokio::fs::metadata(&path).await {
                    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
                    Ok(meta) if meta.is_dir() => Ok(AtSide::Dir),
                    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
                    Ok(meta) if meta.is_file() => Ok(AtSide::There(meta.len())),
                    Ok(_) => Ok(AtSide::Untold(other_kind_of(A_PAGE))),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(AtSide::Absent),
                    Err(source) => Err(StateError::Io { path, source }),
                }
            } else {
                Ok(AtSide::Untold(Self::untold()))
            };
            looked.insert(name.clone(), answer);
        }
        Ok(looked)
    }

    async fn read_outside(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, StateError>, StateError> {
        self.read(side, names, bound).await
    }

    async fn addresses(
        &self,
        _: Side,
        _: &BTreeSet<DocsPath>,
    ) -> Result<BTreeMap<DocsPath, ContentAddress>, StateError> {
        Ok(BTreeMap::new())
    }
}

impl StateList<DocsPath> for OutsideDocs {
    async fn list(
        &self,
        _: Side,
        dir: Option<&DocsPath>,
    ) -> Result<DirListing<DocsPath>, StateError> {
        if !self.one_side {
            return Ok(DirListing::NotADir(NotADir::Untold(Self::untold())));
        }
        self.list_one(dir).await
    }

    async fn list_outside(
        &self,
        side: Side,
        dir: &DocsPath,
    ) -> Result<DirListing<DocsPath>, StateError> {
        self.list(side, Some(dir)).await
    }

    async fn files(&self, _: Side, _: ByteBound) -> Result<TreeFiles<DocsPath>, StateError> {
        Ok(TreeFiles::CannotList(NonBlank::from(TextLiteral::new(
            "the built docs list no files",
        ))))
    }
}

impl BuiltDocs for OutsideDocs {
    async fn outside(&self, _: Side, p: &DocsPath) -> Option<String> {
        if !self.one_side {
            return Some(NonBlank::from(OUTSIDE).into_string());
        }
        let path = p.under(&self.docs);
        let inside = match tokio::fs::canonicalize(&path).await {
            Ok(real) => self.real_places.iter().any(|place| real.starts_with(place)),
            Err(_) => false,
        };
        (!inside).then(|| leads_outside(&path))
    }

    async fn inside_docs(&self, _: Side, p: &DocsPath) -> bool {
        if !self.one_side {
            return false;
        }
        match (
            &self.real_docs,
            tokio::fs::canonicalize(p.under(&self.docs)).await,
        ) {
            (Some(docs), Ok(real)) => real.starts_with(docs),
            _ => false,
        }
    }

    fn shown(&self, _: Side, p: &DocsPath) -> PathBuf {
        p.under(&self.docs)
    }
}

/// Why nothing is read at `path`, which leads outside the places read.
pub(crate) fn leads_outside(path: &Path) -> String {
    format!(
        "{} leads outside the root of the project and the build directory once symbolic links are resolved, or cannot be resolved: nothing is read through it",
        path.display()
    )
}

/// The names of the entries of a directory that are UTF-8, or none when it cannot
/// be listed: what a test asks of the docs a build wrote.
#[cfg(test)]
pub(super) async fn list(dir: &Path) -> Option<BTreeSet<String>> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    let mut names = BTreeSet::new();
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                if let Ok(name) = entry.file_name().into_string() {
                    names.insert(name);
                }
            }
            Ok(None) => return Some(names),
            Err(_) => return None,
        }
    }
}
