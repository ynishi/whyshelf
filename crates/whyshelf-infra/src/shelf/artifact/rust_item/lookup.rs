//! The lookup of a path in the docs: where in the HTML a name is, and what its
//! promise is read from.
//!
//! | what | what it does |
//! |---|---|
//! | `Lookup` | the docs of one side read for one resolution, each directory listed once and each page read once: `find` walks a path to its [`Found`], `promise` reads what the thing found promises, `source` where its page says its source is |
//! | `Reads` | what the pages asked for so far came to: how many were read, their bytes, how many could not be read and how many were too large (what a search for the moves of a name reports and holds to its bounds) |
//! | `reexport_link`, `within` | the link of a re-export in a module's index, and the place a link names when it stays inside the docs as written |
//!
//! The words of the layout are `layout`'s and the grammar of a path is `path`'s; this
//! module is what walks one along the other. It reads the docs through the port of the
//! built docs ([`BuiltDocs`]) and nothing else: whether a place may be read, and
//! whether a re-export leads into the docs, are the port's answers, asked before a
//! place is listed or read.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use whyshelf_core::app::shelf::{
    AtSide, ByteBound, DirListing, Entries, NotADir, StateList, StateRead,
};
use whyshelf_core::domain::shelf::artifact::NotOfSources;

use super::super::on_blocking;
use super::docs_path::DocsPath;
use super::docs_tree::BuiltDocs;
use super::held::HeldDocs;
use super::layout::{GENERATOR_MARK, ITEM_KINDS, MemberKind, Place, TYPE_KINDS};
use super::path::RustPath;
use super::promise::{Promise, Unreadable, promise_of, source_link};

/// What a path comes to in the docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found {
    /// It is there, at this place.
    There(Place),
    /// It is there, and there is no page to read what it promises from, and why (a
    /// re-export into docs that are not here).
    Unread(String),
    Absent,
    CannotTell(String),
    /// The docs of its crate were not held against the sources of the project: a
    /// crate that is none of the project's ([`HeldDocs::crate_of`]).
    NotOfSources(NotOfSources),
}

/// The `index.html` of the directory of a module.
fn index_of(dir: &DocsPath) -> DocsPath {
    dir.join("index.html").unwrap_or_else(|| dir.clone())
}

impl From<At> for Found {
    /// What a walk that ends at `at` comes to: the one place a [`Place`] is built.
    fn from(at: At) -> Self {
        match at {
            At::Module(dir) => Self::There(Place::Module {
                page: index_of(&dir),
            }),
            At::Item { page, kind } => Self::There(Place::Item { page, kind }),
            At::Member { page, kind, anchor } => Self::There(Place::Member { page, kind, anchor }),
            At::Unfollowed(why) => Self::Unread(why),
        }
    }
}

/// Where a walk along a path is.
#[derive(Debug, Clone)]
enum At {
    /// The directory of a module (or of a crate).
    Module(DocsPath),
    /// The page of an item.
    Item { page: DocsPath, kind: String },
    /// A member of a type, found by its anchor on the page of the type: nothing is
    /// under it.
    Member {
        page: DocsPath,
        kind: MemberKind,
        anchor: String,
    },
    /// A re-export whose link cannot be followed, and why: what is under it cannot
    /// be told.
    Unfollowed(String),
}

/// What the pages a [`Lookup`] was asked for came to: each page counted once, when it
/// is first asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Reads {
    /// The pages read.
    pub(super) pages: usize,
    /// Their bytes, in all.
    pub(super) bytes: u64,
    /// The pages that could not be read (one that leads outside the places read is
    /// one of them).
    pub(super) unreadable: usize,
    /// The pages larger than the bound of one page, and not read.
    pub(super) too_large: usize,
}

/// Why a page could not be read.
enum Failure {
    /// It is larger than the bound of one page.
    TooLarge(String),
    /// It cannot be read, for this reason.
    Unreadable(String),
}

impl Failure {
    fn why(self) -> String {
        match self {
            Self::TooLarge(why) | Self::Unreadable(why) => why,
        }
    }
}

/// The docs of one side, read for one resolution: each directory listed once, each
/// page read once. It is made from the mark that the docs were held against the
/// sources ([`HeldDocs`]) and from nothing else, so no page is read of docs that were
/// not.
pub(super) struct Lookup {
    held: HeldDocs,
    max_page: u64,
    listings: BTreeMap<DocsPath, Option<BTreeSet<String>>>,
    pages: BTreeMap<DocsPath, Result<Arc<str>, String>>,
    reads: Reads,
}

impl Lookup {
    /// The docs `held` holds, read at the side it is of, each page of at most
    /// `max_page` bytes.
    pub(super) fn on(held: HeldDocs, max_page: u64) -> Self {
        Self {
            held,
            max_page,
            listings: BTreeMap::new(),
            pages: BTreeMap::new(),
            reads: Reads::default(),
        }
    }

    /// The mark the lookup was made from.
    pub(super) fn held(&self) -> &HeldDocs {
        &self.held
    }

    /// The directory rustdoc wrote the docs in, as a path of the file system.
    pub(super) fn docs_dir(&self) -> PathBuf {
        self.held.docs().dir(self.held.side())
    }

    /// Where `p` is, as a reader names it.
    fn shown(&self, p: &DocsPath) -> String {
        self.held
            .docs()
            .shown(self.held.side(), p)
            .display()
            .to_string()
    }

    /// What the pages asked for so far came to.
    pub(super) fn reads(&self) -> Reads {
        self.reads
    }

    /// Why nothing is read at `p`, when it leads outside the root of the project
    /// and the build directory once symbolic links are resolved, or is a link that
    /// cannot be resolved ([`BuiltDocs::outside`]).
    pub(super) async fn outside(&self, p: &DocsPath) -> Option<String> {
        self.held.docs().outside(self.held.side(), p).await
    }

    /// Whether `p` is a directory, read by listing it (no file is opened): none when
    /// it cannot be looked at. A directory's listing is kept, so that
    /// [`listing`](Self::listing) of it lists it no more.
    pub(super) async fn listed_as_dir(&mut self, p: &DocsPath) -> Option<bool> {
        if let Some(Some(_)) = self.listings.get(p) {
            return Some(true);
        }
        match self
            .held
            .docs()
            .list(self.held.side(), Some(p))
            .await
            .ok()?
        {
            DirListing::All(entries) | DirListing::Cut { entries, .. } => {
                self.listings.insert(p.clone(), Some(names_of(entries)));
                Some(true)
            }
            DirListing::NotADir(NotADir::File | NotADir::Untold(_) | NotADir::Elsewhere { .. }) => {
                Some(false)
            }
            DirListing::NotADir(NotADir::Absent) => None,
        }
    }

    /// The names of the entries of a directory, or none when it cannot be listed.
    pub(super) async fn listing(&mut self, dir: &DocsPath) -> Option<&BTreeSet<String>> {
        if !self.listings.contains_key(dir) {
            let names = match self.held.docs().list(self.held.side(), Some(dir)).await {
                Ok(DirListing::All(entries) | DirListing::Cut { entries, .. }) => {
                    Some(names_of(entries))
                }
                Ok(DirListing::NotADir(_)) | Err(_) => None,
            };
            self.listings.insert(dir.clone(), names);
        }
        self.listings.get(dir).and_then(Option::as_ref)
    }

    /// Whether `name` is an entry of `dir`, as written; none when `dir` cannot be
    /// listed.
    async fn has(&mut self, dir: &DocsPath, name: &str) -> Option<bool> {
        self.listing(dir).await.map(|names| names.contains(name))
    }

    /// The text of a page, or why it cannot be read (one outside the root and the
    /// build directory is not).
    async fn page(&mut self, page: &DocsPath) -> Result<Arc<str>, String> {
        if !self.pages.contains_key(page) {
            let text = match self.outside(page).await {
                Some(why) => {
                    self.reads.unreadable += 1;
                    Err(why)
                }
                None => match self.read(page).await {
                    Ok(text) => {
                        self.reads.pages += 1;
                        self.reads.bytes = self.reads.bytes.saturating_add(len_of(&text));
                        Ok(Arc::from(text))
                    }
                    Err(failure) => {
                        match failure {
                            Failure::TooLarge(_) => self.reads.too_large += 1,
                            Failure::Unreadable(_) => self.reads.unreadable += 1,
                        }
                        Err(failure.why())
                    }
                },
            };
            self.pages.insert(page.clone(), text);
        }
        self.pages
            .get(page)
            .cloned()
            .unwrap_or_else(|| Err(format!("{} cannot be read", self.shown(page))))
    }

    /// The text of a page of at most the bound of a page, or why it cannot be read.
    async fn read(&self, page: &DocsPath) -> Result<String, Failure> {
        let shown = self.shown(page);
        let cannot =
            |why: &dyn Display| Failure::Unreadable(format!("{shown} cannot be read: {why}"));
        let read = self
            .held
            .docs()
            .read(
                self.held.side(),
                &BTreeSet::from([page.clone()]),
                ByteBound::at_most(self.max_page),
            )
            .await;
        let bytes = match read.and_then(|mut read| read.remove(page).transpose()) {
            Ok(Some(AtSide::There(bytes))) => bytes,
            Ok(Some(AtSide::Absent) | None) => {
                return Err(cannot(&io::Error::from(io::ErrorKind::NotFound)));
            }
            Ok(Some(AtSide::Dir)) => {
                return Err(cannot(&io::Error::from(io::ErrorKind::IsADirectory)));
            }
            Ok(Some(AtSide::Untold(why))) => return Err(cannot(&why.as_str())),
            Ok(Some(AtSide::Elsewhere { .. })) => {
                return Err(cannot(&"a symbolic link is not followed"));
            }
            Err(super::super::super::state::StateError::Io { source, .. }) => {
                return Err(cannot(&source));
            }
            Err(other) => return Err(cannot(&other)),
        };
        let max = self.max_page;
        if u64::try_from(bytes.len()).map_or(true, |len| len > max) {
            return Err(Failure::TooLarge(format!(
                "{shown} is larger than {max} bytes, and is not read"
            )));
        }
        // A page may be as large as the bound: a large one is decoded on a blocking
        // thread.
        on_blocking(bytes.len(), move || {
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .await
        .map_err(|why| Failure::Unreadable(stopped(&shown, &why)))
    }

    pub(super) async fn find(&mut self, path: &RustPath) -> Found {
        let top = DocsPath::top();
        let Some(crate_dir) = top.join(&path.krate) else {
            return Found::CannotTell(format!(
                "the crate `{}` is no name of a directory of the docs",
                path.krate
            ));
        };
        let built = match self.has(&top, &path.krate).await {
            Some(true) => {
                // Before it is listed: nothing outside is read.
                if let Some(why) = self.outside(&crate_dir).await {
                    return Found::CannotTell(why);
                }
                self.has(&crate_dir, "index.html").await == Some(true)
            }
            Some(false) | None => false,
        };
        if !built {
            return Found::CannotTell(format!(
                "the docs of the crate `{}` are not built: {} has no index.html (build them with `cargo doc`, or the crate is no longer in the build)",
                path.krate,
                self.shown(&crate_dir)
            ));
        }
        let index = index_of(&crate_dir);
        let html = match self.page(&index).await {
            Ok(html) => html,
            Err(why) => return Found::CannotTell(why),
        };
        match on_blocking(html.len(), move || html.contains(GENERATOR_MARK)).await {
            Ok(true) => {}
            Ok(false) => {
                return Found::CannotTell(format!(
                    "{} is not rustdoc's: it has no {GENERATOR_MARK}",
                    self.shown(&index)
                ));
            }
            Err(why) => return Found::CannotTell(stopped(&self.shown(&index), &why)),
        }
        // The docs of a crate of rustdoc's that were not held (a dependency's).
        if let Err(not) = self.held.crate_of(&path.krate) {
            return Found::NotOfSources(not);
        }
        let mut at = At::Module(crate_dir);
        for segment in &path.rest {
            at = match self.step(at, segment).await {
                Ok(Some(next)) => next,
                Ok(None) => return Found::Absent,
                Err(why) => return Found::CannotTell(why),
            };
        }
        Found::from(at)
    }

    /// Whether the page of `place` was read already (or found unreadable), so that its
    /// promise costs no page more.
    pub(super) fn has_read(&self, place: &Place) -> bool {
        self.pages.contains_key(place.page())
    }

    /// What the thing at `place` promises, read from its page (parsed on a blocking
    /// thread, as a page may be as large as its bound), or why it cannot be.
    pub(super) async fn promise(&mut self, place: &Place) -> Result<Promise, Unreadable> {
        let html = self.page(place.page()).await.map_err(Unreadable::Page)?;
        let at = place.clone();
        let shown = self.shown(place.page());
        on_blocking(html.len(), move || promise_of(&html, &at))
            .await
            .map_err(|why| Unreadable::Page(stopped(&shown, &why)))?
    }

    /// The place in the docs of the page of the source the thing at `place` is
    /// defined in (`src/<crate>/<path>.html`), read from the link `Source` of its page
    /// ([`source_link`], parsed on a blocking thread as a promise is), or why it
    /// cannot be: the page cannot be read, has no such link, or its link leads outside
    /// the docs as written.
    pub(super) async fn source(&mut self, place: &Place) -> Result<DocsPath, String> {
        let html = self.page(place.page()).await?;
        let at = place.clone();
        let shown = self.shown(place.page());
        let link = on_blocking(html.len(), move || source_link(&html, &at))
            .await
            .map_err(|why| stopped(&shown, &why))?
            .ok_or_else(|| format!("{shown} has no link to the source of the item"))?;
        let dir = place.page().parent().unwrap_or_default();
        within(Path::new(""), &dir.under(Path::new("")), &link)
            .and_then(|source| DocsPath::of_relative(&source))
            .ok_or_else(|| {
                format!("the link to the source in {shown} leads outside the docs: {link}")
            })
    }

    /// The place `segment` names under `at`: none when it names nothing there.
    async fn step(&mut self, at: At, segment: &str) -> Result<Option<At>, String> {
        match at {
            At::Module(dir) => self.in_module(&dir, segment).await,
            At::Item { page, kind } => {
                if !TYPE_KINDS.contains(&kind.as_str()) {
                    return Ok(None);
                }
                let html = self.page(&page).await?;
                let wanted = segment.to_owned();
                let named = on_blocking(html.len(), move || {
                    MemberKind::all()
                        .find(|member| html.contains(&format!("id=\"{}.{wanted}\"", member.word())))
                })
                .await
                .map_err(|why| stopped(&self.shown(&page), &why))?;
                Ok(named.map(|member| At::Member {
                    page,
                    kind: member,
                    anchor: format!("{}.{segment}", member.word()),
                }))
            }
            At::Member { .. } => Ok(None),
            At::Unfollowed(why) => Err(why),
        }
    }

    /// The module or item `segment` names in the module at `dir`.
    async fn in_module(&mut self, dir: &DocsPath, segment: &str) -> Result<Option<At>, String> {
        let Some(names) = self.listing(dir).await.cloned() else {
            return Err(format!("{} cannot be listed", self.shown(dir)));
        };
        if names.contains(segment)
            && let Some(module) = dir.join(segment)
        {
            // Before it is listed: nothing outside is read.
            if let Some(why) = self.outside(&module).await {
                return Err(why);
            }
            if self.has(&module, "index.html").await == Some(true) {
                return Ok(Some(At::Module(module)));
            }
        }
        if let Some((kind, page)) = ITEM_KINDS.iter().find_map(|kind| {
            let file = format!("{kind}.{segment}.html");
            names
                .contains(&file)
                .then(|| dir.join(&file))
                .flatten()
                .map(|page| (kind, page))
        }) {
            if let Some(why) = self.outside(&page).await {
                return Err(why);
            }
            return Ok(Some(At::Item {
                page,
                kind: (*kind).to_owned(),
            }));
        }
        if !names.contains("index.html") {
            return Ok(None);
        }
        let index = index_of(dir);
        let html = self.page(&index).await?;
        let wanted = segment.to_owned();
        let Some(link) = on_blocking(html.len(), move || reexport_link(&html, &wanted))
            .await
            .map_err(|why| stopped(&self.shown(&index), &why))?
        else {
            return Ok(None);
        };
        Ok(Some(self.follow(dir, link.as_deref(), segment).await))
    }

    /// The place a re-export of `segment` in the module at `dir` leads to, through
    /// its link (`href`, relative to `dir`). A re-export with no link, or whose link
    /// leads outside the docs (symbolic links resolved, [`BuiltDocs::inside_docs`]) or
    /// to nothing readable, is there, and nothing under it can be told.
    async fn follow(&mut self, dir: &DocsPath, link: Option<&str>, segment: &str) -> At {
        let nowhere = |lookup: &Self| {
            At::Unfollowed(format!(
                "`{segment}` is re-exported in {} from docs that are not here",
                lookup.shown(dir)
            ))
        };
        let Some(target) = link.and_then(|link| {
            within(Path::new(""), &dir.under(Path::new("")), link)
                .and_then(|place| DocsPath::of_relative(&place))
        }) else {
            return nowhere(self);
        };
        if !self
            .held
            .docs()
            .inside_docs(self.held.side(), &target)
            .await
        {
            return nowhere(self);
        }
        let Some(file) = target.file_name().map(str::to_owned) else {
            return nowhere(self);
        };
        let parent = target.parent().unwrap_or_default();
        if file == "index.html" {
            return match self.has(&parent, "index.html").await {
                Some(true) => At::Module(parent),
                _ => nowhere(self),
            };
        }
        let kind = ITEM_KINDS
            .iter()
            .find(|kind| file.starts_with(&format!("{kind}.")) && file.ends_with(".html"));
        match (kind, self.has(&parent, &file).await) {
            (Some(kind), Some(true)) => At::Item {
                page: target,
                kind: (*kind).to_owned(),
            },
            _ => nowhere(self),
        }
    }
}

/// The last part of each entry of a listing of the docs, with the entries whose name
/// is no place of the docs.
fn names_of(entries: Entries<DocsPath>) -> BTreeSet<String> {
    entries
        .named
        .keys()
        .filter_map(|p| p.file_name().map(str::to_owned))
        .chain(entries.unnamed.into_keys())
        .collect()
}

/// Why a page cannot be told, when the blocking thread it was read on stopped (a bug
/// if it ever happens, or a runtime shutting down).
fn stopped(page: &str, why: &str) -> String {
    format!("the reading of {page} stopped: {why}")
}

/// The bytes of a text, as a count that cannot overflow.
fn len_of(text: &str) -> u64 {
    u64::try_from(text.len()).unwrap_or(u64::MAX)
}

/// The re-export of `name` in the HTML of a module's index: none when there is
/// none, and the link of its entry (the last `href` of it) when it has one.
fn reexport_link(html: &str, name: &str) -> Option<Option<String>> {
    let anchor = format!("id=\"reexport.{name}\"");
    let start = html.find(&anchor)? + anchor.len();
    let entry = &html[start..];
    let entry = &entry[..entry.find("</dt>").unwrap_or(entry.len())];
    let link = entry.rfind("href=\"").and_then(|at| {
        let value = &entry[at + "href=\"".len()..];
        value.find('"').map(|end| value[..end].to_owned())
    });
    Some(link)
}

/// The file a relative link of a page in `dir` names, when it stays inside `docs`
/// as written; none for a link to elsewhere (a URL, an absolute path, one that
/// climbs out). `/` and `\` both separate. Symbolic links are the caller's.
fn within(docs: &Path, dir: &Path, link: &str) -> Option<PathBuf> {
    if link.contains("://") || link.starts_with(['/', '\\', '#']) {
        return None;
    }
    let link = link.split(['#', '?']).next()?;
    let mut parts: Vec<String> = dir
        .strip_prefix(docs)
        .ok()?
        .components()
        .map(|c| match c {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Option<_>>()?;
    for part in link.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains(':') => return None,
            part => parts.push(part.to_owned()),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(
        parts
            .iter()
            .fold(docs.to_path_buf(), |path, p| path.join(p)),
    )
}

#[cfg(test)]
mod tests {
    use super::super::fixture::write;
    use super::*;

    #[tokio::test]
    async fn the_source_of_an_item_is_the_page_its_link_names_inside_the_docs_and_none_outside() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("doc");
        let page = |href: &str| {
            format!(
                "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><main><section id=\"main-content\"><div class=\"main-heading\"><a class=\"src\" href=\"{href}\">Source</a></div></section></main></body></html>"
            )
        };
        write(&docs, "demo/index.html", &page("../src/demo/lib.rs.html")).await;
        write(
            &docs,
            "demo/fn.inside.html",
            &page("../src/demo/a.rs.html#3"),
        )
        .await;
        write(&docs, "demo/fn.out.html", &page("../../../x.rs.html")).await;
        let mut lookup = super::super::fixture::walk_lookup(dir.path(), docs, 1 << 20).await;
        let source = |found: Found| match found {
            Found::There(place) => place,
            other => panic!("there: {other:?}"),
        };
        let inside = source(lookup.find(&RustPath::parse("demo::inside").unwrap()).await);
        assert_eq!(
            lookup.source(&inside).await,
            Ok(DocsPath::of_relative(Path::new("src/demo/a.rs.html")).unwrap())
        );
        let out = source(lookup.find(&RustPath::parse("demo::out").unwrap()).await);
        let why = lookup.source(&out).await.unwrap_err();
        assert!(why.contains("leads outside the docs"), "{why}");
    }

    #[test]
    fn a_link_is_followed_only_inside_the_docs() {
        let docs = Path::new("/t/doc");
        let dir = docs.join("demo");
        assert_eq!(
            within(docs, &dir, "inner/struct.A.html#method.x"),
            Some(docs.join("demo/inner/struct.A.html"))
        );
        assert_eq!(
            within(docs, &dir, "inner\\struct.A.html"),
            Some(docs.join("demo/inner/struct.A.html"))
        );
        assert_eq!(
            within(docs, &dir, "../other/index.html"),
            Some(docs.join("other/index.html"))
        );
        for outside in [
            "../../x.html",
            "..\\..\\x.html",
            "/etc/passwd",
            "\\x.html",
            "C:\\x.html",
            "https://docs.rs/x",
            "#anchor",
            "..",
        ] {
            assert_eq!(within(docs, &dir, outside), None, "{outside}");
        }
    }
}
