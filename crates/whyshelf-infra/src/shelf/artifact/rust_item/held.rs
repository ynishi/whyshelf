//! The built docs of one side held against the sources they were built from: the one
//! place it is decided whether a build is of the sources as they are ([`hold`]), and
//! the mark that it was ([`HeldDocs`]), without which no page of the docs is read.
//!
//! | what | what it does |
//! |---|---|
//! | [`hold`] | every page of the source the docs hold of a crate of the project, laid beside the file of the side it is of ([`Crates::file_of`]): all alike, the mark; else why not ([`NotOfSources`]) |
//! | [`HeldDocs`] | the mark: the docs of one side and the crates of the project held in them; made by [`hold`] alone (and, for the tests of how a page is walked, by `for_walk_tests`) |
//! | [`page_text`] | the text of a file a page of the source holds, or none when the page holds no copy |
//! | [`as_compiled`] | the bytes of a file as the compiler reads them: no byte order mark at the start, a CRLF as LF |
//!
//! ```text
//!   the docs of a side
//!     ├─ each crate in them: <crate>/index.html, or src/<crate>/
//!     │    ├─ a dependency (no crate of the manifests) ──► not held, and not walked
//!     │    ├─ two crates of the name, a manifest unread ──► not of the sources (no trace, unread)
//!     │    └─ a crate of the project ──► src/<crate>/ has pages, else no trace
//!     └─ each page, in the order of their paths, within the bounds
//!          ├─ a source a build made, no page of the source ──► not compared
//!          ├─ a path out of the project ──► no trace
//!          └─ a file of the side ──► its text as compiled == the text of the page?
//!               no file ──► gone · another text ──► differs · unread ──► unread
//! ```
//!
//! The unit is the build: one page that is not as its file makes every name read from
//! those docs one that cannot be told, for a re-export crosses crates. The reason is the
//! first, in the order of the paths of the pages. The adapter keeps the answer for each
//! side for the run (a run writes nothing, so the sources and the docs do not change
//! during it).
//!
//! This module reads the docs through their tree ([`DocsTree`]) and the files of the
//! project through its state, as every reader does.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use whyshelf_core::app::shelf::{
    AtSide, ByteBound, DirListing, EntryKind, NotADir, Side, StateList, StateRead,
};
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};
use whyshelf_core::domain::shelf::artifact::{NotOfSources, NotOfSourcesWhy};

use super::super::super::state::{ProjectTree, StateError};
use super::super::file::{self, Bounded};
use super::super::on_blocking;
use super::MAX_PAGE_BYTES;
use super::crates::{Crates, SourceOf};
use super::docs_path::DocsPath;
use super::docs_tree::{BuiltDocs, DocsTree};
use super::sited::Sited;

/// What a person may do when the docs are not of the sources: the docs are the
/// project's to build, and this adapter builds nothing.
pub(super) const HINT: TextLiteral = TextLiteral::new(
    "build the docs again from the sources as they are, as the project builds them (such as `cargo doc --no-deps`)",
);

/// The name rustdoc gives the directory of the pages of the source.
const SOURCE_DIR: &str = "src";

/// The bounds of one holding: the pages of the source, their bytes in all, one page,
/// and the depth of their directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HoldLimits {
    pub(super) pages: usize,
    pub(super) bytes: u64,
    pub(super) page_bytes: u64,
    pub(super) depth: usize,
}

impl Default for HoldLimits {
    /// 2,000 pages and 512 MiB in all, as the walk of the docs for suggestions, a page
    /// of at most [`MAX_PAGE_BYTES`], and 64 directories deep (a page of a source a
    /// build made nests the whole path of the build directory).
    fn default() -> Self {
        Self {
            pages: 2_000,
            bytes: 512 * 1024 * 1024,
            page_bytes: MAX_PAGE_BYTES,
            depth: 64,
        }
    }
}

/// The mark that the docs of one side were held against the sources of that side and
/// are of them: the docs, the crates of the project held in them, and what is needed
/// to place a name (the crates of the manifests and where the docs were built). Its
/// fields are private and [`hold`] alone makes one, so a lookup, which is made from
/// one only, reads no page of docs that were not held.
#[derive(Debug, Clone)]
pub(super) struct HeldDocs {
    side: Side,
    docs: DocsTree,
    /// What the docs are said as.
    read: NonBlank,
    /// The crates of the project held; none for the tests of the walk, every crate.
    held: Option<BTreeSet<String>>,
    /// The crates of the manifests and where the docs were built: none for the tests
    /// of the walk.
    sources: Option<(Crates, Sited)>,
}

impl HeldDocs {
    /// The docs `docs` of `side`, held against nothing: the mark the tests of how a
    /// page is walked use, every crate taken as held.
    #[cfg(test)]
    pub(super) fn for_walk_tests(docs: DocsTree, side: Side) -> Self {
        let read = NonBlank::from(TextLiteral::new("the built docs of a test"));
        Self {
            side,
            docs,
            read,
            held: None,
            sources: None,
        }
    }

    /// The side the docs are of.
    pub(super) fn side(&self) -> Side {
        self.side
    }

    /// The docs.
    pub(super) fn docs(&self) -> &DocsTree {
        &self.docs
    }

    /// The crates of the manifests and where the docs were built, to place a name.
    pub(super) fn sources(&self) -> Option<(&Crates, &Sited)> {
        self.sources.as_ref().map(|(crates, sited)| (crates, sited))
    }

    /// Whether the docs of the crate `krate` were held: a crate of the manifests read
    /// is, and any other (a dependency's, or one of a package those manifests do not
    /// reach) is not of the sources of the project, with no trace of them.
    pub(super) fn crate_of(&self, krate: &str) -> Result<(), NotOfSources> {
        match &self.held {
            Some(held) if !held.contains(krate) => Err(NotOfSources::new(
                reason(format!("the docs of the crate `{krate}` in {}", self.read)),
                NotOfSourcesWhy::NoTrace,
                NonBlank::from(TextLiteral::new(
                    "it is no crate of the manifests read (the one at the root of the project, else the one of each directory at its top): name an item of a crate they name, or make them name it",
                )),
            )),
            Some(_) | None => Ok(()),
        }
    }
}

/// A text of this module as a reason: it is never blank.
fn reason(text: String) -> NonBlank {
    NonBlank::new(text).unwrap_or_else(|_| NonBlank::from(TextLiteral::new("the built docs")))
}

/// The docs `docs` of `side` held against the files of the side `state` reads, with
/// the crates of its manifests and `sited` where the docs were built: the mark when
/// every page of the source of a crate of the project is the text of its file, else
/// why not, the first in the order of the paths of the pages. Docs that are not there
/// (their directory is not) hold nothing to read, and are the mark of no crate: the
/// lookup then says they are not built, as before. A directory of the docs that cannot
/// be listed whole (an error, a listing cut at its bound) is unread, with why.
///
/// A crate held whose package depends by a path on a crate of the project whose
/// pages of the source the docs do not hold has no trace: rustdoc writes the items a
/// crate re-exports from another into its own docs, with no page of the other's
/// source, when the other is not documented (`cargo doc -p <crate>`).
pub(super) async fn hold(
    state: &ProjectTree,
    side: Side,
    docs: DocsTree,
    crates: Crates,
    sited: Sited,
    limits: &HoldLimits,
) -> Result<HeldDocs, NotOfSources> {
    let read = reason(format!("the built docs at {}", docs.dir(side).display()));
    let not = |why: NotOfSourcesWhy| NotOfSources::new(read.clone(), why, NonBlank::from(HINT));
    let unread = |text: String| not(NotOfSourcesWhy::Unread(reason(text)));
    let mut held = BTreeSet::new();
    let top = match listed(&docs, side, &DocsPath::top()).await {
        Listed::Entries(top) => top,
        Listed::Absent => {
            return Ok(HeldDocs {
                side,
                docs,
                read,
                held: Some(held),
                sources: Some((crates, sited)),
            });
        }
        Listed::NotADir(why) | Listed::Unread(why) => return Err(unread(why)),
    };
    let src = DocsPath::top().join(SOURCE_DIR);
    let sourced: BTreeSet<String> = match &src {
        Some(src) if top.contains_key(SOURCE_DIR) => match listed(&docs, side, src).await {
            Listed::Entries(entries) => entries.into_keys().collect(),
            Listed::Absent => BTreeSet::new(),
            Listed::NotADir(why) | Listed::Unread(why) => return Err(unread(why)),
        },
        _ => BTreeSet::new(),
    };
    let mut built = BTreeSet::new();
    for (name, kind) in &top {
        if name == SOURCE_DIR || !matches!(kind, EntryKind::Dir | EntryKind::Link) {
            continue;
        }
        let Some(dir) = DocsPath::top().join(name) else {
            continue;
        };
        match listed(&docs, side, &dir).await {
            Listed::Entries(inside) if inside.contains_key("index.html") => {
                built.insert(name.clone());
            }
            Listed::Entries(_) | Listed::Absent | Listed::NotADir(_) => {}
            Listed::Unread(why) => return Err(unread(why)),
        }
    }
    let mut pages = Vec::new();
    for krate in built.union(&sourced) {
        match crates.crate_dir(krate) {
            Ok(_) => {}
            Err(SourceOf::NoCrate) => continue,
            Err(SourceOf::ManifestsUnread(why)) => return Err(not(NotOfSourcesWhy::Unread(why))),
            Err(_) => return Err(not(NotOfSourcesWhy::NoTrace)),
        }
        let Some(dir) = src.as_ref().and_then(|src| src.join(krate)) else {
            return Err(not(NotOfSourcesWhy::NoTrace));
        };
        if !sourced.contains(krate) {
            return Err(not(NotOfSourcesWhy::NoTrace));
        }
        let found = match walk(&docs, side, &dir, limits, pages.len()).await {
            Ok(found) => found,
            Err(why) => return Err(unread(why)),
        };
        if found.is_empty() {
            return Err(not(NotOfSourcesWhy::NoTrace));
        }
        pages.extend(found);
        held.insert(krate.clone());
    }
    // What a crate held re-exports of a crate of the project it depends on is held
    // only through the pages of that crate's source.
    for krate in &held {
        if crates
            .path_dependencies(krate)
            .iter()
            .any(|dependency| !held.contains(dependency))
        {
            return Err(not(NotOfSourcesWhy::NoTrace));
        }
    }
    pages.sort();
    let mut bytes: u64 = 0;
    for page in &pages {
        let file = match crates.file_of(page, &sited).await {
            SourceOf::File(file) => file,
            SourceOf::MadeByBuild | SourceOf::NotASourcePage => continue,
            SourceOf::ManifestsUnread(why) => return Err(not(NotOfSourcesWhy::Unread(why))),
            SourceOf::LeavesProject | SourceOf::NoCrate | SourceOf::TwoCrates(_) => {
                return Err(not(NotOfSourcesWhy::NoTrace));
            }
        };
        let shown = docs.shown(side, page);
        if let Some(why) = docs.outside(side, page).await {
            return Err(unread(why));
        }
        let html = match docs
            .read(
                side,
                &BTreeSet::from([page.clone()]),
                ByteBound::at_most(limits.page_bytes),
            )
            .await
            .map(|mut read| read.remove(page))
        {
            Ok(Some(Ok(AtSide::There(html)))) => html,
            _ => return Err(unread(format!("{} cannot be read", shown.display()))),
        };
        let len = u64::try_from(html.len()).unwrap_or(u64::MAX);
        if len > limits.page_bytes {
            return Err(unread(format!(
                "{} is larger than {} bytes, and is not read",
                shown.display(),
                limits.page_bytes
            )));
        }
        bytes = bytes.saturating_add(len);
        if bytes > limits.bytes {
            return Err(unread(format!(
                "the pages of the source are more than {} bytes in all",
                limits.bytes
            )));
        }
        let source = match file::read_within(state, side, &file, limits.page_bytes).await {
            Ok(Bounded::Bytes(source)) => source,
            Ok(Bounded::NoFile) => return Err(not(NotOfSourcesWhy::Gone { file })),
            Ok(Bounded::Outside) => {
                return Err(unread(format!(
                    "{file}: a symbolic link on its path leads outside the root of the project, or is broken: nothing is read through it"
                )));
            }
            Ok(Bounded::TooLarge) => {
                return Err(unread(format!(
                    "{file} is larger than {} bytes, and is not read",
                    limits.page_bytes
                )));
            }
            Ok(Bounded::Untold(why)) => return Err(unread(format!("{file}: {why}"))),
            Err(e) => return Err(unread(format!("{file} cannot be read: {e}"))),
        };
        let size = html.len().max(source.len());
        let alike = on_blocking(size, move || {
            let html = String::from_utf8_lossy(&html);
            page_text(&html).map(|text| text.as_bytes() == as_compiled(&source).as_slice())
        })
        .await;
        match alike {
            Ok(Some(true)) => {}
            Ok(Some(false)) => return Err(not(NotOfSourcesWhy::Differs { file })),
            Ok(None) => return Err(not(NotOfSourcesWhy::NoTrace)),
            Err(why) => {
                return Err(unread(format!(
                    "the reading of {} stopped: {why}",
                    shown.display()
                )));
            }
        }
    }
    Ok(HeldDocs {
        side,
        docs,
        read,
        held: Some(held),
        sources: Some((crates, sited)),
    })
}

/// What the listing of a directory of the docs comes to. Only [`Listed::Absent`] is
/// docs that are not there; a listing cut at its bound, as one that fails, is not read
/// as the entries it has ([`Listed::Unread`]), as the lookup reads one.
#[derive(Debug, PartialEq, Eq)]
enum Listed {
    /// Its entries, each by the text of its name, with its kind.
    Entries(BTreeMap<String, EntryKind>),
    /// Nothing is there.
    Absent,
    /// It is there and is no directory, and why it is said so.
    NotADir(String),
    /// It cannot be listed whole, and why.
    Unread(String),
}

/// The listing of `dir` of the docs.
async fn listed(docs: &DocsTree, side: Side, dir: &DocsPath) -> Listed {
    let shown = docs.shown(side, dir);
    of_listing(&shown, docs.list(side, Some(dir)).await)
}

/// What the answer `listing` of the directory shown as `shown` comes to: the one place
/// a listing of the docs is read for the holding.
fn of_listing(
    shown: &std::path::Path,
    listing: Result<DirListing<DocsPath>, StateError>,
) -> Listed {
    let shown = shown.display();
    match listing {
        Ok(DirListing::All(entries)) => Listed::Entries(
            entries
                .named
                .into_iter()
                .filter_map(|(p, kind)| p.file_name().map(|name| (name.to_owned(), kind)))
                .chain(
                    entries
                        .unnamed
                        .into_iter()
                        .map(|(name, entry)| (name, entry.kind)),
                )
                .collect(),
        ),
        Ok(DirListing::Cut { why, .. }) => {
            Listed::Unread(format!("the listing of {shown} was cut: {why}"))
        }
        Ok(DirListing::NotADir(NotADir::Absent)) => Listed::Absent,
        Ok(DirListing::NotADir(NotADir::File)) => {
            Listed::NotADir(format!("{shown} is no directory"))
        }
        Ok(DirListing::NotADir(NotADir::Untold(why))) => {
            Listed::Unread(format!("{shown} cannot be listed: {why}"))
        }
        Ok(DirListing::NotADir(NotADir::Elsewhere { .. })) => Listed::Unread(format!(
            "{shown} is reached through a symbolic link that is not followed"
        )),
        Err(e) => Listed::Unread(format!("{shown} cannot be listed: {e}")),
    }
}

/// The pages (`*.html`) under `dir` of the docs, breadth first, within the bounds of
/// `limits` with `before` pages found already, or why they cannot be told.
async fn walk(
    docs: &DocsTree,
    side: Side,
    dir: &DocsPath,
    limits: &HoldLimits,
    before: usize,
) -> Result<Vec<DocsPath>, String> {
    let mut pages = Vec::new();
    let mut queue = VecDeque::from([(dir.clone(), 0_usize)]);
    while let Some((at, depth)) = queue.pop_front() {
        let shown = docs.shown(side, &at);
        if depth > limits.depth {
            return Err(format!(
                "{} is more than {} directories deep",
                shown.display(),
                limits.depth
            ));
        }
        let entries = match docs.list(side, Some(&at)).await {
            Ok(DirListing::All(entries)) => entries,
            Ok(DirListing::Cut { .. } | DirListing::NotADir(_)) | Err(_) => {
                return Err(format!("{} cannot be listed", shown.display()));
            }
        };
        for (entry, kind) in entries.named {
            match kind {
                EntryKind::Dir => queue.push_back((entry, depth + 1)),
                EntryKind::File | EntryKind::Link
                    if entry
                        .file_name()
                        .is_some_and(|name| name.ends_with(".html")) =>
                {
                    pages.push(entry);
                    if before + pages.len() > limits.pages {
                        return Err(format!(
                            "the docs hold more than {} pages of the source",
                            limits.pages
                        ));
                    }
                }
                EntryKind::File | EntryKind::Link | EntryKind::Other => {}
            }
        }
    }
    Ok(pages)
}

/// The text of the file a page of the source holds: its code block with the anchors
/// of the line numbers and every tag taken out and the entities read back; none when
/// the page holds no code block (no copy of a file). rustdoc writes the text of the
/// file as the compiler read it ([`as_compiled`]).
pub(super) fn page_text(html: &str) -> Option<String> {
    const OPEN: &str = "<pre class=\"rust\"><code>";
    const CLOSE: &str = "</code></pre>";
    let start = html.find(OPEN)? + OPEN.len();
    let end = start + html[start..].find(CLOSE)?;
    let mut rest = &html[start..end];
    let mut text = String::with_capacity(rest.len());
    while let Some(lt) = rest.find('<') {
        text.push_str(&rest[..lt]);
        let tag_end = rest[lt..].find('>')? + lt;
        let tag = &rest[lt..=tag_end];
        rest = &rest[tag_end + 1..];
        // The number of a line: an anchor with its number inside, none of the file.
        if tag.starts_with("<a ") && tag.contains("data-nosnippet") {
            let close = rest.find("</a>")?;
            rest = &rest[close + "</a>".len()..];
        }
    }
    text.push_str(rest);
    Some(unescaped(&text))
}

/// `text` with the entities of HTML read back: the named ones rustdoc writes and the
/// numeric ones; any other is kept as written (and so the text differs from its file).
fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let entity = rest
            .find(';')
            .filter(|end| *end <= 10)
            .map(|end| (&rest[1..end], end));
        let read = entity.and_then(|(name, end)| {
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match read {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The bytes of a file as the compiler reads them, and rustdoc writes them in a page:
/// a byte order mark at the start taken out, and each CRLF as LF. A last line break is
/// kept as it is.
pub(super) fn as_compiled(bytes: &[u8]) -> Vec<u8> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            at += 1;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
