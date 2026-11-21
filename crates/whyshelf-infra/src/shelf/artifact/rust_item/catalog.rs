//! The names there are now in the docs: the modules and the items of the crates
//! rustdoc wrote, by their path, for the search for where a gone name moved.
//!
//! | what | what it does |
//! |---|---|
//! | [`Catalog::walk`] | the one reading of the docs for the names there are: each crate with the mark of rustdoc, each module of it, each item page, within the bounds of [`CatalogLimits`] |
//! | [`CatalogLimits`] | the bounds of one walk, and of the reading of the pages a search makes after it |
//!
//! ```text
//!   the docs directory
//!     └─ each directory that has an index.html with the mark of rustdoc   (a crate; the
//!          │                                                               crates of the
//!          │                                                               gone names first)
//!          └─ each module: a directory with an index.html, not one that leaves the places read
//!               ├─ each `<kind>.<Name>.html` of the kinds of items ──► the path of the item
//!               └─ each module directory ──► walked in its turn, breadth first
//! ```
//!
//! A name is in the catalog when a path in the docs gives it: a module is a directory
//! with an `index.html` (a private module has none: rustdoc writes only pages that
//! redirect, and the directory is not walked), an item is a page `<kind>.<Name>.html`
//! of a kind rustdoc writes ([`ITEM_KINDS`]). The anchor of a
//! re-export in the index of a module is no name of the catalog, nor is a page that
//! redirects, nor a path that leads outside the places read once symbolic links are
//! resolved. Every name of the catalog is one [`Lookup::find`] answers as there for, at
//! the place the catalog holds, because the catalog reads each thing by the rules
//! `find` does (the order of kinds, the modules before the items, what is outside); a
//! test holds it to that.
//!
//! A bound is a note and no error: a walk that reaches one stops where it is, and says
//! so ([`Catalog::cut`]); a directory that cannot be listed is passed over and counted
//! ([`Catalog::unreadable`]); a page that cannot be read or is too large is counted by
//! the [`Lookup`] that reads it. This module depends on `lookup`, `layout` and `path`
//! and on nothing that depends on it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::MAX_PAGE_BYTES;
use super::layout::{ITEM_KINDS, Place};
use super::lookup::{Found, Lookup, Reads};
use super::path::RustPath;

/// The bounds of one walk of the docs ([`Catalog::walk`]) and of the reading of pages
/// that goes with it: a search that reaches one stops where it is and says so, and is
/// no error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CatalogLimits {
    /// The most names (modules and items) the catalog holds.
    pub(super) names: usize,
    /// The deepest a module is walked into: the crate is at 0, the modules in it at 1.
    pub(super) depth: usize,
    /// The most pages read in all.
    pub(super) pages: usize,
    /// The most bytes of pages read in all.
    pub(super) bytes: u64,
    /// The most bytes of one page; a larger one is not read.
    pub(super) page_bytes: u64,
}

impl Default for CatalogLimits {
    /// 20,000 names, 64 deep, 2,000 pages, 512 MiB in all, and pages of at most
    /// [`MAX_PAGE_BYTES`].
    ///
    /// Why these: the same order as the bounds of the search for the same content of
    /// the space `file` (20,000 files, 512 MiB): the docs of a workspace of many crates
    /// hold some thousands of names, a module nested 64 deep is no layout of rustdoc's
    /// but a loop or a hostile tree, and the pages read after the walk are the ones of
    /// candidates, a few a gone name.
    fn default() -> Self {
        Self {
            names: 20_000,
            depth: 64,
            pages: 2_000,
            bytes: 512 * 1024 * 1024,
            page_bytes: MAX_PAGE_BYTES,
        }
    }
}

impl CatalogLimits {
    /// Whether the pages read so far reach the bound of pages or of bytes: no page is
    /// read after that.
    pub(super) fn spent(&self, reads: Reads) -> bool {
        reads.pages >= self.pages || reads.bytes >= self.bytes
    }
}

/// The modules and the items there are now in the docs, by their path, each with the
/// place it is told of.
#[derive(Debug)]
pub(super) struct Catalog {
    names: BTreeMap<RustPath, Place>,
    cut: bool,
    unreadable: usize,
    listed: bool,
}

impl Catalog {
    /// The names of the docs of `lookup`, the crates of `first` before the others, each
    /// crate walked whole before the next, each breadth first (so that a walk that is
    /// cut has the names nearest a crate): the one place the docs are listed for names.
    pub(super) async fn walk(
        lookup: &mut Lookup,
        first: &[String],
        limits: &CatalogLimits,
    ) -> Self {
        let mut catalog = Self {
            names: BTreeMap::new(),
            cut: false,
            unreadable: 0,
            listed: true,
        };
        let docs = lookup.docs().to_path_buf();
        let Some(entries) = lookup.listing(&docs).await.cloned() else {
            catalog.listed = false;
            return catalog;
        };
        let wanted: BTreeSet<&String> = first.iter().collect();
        let mut crates: Vec<&String> = Vec::new();
        for krate in first {
            if entries.contains(krate) && !crates.contains(&krate) {
                crates.push(krate);
            }
        }
        crates.extend(entries.iter().filter(|krate| !wanted.contains(krate)));
        for krate in crates {
            if catalog.cut {
                break;
            }
            if limits.spent(lookup.reads()) {
                catalog.cut = true;
                break;
            }
            let Some(path) = RustPath::of(std::slice::from_ref(krate)) else {
                continue;
            };
            // A crate is a directory with an index rustdoc wrote: any other (`src`,
            // `implementors`, a crate of docs that are not rustdoc's) is no crate.
            let Found::There(place) = lookup.find(&path).await else {
                continue;
            };
            if !catalog.insert(path.clone(), place, limits) {
                break;
            }
            catalog.module(lookup, path, limits).await;
        }
        catalog
    }

    /// The names, each with the place it is told of.
    pub(super) fn names(&self) -> &BTreeMap<RustPath, Place> {
        &self.names
    }

    /// Whether the walk stopped at a bound of [`CatalogLimits`]: there are names it
    /// did not reach.
    pub(super) fn cut(&self) -> bool {
        self.cut
    }

    /// How many directories could not be listed, and were passed over.
    pub(super) fn unreadable(&self) -> usize {
        self.unreadable
    }

    /// Whether the docs directory itself could be listed: when not, the docs are not
    /// built (or cannot be read), and there is nothing to walk.
    pub(super) fn listed(&self) -> bool {
        self.listed
    }

    /// Adds a name, a module over an item of the same path (as `find` reads a module
    /// first); false, with the walk cut, when it would be one name more than the bound.
    fn insert(&mut self, path: RustPath, place: Place, limits: &CatalogLimits) -> bool {
        match self.names.get(&path) {
            Some(Place::Module { .. }) => return true,
            Some(_) => {}
            None if self.names.len() >= limits.names => {
                self.cut = true;
                return false;
            }
            None => {}
        }
        self.names.insert(path, place);
        true
    }

    /// The modules and the items under the crate `root`, breadth first.
    async fn module(&mut self, lookup: &mut Lookup, root: RustPath, limits: &CatalogLimits) {
        let mut queue = VecDeque::from([(root, 0_usize)]);
        while let Some((module, depth)) = queue.pop_front() {
            let Some(Place::Module { page }) = self.names.get(&module).cloned() else {
                continue;
            };
            let Some(dir) = page.parent().map(std::path::Path::to_path_buf) else {
                continue;
            };
            let Some(entries) = lookup.listing(&dir).await.cloned() else {
                self.unreadable += 1;
                continue;
            };
            for entry in &entries {
                if entry == "index.html" {
                    continue;
                }
                if let Some(item) = item_page(&entries, entry) {
                    let Some(path) = module.with(item.name) else {
                        continue;
                    };
                    let file = dir.join(entry);
                    if lookup.outside(&file).await.is_some() {
                        continue;
                    }
                    // `find` reads an entry of the item's name as a module first, and
                    // cannot tell the name when that entry leads outside.
                    if entries.contains(item.name)
                        && lookup.outside(&dir.join(item.name)).await.is_some()
                    {
                        continue;
                    }
                    let place = Place::Item {
                        page: file,
                        kind: item.kind.to_owned(),
                    };
                    if !self.insert(path, place, limits) {
                        return;
                    }
                    continue;
                }
                // A module: a directory with an index, in the places read.
                let Some(path) = module.with(entry) else {
                    continue;
                };
                let child = dir.join(entry);
                if lookup.outside(&child).await.is_some() {
                    continue;
                }
                match tokio::fs::metadata(&child).await {
                    // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
                    Ok(meta) if meta.is_dir() => {}
                    Ok(_) => continue,
                    Err(_) => {
                        self.unreadable += 1;
                        continue;
                    }
                }
                let Some(inside) = lookup.listing(&child).await.cloned() else {
                    self.unreadable += 1;
                    continue;
                };
                if !inside.contains("index.html") {
                    continue;
                }
                let place = Place::Module {
                    page: child.join("index.html"),
                };
                if !self.insert(path.clone(), place, limits) {
                    return;
                }
                if depth < limits.depth {
                    queue.push_back((path, depth + 1));
                } else {
                    self.cut = true;
                }
            }
        }
    }
}

/// An entry of a module's directory that is the page of an item.
struct ItemPage<'a> {
    /// The kind of the page, one of [`ITEM_KINDS`].
    kind: &'static str,
    /// The name of the item, as the file says it.
    name: &'a str,
}

/// The item the file `entry` is the page of, when it is the one `find` reads for that
/// name: the first of the kinds in the order of [`ITEM_KINDS`] that has a page of the
/// name in `entries` (a struct and a function of one name are told by that order).
fn item_page<'a>(entries: &BTreeSet<String>, entry: &'a str) -> Option<ItemPage<'a>> {
    let (kind, name) = ITEM_KINDS.iter().find_map(|kind| {
        let name = entry
            .strip_prefix(kind)?
            .strip_prefix('.')?
            .strip_suffix(".html")?;
        Some((*kind, name))
    })?;
    let first = ITEM_KINDS
        .iter()
        .find(|kind| entries.contains(&format!("{kind}.{name}.html")))?;
    (*first == kind).then_some(ItemPage { kind, name })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::fixture::{index, page, redirect, write};
    use super::*;

    const PAGE_BOUND: u64 = 1024 * 1024;

    fn limits() -> CatalogLimits {
        CatalogLimits {
            page_bytes: PAGE_BOUND,
            ..CatalogLimits::default()
        }
    }

    async fn lookup_at(target: &Path) -> Lookup {
        Lookup::new(target, target.join("doc"), PAGE_BOUND).await
    }

    fn path(text: &str) -> RustPath {
        RustPath::parse(text).unwrap()
    }

    fn texts(catalog: &Catalog) -> Vec<String> {
        catalog.names().keys().map(RustPath::as_name).collect()
    }

    /// Docs laid out as rustdoc writes them: the crate `demo` with a module `inner`,
    /// a re-export, a private module whose pages only redirect, and the directories of
    /// the docs that are no crate's; the crate `demo_infra`; the crate `fake`, whose
    /// index is not rustdoc's.
    async fn docs() -> tempfile::TempDir {
        let target = tempfile::tempdir().unwrap();
        let d = target.path();
        write(
            d,
            "doc/demo/index.html",
            &index(&[("Thing", Some("inner/struct.Thing.html"))]),
        )
        .await;
        write(d, "doc/demo/struct.Shelf.html", &page(&["method.build"])).await;
        write(d, "doc/demo/enum.Cause.html", &page(&["variant.Gone"])).await;
        write(d, "doc/demo/fn.expand.html", &page(&[])).await;
        // Two kinds of one name: the first of the order of kinds is the one read.
        write(d, "doc/demo/struct.Twice.html", &page(&[])).await;
        write(d, "doc/demo/fn.Twice.html", &page(&[])).await;
        write(d, "doc/demo/sidebar-items.js", "").await;
        write(d, "doc/demo/all.html", &page(&[])).await;
        write(d, "doc/demo/inner/index.html", &index(&[])).await;
        write(
            d,
            "doc/demo/inner/struct.Thing.html",
            &page(&["method.new"]),
        )
        .await;
        write(d, "doc/demo/inner/fn.helper.html", &page(&[])).await;
        write(d, "doc/demo/inner/deep/index.html", &index(&[])).await;
        write(d, "doc/demo/inner/deep/fn.deeper.html", &page(&[])).await;
        write(
            d,
            "doc/demo/hidden/fn.expand.html",
            &redirect("../../demo/fn.expand.html"),
        )
        .await;
        write(d, "doc/demo_infra/index.html", &index(&[])).await;
        write(d, "doc/demo_infra/fn.open.html", &page(&[])).await;
        write(
            d,
            "doc/fake/index.html",
            "<!DOCTYPE html><html><head></head><body></body></html>",
        )
        .await;
        write(d, "doc/fake/struct.A.html", &page(&[])).await;
        write(d, "doc/src/demo/lib.rs.html", &page(&[])).await;
        write(d, "doc/static.files/main.js", "").await;
        write(d, "doc/trait.impl/demo/trait.X.js", "").await;
        write(d, "doc/help.html", &page(&[])).await;
        target
    }

    #[tokio::test]
    async fn the_catalog_is_the_modules_and_the_items_of_the_crates_rustdoc_wrote() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        assert!(catalog.listed());
        assert!(!catalog.cut());
        assert_eq!(catalog.unreadable(), 0);
        // No re-export (`demo::Thing`), no redirect page (`demo::hidden`), no page that
        // is no item's, no crate whose index is not rustdoc's, no directory of the docs
        // that is no crate.
        assert_eq!(
            texts(&catalog),
            [
                "demo",
                "demo::Cause",
                "demo::Shelf",
                "demo::Twice",
                "demo::expand",
                "demo::inner",
                "demo::inner::Thing",
                "demo::inner::deep",
                "demo::inner::deep::deeper",
                "demo::inner::helper",
                "demo_infra",
                "demo_infra::open",
            ]
        );
        // The first kind in the order of kinds tells `Twice`.
        assert!(matches!(
            catalog.names().get(&path("demo::Twice")),
            Some(Place::Item { kind, .. }) if kind == "struct"
        ));
    }

    #[tokio::test]
    async fn every_name_of_the_catalog_is_one_the_lookup_finds_there_at_its_place() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        assert!(!catalog.names().is_empty());
        for (name, place) in catalog.names() {
            assert_eq!(
                lookup.find(name).await,
                Found::There(place.clone()),
                "{}",
                name.as_name()
            );
        }
    }

    #[tokio::test]
    async fn a_module_is_told_before_an_item_of_its_name() {
        let target = docs().await;
        // `inner` is a module and a function of one name: `find` reads the module.
        write(target.path(), "doc/demo/fn.inner.html", &page(&[])).await;
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        assert!(matches!(
            catalog.names().get(&path("demo::inner")),
            Some(Place::Module { .. })
        ));
        for (name, place) in catalog.names() {
            assert_eq!(lookup.find(name).await, Found::There(place.clone()));
        }
    }

    #[tokio::test]
    async fn the_crates_of_the_gone_names_are_walked_first() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        let bound = CatalogLimits {
            names: 2,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &["demo_infra".to_owned()], &bound).await;
        assert!(catalog.cut());
        assert_eq!(texts(&catalog), ["demo_infra", "demo_infra::open"]);
        // A crate named that the docs do not have is no crate to walk.
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &["gone".to_owned()], &limits()).await;
        assert!(texts(&catalog).contains(&"demo::Shelf".to_owned()));
    }

    #[tokio::test]
    async fn a_walk_past_the_most_names_stops_and_says_so() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        let bound = CatalogLimits {
            names: 3,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
        assert!(catalog.cut());
        assert_eq!(catalog.names().len(), 3);
        // Exactly the bound is not past it.
        let mut lookup = lookup_at(target.path()).await;
        let all = Catalog::walk(&mut lookup, &[], &limits())
            .await
            .names()
            .len();
        let mut lookup = lookup_at(target.path()).await;
        let bound = CatalogLimits {
            names: all,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
        assert!(!catalog.cut());
        assert_eq!(catalog.names().len(), all);
    }

    #[tokio::test]
    async fn a_module_nested_past_the_deepest_is_named_and_not_walked() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        // The crate is at 0, `inner` at 1, `deep` at 2: `deep` is named, and what is
        // in it is not reached.
        let bound = CatalogLimits {
            depth: 1,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
        assert!(catalog.cut());
        let names = texts(&catalog);
        assert!(names.contains(&"demo::inner::deep".to_owned()));
        assert!(!names.contains(&"demo::inner::deep::deeper".to_owned()));
        assert!(names.contains(&"demo::inner::helper".to_owned()));
    }

    #[tokio::test]
    async fn a_walk_that_has_read_the_most_pages_or_bytes_reads_no_more_crates() {
        let target = docs().await;
        let mut lookup = lookup_at(target.path()).await;
        let bound = CatalogLimits {
            pages: 1,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
        assert!(catalog.cut());
        // The first crate was read whole; the next was not read.
        assert!(texts(&catalog).iter().all(|name| name.starts_with("demo")));
        assert!(!texts(&catalog).contains(&"demo_infra".to_owned()));
        assert_eq!(lookup.reads().pages, 1);

        let mut lookup = lookup_at(target.path()).await;
        let bound = CatalogLimits {
            bytes: 1,
            ..limits()
        };
        let catalog = Catalog::walk(&mut lookup, &[], &bound).await;
        assert!(catalog.cut());
        assert!(!texts(&catalog).contains(&"demo_infra".to_owned()));
    }

    #[tokio::test]
    async fn docs_that_are_not_there_are_no_catalog_and_say_so() {
        let empty = tempfile::tempdir().unwrap();
        let mut lookup = lookup_at(empty.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        assert!(!catalog.listed());
        assert!(catalog.names().is_empty());
        assert!(!catalog.cut());
    }

    #[tokio::test]
    async fn a_crate_with_a_page_too_large_is_counted_and_passed_over() {
        let target = docs().await;
        let mut lookup = Lookup::new(target.path(), target.path().join("doc"), 100).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        // The index of each crate rustdoc wrote is past 100 bytes: none is read, and
        // each is counted.
        assert!(catalog.names().is_empty());
        assert_eq!(lookup.reads().too_large, 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_directory_that_cannot_be_listed_is_counted_and_passed_over() {
        use std::os::unix::fs::PermissionsExt;
        let target = docs().await;
        let locked = target.path().join("doc/demo/inner");
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // Run as a user every directory opens to (root), there is nothing to test.
        let opens = tokio::fs::read_dir(&locked).await.is_ok();
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        if opens {
            return;
        }
        assert_eq!(catalog.unreadable(), 1);
        let names = texts(&catalog);
        assert!(names.contains(&"demo::Shelf".to_owned()));
        assert!(!names.contains(&"demo::inner".to_owned()));
        assert!(!names.contains(&"demo::inner::helper".to_owned()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_path_through_a_symbolic_link_out_of_the_docs_is_no_name() {
        let target = docs().await;
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "mod/index.html", &index(&[])).await;
        write(outside.path(), "mod/fn.escaped.html", &page(&[])).await;
        write(outside.path(), "struct.Esc.html", &page(&[])).await;
        tokio::fs::symlink(
            outside.path().join("mod"),
            target.path().join("doc/demo/out"),
        )
        .await
        .unwrap();
        tokio::fs::symlink(
            outside.path().join("struct.Esc.html"),
            target.path().join("doc/demo/struct.Esc.html"),
        )
        .await
        .unwrap();
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        let names = texts(&catalog);
        for escaped in ["demo::out", "demo::out::escaped", "demo::Esc"] {
            assert!(!names.contains(&escaped.to_owned()), "{escaped}");
        }
        assert!(names.contains(&"demo::Shelf".to_owned()));
        for (name, place) in catalog.names() {
            assert_eq!(lookup.find(name).await, Found::There(place.clone()));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_item_whose_name_is_also_an_entry_out_of_the_docs_is_no_name() {
        let target = docs().await;
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "index.html", &index(&[])).await;
        write(target.path(), "doc/demo/fn.foo.html", &page(&[])).await;
        tokio::fs::symlink(outside.path(), target.path().join("doc/demo/foo"))
            .await
            .unwrap();
        let mut lookup = lookup_at(target.path()).await;
        let catalog = Catalog::walk(&mut lookup, &[], &limits()).await;
        let names = texts(&catalog);
        assert!(!names.contains(&"demo::foo".to_owned()));
        assert!(names.contains(&"demo::Shelf".to_owned()));
        for (name, place) in catalog.names() {
            assert_eq!(lookup.find(name).await, Found::There(place.clone()));
        }
    }
}
