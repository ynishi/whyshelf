//! The search for where the gone names of the space may have moved: the names there
//! are now in the docs ([`Catalog`]) that end as a gone name does.
//!
//! | what | what it does |
//! |---|---|
//! | [`suggest`] | for each gone name, the candidates and the notes of what could not be looked at: the one place the search is made |
//!
//! ```text
//!   the docs place ──unknown──► a note `SearchUnavailable`, each name
//!        │ known
//!   Catalog::walk (the crates of the gone names first)
//!        │
//!   each gone name ─► the names that end as it does, as many segments as the best of
//!        │            their crate (k, from the last); a member `Type::m` is the
//!        │            members of the types that end as `Type` does and have the anchor
//!        │            `m` (of those, the best k of their crate, not of the types
//!        │            before the anchor is looked for); with no type that has the
//!        │            anchor, the names that end as the member does, as any other
//!        ├─ a pin that holds content ──► the promise of the candidate hashed as the
//!        │                               pin was (the crate of the gone name read
//!        │                               first): the same ──► SameContent, else SameEnd
//!        ├─ every move of a prefix that leads it there, the whole name included
//!        └─ at most 16 kept: the first in the order of candidates, of one rank the
//!           crate of the gone name first, then by name; more ──► the note
//!           `CandidatesCut`
//! ```
//!
//! What this module decides is how candidates are found and which are kept within the
//! bound of 16 (the crate of the gone name before the others decides that, and which
//! pages are read first when a bound of pages is reached, and nothing else: the answer
//! is in the order of candidates, then by name, as the domain makes it); the order of candidates is the domain's ([`MoveCandidate::cmp_rank`]), the
//! names there are now are the catalog's, the boundary of a name and the moves of a
//! prefix are the path's ([`RustPath::shared_end`], [`RustPath::moves_between`]), and
//! which prefixes are tried and when one is suggested is the use case's. It reads the
//! docs through the [`Lookup`] it makes, and nothing else.

use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{ArtifactHash, ArtifactName, Comparison};
use whyshelf_core::domain::shelf::ledger::{
    FoundBy, GoneName, MoveCandidate, MoveNote, NameSuggestions, PinnedThere,
};

use super::super::on_blocking;
use super::catalog::{Catalog, CatalogLimits};
use super::layout::{Place, TYPE_KINDS};
use super::lookup::{Found, Lookup};
use super::path::RustPath;

/// The most candidates one gone name keeps; the others are counted
/// ([`MoveNote::CandidatesCut`]).
const MAX_CANDIDATES: usize = 16;

/// For each of `gone`, the names of the docs that it may have moved to, with the notes
/// of what could not be looked at; every name asked has an entry.
///
/// `lookup` is the lookup of the docs the adapter reads, at the side it reads them
/// (the project as it is), with the bound of a page of `limits`, or why there is none:
/// when there is none, nothing is searched and each name has the note
/// [`MoveNote::SearchUnavailable`]. A bound of `limits` stops the search where it is
/// and is a note, never an error; the only error is a text of the adapter that the
/// domain refuses as a reason.
pub(super) async fn suggest(
    lookup: Result<Lookup, String>,
    limits: &CatalogLimits,
    gone: &[GoneName],
) -> Result<BTreeMap<ArtifactName, NameSuggestions>, ValueError> {
    let mut lookup = match lookup {
        Ok(lookup) => lookup,
        Err(why) => return unavailable(gone, why),
    };
    let asked: Vec<(&GoneName, Option<RustPath>)> = gone
        .iter()
        .map(|name| (name, RustPath::parse(name.name().as_str()).ok()))
        .collect();
    let crates: Vec<String> = asked
        .iter()
        .filter_map(|(_, path)| path.as_ref().map(|path| path.krate.clone()))
        .collect();
    let catalog = Catalog::walk(&mut lookup, &crates, limits).await;
    if !catalog.listed() {
        return unavailable(
            gone,
            format!(
                "the docs are not at {}: build them with `cargo doc`, or the directory cannot be read",
                lookup.docs_dir().display()
            ),
        );
    }
    let mut search = Search {
        lookup,
        catalog: &catalog,
        limits,
        gone: asked.iter().filter_map(|(_, path)| path.clone()).collect(),
        cut: false,
        unhashed: 0,
        promises: BTreeMap::new(),
    };
    let mut found: BTreeMap<ArtifactName, (Vec<MoveCandidate>, Option<usize>)> = BTreeMap::new();
    for (name, path) in &asked {
        let entry = match path {
            Some(path) => search.candidates(name, path).await,
            None => (Vec::new(), None),
        };
        found.insert(name.name().clone(), entry);
    }
    let reads = search.lookup.reads();
    let mut scanned = Vec::new();
    if catalog.cut() || search.cut {
        scanned.push(MoveNote::ScanTruncated {
            read: catalog.names().len() + reads.pages,
        });
    }
    let unreadable = catalog.unreadable() + reads.unreadable + search.unhashed;
    if unreadable > 0 {
        scanned.push(MoveNote::UnreadableSkipped { count: unreadable });
    }
    if reads.too_large > 0 {
        scanned.push(MoveNote::TooLargeSkipped {
            count: reads.too_large,
            limit: limits.page_bytes,
        });
    }
    Ok(asked
        .iter()
        .map(|(name, path)| {
            let (candidates, cut) = found.remove(name.name()).unwrap_or_default();
            let mut notes = Vec::new();
            if path.is_some() {
                notes.extend(scanned.iter().cloned());
            }
            notes.extend(cut.map(|found| MoveNote::CandidatesCut { found }));
            (name.name().clone(), NameSuggestions { candidates, notes })
        })
        .collect())
}

/// Every name has no candidate and the note that the search could not be made, for
/// this reason.
fn unavailable(
    gone: &[GoneName],
    why: String,
) -> Result<BTreeMap<ArtifactName, NameSuggestions>, ValueError> {
    let detail = NonBlank::new(why)?;
    Ok(gone
        .iter()
        .map(|name| {
            (
                name.name().clone(),
                NameSuggestions {
                    candidates: Vec::new(),
                    notes: vec![MoveNote::SearchUnavailable {
                        detail: detail.clone(),
                    }],
                },
            )
        })
        .collect())
}

/// A name of the catalog that ends as a gone name does.
struct Near {
    path: RustPath,
    /// Where the docs tell of it.
    place: Place,
    /// How many segments it ends with alike, from the last.
    shared: usize,
}

/// The most segments (k) any of these names ends with alike, for each crate (the first
/// segment of a name): what is kept of the names that end alike is the best of its crate.
fn best_per_crate<'p>(
    names: impl Iterator<Item = (&'p RustPath, usize)>,
) -> BTreeMap<String, usize> {
    let mut best: BTreeMap<String, usize> = BTreeMap::new();
    for (path, shared) in names {
        let most = best.entry(path.krate.clone()).or_insert(0);
        *most = (*most).max(shared);
    }
    best
}

/// One search, over the docs of one [`Lookup`]: what a gone name's candidates are.
struct Search<'a> {
    lookup: Lookup,
    catalog: &'a Catalog,
    limits: &'a CatalogLimits,
    /// The paths of every gone name asked: none of them is a candidate of another (the
    /// domain leaves them out too, but they would take a place of the few kept).
    gone: BTreeSet<RustPath>,
    /// Whether a bound of pages or of bytes kept a page from being read.
    cut: bool,
    /// How many promises were read but could not be hashed (the blocking thread
    /// stopped): counted with the pages that cannot be read.
    unhashed: usize,
    /// What each candidate promises, read once (none when it cannot be read).
    promises: BTreeMap<RustPath, Option<Arc<str>>>,
}

impl Search<'_> {
    /// The candidates of one gone name, at most [`MAX_CANDIDATES`] in the order of
    /// candidates, and how many were found when that is more.
    async fn candidates(
        &mut self,
        name: &GoneName,
        gone: &RustPath,
    ) -> (Vec<MoveCandidate>, Option<usize>) {
        // The names that end as it does, the most segments of them (k) alone, in each
        // crate: a name of a crate that ends alike in fewer segments than a name of
        // another is a candidate still.
        let mut near = self.items(gone);
        near.extend(self.members(gone).await);
        if near.is_empty() {
            return (Vec::new(), None);
        }
        let best = best_per_crate(near.iter().map(|near| (&near.path, near.shared)));
        near.retain(|near| best.get(near.path.krate.as_str()) == Some(&near.shared));

        // The content a pin holds, looked for in the promise of each.
        let content: Vec<(Comparison, ArtifactHash)> = name
            .pins()
            .iter()
            .filter_map(PinnedThere::content)
            .map(|(comparison, hash)| (comparison.clone(), *hash))
            .collect();
        // The pages of the promises are read within one bound for all the gone names, so
        // the candidates in the crate of the gone name are read before the others (the
        // order is stable: which candidates there are is not changed).
        near.sort_by_key(|near| near.path.krate != gone.krate);
        let mut found = Vec::new();
        for near in near {
            let Ok(artifact) = ArtifactName::new(&near.path.as_name()) else {
                continue;
            };
            let by = if !content.is_empty() && self.promises_alike(&near, &content).await {
                FoundBy::SameContent
            } else {
                FoundBy::SameEnd
            };
            // Every move of a prefix that leads to it; the use case reads which are
            // tried.
            let candidate = gone
                .moves_between(&near.path)
                .into_iter()
                .fold(MoveCandidate::new(artifact, by, true), MoveCandidate::under);
            found.push((candidate, near.path.krate == gone.krate));
        }

        // Which are kept within the bound: the order of candidates, then the crate of
        // the gone name, then the name. The domain orders the answer without the crate.
        found.sort_by(|(a, a_here), (b, b_here)| {
            a.cmp_rank(b)
                .then_with(|| b_here.cmp(a_here))
                .then_with(|| a.name().cmp(b.name()))
        });
        let total = found.len();
        found.truncate(MAX_CANDIDATES);
        (
            found.into_iter().map(|(candidate, _)| candidate).collect(),
            (total > MAX_CANDIDATES).then_some(total),
        )
    }

    /// The names of the catalog, of any kind, that end as `gone` does.
    fn items(&self, gone: &RustPath) -> Vec<Near> {
        self.ending_as(gone, |_| true)
            .into_iter()
            .map(|(path, place, shared)| Near {
                path: path.clone(),
                place: place.clone(),
                shared,
            })
            .collect()
    }

    /// For a name that may be a member (`path::Type::m`), the members of the types
    /// of the catalog that end as its type does, that the page of the type has the
    /// anchor of too: the same member, by the rule `find` reads one by. The types are
    /// every one that ends alike in a segment or more, and the best of a crate is taken
    /// of those that have the anchor, not before: a type that ends alike in more
    /// segments and has no such member does not take the place of one that has it.
    ///
    /// The types are read in each crate from the one that ends alike in the most
    /// segments, the crate of the gone name first; once one has the anchor, those that
    /// end alike in fewer in that crate are not read, as they would not be kept. Each page
    /// read is held to the bounds of the search.
    async fn members(&mut self, gone: &RustPath) -> Vec<Near> {
        let Some((owner, member)) = gone.owner() else {
            return Vec::new();
        };
        let mut owners: Vec<(RustPath, usize)> = self
            .ending_alike(&owner, |place| {
                matches!(place, Place::Item { kind, .. } if TYPE_KINDS.contains(&kind.as_str()))
            })
            .into_iter()
            .map(|(path, _, shared)| (path.clone(), shared))
            .collect();
        owners.sort_by(|(a, a_shared), (b, b_shared)| {
            (a.krate != gone.krate)
                .cmp(&(b.krate != gone.krate))
                .then_with(|| a.krate.cmp(&b.krate))
                .then_with(|| b_shared.cmp(a_shared))
                .then_with(|| a.cmp(b))
        });
        let mut near = Vec::new();
        // The most segments a type with the anchor ends alike in, for each crate.
        let mut best: BTreeMap<String, usize> = BTreeMap::new();
        for (owner, shared) in owners {
            if best
                .get(owner.krate.as_str())
                .is_some_and(|best| *best > shared)
            {
                continue;
            }
            let Some(path) = owner.with(member) else {
                continue;
            };
            if self.gone.contains(&path) {
                continue;
            }
            if self.limits.spent(self.lookup.reads()) {
                self.cut = true;
                break;
            }
            if let Found::There(place @ Place::Member { .. }) = self.lookup.find(&path).await {
                best.entry(owner.krate.clone()).or_insert(shared);
                near.push(Near {
                    shared: gone.shared_end(&path),
                    path,
                    place,
                });
            }
        }
        near
    }

    /// The names of the catalog, but `target` itself and the gone names, that `keep`
    /// takes and that end as `target` does for as many segments as any of them does
    /// in the same crate (and one at least): each with its place and the count, in the
    /// order of the catalog.
    fn ending_as<'s>(
        &'s self,
        target: &RustPath,
        keep: impl Fn(&Place) -> bool,
    ) -> Vec<(&'s RustPath, &'s Place, usize)> {
        let mut ending = self.ending_alike(target, keep);
        let best = best_per_crate(ending.iter().map(|(path, _, shared)| (*path, *shared)));
        ending.retain(|(path, _, shared)| best.get(path.krate.as_str()) == Some(shared));
        ending
    }

    /// The names of the catalog, but `target` itself and the gone names, that `keep`
    /// takes and that end as `target` does in one segment or more, whatever the count:
    /// each with its place and the count, in the order of the catalog.
    fn ending_alike<'s>(
        &'s self,
        target: &RustPath,
        keep: impl Fn(&Place) -> bool,
    ) -> Vec<(&'s RustPath, &'s Place, usize)> {
        let mut ending = Vec::new();
        for (path, place) in self.catalog.names() {
            if path == target || self.gone.contains(path) || !keep(place) {
                continue;
            }
            let shared = path.shared_end(target);
            if shared > 0 {
                ending.push((path, place, shared));
            }
        }
        ending
    }

    /// Whether what a candidate promises, hashed as a pin was, is what was pinned: any
    /// of the pins that hold content says so. A promise that cannot be read says no; one
    /// that cannot be hashed says no too, and is counted as a page that cannot be read.
    async fn promises_alike(
        &mut self,
        near: &Near,
        content: &[(Comparison, ArtifactHash)],
    ) -> bool {
        let Some(text) = self.promise(near).await else {
            return false;
        };
        let content = content.to_vec();
        match on_blocking(text.len(), move || {
            content
                .iter()
                .any(|(comparison, hash)| comparison.fingerprint(text.as_bytes()) == *hash)
        })
        .await
        {
            Ok(alike) => alike,
            Err(_) => {
                self.unhashed += 1;
                false
            }
        }
    }

    /// The text of what a candidate promises, read once; none when its page cannot be
    /// read (the page was counted) or no page may be read any more (a bound of the
    /// search, which is then cut); a page read already, such as the page of a member's
    /// type, is used whatever the bound.
    async fn promise(&mut self, near: &Near) -> Option<Arc<str>> {
        if let Some(known) = self.promises.get(&near.path) {
            return known.clone();
        }
        if !self.lookup.has_read(&near.place) && self.limits.spent(self.lookup.reads()) {
            self.cut = true;
            return None;
        }
        let text = self
            .lookup
            .promise(&near.place)
            .await
            .ok()
            .map(|promise| Arc::from(promise.text()));
        self.promises.insert(near.path.clone(), text.clone());
        text
    }
}

#[cfg(test)]
mod tests {
    //! The gone names of these tests are the real thing: a project whose note links to
    //! items is approved over docs written by hand, the docs are then changed, and the
    //! gone names are read from the ledger as the use case reads them.

    use whyshelf_core::app::shelf::{
        JudgeRequest, Judgement, LedgerRead, ShelfApp, Side, SpaceSuggest,
    };
    use whyshelf_core::domain::shelf::artifact::{
        ArtifactReadings, NamePrefix, PrefixMove, SpaceAsks,
    };
    use whyshelf_core::domain::shelf::ledger::{Admit, Ledger, NoteKind, PinnedNames};

    use super::super::RustItemSpace;
    use super::super::fixture::{item_page, module_index, write};
    use super::*;
    use crate::shelf::Project;
    use crate::shelf::ledger::JsonLedger;

    const BOOKS: &str = "[[book]]\nname = \"note\"\ndir = \"notes\"\nkey = { prefix = \"N\" }\n[[book.field]]\nname = \"title\"\ntype = \"text\"\nrequired = true\n[[book.link]]\nname = \"items\"\ntarget = \"artifact\"\n";

    const PUT: &str = "pub fn put(&amp;mut self)";

    /// A project whose note links to items with a preset, the docs of its crates in
    /// `build/doc`.
    struct World {
        dir: tempfile::TempDir,
    }

    impl World {
        /// A project whose link has this preset (`rustdoc`: presence only;
        /// `rustdoc-promise`: the text of what an item promises) and names these items.
        async fn new(preset: &str, items: &[&str]) -> Self {
            let world = Self {
                dir: tempfile::tempdir().unwrap(),
            };
            let items: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
            write(
                world.root(),
                ".whyshelf/books.toml",
                &format!("{BOOKS}preset = \"{preset}\"\n"),
            )
            .await;
            write(
                world.root(),
                "notes/N-1.md",
                &format!(
                    "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\nitems: [{}]\n---\nThe body.\n",
                    items.join(", ")
                ),
            )
            .await;
            world
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn build(&self) -> PathBuf {
            self.root().join("build")
        }

        fn space(&self) -> RustItemSpace {
            RustItemSpace::new(self.root().to_path_buf(), Some(self.build()))
        }

        /// The module (or crate) `dir` of the docs, as `demo/inner`.
        async fn module(&self, dir: &str) {
            write(
                &self.build(),
                &format!("doc/{dir}/index.html"),
                &module_index("A module."),
            )
            .await;
        }

        /// The page `at` of the docs, as `demo/struct.Foo.html`, of an item with this
        /// declaration, these docs and these methods.
        async fn item(&self, at: &str, decl: &str, docs: &str, methods: &[(&str, &str)]) {
            write(
                &self.build(),
                &format!("doc/{at}"),
                &item_page(decl, Some(docs), methods),
            )
            .await;
        }

        /// The docs of `dir` (and what is under it) taken away.
        async fn remove(&self, dir: &str) {
            tokio::fs::remove_dir_all(self.build().join("doc").join(dir))
                .await
                .unwrap();
        }

        /// The note approved over the docs as they are.
        async fn approve(&self) {
            let mut app = ShelfApp::new(
                Project::new(
                    self.root().to_path_buf(),
                    Some("t".to_owned()),
                    None,
                    Some(self.build()),
                )
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
        }

        /// The gone names, as the use case reads them: the pinned names of the
        /// ledger, resolved by this adapter.
        async fn gone(&self) -> Vec<GoneName> {
            let ledger = JsonLedger::new(self.root(), ".whyshelf/ledger".parse().unwrap());
            let pinned = PinnedNames::of(&Ledger::build(
                ledger.read(Side::After).await.unwrap().records,
                &Admit::unruled(),
            ));
            let space = self.space();
            let mut readings = ArtifactReadings::new();
            for (name, asks) in pinned.asks().spaces() {
                readings.extend_space(name.clone(), space.resolve(asks).await.unwrap());
            }
            pinned
                .gone(&readings)
                .spaces()
                .flat_map(|(_, names)| names)
                .collect()
        }

        /// What the search says of the gone names, under these limits.
        async fn suggest(&self, limits: &CatalogLimits) -> BTreeMap<ArtifactName, NameSuggestions> {
            let gone = self.gone().await;
            assert!(!gone.is_empty());
            let docs = self.space().docs().await;
            suggest(lookup_of(docs, self.root(), limits).await, limits, &gone)
                .await
                .unwrap()
        }
    }

    /// The lookup of the docs at `docs` of the project at `root`, as the adapter makes
    /// one, or why there is none.
    async fn lookup_of(
        docs: Result<PathBuf, String>,
        root: &Path,
        limits: &CatalogLimits,
    ) -> Result<Lookup, String> {
        match docs {
            Ok(docs) => Ok(Lookup::new(root, docs, limits.page_bytes).await),
            Err(why) => Err(why),
        }
    }

    fn limits() -> CatalogLimits {
        CatalogLimits::default()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn mv(from: &str, to: &str) -> PrefixMove {
        PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
    }

    /// The candidates of a name: their names and how each was found.
    fn found(
        suggested: &BTreeMap<ArtifactName, NameSuggestions>,
        of: &str,
    ) -> Vec<(String, FoundBy)> {
        suggested[&name(of)]
            .candidates
            .iter()
            .map(|candidate| (candidate.name().as_str().to_owned(), candidate.found()))
            .collect()
    }

    fn kinds(suggested: &BTreeMap<ArtifactName, NameSuggestions>, of: &str) -> Vec<NoteKind> {
        suggested[&name(of)]
            .notes
            .iter()
            .map(MoveNote::kind)
            .collect()
    }

    /// The docs of a crate `demo` with a module `old` of two items, approved under
    /// `preset`.
    async fn old_module(preset: &str) -> World {
        let world = World::new(preset, &["demo::old::Foo", "demo::old::bar"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item("demo/old/struct.Foo.html", "pub struct Foo;", "A foo.", &[])
            .await;
        world
            .item("demo/old/fn.bar.html", "pub fn bar()", "A bar.", &[])
            .await;
        world.approve().await;
        world
    }

    /// Every candidate is a name the lookup of this adapter says is there.
    async fn assert_there(world: &World, suggested: &BTreeMap<ArtifactName, NameSuggestions>) {
        let mut asks = SpaceAsks::new();
        for one in suggested.values() {
            for candidate in &one.candidates {
                assert!(candidate.present());
                asks.insert(
                    candidate.name().clone(),
                    BTreeSet::from([Comparison::Presence]),
                );
            }
        }
        assert!(!asks.is_empty());
        let readings = world.space().resolve(&asks).await.unwrap();
        for candidate in asks.keys() {
            assert!(readings[candidate].is_present(), "{candidate}");
        }
    }

    #[tokio::test]
    async fn a_module_that_became_another_has_the_names_that_end_as_its_items_do() {
        let world = old_module("rustdoc").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world
            .item("demo/new/struct.Foo.html", "pub struct Foo;", "A foo.", &[])
            .await;
        world
            .item("demo/new/fn.bar.html", "pub fn bar()", "A bar.", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        // Presence only: nothing but the form of the name says it is the same thing.
        assert_eq!(
            found(&suggested, "demo::old::Foo"),
            [("demo::new::Foo".to_owned(), FoundBy::SameEnd)]
        );
        assert_eq!(
            found(&suggested, "demo::old::bar"),
            [("demo::new::bar".to_owned(), FoundBy::SameEnd)]
        );
        for (of, to) in [
            ("demo::old::Foo", "demo::new::Foo"),
            ("demo::old::bar", "demo::new::bar"),
        ] {
            assert_eq!(kinds(&suggested, of), []);
            // The module, then the name itself.
            assert_eq!(
                suggested[&name(of)].candidates[0].prefixes(),
                [mv("demo::old", "demo::new"), mv(of, to)]
            );
        }
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn a_module_moved_under_another_parent_has_the_move_of_the_parent_of_the_module_and_of_each_name()
     {
        let world = World::new(
            "rustdoc",
            &["demo::app::x", "demo::app::x::Foo", "demo::app::x::bar"],
        )
        .await;
        for dir in ["demo", "demo/app", "demo/app/x"] {
            world.module(dir).await;
        }
        world
            .item("demo/app/x/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        world
            .item("demo/app/x/fn.bar.html", "pub fn bar()", "", &[])
            .await;
        world.approve().await;
        // The module `x` is now under `domain` and keeps its name.
        world.remove("demo/app/x").await;
        for dir in ["demo/domain", "demo/domain/x"] {
            world.module(dir).await;
        }
        world
            .item("demo/domain/x/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        world
            .item("demo/domain/x/fn.bar.html", "pub fn bar()", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        for (of, to) in [
            ("demo::app::x::Foo", "demo::domain::x::Foo"),
            ("demo::app::x::bar", "demo::domain::x::bar"),
        ] {
            assert_eq!(found(&suggested, of), [(to.to_owned(), FoundBy::SameEnd)]);
            // The widest first: the parent, the module with it, then the name itself.
            assert_eq!(
                suggested[&name(of)].candidates[0].prefixes(),
                [
                    mv("demo::app", "demo::domain"),
                    mv("demo::app::x", "demo::domain::x"),
                    mv(of, to)
                ]
            );
        }
        // The module itself has the move of the parent and the move of its whole name,
        // which is the move of the module the names in it have.
        assert_eq!(
            found(&suggested, "demo::app::x"),
            [("demo::domain::x".to_owned(), FoundBy::SameEnd)]
        );
        assert_eq!(
            suggested[&name("demo::app::x")].candidates[0].prefixes(),
            [
                mv("demo::app", "demo::domain"),
                mv("demo::app::x", "demo::domain::x")
            ]
        );
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn a_candidate_of_the_content_pinned_is_the_same_content_and_the_others_the_end_only() {
        let world = old_module("rustdoc-promise").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world.module("demo/other").await;
        // `new::Foo` promises what `old::Foo` did; `other::Foo` does not, and nor does
        // `new::bar`: its docs changed.
        world
            .item("demo/new/struct.Foo.html", "pub struct Foo;", "A foo.", &[])
            .await;
        world
            .item(
                "demo/other/struct.Foo.html",
                "pub struct Foo(u8);",
                "A foo.",
                &[],
            )
            .await;
        world
            .item(
                "demo/new/fn.bar.html",
                "pub fn bar()",
                "A bar, changed.",
                &[],
            )
            .await;
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "demo::old::Foo"),
            [
                ("demo::new::Foo".to_owned(), FoundBy::SameContent),
                ("demo::other::Foo".to_owned(), FoundBy::SameEnd),
            ]
        );
        assert_eq!(
            found(&suggested, "demo::old::bar"),
            [("demo::new::bar".to_owned(), FoundBy::SameEnd)]
        );
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn only_the_names_that_end_as_the_gone_name_the_most_in_their_crate_are_candidates() {
        let world = World::new("rustdoc", &["demo::a::b::Foo"]).await;
        world.module("demo").await;
        world.module("demo/a").await;
        world.module("demo/a/b").await;
        world
            .item("demo/a/b/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        world.approve().await;
        world.remove("demo/a").await;
        for dir in ["x", "z"] {
            world.module(&format!("demo/{dir}")).await;
        }
        for dir in ["x/b", "z/b", "y"] {
            world.module(&format!("demo/{dir}")).await;
            world
                .item(
                    &format!("demo/{dir}/struct.Foo.html"),
                    "pub struct Foo;",
                    "",
                    &[],
                )
                .await;
        }
        // Another crate has the one name, which ends alike in one segment: the best of
        // its crate, so it is kept where `demo::y::Foo` (the same count) is not.
        world.module("lib").await;
        world.module("lib/y").await;
        world
            .item("lib/y/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        // In `demo`, `y::Foo` ends alike in one segment, the other two in two.
        let candidates = found(&suggested, "demo::a::b::Foo");
        assert_eq!(
            candidates,
            [
                ("demo::x::b::Foo".to_owned(), FoundBy::SameEnd),
                ("demo::z::b::Foo".to_owned(), FoundBy::SameEnd),
                ("lib::y::Foo".to_owned(), FoundBy::SameEnd),
            ]
        );
        assert_eq!(
            suggested[&name("demo::a::b::Foo")].candidates[0].prefixes(),
            [
                mv("demo::a", "demo::x"),
                mv("demo::a::b", "demo::x::b"),
                mv("demo::a::b::Foo", "demo::x::b::Foo")
            ]
        );
    }

    #[tokio::test]
    async fn a_name_of_the_crate_of_the_gone_one_is_kept_and_found_by_content_beside_a_dependency_that_ends_alike_in_more()
     {
        let world = World::new("rustdoc-promise", &["myapp::http::Error"]).await;
        world.module("myapp").await;
        world.module("myapp/http").await;
        world
            .item(
                "myapp/http/struct.Error.html",
                "pub struct Error;",
                "An error.",
                &[],
            )
            .await;
        world.approve().await;
        world.remove("myapp/http").await;
        // The same content, one segment alike in the crate of the gone name; and in a
        // dependency another content, two segments alike (`http::Error`).
        world.module("myapp/net").await;
        world
            .item(
                "myapp/net/struct.Error.html",
                "pub struct Error;",
                "An error.",
                &[],
            )
            .await;
        world.module("http").await;
        world
            .item(
                "http/struct.Error.html",
                "pub struct Error(u8);",
                "An error.",
                &[],
            )
            .await;
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "myapp::http::Error"),
            [
                ("myapp::net::Error".to_owned(), FoundBy::SameContent),
                ("http::Error".to_owned(), FoundBy::SameEnd),
            ]
        );
        let candidates = &suggested[&name("myapp::http::Error")].candidates;
        assert_eq!(
            candidates[0].prefixes(),
            [
                mv("myapp::http", "myapp::net"),
                mv("myapp::http::Error", "myapp::net::Error")
            ]
        );
        // `http::Error` is what the gone name ends with: the move is the module `http`
        // of the crate, which is the crate `http` now, and the whole name.
        assert_eq!(
            candidates[1].prefixes(),
            [
                mv("myapp::http", "http"),
                mv("myapp::http::Error", "http::Error")
            ]
        );
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn a_name_nothing_ends_as_has_no_candidate_and_no_note() {
        let world = old_module("rustdoc").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world
            .item("demo/new/struct.Baz.html", "pub struct Baz;", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        for of in ["demo::old::Foo", "demo::old::bar"] {
            assert_eq!(found(&suggested, of), []);
            assert_eq!(kinds(&suggested, of), []);
        }
    }

    #[tokio::test]
    async fn a_name_that_ends_with_all_of_the_gone_one_is_a_candidate_with_the_moves_before_its_end_and_the_whole()
     {
        let world = World::new("rustdoc", &["demo::old::Foo"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item("demo/old/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        // The crate `other` has a module `demo` with `old::Foo` in it.
        for dir in ["other", "other/demo", "other/demo/old"] {
            world.module(dir).await;
        }
        world
            .item("other/demo/old/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        let candidates = &suggested[&name("demo::old::Foo")].candidates;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name().as_str(), "other::demo::old::Foo");
        // The split of all the shared end has no segment on the gone side and is none;
        // the next two and the whole name are moves, the widest first.
        assert_eq!(
            candidates[0].prefixes(),
            [
                mv("demo", "other::demo"),
                mv("demo::old", "other::demo::old"),
                mv("demo::old::Foo", "other::demo::old::Foo")
            ]
        );
    }

    #[tokio::test]
    async fn the_promise_of_a_member_is_read_from_the_page_of_its_type_when_the_bound_is_reached() {
        let world = World::new("rustdoc-promise", &["demo::old::Shelf::put"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item(
                "demo/old/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world
            .item(
                "demo/new/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        // The index of the crate and the page of the type use the bound up; the promise
        // of the member is on the page of its type, read already.
        let pages = CatalogLimits {
            pages: 2,
            ..limits()
        };
        let suggested = world.suggest(&pages).await;
        assert_eq!(
            found(&suggested, "demo::old::Shelf::put"),
            [("demo::new::Shelf::put".to_owned(), FoundBy::SameContent)]
        );
    }

    #[tokio::test]
    async fn a_member_is_a_candidate_where_the_page_of_a_type_that_ends_alike_has_its_anchor() {
        let world = World::new("rustdoc-promise", &["demo::old::Shelf::put"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item(
                "demo/old/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        for (dir, methods) in [
            // The same method, then another's signature, then no such method.
            ("new", vec![("put", PUT)]),
            ("third", vec![("put", "pub fn put(&amp;self)")]),
            ("other", vec![("take", "pub fn take(&amp;self)")]),
        ] {
            world.module(&format!("demo/{dir}")).await;
            world
                .item(
                    &format!("demo/{dir}/struct.Shelf.html"),
                    "pub struct Shelf;",
                    "",
                    &methods,
                )
                .await;
        }
        // A function of the name of the member ends alike in one segment only: the
        // members end alike in two.
        world.module("demo/util").await;
        world
            .item("demo/util/fn.put.html", "pub fn put()", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "demo::old::Shelf::put"),
            [
                ("demo::new::Shelf::put".to_owned(), FoundBy::SameContent),
                ("demo::third::Shelf::put".to_owned(), FoundBy::SameEnd),
            ]
        );
        // The two end segments they share, the member and its type, are the two splits
        // before the whole name.
        assert_eq!(
            suggested[&name("demo::old::Shelf::put")].candidates[0].prefixes(),
            [
                mv("demo::old", "demo::new"),
                mv("demo::old::Shelf", "demo::new::Shelf"),
                mv("demo::old::Shelf::put", "demo::new::Shelf::put")
            ]
        );
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn a_member_is_a_candidate_where_its_type_ends_alike_in_fewer_segments_than_a_type_of_another_crate()
     {
        let world = World::new("rustdoc-promise", &["myapp::old::Shelf::put"]).await;
        world.module("myapp").await;
        world.module("myapp/old").await;
        world
            .item(
                "myapp/old/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.approve().await;
        world.remove("myapp/old").await;
        // The type of the crate of the gone name ends alike in one segment (`Shelf`) and
        // has the same member; the type of another crate, `old::Shelf`, in two and has
        // the member with another signature.
        world.module("myapp/new").await;
        world
            .item(
                "myapp/new/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.module("old").await;
        world
            .item(
                "old/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", "pub fn put(&amp;self)")],
            )
            .await;
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "myapp::old::Shelf::put"),
            [
                ("myapp::new::Shelf::put".to_owned(), FoundBy::SameContent),
                ("old::Shelf::put".to_owned(), FoundBy::SameEnd),
            ]
        );
        assert_there(&world, &suggested).await;
    }

    #[tokio::test]
    async fn a_member_no_type_has_the_anchor_of_has_the_names_that_end_as_it_does_as_candidates() {
        let world = World::new("rustdoc-promise", &["demo::old::Shelf::put"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item(
                "demo/old/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        // A type that ends alike but has no such member, and a function of the name of
        // the member: only the second ends as the gone name does.
        world.module("demo/new").await;
        world
            .item(
                "demo/new/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("take", "pub fn take(&amp;self)")],
            )
            .await;
        world.module("demo/util").await;
        world
            .item("demo/util/fn.put.html", "pub fn put()", "", &[])
            .await;
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "demo::old::Shelf::put"),
            [("demo::util::put".to_owned(), FoundBy::SameEnd)]
        );
    }

    #[tokio::test]
    async fn a_type_that_has_the_anchor_is_not_lost_to_one_that_ends_alike_in_more_segments_and_has_none()
     {
        let world = World::new("rustdoc", &["demo::a::b::Shelf::put"]).await;
        for dir in ["demo", "demo/a", "demo/a/b"] {
            world.module(dir).await;
        }
        world
            .item(
                "demo/a/b/struct.Shelf.html",
                "pub struct Shelf;",
                "",
                &[("put", PUT)],
            )
            .await;
        world.approve().await;
        world.remove("demo/a").await;
        // In `demo`: `x::b::Shelf` ends alike in two segments and has no `put`; `y::Shelf`
        // and `z::Shelf` end alike in one and have it. In `lib`: `q::b::Shelf` ends alike
        // in two and has it, `Shelf` in one and has it too: not the best of its crate.
        let take = [("take", "pub fn take(&amp;self)")];
        let put = [("put", PUT)];
        for (dir, methods) in [
            ("demo/x/b", take),
            ("demo/y", put),
            ("demo/z", put),
            ("lib/q/b", put),
            ("lib", put),
        ] {
            let mut at = String::new();
            for part in dir.split('/') {
                at.push_str(part);
                world.module(&at).await;
                at.push('/');
            }
            world
                .item(
                    &format!("{dir}/struct.Shelf.html"),
                    "pub struct Shelf;",
                    "",
                    &methods,
                )
                .await;
        }
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "demo::a::b::Shelf::put"),
            [
                ("demo::y::Shelf::put".to_owned(), FoundBy::SameEnd),
                ("demo::z::Shelf::put".to_owned(), FoundBy::SameEnd),
                ("lib::q::b::Shelf::put".to_owned(), FoundBy::SameEnd),
            ]
        );
        assert_there(&world, &suggested).await;
        // The pages of the types are read within the bound of pages: the indexes of the
        // two crates and the page of the first type are all it reads, and it says so.
        let pages = CatalogLimits {
            pages: 3,
            ..limits()
        };
        let suggested = world.suggest(&pages).await;
        assert_eq!(found(&suggested, "demo::a::b::Shelf::put"), []);
        assert_eq!(
            kinds(&suggested, "demo::a::b::Shelf::put"),
            [NoteKind::ScanTruncated]
        );
    }

    #[tokio::test]
    async fn the_promises_of_the_crate_of_the_gone_name_are_read_before_those_of_the_others() {
        let world = World::new("rustdoc-promise", &["myapp::http::Error"]).await;
        world.module("myapp").await;
        world.module("myapp/http").await;
        world
            .item(
                "myapp/http/struct.Error.html",
                "pub struct Error;",
                "An error.",
                &[],
            )
            .await;
        world.approve().await;
        world.remove("myapp/http").await;
        // The same content in two crates, a dependency that sorts before the crate of the
        // gone name and that one: each ends alike in one segment.
        for krate in ["aaa", "myapp"] {
            world.module(krate).await;
            world.module(&format!("{krate}/net")).await;
            world
                .item(
                    &format!("{krate}/net/struct.Error.html"),
                    "pub struct Error;",
                    "An error.",
                    &[],
                )
                .await;
        }
        // The two crate indexes are two pages read, and the bound leaves one more: the
        // page of the crate of the gone name is the one read.
        let pages = CatalogLimits {
            pages: 3,
            ..limits()
        };
        let suggested = world.suggest(&pages).await;
        assert_eq!(
            found(&suggested, "myapp::http::Error"),
            [
                ("myapp::net::Error".to_owned(), FoundBy::SameContent),
                ("aaa::net::Error".to_owned(), FoundBy::SameEnd),
            ]
        );
        assert_eq!(
            kinds(&suggested, "myapp::http::Error"),
            [NoteKind::ScanTruncated]
        );
        // With the pages to read both, both are of the same content, the crate of the
        // gone name first.
        let suggested = world.suggest(&limits()).await;
        assert_eq!(
            found(&suggested, "myapp::http::Error"),
            [
                ("myapp::net::Error".to_owned(), FoundBy::SameContent),
                ("aaa::net::Error".to_owned(), FoundBy::SameContent),
            ]
        );
    }

    #[tokio::test]
    async fn a_gone_name_keeps_sixteen_candidates_and_the_same_content_is_never_the_one_cut() {
        let world = World::new("rustdoc-promise", &["demo::old::Thing"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item(
                "demo/old/struct.Thing.html",
                "pub struct Thing;",
                "A thing.",
                &[],
            )
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        // Seventeen places for the name; the last by name is the one of the content
        // that was pinned.
        for at in 1..=17 {
            let dir = format!("demo/m{at:02}");
            world.module(&dir).await;
            let decl = if at == 17 {
                "pub struct Thing;"
            } else {
                "pub struct Thing(u8);"
            };
            world
                .item(&format!("{dir}/struct.Thing.html"), decl, "A thing.", &[])
                .await;
        }
        let suggested = world.suggest(&limits()).await;
        let candidates = found(&suggested, "demo::old::Thing");
        assert_eq!(candidates.len(), 16);
        assert_eq!(
            candidates[0],
            ("demo::m17::Thing".to_owned(), FoundBy::SameContent)
        );
        let rest: Vec<&str> = candidates[1..].iter().map(|(n, _)| n.as_str()).collect();
        let expected: Vec<String> = (1..=15)
            .map(|at| format!("demo::m{at:02}::Thing"))
            .collect();
        assert_eq!(rest, expected);
        assert!(
            candidates[1..]
                .iter()
                .all(|(_, by)| *by == FoundBy::SameEnd)
        );
        assert_eq!(
            suggested[&name("demo::old::Thing")].notes,
            [MoveNote::CandidatesCut { found: 17 }]
        );
    }

    #[tokio::test]
    async fn among_candidates_of_one_rank_the_crate_of_the_gone_name_comes_first() {
        let world = World::new("rustdoc", &["demo::old::Foo"]).await;
        world.module("demo").await;
        world.module("demo/old").await;
        world
            .item("demo/old/struct.Foo.html", "pub struct Foo;", "", &[])
            .await;
        world.approve().await;
        world.remove("demo/old").await;
        for krate in ["aaa", "demo"] {
            world.module(krate).await;
            world.module(&format!("{krate}/m")).await;
            world
                .item(
                    &format!("{krate}/m/struct.Foo.html"),
                    "pub struct Foo;",
                    "",
                    &[],
                )
                .await;
        }
        let suggested = world.suggest(&limits()).await;
        let names: Vec<String> = found(&suggested, "demo::old::Foo")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["demo::m::Foo", "aaa::m::Foo"]);
    }

    #[tokio::test]
    async fn where_the_docs_are_unknown_each_name_has_the_note_and_no_candidate() {
        let world = old_module("rustdoc").await;
        world.remove("demo/old").await;
        let gone = world.gone().await;
        assert_eq!(gone.len(), 2);
        // The place the adapter could not tell, and a place there is nothing at.
        let nowhere = world.root().join("nowhere/doc");
        for docs in [
            Err("where the docs are built is not known".to_owned()),
            Ok(nowhere),
        ] {
            let lookup = lookup_of(docs, world.root(), &limits()).await;
            let said = suggest(lookup, &limits(), &gone).await.unwrap();
            assert_eq!(said.len(), 2);
            for one in said.values() {
                assert!(one.candidates.is_empty());
                assert_eq!(one.notes.len(), 1);
                assert_eq!(one.notes[0].kind(), NoteKind::SearchUnavailable);
            }
        }
        // As the adapter is asked, through the trait the dispatch calls.
        let nothing = tempfile::tempdir().unwrap();
        let space = RustItemSpace::new(nothing.path().to_path_buf(), None);
        let said = SpaceSuggest::suggest(&space, &gone).await.unwrap();
        assert!(said.values().all(|one| one.notes.len() == 1
            && matches!(&one.notes[0], MoveNote::SearchUnavailable { detail }
                        if detail.as_str().contains("is not known"))));
        // And with docs, the same trait answers every name.
        let said = SpaceSuggest::suggest(&world.space(), &gone).await.unwrap();
        assert_eq!(said.len(), 2);
    }

    #[tokio::test]
    async fn a_page_that_is_too_large_is_counted_and_its_candidate_is_the_end_only() {
        let world = old_module("rustdoc-promise").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        // The same content as pinned, on a page past the bound of a page.
        world
            .item(
                "demo/new/struct.Foo.html",
                "pub struct Foo;",
                &format!("A foo.{}", " ".repeat(5_000)),
                &[],
            )
            .await;
        world
            .item("demo/new/fn.bar.html", "pub fn bar()", "A bar.", &[])
            .await;
        let bound = CatalogLimits {
            page_bytes: 4_000,
            ..limits()
        };
        let suggested = world.suggest(&bound).await;
        assert_eq!(
            found(&suggested, "demo::old::Foo"),
            [("demo::new::Foo".to_owned(), FoundBy::SameEnd)]
        );
        assert_eq!(
            found(&suggested, "demo::old::bar"),
            [("demo::new::bar".to_owned(), FoundBy::SameContent)]
        );
        assert_eq!(
            suggested[&name("demo::old::Foo")].notes,
            [MoveNote::TooLargeSkipped {
                count: 1,
                limit: 4_000
            }]
        );
    }

    #[tokio::test]
    async fn a_search_at_its_bound_says_so_and_the_candidates_found_stand() {
        let world = old_module("rustdoc-promise").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world
            .item("demo/new/struct.Foo.html", "pub struct Foo;", "A foo.", &[])
            .await;
        world
            .item("demo/new/fn.bar.html", "pub fn bar()", "A bar.", &[])
            .await;
        // The index of the crate is the one page read: no page is read for content.
        let pages = CatalogLimits {
            pages: 1,
            ..limits()
        };
        let suggested = world.suggest(&pages).await;
        assert_eq!(
            found(&suggested, "demo::old::Foo"),
            [("demo::new::Foo".to_owned(), FoundBy::SameEnd)]
        );
        assert_eq!(
            kinds(&suggested, "demo::old::Foo"),
            [NoteKind::ScanTruncated]
        );
        // The names of the catalog stop at their bound, and the note says it too.
        let names = CatalogLimits {
            names: 3,
            ..limits()
        };
        let suggested = world.suggest(&names).await;
        assert_eq!(
            kinds(&suggested, "demo::old::bar"),
            [NoteKind::ScanTruncated]
        );
        let none = CatalogLimits {
            bytes: 1,
            ..limits()
        };
        let suggested = world.suggest(&none).await;
        assert_eq!(
            kinds(&suggested, "demo::old::bar"),
            [NoteKind::ScanTruncated]
        );
        // Within the bounds, nothing is said.
        let suggested = world.suggest(&limits()).await;
        assert_eq!(kinds(&suggested, "demo::old::bar"), []);
        assert_eq!(
            found(&suggested, "demo::old::bar"),
            [("demo::new::bar".to_owned(), FoundBy::SameContent)]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_page_that_cannot_be_read_is_counted_and_its_candidate_is_the_end_only() {
        use std::os::unix::fs::PermissionsExt;
        let world = old_module("rustdoc-promise").await;
        world.remove("demo/old").await;
        world.module("demo/new").await;
        world
            .item("demo/new/struct.Foo.html", "pub struct Foo;", "A foo.", &[])
            .await;
        world
            .item("demo/new/fn.bar.html", "pub fn bar()", "A bar.", &[])
            .await;
        let locked = world.build().join("doc/demo/new/struct.Foo.html");
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // Run as a user every file opens to (root), there is nothing to test.
        let opens = tokio::fs::File::open(&locked).await.is_ok();
        let suggested = world.suggest(&limits()).await;
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644))
            .await
            .unwrap();
        if opens {
            return;
        }
        assert_eq!(
            found(&suggested, "demo::old::Foo"),
            [("demo::new::Foo".to_owned(), FoundBy::SameEnd)]
        );
        assert_eq!(
            suggested[&name("demo::old::Foo")].notes,
            [MoveNote::UnreadableSkipped { count: 1 }]
        );
    }
}
