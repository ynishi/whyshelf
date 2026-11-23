//! The tree of the built docs of one side: where the docs are ([`DocsAt`]), the port
//! the lookup reads them through ([`BuiltDocs`]), and its two adapters.
//!
//! | where the docs are | adapter | what is read, and through which link |
//! |---|---|---|
//! | in the project ([`DocsAt::Inside`]) | [`UnderTree`] | each place of the docs as the name `<docs>/<place>` of the project's tree, read through no link out of the root (`Leave::Refused`): the root holds the build directory, so "inside the root or the build directory" is "inside the root" |
//! | outside it ([`DocsAt::Outside`]) | [`OutsideDocs`](super::outside_docs::OutsideDocs) | the file system, by the canonical paths of the root and the build directory; nothing at all in a run of two sides |
//!
//! A re-export is followed only into the docs ([`BuiltDocs::inside_docs`]): under the
//! project, when the name its link comes to, links resolved, is under the name the
//! docs come to.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;

use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, Entries, EntryKind, NameAnswers, NotADir, RawPart, Side,
    StateList, StateRead, TreeFiles, Unnamed,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::super::super::state::StateError;
use super::super::super::state::files::{FileStates, name_of};
use super::super::super::state::paths::{
    Followed, Leave, TreeName, follow, follow_dir, look_through,
};
use super::docs_path::DocsPath;
use super::outside_docs::{OutsideDocs, leads_outside};

/// Where the built docs of one side are: a directory of the project, by its name in
/// the project's tree, or a directory outside the root of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DocsAt {
    Inside(RelPath),
    Outside(PathBuf),
}

/// Where the docs at `docs` (a path of the file system, made from the side's
/// directory) are at `side`: inside the project when the path is under the side's
/// directory as written and no link on its way leads out of the root; outside it
/// otherwise.
pub(crate) async fn placed(
    state: &FileStates,
    side: Side,
    docs: PathBuf,
) -> Result<DocsAt, StateError> {
    let top = state.dir(side)?;
    let Some(rel) = docs.strip_prefix(top).ok().and_then(name_of) else {
        return Ok(DocsAt::Outside(docs));
    };
    // Docs reached through a link out of the root are read where the system finds
    // them, by the rules of docs outside the project.
    Ok(
        match follow_dir(state, side, Some(&rel), Leave::Refused).await {
            Ok(Followed::Left) => DocsAt::Outside(docs),
            _ => DocsAt::Inside(rel),
        },
    )
}

/// The built docs of a run, read at a side: what the lookup of a path reads them
/// through.
pub(crate) trait BuiltDocs: StateRead<DocsPath> + StateList<DocsPath> {
    /// Why nothing is read at `p`: it leads outside the root of the project and the
    /// build directory once links are resolved, or is a link that cannot be resolved,
    /// or nothing is there to resolve. None when it may be read.
    fn outside(&self, side: Side, p: &DocsPath) -> impl Future<Output = Option<String>> + Send;

    /// Whether `p`, links resolved, is under the built docs: where a re-export is
    /// followed.
    fn inside_docs(&self, side: Side, p: &DocsPath) -> impl Future<Output = bool> + Send;

    /// The path of the file system of `p`, at `side`: what a reader names in what it
    /// says.
    fn shown(&self, side: Side, p: &DocsPath) -> PathBuf;
}

/// The built docs inside the project, read through the project's tree `S`: the place
/// `p` of the docs is the name `<docs>/<p>`.
#[derive(Debug, Clone)]
pub(crate) struct UnderTree<S = FileStates> {
    state: S,
    /// The directory of the docs, as the project names it.
    docs: RelPath,
    /// The directory of the docs at the side read, as a path of the file system.
    shown: PathBuf,
}

/// What is said of a place of the docs that cannot be named in the project.
const NO_NAME: TextLiteral =
    TextLiteral::new("the place in the docs is no path of the project, and is not read");

/// What is said of a place of the docs whose way leaves the root of the project.
const LEFT: TextLiteral = TextLiteral::new(
    "a symbolic link on the way leads outside the root of the project, or is broken: nothing is read through it",
);

impl<S: StateList<RelPath>> UnderTree<S> {
    /// The docs at `docs` of the project's tree `state`, shown at `shown`.
    pub(crate) fn new(state: S, docs: RelPath, shown: PathBuf) -> Self {
        Self { state, docs, shown }
    }

    fn name(&self, p: &DocsPath) -> Option<RelPath> {
        p.in_project(&self.docs)
    }

    /// `p`, every link on it followed while it stays inside the root.
    async fn reached(
        &self,
        side: Side,
        p: &DocsPath,
        bound: ByteBound,
    ) -> Result<Followed<RelPath, AtSide<RelPath>>, S::Error> {
        match self.name(p) {
            Some(name) => follow(&self.state, side, &name, Leave::Refused, bound).await,
            None => Ok(Followed::Broken),
        }
    }

    /// `p`, looked at as [`reached`](Self::reached) reads it: nothing is opened.
    async fn looked(
        &self,
        side: Side,
        p: &DocsPath,
    ) -> Result<Followed<RelPath, AtSide<RelPath, u64>>, S::Error> {
        match self.name(p) {
            Some(name) => look_through(&self.state, side, &name).await,
            None => Ok(Followed::Broken),
        }
    }

    /// What cannot be told of `p` whose way is not followed.
    fn unfollowed<C>(&self, p: &DocsPath) -> AtSide<DocsPath, C> {
        AtSide::Untold(Untold::Said(NonBlank::from(match self.name(p) {
            Some(_) => LEFT,
            None => NO_NAME,
        })))
    }
}

/// The answer of the project's tree for a place of the docs, with no name of the
/// project in it.
fn of_project<C>(answer: AtSide<RelPath, C>) -> AtSide<DocsPath, C> {
    match answer {
        AtSide::There(bytes) => AtSide::There(bytes),
        AtSide::Absent => AtSide::Absent,
        AtSide::Dir => AtSide::Dir,
        AtSide::Untold(why) => AtSide::Untold(why),
        AtSide::Elsewhere { .. } => AtSide::Untold(Untold::Said(NonBlank::from(LEFT))),
    }
}

impl<S: StateList<RelPath>> StateRead<DocsPath> for UnderTree<S> {
    type Error = S::Error;

    /// Each place as the project's tree reads its name, every link on it followed
    /// while it stays inside the root: never [`AtSide::Elsewhere`]; a place whose way
    /// leaves the root, or that is no name of the project, cannot be told.
    async fn read(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, S::Error>, S::Error> {
        let mut read = BTreeMap::new();
        for p in names {
            let answer = self
                .reached(side, p, bound)
                .await
                .map(|walked| match walked {
                    Followed::Reached { answer, .. } => of_project(answer),
                    Followed::Left | Followed::Broken | Followed::TooMany => self.unfollowed(p),
                });
            read.insert(p.clone(), answer);
        }
        Ok(read)
    }

    /// Each place as [`read`](StateRead::read) reads it, a file told by its length.
    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
    ) -> Result<NameAnswers<DocsPath, u64, S::Error>, S::Error> {
        let mut looked = BTreeMap::new();
        for p in names {
            let answer = self.looked(side, p).await.map(|walked| match walked {
                Followed::Reached { answer, .. } => of_project(answer),
                Followed::Left | Followed::Broken | Followed::TooMany => self.unfollowed(p),
            });
            looked.insert(p.clone(), answer);
        }
        Ok(looked)
    }

    /// Never asked: the docs in the project are read through no link out of it.
    async fn read_outside(
        &self,
        _: Side,
        names: &BTreeSet<DocsPath>,
        _: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, S::Error>, S::Error> {
        Ok(names
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    Ok(AtSide::Untold(Untold::Said(NonBlank::from(LEFT)))),
                )
            })
            .collect())
    }

    async fn addresses(
        &self,
        _: Side,
        _: &BTreeSet<DocsPath>,
    ) -> Result<BTreeMap<DocsPath, ContentAddress>, S::Error> {
        Ok(BTreeMap::new())
    }
}

impl<S: StateList<RelPath>> StateList<DocsPath> for UnderTree<S> {
    /// The entries of the place (none: the docs) as the project's tree lists its
    /// name, links on the way followed while they stay inside the root, each by its
    /// place under the one asked.
    async fn list(
        &self,
        side: Side,
        dir: Option<&DocsPath>,
    ) -> Result<DirListing<DocsPath>, S::Error> {
        let at = dir.cloned().unwrap_or_default();
        let Some(name) = self.name(&at) else {
            return Ok(DirListing::NotADir(NotADir::Untold(Untold::Said(
                NonBlank::from(NO_NAME),
            ))));
        };
        let listing = match follow_dir(&self.state, side, Some(&name), Leave::Refused).await? {
            Followed::Reached { answer, .. } => answer,
            Followed::Left | Followed::Broken | Followed::TooMany => {
                return Ok(DirListing::NotADir(NotADir::Untold(Untold::Said(
                    NonBlank::from(LEFT),
                ))));
            }
        };
        // An entry the docs cannot name is kept by its text, under the place listed:
        // the docs read no such place.
        let unnamed = |text: &str, kind: EntryKind| Unnamed {
            kind,
            at: Beyond {
                dir: Some(at.clone()),
                parts: vec![RawPart::new(OsString::from(text))],
            },
        };
        let under = |entries: Entries<RelPath>| {
            let mut placed = Entries::default();
            for (name, kind) in entries.named {
                let part = name.parts().last().copied().unwrap_or_default().to_owned();
                match at.join(&part) {
                    Some(p) => {
                        placed.named.insert(p, kind);
                    }
                    None => {
                        placed.unnamed.insert(part.clone(), unnamed(&part, kind));
                    }
                }
            }
            for (text, entry) in entries.unnamed {
                placed
                    .unnamed
                    .insert(text.clone(), unnamed(&text, entry.kind));
            }
            placed
        };
        Ok(match listing {
            DirListing::All(entries) => DirListing::All(under(entries)),
            DirListing::Cut { entries, why } => DirListing::Cut {
                entries: under(entries),
                why,
            },
            DirListing::NotADir(not) => DirListing::NotADir(match not {
                NotADir::Absent => NotADir::Absent,
                NotADir::File => NotADir::File,
                NotADir::Untold(why) => NotADir::Untold(why),
                NotADir::Elsewhere { .. } => NotADir::Untold(Untold::Said(NonBlank::from(LEFT))),
            }),
        })
    }

    /// Never asked, as [`read_outside`](StateRead::read_outside).
    async fn list_outside(&self, _: Side, _: &DocsPath) -> Result<DirListing<DocsPath>, S::Error> {
        Ok(DirListing::NotADir(NotADir::Untold(Untold::Said(
            NonBlank::from(LEFT),
        ))))
    }

    async fn files(&self, _: Side, _: ByteBound) -> Result<TreeFiles<DocsPath>, S::Error> {
        Ok(TreeFiles::CannotList(NonBlank::from(TextLiteral::new(
            "the built docs list no files",
        ))))
    }
}

impl<S: StateList<RelPath>> BuiltDocs for UnderTree<S> {
    /// None when the name of `p` is reached, links followed inside the root, and
    /// something is there (read or not): the place's canonical path is then under the
    /// root, and so under the root or the build directory, which is in it.
    async fn outside(&self, side: Side, p: &DocsPath) -> Option<String> {
        match self.looked(side, p).await {
            Ok(Followed::Reached { answer, .. }) if answer != AtSide::Absent => None,
            _ => Some(leads_outside(&self.shown(side, p))),
        }
    }

    /// Whether the name `p` comes to, links followed inside the root, is there and
    /// under the name the docs come to.
    async fn inside_docs(&self, side: Side, p: &DocsPath) -> bool {
        let Ok(Followed::Reached {
            at: Some(at),
            answer,
        }) = self.looked(side, p).await
        else {
            return false;
        };
        if answer == AtSide::Absent {
            return false;
        }
        match look_through(&self.state, side, &self.docs).await {
            Ok(Followed::Reached { at: Some(docs), .. }) => at.parts().starts_with(&docs.parts()),
            Ok(Followed::Reached { at: None, .. }) => true,
            _ => false,
        }
    }

    fn shown(&self, _: Side, p: &DocsPath) -> PathBuf {
        p.under(&self.shown)
    }
}

/// The built docs of one side, by where they are.
#[derive(Debug, Clone)]
pub(crate) enum DocsTree {
    Under(UnderTree),
    Outside(OutsideDocs),
}

impl DocsTree {
    /// The docs `at` of `side`, read through `state`.
    pub(crate) async fn of(state: &FileStates, side: Side, at: DocsAt) -> Result<Self, StateError> {
        Ok(match at {
            DocsAt::Inside(docs) => {
                let shown = state.path_of(side, &docs)?;
                Self::Under(UnderTree::new(state.clone(), docs, shown))
            }
            DocsAt::Outside(docs) => {
                Self::Outside(OutsideDocs::new(state.dir(side)?, docs, state.one_side()).await)
            }
        })
    }

    /// The docs at `docs` of the project at `root`, read at one side: what a test
    /// makes a lookup over.
    #[cfg(test)]
    pub(crate) async fn at(root: &std::path::Path, docs: PathBuf) -> Self {
        let state = FileStates::new(root.to_path_buf());
        let at = placed(&state, Side::After, docs.clone())
            .await
            .unwrap_or(DocsAt::Outside(docs.clone()));
        match Self::of(&state, Side::After, at).await {
            Ok(tree) => tree,
            Err(_) => Self::Outside(OutsideDocs::new(root, docs, true).await),
        }
    }

    /// The directory of the docs, at `side`, as a path of the file system.
    pub(crate) fn dir(&self, side: Side) -> PathBuf {
        self.shown(side, &DocsPath::top())
    }
}

impl StateRead<DocsPath> for DocsTree {
    type Error = StateError;

    async fn read(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, StateError>, StateError> {
        match self {
            Self::Under(docs) => docs.read(side, names, bound).await,
            Self::Outside(docs) => docs.read(side, names, bound).await,
        }
    }

    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
    ) -> Result<NameAnswers<DocsPath, u64, StateError>, StateError> {
        match self {
            Self::Under(docs) => docs.look(side, names).await,
            Self::Outside(docs) => docs.look(side, names).await,
        }
    }

    async fn read_outside(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<DocsPath, Vec<u8>, StateError>, StateError> {
        match self {
            Self::Under(docs) => docs.read_outside(side, names, bound).await,
            Self::Outside(docs) => docs.read_outside(side, names, bound).await,
        }
    }

    async fn addresses(
        &self,
        side: Side,
        names: &BTreeSet<DocsPath>,
    ) -> Result<BTreeMap<DocsPath, ContentAddress>, StateError> {
        match self {
            Self::Under(docs) => docs.addresses(side, names).await,
            Self::Outside(docs) => docs.addresses(side, names).await,
        }
    }
}

impl StateList<DocsPath> for DocsTree {
    async fn list(
        &self,
        side: Side,
        dir: Option<&DocsPath>,
    ) -> Result<DirListing<DocsPath>, StateError> {
        match self {
            Self::Under(docs) => docs.list(side, dir).await,
            Self::Outside(docs) => docs.list(side, dir).await,
        }
    }

    async fn list_outside(
        &self,
        side: Side,
        dir: &DocsPath,
    ) -> Result<DirListing<DocsPath>, StateError> {
        match self {
            Self::Under(docs) => docs.list_outside(side, dir).await,
            Self::Outside(docs) => docs.list_outside(side, dir).await,
        }
    }

    async fn files(&self, side: Side, bound: ByteBound) -> Result<TreeFiles<DocsPath>, StateError> {
        match self {
            Self::Under(docs) => docs.files(side, bound).await,
            Self::Outside(docs) => docs.files(side, bound).await,
        }
    }
}

impl BuiltDocs for DocsTree {
    async fn outside(&self, side: Side, p: &DocsPath) -> Option<String> {
        match self {
            Self::Under(docs) => docs.outside(side, p).await,
            Self::Outside(docs) => docs.outside(side, p).await,
        }
    }

    async fn inside_docs(&self, side: Side, p: &DocsPath) -> bool {
        match self {
            Self::Under(docs) => docs.inside_docs(side, p).await,
            Self::Outside(docs) => docs.inside_docs(side, p).await,
        }
    }

    fn shown(&self, side: Side, p: &DocsPath) -> PathBuf {
        match self {
            Self::Under(docs) => docs.shown(side, p),
            Self::Outside(docs) => docs.shown(side, p),
        }
    }
}
