//! The lookup of a path in the docs: where in the HTML a name is, and what its
//! promise is read from.
//!
//! | what | what it does |
//! |---|---|
//! | `Lookup` | the docs read for one resolution, each directory listed once and each page read once: `find` walks a path to its [`Found`], `promise` reads what the thing found promises |
//! | `Reads` | what the pages asked for so far came to: how many were read, their bytes, how many could not be read and how many were too large (what a search for the moves of a name reports and holds to its bounds) |
//! | `list`, `read` | the one listing of a directory and the one bounded reading of a page |
//! | `reexport_link`, `within` | the link of a re-export in a module's index, and the file a link names when it stays inside the docs |
//!
//! The words of the layout are `layout`'s and the grammar of a path is `path`'s; this
//! module is what walks one along the other, and reads the files of the docs by the
//! rules of the page of the module above.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncReadExt;

use super::super::on_blocking;
use super::layout::{GENERATOR_MARK, ITEM_KINDS, MemberKind, Place, TYPE_KINDS};
use super::path::RustPath;
use super::promise::{Promise, Unreadable, promise_of};

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
}

impl From<At> for Found {
    /// What a walk that ends at `at` comes to: the one place a [`Place`] is built.
    fn from(at: At) -> Self {
        match at {
            At::Module(dir) => Self::There(Place::Module {
                page: dir.join("index.html"),
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
    Module(PathBuf),
    /// The page of an item.
    Item { page: PathBuf, kind: String },
    /// A member of a type, found by its anchor on the page of the type: nothing is
    /// under it.
    Member {
        page: PathBuf,
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

/// The docs, read for one resolution: each directory listed once, each page read
/// once.
pub(super) struct Lookup {
    docs: PathBuf,
    /// The docs with every symbolic link resolved, when they can be.
    real_docs: Option<PathBuf>,
    /// The root of the project and the build directory (the parent of the docs),
    /// each with every symbolic link resolved, when it can be: no page and no
    /// directory of a module is read outside both.
    real_places: Vec<PathBuf>,
    max_page: u64,
    listings: BTreeMap<PathBuf, Option<BTreeSet<String>>>,
    pages: BTreeMap<PathBuf, Result<Arc<str>, String>>,
    reads: Reads,
}

impl Lookup {
    pub(super) async fn new(root: &Path, docs: PathBuf, max_page: u64) -> Self {
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
            max_page,
            listings: BTreeMap::new(),
            pages: BTreeMap::new(),
            reads: Reads::default(),
        }
    }

    /// The directory rustdoc wrote the docs in.
    pub(super) fn docs(&self) -> &Path {
        &self.docs
    }

    /// What the pages asked for so far came to.
    pub(super) fn reads(&self) -> Reads {
        self.reads
    }

    /// Why nothing is read at `path`, when it leads outside the root of the project
    /// and the build directory once symbolic links are resolved, or is a link that
    /// cannot be resolved.
    pub(super) async fn outside(&self, path: &Path) -> Option<String> {
        let inside = match tokio::fs::canonicalize(path).await {
            Ok(real) => self.real_places.iter().any(|place| real.starts_with(place)),
            Err(_) => false,
        };
        (!inside).then(|| {
            format!(
                "{} leads outside the root of the project and the build directory once symbolic links are resolved, or cannot be resolved: nothing is read through it",
                path.display()
            )
        })
    }

    /// The names of the entries of a directory, or none when it cannot be listed.
    pub(super) async fn listing(&mut self, dir: &Path) -> Option<&BTreeSet<String>> {
        if !self.listings.contains_key(dir) {
            let names = list(dir).await;
            self.listings.insert(dir.to_path_buf(), names);
        }
        self.listings.get(dir).and_then(Option::as_ref)
    }

    /// Whether `name` is an entry of `dir`, as written; none when `dir` cannot be
    /// listed.
    async fn has(&mut self, dir: &Path, name: &str) -> Option<bool> {
        self.listing(dir).await.map(|names| names.contains(name))
    }

    /// The text of a page, or why it cannot be read (one outside the root and the
    /// build directory is not).
    async fn page(&mut self, page: &Path) -> Result<Arc<str>, String> {
        if !self.pages.contains_key(page) {
            let text = match self.outside(page).await {
                Some(why) => {
                    self.reads.unreadable += 1;
                    Err(why)
                }
                None => match read(page, self.max_page).await {
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
            self.pages.insert(page.to_path_buf(), text);
        }
        self.pages
            .get(page)
            .cloned()
            .unwrap_or_else(|| Err(format!("{} cannot be read", page.display())))
    }

    pub(super) async fn find(&mut self, path: &RustPath) -> Found {
        let crate_dir = self.docs.join(&path.krate);
        let built = match self.has(&self.docs.clone(), &path.krate).await {
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
                crate_dir.display()
            ));
        }
        let index = crate_dir.join("index.html");
        let html = match self.page(&index).await {
            Ok(html) => html,
            Err(why) => return Found::CannotTell(why),
        };
        match on_blocking(html.len(), move || html.contains(GENERATOR_MARK)).await {
            Ok(true) => {}
            Ok(false) => {
                return Found::CannotTell(format!(
                    "{} is not rustdoc's: it has no {GENERATOR_MARK}",
                    index.display()
                ));
            }
            Err(why) => return Found::CannotTell(stopped(&index, &why)),
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
        on_blocking(html.len(), move || promise_of(&html, &at))
            .await
            .map_err(|why| Unreadable::Page(stopped(place.page(), &why)))?
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
                .map_err(|why| stopped(&page, &why))?;
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
    async fn in_module(&mut self, dir: &Path, segment: &str) -> Result<Option<At>, String> {
        let Some(names) = self.listing(dir).await.cloned() else {
            return Err(format!("{} cannot be listed", dir.display()));
        };
        if names.contains(segment) {
            let module = dir.join(segment);
            // Before it is listed: nothing outside is read.
            if let Some(why) = self.outside(&module).await {
                return Err(why);
            }
            if self.has(&module, "index.html").await == Some(true) {
                return Ok(Some(At::Module(module)));
            }
        }
        if let Some(kind) = ITEM_KINDS
            .iter()
            .find(|kind| names.contains(&format!("{kind}.{segment}.html")))
        {
            let page = dir.join(format!("{kind}.{segment}.html"));
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
        let index = dir.join("index.html");
        let html = self.page(&index).await?;
        let wanted = segment.to_owned();
        let Some(link) = on_blocking(html.len(), move || reexport_link(&html, &wanted))
            .await
            .map_err(|why| stopped(&index, &why))?
        else {
            return Ok(None);
        };
        Ok(Some(self.follow(dir, link.as_deref(), segment).await))
    }

    /// The place a re-export of `segment` in the module at `dir` leads to, through
    /// its link (`href`, relative to `dir`). A re-export with no link, or whose link
    /// leads outside the docs (symbolic links resolved) or to nothing readable, is
    /// there, and nothing under it can be told.
    async fn follow(&mut self, dir: &Path, link: Option<&str>, segment: &str) -> At {
        let nowhere = || {
            At::Unfollowed(format!(
                "`{segment}` is re-exported in {} from docs that are not here",
                dir.display()
            ))
        };
        let Some(target) = link.and_then(|link| within(&self.docs, dir, link)) else {
            return nowhere();
        };
        let inside = match (&self.real_docs, tokio::fs::canonicalize(&target).await) {
            (Some(docs), Ok(real)) => real.starts_with(docs),
            _ => false,
        };
        if !inside {
            return nowhere();
        }
        let Some(file) = target
            .file_name()
            .and_then(|f| f.to_str())
            .map(str::to_owned)
        else {
            return nowhere();
        };
        let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
        if file == "index.html" {
            return match self.has(&parent, "index.html").await {
                Some(true) => At::Module(parent),
                _ => nowhere(),
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
            _ => nowhere(),
        }
    }
}

/// The names of the entries of a directory that are UTF-8, or none when it cannot
/// be listed.
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

/// Why a page cannot be told, when the blocking thread it was read on stopped (a bug
/// if it ever happens, or a runtime shutting down).
fn stopped(page: &Path, why: &str) -> String {
    format!("the reading of {} stopped: {why}", page.display())
}

/// The text of a page of at most `max` bytes, or why it cannot be read.
async fn read(page: &Path, max: u64) -> Result<String, Failure> {
    let cannot =
        |e: std::io::Error| Failure::Unreadable(format!("{} cannot be read: {e}", page.display()));
    let file = tokio::fs::File::open(page).await.map_err(cannot)?;
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(cannot)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > max) {
        return Err(Failure::TooLarge(format!(
            "{} is larger than {max} bytes, and is not read",
            page.display()
        )));
    }
    // A page may be as large as the bound: a large one is decoded on a blocking
    // thread.
    on_blocking(bytes.len(), move || {
        String::from_utf8_lossy(&bytes).into_owned()
    })
    .await
    .map_err(|why| Failure::Unreadable(stopped(page, &why)))
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
    use super::*;

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
