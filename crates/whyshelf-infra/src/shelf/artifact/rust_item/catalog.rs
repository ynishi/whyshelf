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
//! the [`Lookup`] that reads it. This module depends on `lookup`, `layout`, `path` and
//! `docs_path` and on nothing that depends on it; it reads the docs through the
//! [`Lookup`] alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::MAX_PAGE_BYTES;
use super::docs_path::DocsPath;
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
        let Some(entries) = lookup.listing(&DocsPath::top()).await.cloned() else {
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
            let Some(dir) = page.parent() else {
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
                    let Some(file) = dir.join(entry) else {
                        continue;
                    };
                    if lookup.outside(&file).await.is_some() {
                        continue;
                    }
                    // `find` reads an entry of the item's name as a module first, and
                    // cannot tell the name when that entry leads outside.
                    if entries.contains(item.name)
                        && let Some(module) = dir.join(item.name)
                        && lookup.outside(&module).await.is_some()
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
                let Some(child) = dir.join(entry) else {
                    continue;
                };
                if lookup.outside(&child).await.is_some() {
                    continue;
                }
                match lookup.listed_as_dir(&child).await {
                    Some(true) => {}
                    Some(false) => continue,
                    None => {
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
                    page: child.join("index.html").unwrap_or_else(|| child.clone()),
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
#[path = "catalog_tests.rs"]
mod tests;
