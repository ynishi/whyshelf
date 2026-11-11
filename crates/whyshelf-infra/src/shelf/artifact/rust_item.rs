//! The adapter of the space `rust-item`: the items of the Rust crates of a project,
//! by their path, as the documentation `cargo doc` writes knows them.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`RustItemSpace::resolve`] | finds the build directory of the project, and looks up each path in the HTML rustdoc wrote under it | `tokio::fs`, `toml` |
//! | [`RustItemSpace::current`] | nothing: an item has no content this adapter reads | none |
//! | [`presets`] | the preset `rustdoc`: the space, `compare = "presence"`, `means = "depends"` | values |
//!
//! ```text
//!   name ──a Rust path?── no ──► not a name (why)
//!     │ yes
//!   the build directory: CARGO_TARGET_DIR | build.target-dir of the root's cargo
//!   config | the conventional one beside the root's Cargo.toml
//!     ├─ none of them ──► cannot tell, each name
//!     └─ its doc directory, then the directory of the crate in it
//!          ├─ no index.html, or one rustdoc did not write ──► cannot tell
//!          └─ the path, segment by segment, in the HTML ──► there (presence only)
//!                                                         | absent | cannot tell (why)
//! ```
//!
//! Resolving runs no program: it reads the environment it is given, the files of
//! the project's cargo settings, and the HTML. A repository is read, never executed,
//! even by a command that only reads.
//!
//! # The names of the space
//!
//! A name is a Rust path: identifiers separated by `::`, the first the name of a
//! crate as rustdoc writes it (`whyshelf_core`, with `_` where the package name
//! has `-`), the last an item or a member of a type (`Type::method`,
//! `Enum::Variant`, `Struct::field`). There are no generics, no leading `::`, no
//! raw identifier (`r#type`), and no `crate`, `self`, `super` or `Self`. Any other
//! text is not a name of the space, and the adapter says so with why. A crate's own
//! name is a path too: its index.
//!
//! # Where the docs are
//!
//! The build directory is the first of these that can be told, and the docs are in
//! its directory `doc`:
//!
//! 1. `CARGO_TARGET_DIR`, as the caller read it from the environment;
//! 2. `build.target-dir` in the cargo settings at the root of the project
//!    (`.cargo/config` when it is there, as cargo uses it over
//!    `.cargo/config.toml` when both are, else `.cargo/config.toml`), read as TOML;
//! 3. the directory [`TARGET_DIR_NAME`] beside the `Cargo.toml` at the root.
//!
//! A relative directory is taken from the root of the project. When none can be
//! told (no variable, no setting, no `Cargo.toml` at the root), or the settings
//! cannot be read, no name can be told. Cargo settings in the directories above the
//! root or in cargo's home, and docs built for another target triple (`--target`),
//! are not read.
//!
//! The docs are read as they are: docs left there by an earlier build, of an item
//! or a crate the project no longer has, read as there. Build them afresh before a
//! check that must see the tree as it is (this repository's `just rust-doc` removes
//! them first).
//!
//! # Reading the HTML
//!
//! rustdoc's HTML is no documented contract: this adapter reads the layout rustdoc
//! writes today, and a layout it does not find is "cannot tell" or "absent" as
//! below, never an error. The index of each crate must carry rustdoc's mark
//! ([`GENERATOR_MARK`]); a crate whose index does not is not read, and each of its
//! names cannot be told. Each name of a directory is compared as written, case
//! included, from the listing of its directory, so a file system that does not tell
//! case apart answers as one that does. A page larger than [`MAX_PAGE_BYTES`] is
//! not read, and what it would tell cannot be told.
//!
//! | a segment | is found as |
//! |---|---|
//! | the crate | the directory of the crate with an `index.html` that rustdoc wrote |
//! | a module | a directory `<name>/` with an `index.html`, in its module's directory (a private module has none: rustdoc writes only pages that redirect) |
//! | an item | a file `<kind>.<Name>.html` in its module's directory, for each kind rustdoc writes ([`ITEM_KINDS`]) |
//! | an item re-exported with no page of its own | the anchor `id="reexport.<Name>"` in its module's `index.html`, whose link is followed to the item's page |
//! | a member of a type | an anchor `id="<kind>.<name>"` in the type's page, for each kind of member ([`MEMBER_KINDS`]) |
//!
//! A segment before the last is a module first, then an item whose members the
//! last segment names. A path is there when every segment is found, and absent when
//! the docs of its crate are there and a segment is not. It cannot be told when the
//! docs of its crate are not built or not rustdoc's, when a directory or a page on
//! the way cannot be read, or when it goes on through a re-export whose link leads
//! nowhere readable (another crate whose docs are not built, a URL, or a place
//! outside the docs once symbolic links are resolved). A link is followed with `/`
//! and `\` both read as separators.
//!
//! # Known limits
//!
//! - A name reached only through a glob re-export of a public module
//!   (`pub use module::*`) reads as absent: rustdoc writes no anchor of its own for it.
//! - A member reached only through a type alias reads as absent: rustdoc writes the
//!   members of the aliased type outside the page of the alias.
//! - An item hidden from the docs (`#[doc(hidden)]`) reads as absent.
//! - A member that a type has through `Deref`, or through a blanket impl, reads as
//!   there: rustdoc writes it on the type's page.
//! - A raw identifier (`r#type`) is not a name.
//!
//! # What a reading has
//!
//! An item that is there has the hash of [`Comparison::Presence`] only
//! ([`ArtifactReading::there`]): its docs and its code are not compared, so a link
//! that compares anything else cannot be told, with that reason. There is no
//! address, and nothing is read back.
//!
//! [`Comparison::Presence`]: whyshelf_core::domain::shelf::artifact::Comparison::Presence

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, Space, SpaceAsks, SpaceReadings,
};
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

/// The name of the space this adapter resolves names in.
pub const RUST_ITEM_SPACE: &str = "rust-item";

/// The name of the preset this adapter offers.
pub const RUSTDOC_PRESET: &str = "rustdoc";

/// The name of the build directory cargo uses beside a `Cargo.toml` when nothing
/// names another.
pub const TARGET_DIR_NAME: &str = "target";

/// The mark rustdoc writes in the head of each page: an index without it is no
/// docs this adapter reads.
pub const GENERATOR_MARK: &str = "<meta name=\"generator\" content=\"rustdoc\">";

/// The most bytes of a page this adapter reads; a larger one cannot be told.
pub const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;

/// What a reading says of any comparison but presence.
const PRESENCE_ONLY: &str =
    "the space `rust-item` compares only presence: the docs and the code of an item are not read";

/// The kinds of item rustdoc writes a page `<kind>.<Name>.html` for.
pub const ITEM_KINDS: &[&str] = &[
    "struct",
    "enum",
    "union",
    "trait",
    "traitalias",
    "fn",
    "type",
    "constant",
    "static",
    "macro",
    "attr",
    "derive",
    "primitive",
    "keyword",
    "foreigntype",
];

/// The kinds of item whose page has members a path can name.
const TYPE_KINDS: &[&str] = &[
    "struct",
    "enum",
    "union",
    "trait",
    "traitalias",
    "primitive",
    "foreigntype",
];

/// The kinds of member rustdoc writes an anchor `id="<kind>.<name>"` for in the page
/// of a type.
pub const MEMBER_KINDS: &[&str] = &[
    "method",
    "tymethod",
    "variant",
    "structfield",
    "associatedconstant",
    "associatedtype",
];

/// The presets this adapter offers when it resolves the names of `space`: one,
/// [`RUSTDOC_PRESET`], for a link to the items whose docs state a rule, that stands
/// on their being there (`space` that space, `compare = "presence"`,
/// `means = "depends"`): an edit of an item changes nothing, and an item that goes
/// away breaks the approvals that stand on it.
pub fn presets(space: &Space) -> BuiltinPresets {
    let values = PresetValues {
        space: Some(space.as_str().to_owned()),
        compare: Some("presence".to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    match PresetName::new(RUSTDOC_PRESET) {
        Ok(name) => BuiltinPresets::from([(name, values)]),
        // Not a name: no preset. The test of this module holds it to be one.
        Err(_) => BuiltinPresets::new(),
    }
}

/// Why the items could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum RustItemError {
    /// The domain refused a reason this adapter gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// The items of the Rust crates of a project, as their built docs know them.
#[derive(Debug, Clone)]
pub struct RustItemSpace {
    root: PathBuf,
    /// `CARGO_TARGET_DIR`, as the caller read it.
    target_dir: Option<PathBuf>,
    /// The most bytes of a page read.
    max_page: u64,
}

impl RustItemSpace {
    /// The items of the project at `root`, with `target_dir` the value of
    /// `CARGO_TARGET_DIR` (read by the caller; none when it is not set).
    pub fn new(root: PathBuf, target_dir: Option<PathBuf>) -> Self {
        Self {
            root,
            target_dir: target_dir.filter(|dir| !dir.as_os_str().is_empty()),
            max_page: MAX_PAGE_BYTES,
        }
    }

    /// For each name: not a name (no Rust path), there, absent, or cannot tell
    /// (with why). Every name asked about has an answer.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, RustItemError> {
        let mut readings = SpaceReadings::new();
        let mut paths = Vec::new();
        for name in asks.keys() {
            match RustPath::parse(name.as_str()) {
                Ok(path) => paths.push((name, path)),
                Err(why) => {
                    readings.insert(name.clone(), ArtifactReading::not_a_name(reason(why)?));
                }
            }
        }
        if paths.is_empty() {
            return Ok(readings);
        }
        let docs = match self.docs().await {
            Ok(docs) => docs,
            Err(why) => {
                let why = reason(why)?;
                for (name, _) in paths {
                    readings.insert(name.clone(), ArtifactReading::cannot_tell(why.clone()));
                }
                return Ok(readings);
            }
        };
        let presence_only = reason(PRESENCE_ONLY.to_owned())?;
        let mut lookup = Lookup::new(docs, self.max_page).await;
        for (name, path) in paths {
            let reading = match lookup.find(&path).await {
                Found::There => ArtifactReading::there(presence_only.clone()),
                Found::Absent => ArtifactReading::absent(),
                Found::CannotTell(why) => ArtifactReading::cannot_tell(reason(why)?),
            };
            readings.insert(name.clone(), reading);
        }
        Ok(readings)
    }

    /// The bytes of an item now: none, since an item has no content this adapter
    /// reads.
    pub async fn current(&self, _name: &ArtifactName) -> Result<Option<Vec<u8>>, RustItemError> {
        Ok(None)
    }

    /// The directory rustdoc writes the docs of the project in (the rule on the
    /// page of the module), or why it cannot be told.
    async fn docs(&self) -> Result<PathBuf, String> {
        if let Some(dir) = &self.target_dir {
            return Ok(self.root.join(dir).join("doc"));
        }
        if let Some(dir) = self.configured_target_dir().await? {
            return Ok(self.root.join(dir).join("doc"));
        }
        if list(&self.root)
            .await
            .is_some_and(|names| names.contains("Cargo.toml"))
        {
            return Ok(self.root.join(TARGET_DIR_NAME).join("doc"));
        }
        Err(format!(
            "where the docs are built is not known: CARGO_TARGET_DIR is not set, no cargo setting at {} names build.target-dir, and it has no Cargo.toml",
            self.root.display()
        ))
    }

    /// `build.target-dir` of the first cargo settings file at the root, if one
    /// names it; why, when a file there cannot be read as settings.
    async fn configured_target_dir(&self) -> Result<Option<PathBuf>, String> {
        // Cargo uses the legacy name when both are there (with a warning).
        for name in ["config", "config.toml"] {
            let file = self.root.join(".cargo").join(name);
            let text = match tokio::fs::read(&file).await {
                Ok(bytes) => bytes,
                Err(e) if is_missing(&e) => continue,
                Err(e) => return Err(format!("{} cannot be read: {e}", file.display())),
            };
            let text = String::from_utf8(text)
                .map_err(|_| format!("{} is not UTF-8 text", file.display()))?;
            let settings: toml::Table = text
                .parse()
                .map_err(|e| format!("{} is not TOML: {e}", file.display()))?;
            return match settings.get("build").and_then(|b| b.get("target-dir")) {
                None => Ok(None),
                Some(toml::Value::String(dir)) if !dir.is_empty() => Ok(Some(PathBuf::from(dir))),
                Some(_) => Err(format!(
                    "{} names a build.target-dir that is no directory",
                    file.display()
                )),
            };
        }
        Ok(None)
    }
}

/// Whether a read failed because nothing is at the path.
fn is_missing(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// The domain's reason of a text this adapter wrote.
fn reason(why: String) -> Result<NonBlank, RustItemError> {
    NonBlank::new(why).map_err(RustItemError::Reason)
}

/// A name of the space, read: the crate and the segments after it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RustPath {
    krate: String,
    rest: Vec<String>,
}

impl RustPath {
    /// The one reader of the grammar of the names (on the page of the module).
    fn parse(text: &str) -> Result<Self, String> {
        let segments: Vec<&str> = text.split("::").collect();
        let krate = segments[0];
        if krate.contains('-') && segment_problem(&krate.replace('-', "_")).is_none() {
            return Err(format!(
                "no Rust path: a crate is named as rustdoc writes it, with `_` for `-` (`{}`)",
                krate.replace('-', "_")
            ));
        }
        for segment in &segments {
            if let Some(why) = segment_problem(segment) {
                return Err(format!("no Rust path (`crate::module::Item`): {why}"));
            }
        }
        Ok(Self {
            krate: krate.to_owned(),
            rest: segments[1..].iter().map(|s| (*s).to_owned()).collect(),
        })
    }
}

/// Why a segment of a path is no identifier, if it is none.
fn segment_problem(segment: &str) -> Option<String> {
    if segment.is_empty() {
        return Some("a segment between `::` is empty".to_owned());
    }
    if ["crate", "self", "super", "Self"].contains(&segment) {
        return Some(format!(
            "`{segment}` is relative: a path starts from the name of a crate"
        ));
    }
    if segment.starts_with("r#") {
        return Some(format!(
            "`{segment}` is a raw identifier, which is no name of the space"
        ));
    }
    let first = segment
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_');
    let rest = segment.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !first || !rest || segment == "_" {
        return Some(format!("`{segment}` is no identifier"));
    }
    None
}

/// What a path comes to in the docs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    There,
    Absent,
    CannotTell(String),
}

/// Where a walk along a path is.
#[derive(Debug, Clone)]
enum At {
    /// The directory of a module (or of a crate).
    Module(PathBuf),
    /// The page of an item.
    Item { page: PathBuf, kind: String },
    /// A member of a type: nothing is under it.
    Member,
    /// A re-export whose link cannot be followed, and why: what is under it cannot
    /// be told.
    Unfollowed(String),
}

/// The docs, read for one resolution: each directory listed once, each page read
/// once.
struct Lookup {
    docs: PathBuf,
    /// The docs with every symbolic link resolved, when they can be.
    real_docs: Option<PathBuf>,
    max_page: u64,
    listings: BTreeMap<PathBuf, Option<BTreeSet<String>>>,
    pages: BTreeMap<PathBuf, Result<Arc<str>, String>>,
}

impl Lookup {
    async fn new(docs: PathBuf, max_page: u64) -> Self {
        Self {
            real_docs: tokio::fs::canonicalize(&docs).await.ok(),
            docs,
            max_page,
            listings: BTreeMap::new(),
            pages: BTreeMap::new(),
        }
    }

    /// The names of the entries of a directory, or none when it cannot be listed.
    async fn listing(&mut self, dir: &Path) -> Option<&BTreeSet<String>> {
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

    /// The text of a page, or why it cannot be read.
    async fn page(&mut self, page: &Path) -> Result<Arc<str>, String> {
        if !self.pages.contains_key(page) {
            let text = read(page, self.max_page).await.map(Arc::from);
            self.pages.insert(page.to_path_buf(), text);
        }
        self.pages
            .get(page)
            .cloned()
            .unwrap_or_else(|| Err(format!("{} cannot be read", page.display())))
    }

    async fn find(&mut self, path: &RustPath) -> Found {
        let crate_dir = self.docs.join(&path.krate);
        let built = match self.has(&self.docs.clone(), &path.krate).await {
            Some(true) => self.has(&crate_dir, "index.html").await == Some(true),
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
        match self.page(&index).await {
            Ok(html) if html.contains(GENERATOR_MARK) => {}
            Ok(_) => {
                return Found::CannotTell(format!(
                    "{} is not rustdoc's: it has no {GENERATOR_MARK}",
                    index.display()
                ));
            }
            Err(why) => return Found::CannotTell(why),
        }
        let mut at = At::Module(crate_dir);
        for segment in &path.rest {
            at = match self.step(at, segment).await {
                Ok(Some(next)) => next,
                Ok(None) => return Found::Absent,
                Err(why) => return Found::CannotTell(why),
            };
        }
        Found::There
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
                let named = MEMBER_KINDS
                    .iter()
                    .any(|member| html.contains(&format!("id=\"{member}.{segment}\"")));
                Ok(named.then_some(At::Member))
            }
            At::Member => Ok(None),
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
            if self.has(&module, "index.html").await == Some(true) {
                return Ok(Some(At::Module(module)));
            }
        }
        if let Some(kind) = ITEM_KINDS
            .iter()
            .find(|kind| names.contains(&format!("{kind}.{segment}.html")))
        {
            return Ok(Some(At::Item {
                page: dir.join(format!("{kind}.{segment}.html")),
                kind: (*kind).to_owned(),
            }));
        }
        if !names.contains("index.html") {
            return Ok(None);
        }
        let html = self.page(&dir.join("index.html")).await?;
        let Some(link) = reexport_link(&html, segment) else {
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
async fn list(dir: &Path) -> Option<BTreeSet<String>> {
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

/// The text of a page of at most `max` bytes, or why it cannot be read.
async fn read(page: &Path, max: u64) -> Result<String, String> {
    let cannot = |e: std::io::Error| format!("{} cannot be read: {e}", page.display());
    let file = tokio::fs::File::open(page).await.map_err(cannot)?;
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(cannot)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > max) {
        return Err(format!(
            "{} is larger than {max} bytes, and is not read",
            page.display()
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Comparison, Untold};

    use super::*;

    fn n(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn asks(names: &[&str]) -> SpaceAsks {
        names
            .iter()
            .map(|t| (n(t), BTreeSet::from([Comparison::Presence])))
            .collect()
    }

    /// The items whose docs are under `target`, given as `CARGO_TARGET_DIR`.
    fn at_target(target: &Path) -> RustItemSpace {
        RustItemSpace::new(PathBuf::from("/nowhere"), Some(target.to_path_buf()))
    }

    fn there() -> ArtifactReading {
        ArtifactReading::there(NonBlank::new(PRESENCE_ONLY).unwrap())
    }

    async fn write(dir: &Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(full, text).await.unwrap();
    }

    /// A page with these anchors, as rustdoc writes them.
    fn page(ids: &[&str]) -> String {
        let anchors: String = ids
            .iter()
            .map(|id| format!("<section id=\"{id}\" class=\"method\"></section>"))
            .collect();
        format!(
            "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><main>{anchors}</main></body></html>"
        )
    }

    /// The index of a module with these re-exports, each `(name, link)`.
    fn index(reexports: &[(&str, Option<&str>)]) -> String {
        let entries: String = reexports
            .iter()
            .map(|(name, link)| match link {
                Some(href) => format!(
                    "<dt id=\"reexport.{name}\"><code>pub use inner::<a class=\"struct\" href=\"{href}\" title=\"x\">{name}</a>;</code></dt>"
                ),
                None => format!("<dt id=\"reexport.{name}\"><code>pub use {name};</code></dt>"),
            })
            .collect();
        format!(
            "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><h2 id=\"reexports\">Re-exports</h2><dl>{entries}</dl></body></html>"
        )
    }

    /// A page rustdoc writes for an item of a private module: it only redirects.
    fn redirect(to: &str) -> String {
        format!(
            "<!DOCTYPE html><html><head><meta http-equiv=\"refresh\" content=\"0;URL={to}\"><title>Redirection</title></head><body><p>Redirecting to <a href=\"{to}\">{to}</a>...</p></body></html>"
        )
    }

    /// A build directory with the docs of the crate `demo`, laid out as rustdoc
    /// writes them; the crate `other` is not built, and `fake` is not rustdoc's.
    async fn demo() -> tempfile::TempDir {
        let target = tempfile::tempdir().unwrap();
        let d = target.path();
        write(
            d,
            "doc/demo/index.html",
            &index(&[
                ("Thing", Some("inner/struct.Thing.html")),
                ("Back", Some("inner\\struct.Thing.html")),
                ("infra", Some("../demo_infra/index.html")),
                ("ext", Some("../other/index.html")),
                (
                    "Far",
                    Some("https://docs.rs/far/latest/far/struct.Far.html"),
                ),
                ("bare", None),
            ]),
        )
        .await;
        write(
            d,
            "doc/demo/struct.Shelf.html",
            &page(&[
                "method.build",
                "structfield.root",
                "associatedconstant.MAX",
                "method.fmt-1",
            ]),
        )
        .await;
        write(
            d,
            "doc/demo/enum.Cause.html",
            &page(&["variant.Gone", "method.severity"]),
        )
        .await;
        write(
            d,
            "doc/demo/trait.Source.html",
            &page(&["tymethod.resolve", "method.spaces", "associatedtype.Error"]),
        )
        .await;
        for kind_and_name in [
            "fn.expand",
            "type.Asks",
            "constant.MAX",
            "static.COUNT",
            "macro.shelf",
            "union.Raw",
            "attr.marker",
            "derive.Shelfish",
        ] {
            write(d, &format!("doc/demo/{kind_and_name}.html"), &page(&[])).await;
        }
        write(d, "doc/demo/sidebar-items.js", "").await;
        write(d, "doc/demo/inner/index.html", &index(&[])).await;
        write(
            d,
            "doc/demo/inner/struct.Thing.html",
            &page(&["method.new"]),
        )
        .await;
        write(d, "doc/demo/inner/fn.helper.html", &page(&[])).await;
        // A private module: no index, only pages that redirect to the item's place.
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
        target
    }

    fn unknown_why(reading: &ArtifactReading) -> Option<String> {
        match reading.state(&Comparison::Presence) {
            ArtifactState::Unknown(Untold::Said(why)) => Some(why.as_str().to_owned()),
            _ => None,
        }
    }

    #[test]
    fn the_adapter_offers_the_preset_rustdoc_for_its_space() {
        let space = Space::new(RUST_ITEM_SPACE).unwrap();
        assert_eq!(
            presets(&space),
            BuiltinPresets::from([(
                PresetName::new(RUSTDOC_PRESET).unwrap(),
                PresetValues {
                    space: Some("rust-item".to_owned()),
                    compare: Some("presence".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                }
            )])
        );
    }

    #[tokio::test]
    async fn a_path_found_in_the_docs_is_there_with_the_presence_hash_only() {
        let target = demo().await;
        let source = at_target(target.path());
        let present = [
            "demo",
            "demo::inner",
            "demo::Shelf",
            "demo::Shelf::build",
            "demo::Shelf::root",
            "demo::Shelf::MAX",
            "demo::Cause",
            "demo::Cause::Gone",
            "demo::Cause::severity",
            "demo::Source",
            "demo::Source::resolve",
            "demo::Source::spaces",
            "demo::Source::Error",
            "demo::expand",
            "demo::Asks",
            "demo::MAX",
            "demo::COUNT",
            "demo::shelf",
            "demo::Raw",
            "demo::marker",
            "demo::Shelfish",
            "demo::inner::Thing",
            "demo::inner::Thing::new",
            "demo::inner::helper",
            // Re-exported with no page of its own: found through the index, and
            // followed to the page or the module it links.
            "demo::Thing",
            "demo::Thing::new",
            "demo::Back::new",
            "demo::infra",
            "demo::infra::open",
            "demo::ext",
            "demo::Far",
            "demo::bare",
        ];
        let found = source.resolve(&asks(&present)).await.unwrap();
        for name in present {
            assert_eq!(found[&n(name)], there(), "{name}");
        }
        // Any comparison but presence cannot be told, and says why.
        let why = match found[&n("demo::Shelf")].state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => why.as_str().to_owned(),
            other => panic!("{other:?}"),
        };
        assert!(why.contains("compares only presence"), "{why}");
    }

    #[tokio::test]
    async fn a_path_the_built_docs_do_not_have_is_absent() {
        let target = demo().await;
        let source = at_target(target.path());
        let absent = [
            "demo::nowhere",
            "demo::nowhere::Shelf",
            "demo::Missing",
            "demo::Shelf::missing",
            // A member of a page that is no type's, and under a member.
            "demo::expand::x",
            "demo::Shelf::build::x",
            "demo::Asks::len",
            // Compared as written, on every file system.
            "demo::shelF",
            "demo::Shelf::Build",
            "demo::INNER",
            // A page that is no item's, and an anchor that is no member's.
            "demo::sidebar",
            "demo::Thing::missing",
            "demo::infra::close",
            // Through a private module, whose pages only redirect.
            "demo::hidden",
            "demo::hidden::expand",
        ];
        let found = source.resolve(&asks(&absent)).await.unwrap();
        for name in absent {
            assert_eq!(found[&n(name)], ArtifactReading::absent(), "{name}");
        }
    }

    #[tokio::test]
    async fn a_crate_whose_docs_are_not_built_cannot_be_told() {
        let target = demo().await;
        let source = at_target(target.path());
        let names = [
            "other",
            "other::Item",
            // Through a re-export into docs that are not built, or not here.
            "demo::ext::Item",
            "demo::Far::new",
            "demo::bare::x",
        ];
        let found = source.resolve(&asks(&names)).await.unwrap();
        for name in names {
            assert!(unknown_why(&found[&n(name)]).is_some(), "{name}");
        }
        let why = unknown_why(&found[&n("other::Item")]).unwrap();
        assert!(
            why.contains("the docs of the crate `other` are not built"),
            "{why}"
        );
        // A crate directory without its index is not built either.
        write(target.path(), "doc/half/struct.A.html", &page(&[])).await;
        let found = source.resolve(&asks(&["half::A"])).await.unwrap();
        assert!(unknown_why(&found[&n("half::A")]).is_some());
        // Nor is anything, with no docs at all.
        let empty = tempfile::tempdir().unwrap();
        let found = at_target(empty.path())
            .resolve(&asks(&["demo::Shelf"]))
            .await
            .unwrap();
        assert!(unknown_why(&found[&n("demo::Shelf")]).is_some());
    }

    #[tokio::test]
    async fn a_layout_that_is_not_rustdocs_cannot_be_told() {
        let target = demo().await;
        let source = at_target(target.path());
        let found = source
            .resolve(&asks(&["fake", "fake::A", "fake::Missing"]))
            .await
            .unwrap();
        for name in ["fake", "fake::A", "fake::Missing"] {
            let why = unknown_why(&found[&n(name)]);
            assert!(
                why.as_deref()
                    .is_some_and(|w| w.contains("is not rustdoc's")),
                "{name}: {why:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_page_larger_than_the_most_read_cannot_be_told() {
        let target = demo().await;
        let source = RustItemSpace {
            max_page: 200,
            ..at_target(target.path())
        };
        write(
            target.path(),
            "doc/tiny/index.html",
            &format!("<html><head>{GENERATOR_MARK}</head></html>"),
        )
        .await;
        write(
            target.path(),
            "doc/tiny/struct.Big.html",
            &page(&["method.x"; 20]),
        )
        .await;
        let found = source
            .resolve(&asks(&["tiny::Big", "tiny::Big::x", "demo::Shelf"]))
            .await
            .unwrap();
        // The page is listed, so the item is there: only what the page would tell
        // cannot be told, and neither can anything of a crate whose index is large.
        assert_eq!(found[&n("tiny::Big")], there());
        for name in ["tiny::Big::x", "demo::Shelf"] {
            let why = unknown_why(&found[&n(name)]).unwrap();
            assert!(why.contains("larger than 200 bytes"), "{name}: {why}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_through_a_symbolic_link_out_of_the_docs_is_not_followed() {
        let target = demo().await;
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "struct.Esc.html", &page(&["method.new"])).await;
        tokio::fs::symlink(outside.path(), target.path().join("doc/demo/out"))
            .await
            .unwrap();
        write(
            target.path(),
            "doc/demo/index.html",
            &index(&[("Esc", Some("out/struct.Esc.html"))]),
        )
        .await;
        let found = at_target(target.path())
            .resolve(&asks(&["demo::Esc", "demo::Esc::new"]))
            .await
            .unwrap();
        assert_eq!(found[&n("demo::Esc")], there());
        let why = unknown_why(&found[&n("demo::Esc::new")]).unwrap();
        assert!(why.contains("from docs that are not here"), "{why}");
    }

    #[tokio::test]
    async fn a_text_that_is_no_rust_path_is_not_a_name() {
        // No build directory is looked for for a text that is no name, so one that
        // cannot be found changes nothing for it.
        let nowhere = tempfile::tempdir().unwrap();
        let wrongs = [
            ("demo::", "empty"),
            ("::demo", "empty"),
            ("demo:::Shelf", "no identifier"),
            ("demo:Shelf", "no identifier"),
            ("demo::Vec<T>", "no identifier"),
            ("demo::a b", "no identifier"),
            ("demo/Shelf", "no identifier"),
            ("demo::1st", "no identifier"),
            ("demo::_", "no identifier"),
            ("demo::r#type", "raw identifier"),
            ("crate::Shelf", "relative"),
            ("self::Shelf", "relative"),
            ("demo::super::Shelf", "relative"),
            ("demo::Shelf::Self", "relative"),
            ("my-crate::Shelf", "with `_` for `-` (`my_crate`)"),
            ("demo::my-mod", "no identifier"),
        ];
        let names: Vec<&str> = wrongs.iter().map(|(w, _)| *w).collect();
        let target = demo().await;
        for source in [
            at_target(target.path()),
            RustItemSpace::new(nowhere.path().to_path_buf(), None),
        ] {
            let found = source.resolve(&asks(&names)).await.unwrap();
            for (wrong, says) in wrongs {
                let why = found[&n(wrong)].not_a_name_why().map(|w| w.as_str());
                assert!(
                    why.is_some_and(|w| w.starts_with("no Rust path") && w.contains(says)),
                    "{wrong}: {why:?}"
                );
            }
            assert_eq!(source.current(&n("demo::Shelf")).await.unwrap(), None);
        }
    }

    #[tokio::test]
    async fn the_build_directory_is_the_variable_then_the_setting_then_beside_cargo_toml() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let docs = |space: RustItemSpace| async move { space.docs().await };
        // Nothing to tell it by.
        let why = docs(RustItemSpace::new(r.to_path_buf(), None))
            .await
            .unwrap_err();
        assert!(
            why.contains("where the docs are built is not known"),
            "{why}"
        );
        // Beside the Cargo.toml.
        write(r, "Cargo.toml", "[workspace]\n").await;
        assert_eq!(
            docs(RustItemSpace::new(r.to_path_buf(), None)).await,
            Ok(r.join(TARGET_DIR_NAME).join("doc"))
        );
        // The settings, by their name of today; then, with both names there, the
        // legacy one, which cargo uses (with a warning) when it finds both.
        write(r, ".cargo/config.toml", "[build]\ntarget-dir = \"built\"\n").await;
        assert_eq!(
            docs(RustItemSpace::new(r.to_path_buf(), None)).await,
            Ok(r.join("built/doc"))
        );
        write(r, ".cargo/config", "[build]\ntarget-dir = \"old\"\n").await;
        assert_eq!(
            docs(RustItemSpace::new(r.to_path_buf(), None)).await,
            Ok(r.join("old/doc"))
        );
        // A legacy file that names no build directory leaves it to none: cargo
        // reads one of the two, not both.
        write(r, ".cargo/config", "[build]\njobs = 2\n").await;
        assert_eq!(
            docs(RustItemSpace::new(r.to_path_buf(), None)).await,
            Ok(r.join(TARGET_DIR_NAME).join("doc"))
        );
        tokio::fs::remove_file(r.join(".cargo/config"))
            .await
            .unwrap();
        // The variable wins, relative to the root or not.
        assert_eq!(
            docs(RustItemSpace::new(
                r.to_path_buf(),
                Some(PathBuf::from("env"))
            ))
            .await,
            Ok(r.join("env/doc"))
        );
        assert_eq!(
            docs(RustItemSpace::new(
                r.to_path_buf(),
                Some(PathBuf::from("/abs/dir"))
            ))
            .await,
            Ok(PathBuf::from("/abs/dir/doc"))
        );
        // An empty variable is none.
        assert_eq!(
            docs(RustItemSpace::new(r.to_path_buf(), Some(PathBuf::new()))).await,
            Ok(r.join("built/doc"))
        );
        // Settings that cannot be read tell nothing.
        write(r, ".cargo/config.toml", "[build\n").await;
        let why = docs(RustItemSpace::new(r.to_path_buf(), None))
            .await
            .unwrap_err();
        assert!(why.contains("is not TOML"), "{why}");
        write(r, ".cargo/config.toml", "[build]\ntarget-dir = 3\n").await;
        let why = docs(RustItemSpace::new(r.to_path_buf(), None))
            .await
            .unwrap_err();
        assert!(why.contains("no directory"), "{why}");
        // And every name it is asked of cannot be told.
        let found = RustItemSpace::new(r.to_path_buf(), None)
            .resolve(&asks(&["demo::Shelf"]))
            .await
            .unwrap();
        assert!(unknown_why(&found[&n("demo::Shelf")]).is_some());
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

    /// The `rustdoc` paths the decisions of this repository list, each with the key
    /// of its decision.
    async fn decisions_rustdoc(root: &Path) -> Vec<(String, String)> {
        let mut paths = Vec::new();
        let mut entries = tokio::fs::read_dir(root.join("books/decision"))
            .await
            .unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let file = entry.file_name().into_string().unwrap();
            let Some(key) = file.strip_suffix(".md") else {
                continue;
            };
            let text = tokio::fs::read_to_string(entry.path()).await.unwrap();
            for line in text.lines() {
                if let Some(list) = line.strip_prefix("rustdoc: ") {
                    let items: Vec<String> = serde_json::from_str(list).unwrap();
                    paths.extend(items.into_iter().map(|p| (key.to_owned(), p)));
                }
            }
        }
        paths.sort();
        paths
    }

    /// Every `rustdoc` path of this repository's decisions resolves in the docs
    /// built here (`just rust-doc` builds them). Skipped, with a note, only when
    /// they are not built.
    #[tokio::test]
    async fn the_rustdoc_paths_of_this_repository_resolve_in_its_built_docs() {
        let root = tokio::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .await
            .unwrap();
        let source = RustItemSpace::new(
            root.clone(),
            std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from),
        );
        let docs = source.docs().await.unwrap();
        if list(&docs.join("whyshelf_core")).await.is_none() {
            eprintln!(
                "skipped: the docs of this repository are not built (`just rust-doc` builds them)"
            );
            return;
        }
        let paths = decisions_rustdoc(&root).await;
        assert!(!paths.is_empty());
        let names: SpaceAsks = paths
            .iter()
            .map(|(_, p)| (n(p), BTreeSet::from([Comparison::Presence])))
            .collect();
        let found = source.resolve(&names).await.unwrap();
        let wrong: Vec<String> = paths
            .iter()
            .filter_map(|(key, path)| {
                let reading = &found[&n(path)];
                if let Some(why) = reading.not_a_name_why() {
                    return Some(format!("{key}: {path}: not a name: {why}"));
                }
                match reading.state(&Comparison::Presence) {
                    ArtifactState::Present(_) => None,
                    ArtifactState::Absent => Some(format!("{key}: {path}: absent")),
                    ArtifactState::Unknown(why) => {
                        Some(format!("{key}: {path}: cannot tell: {why}"))
                    }
                }
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "rustdoc paths that do not resolve:\n{}",
            wrong.join("\n")
        );
    }
}
