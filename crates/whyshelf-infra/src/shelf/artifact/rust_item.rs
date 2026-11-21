//! The adapter of the space `rust-item`: the items of the Rust crates of a project,
//! by their path, as the documentation `cargo doc` writes knows them.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`RustItemSpace::resolve`] | finds the build directory of the project, looks up each path in the HTML rustdoc wrote under it, and reads what the item promises from its page | `tokio::fs`, `toml` |
//! | [`RustItemSpace::current`] | the text of what an item promises now | the same |
//! | [`presets`] | the presets `rustdoc` (the space, `compare = "presence"`, `means = "depends"`) and `rustdoc-promise` (the space, `compare = "lines"`, `means = "depends"`) | values |
//!
//! ```text
//!   name ──a Rust path?── no ──► not a name (why)
//!     │ yes
//!   the build directory: CARGO_TARGET_DIR | build.target-dir of the root's cargo
//!   config | the conventional one beside the root's Cargo.toml
//!     ├─ none of them ──► cannot tell, each name
//!     └─ its doc directory, then the directory of the crate in it
//!          ├─ no index.html, or one rustdoc did not write ──► cannot tell
//!          └─ the path, segment by segment, in the HTML ──► absent | cannot tell (why)
//!               │ there                                      (a place: page, kind, member)
//!               └─ the promise of the place, read from its page
//!                    ├─ a promise ──► its text, hashed by each comparison asked
//!                    └─ not readable ──► there, for presence only (why)
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
//! # Positions
//!
//! A name holds no position in a page or in the source: it names an item whole.
//!
//! # Equal names
//!
//! Segments are compared as written, case included. A name through a re-export and
//! the name of the item it leads to name one item, and have its promise (below). A
//! crate is written as rustdoc writes it (`whyshelf_core`), and a raw identifier is
//! no name, so no other spelling of a segment names the same item. No name is
//! rewritten into another.
//!
//! # One thing for a name
//!
//! A segment before the last is a module first, then an item whose members the last
//! segment names; an item is looked up in the order of [`ITEM_KINDS`]. Members of one
//! name on a page (`method.from` and `method.from-1`) are told by the first the page
//! has, and members of one name and two kinds by the order of [`MEMBER_KINDS`] (Known
//! limits, below).
//!
//! # Prefixes of the names
//!
//! `judge moved --prefix` moves every name under a prefix at once, and this adapter
//! says which names are under it and what each becomes
//! ([`NamePrefixes`]).
//! A prefix is a Rust path of one segment or more, read by the rules of a name's path
//! (so `crate::m`, a raw identifier or a directory is none). A name is under it when
//! its segments start with all of the prefix's: `app::m` is a prefix of `app::m`,
//! `app::m::f` and `app::m::Type::member`, and of no `app::mx::f`; the same name under
//! `to` is the segments of `to` and the rest of the name's. A `from` that is no Rust
//! path is no prefix of the space's names, and a `to` that is none makes each name
//! under `from` unbuildable, with why. Nothing is read of the docs: the answer is of
//! the names alone, and `resolve` tells whether each new name is there.
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
//! cannot be read, are larger than [`MAX_SETTINGS_BYTES`], or are reached through a
//! symbolic link that leaves the root, no name can be told.
//! Cargo settings in the directories above the
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
//! not read, and what it would tell cannot be told. Neither is a page, or the
//! directory of a module, that leads outside both the root of the project and the
//! build directory once symbolic links are resolved: the build directory may be
//! outside the root, and is read where it is.
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
//! # What an item promises
//!
//! What an item promises is its declaration and its docs, as its page says them:
//!
//! | of | the declaration | the docs |
//! |---|---|---|
//! | an item (a struct, an enum, a trait, a function, a macro, ...) | the `item-decl` of its page | its top doc |
//! | a member (a method, a field, a variant, ...) | its code header | the docblock right after it (a method's, inside its toggle) |
//! | a variant | its code header | the docblock, then the fields the page lists under it |
//! | a module, a crate, a primitive type, a keyword | none: they have none | their top doc |
//!
//! The body of a function, the list of the impls of a type, the other members of its
//! page, the line `Source` and the sidebar are not in it. An edit of a function's
//! body, of an inherent method or of an impl changes nothing of the type. The
//! declaration of a type as rustdoc writes it lists its public fields (a struct), its
//! variants (an enum) and, for a trait, the signatures of its items (`fn resolve(&self)
//! -> u32;`, `type Item;`), so those are part of the type's promise: a public field,
//! a variant or a trait item added, dropped or retyped makes an approval that names
//! the type stale. The declaration of a constant holds its value (`pub const MAX:
//! u32 = 1;`, an associated constant's too), so a changed value does the same. A
//! member named on its own is its own promise. An item with no docs promises that,
//! and is told so: writing docs for it, or taking them away, is a change of its
//! promise. The text is the declaration and the docs, each line after `decl ` or
//! `doc ` (an empty line, empty), with the line breaks rustdoc wrote and no white
//! space collapsed by this adapter; whether white space and blank lines count is the
//! link's `compare` (`lines` collapses white space and leaves out blank lines,
//! `bytes` does neither). The rules of the text are on the page of the module that
//! reads a page (`promise`, private).
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
//! - rustdoc's HTML is no contract: the layout read is the one of today. After the
//!   toolchain is raised, a declaration written another way (or a layout this adapter
//!   no longer reads) can make an approval stale, or an item unreadable, with no
//!   change of the code; this repository's end-to-end tests run `cargo doc` and watch
//!   the layout of the pinned toolchain.
//! - Members of one name (`method.from` and `method.from-1`) are told by the first the
//!   page has: the promise is that one's. Members of one name and two kinds (a field
//!   and a method `len`) are told by the order of [`MEMBER_KINDS`], not of the page:
//!   the promise is the method's.
//! - The docs of a method of a trait impl or of a blanket impl are the trait's (the
//!   text of the standard library, with its "Read more", included) and can change
//!   with the toolchain.
//! - What `span.item-info` says (deprecated, a feature a thing needs) is not in the
//!   promise.
//! - The impls of a type (the traits it implements, the methods it gains or loses)
//!   are not in the promise of the type: name a member to pin it. Its public fields,
//!   its variants and (a trait) its items' signatures are, for they are in its
//!   declaration.
//! - The value of a constant is in its declaration (`pub const MAX: u32 = 1;`, an
//!   associated constant's too), so a changed value is a change of its promise.
//! - A re-export is the promise of the item it leads to. A name through a glob
//!   re-export, a type alias or `#[doc(hidden)]` is as above. An item a macro makes
//!   is as rustdoc drew it.
//!
//! # What a reading has
//!
//! An item that is there and whose promise is read has the hashes of that promise's
//! text under each comparison asked ([`ArtifactReading::of_bytes`]): the preset
//! `rustdoc` (`presence`) tells only that it is there, and its page is not read when
//! presence is all that is asked; an edit of its docs or declaration is a change for
//! `lines` and `bytes`, and an edit of a body is not. One whose page cannot be read, or
//! has no declaration where its kind has one, or is laid out another way, has the hash
//! of [`Comparison::Presence`] only ([`ArtifactReading::there`]), and says why for any
//! other comparison: it is told as cannot tell, not as changed. There is no address,
//! and nothing is read back; but [`RustItemSpace::current`] returns the text of the
//! promise now, whatever the link compares, so the library's diff report item of an
//! approval of a link of any comparison, `presence` of the preset `rustdoc` included,
//! carries the text of what the item promises now (`current`). The `diff` command
//! shows none: it shows the current text only when the approved text can be read
//! back, and no text is kept at approval (see DEC-83's revisit). An approval's
//! freshness is still the comparison's, not the text's: an edit of the docs leaves a
//! `presence` approval fresh.
//!
//! # The answers
//!
//! | answer | when |
//! |---|---|
//! | there | every segment is found; with the hashes of its promise, or, when its page cannot be read as a promise (or only presence is asked), for presence only, with why |
//! | absent | the docs of its crate are there and rustdoc's, and a segment is not found |
//! | cannot tell | where the docs are cannot be told; the docs of its crate are not built or not rustdoc's; a directory or a page on the way cannot be read, is too large, or leads outside the root and the build directory; or it goes on through a re-export whose link leads nowhere readable |
//! | not a name | the text is no Rust path of the grammar above |
//!
//! # What is compared
//!
//! The content of a name is the text of what its item promises (above), read from a
//! page of at most [`MAX_PAGE_BYTES`]; the cargo settings are read up to
//! [`MAX_SETTINGS_BYTES`]. What of the text counts is the link's comparison.
//!
//! # Where it reads
//!
//! The cargo settings at the root of the project, and the docs in the build
//! directory it declares there or the caller gives (where the docs are, above),
//! which may be outside the root; nothing outside the root and that directory.
//!
//! [`Comparison::Presence`]: whyshelf_core::domain::shelf::artifact::Comparison::Presence
//! [`ArtifactReading::of_bytes`]: whyshelf_core::domain::shelf::artifact::ArtifactReading::of_bytes

mod promise;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use whyshelf_core::app::shelf::{
    Answers, CannotList, CannotSuggest, NamePrefixes, NoReadBack, SpaceAdapter,
};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::{PathLiteral, RelPath};
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, Comparison, PrefixMove, Renamed, SpaceAsks, SpacePrefixes,
    SpaceReadings, SpaceRenamed,
};
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::file::{self, Bounded};
use promise::{Promise, Unreadable, promise_of};

/// The name of the space this adapter resolves names in.
pub const RUST_ITEM_SPACE: &str = "rust-item";

/// The name of the preset of a link that stands on an item being there.
pub const RUSTDOC_PRESET: &str = "rustdoc";

/// The name of the preset of a link that stands on what an item promises (its
/// declaration and its docs).
pub const RUSTDOC_PROMISE_PRESET: &str = "rustdoc-promise";

/// The name of the build directory cargo uses beside a `Cargo.toml` when nothing
/// names another.
pub const TARGET_DIR_NAME: &str = "target";

/// The mark rustdoc writes in the head of each page: an index without it is no
/// docs this adapter reads.
pub const GENERATOR_MARK: &str = "<meta name=\"generator\" content=\"rustdoc\">";

/// The most bytes of a page this adapter reads; a larger one cannot be told.
pub const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;

/// The most bytes of a file of cargo settings this adapter reads; with a larger one,
/// where the docs are cannot be told. Settings are far smaller, and a file this small
/// is read and parsed where it is asked for.
pub const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

/// The kind of the place of a module, and of a crate (its index).
const MODULE_KIND: &str = "module";

/// What a reading asked of presence only says of any other comparison.
const PRESENCE_ONLY: &str = "only presence was asked of the item, so what it promises was not read";

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
/// of a type, closed: the one table of them and of their anchor words is
/// [`MemberKind::TABLE`], in the order a name is looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberKind {
    /// A method with a body, of an impl.
    Method,
    /// A method of a trait, with no body.
    TyMethod,
    /// A variant of an enum.
    Variant,
    /// A field of a struct or a union.
    StructField,
    /// An associated constant.
    AssociatedConstant,
    /// An associated type.
    AssociatedType,
}

impl MemberKind {
    /// Every kind with the word of its anchor (`method` in `id="method.new"`), the
    /// one table of them: a name is looked up in this order.
    const TABLE: [(Self, &'static str); 6] = [
        (Self::Method, "method"),
        (Self::TyMethod, "tymethod"),
        (Self::Variant, "variant"),
        (Self::StructField, "structfield"),
        (Self::AssociatedConstant, "associatedconstant"),
        (Self::AssociatedType, "associatedtype"),
    ];

    /// The word of the anchor of this kind.
    fn word(self) -> &'static str {
        Self::TABLE
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or("", |(_, word)| word)
    }

    /// The kinds in the order a name is looked up in.
    fn all() -> impl Iterator<Item = Self> {
        Self::TABLE.iter().map(|(kind, _)| *kind)
    }

    /// The anchor words of [`Self::TABLE`], in its order.
    const fn words() -> [&'static str; 6] {
        let mut words = [""; 6];
        let mut at = 0;
        while at < words.len() {
            words[at] = Self::TABLE[at].1;
            at += 1;
        }
        words
    }
}

/// The words of the anchors of the kinds of member rustdoc writes, made from the one
/// table of the kinds of member: the lookup of a member reads the kinds in this order.
pub const MEMBER_KINDS: &[&str] = &MemberKind::words();

/// The presets this adapter offers, both `space` [`RUST_ITEM_SPACE`] and
/// `means = "depends"`:
///
/// - [`RUSTDOC_PRESET`], for a link to the items whose docs state a rule, that stands
///   on their being there (`compare = "presence"`): an edit of an item changes
///   nothing, and an item that goes away breaks the approvals that stand on it;
/// - [`RUSTDOC_PROMISE_PRESET`], for a link that stands on what an item promises
///   (`compare = "lines"`): a change of its declaration or its docs makes the
///   approvals that stand on it stale, and an edit of a body does not.
pub fn presets() -> BuiltinPresets {
    let preset = |name: &str, compare: &str| {
        let values = PresetValues {
            space: Some(RUST_ITEM_SPACE.to_owned()),
            compare: Some(compare.to_owned()),
            ignore: None,
            means: Some(Propagation::Depends),
        };
        // Not a name: no preset. The test of this module holds both to be names.
        PresetName::new(name).ok().map(|name| (name, values))
    };
    [
        preset(RUSTDOC_PRESET, "presence"),
        preset(RUSTDOC_PROMISE_PRESET, "lines"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Why the items could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum RustItemError {
    /// The domain refused a reason this adapter gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// The space [`RUST_ITEM_SPACE`]: it answers a move of a prefix, cannot suggest where
/// an item moved, keeps nothing by address, and lists no names ([`CannotList`]).
impl SpaceAdapter for RustItemSpace {
    type Error = RustItemError;
    type Prefixes = Answers;
    type Suggest = CannotSuggest;
    type ReadBack = NoReadBack;
    type List = CannotList;

    fn space(&self) -> &str {
        RUST_ITEM_SPACE
    }

    fn presets(&self) -> BuiltinPresets {
        presets()
    }

    async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, RustItemError> {
        RustItemSpace::resolve(self, asks).await
    }

    async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, RustItemError> {
        RustItemSpace::current(self, name).await
    }
}

impl NamePrefixes for RustItemSpace {
    fn renamed(
        &self,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), RustItemError> {
        renamed(names, of).map_err(RustItemError::Reason)
    }
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

    /// For each name: not a name (no Rust path), there (with the hash of what it
    /// promises under each comparison asked, or under presence only, with why, when
    /// its page cannot be read), absent, or cannot tell (with why). Every name asked
    /// about has an answer.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, RustItemError> {
        let mut readings = SpaceReadings::new();
        let mut paths = Vec::new();
        for (name, comparisons) in asks {
            match RustPath::parse(name.as_str()) {
                Ok(path) => paths.push((name, comparisons, path)),
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
                for (name, _, _) in paths {
                    readings.insert(name.clone(), ArtifactReading::cannot_tell(why.clone()));
                }
                return Ok(readings);
            }
        };
        let presence_only = reason(PRESENCE_ONLY.to_owned())?;
        let mut lookup = Lookup::new(&self.root, docs, self.max_page).await;
        for (name, comparisons, path) in paths {
            let reading = match lookup.find(&path).await {
                // Only presence asked (the preset `rustdoc`): its page is not read.
                Found::There(_) if comparisons.iter().all(|c| *c == Comparison::Presence) => {
                    ArtifactReading::there(presence_only.clone())
                }
                Found::There(place) => {
                    let promise = lookup.promise(&place).await;
                    reading_of(promise, comparisons).await?
                }
                Found::Unread(why) => ArtifactReading::there(reason(why)?),
                Found::Absent => ArtifactReading::absent(),
                Found::CannotTell(why) => ArtifactReading::cannot_tell(reason(why)?),
            };
            readings.insert(name.clone(), reading);
        }
        Ok(readings)
    }

    /// The text of what an item promises now (what a link that compares its text
    /// compares), or none when the name is none, or the item is not there, or its
    /// promise cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, RustItemError> {
        let Ok(path) = RustPath::parse(name.as_str()) else {
            return Ok(None);
        };
        let Ok(docs) = self.docs().await else {
            return Ok(None);
        };
        let mut lookup = Lookup::new(&self.root, docs, self.max_page).await;
        Ok(match lookup.find(&path).await {
            Found::There(place) => lookup
                .promise(&place)
                .await
                .ok()
                .map(|promise| promise.text().into_bytes()),
            Found::Unread(_) | Found::Absent | Found::CannotTell(_) => None,
        })
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
        // Read by the rules of the space `file`: nothing through a link that leaves the
        // root, and within a bound of bytes.
        const SETTINGS: [PathLiteral; 2] = [
            PathLiteral::new(".cargo/config"),
            PathLiteral::new(".cargo/config.toml"),
        ];
        for at in SETTINGS {
            let at = RelPath::from(at);
            let file = self.root.join(at.as_str());
            let text = match file::read_within(&self.root, &at, MAX_SETTINGS_BYTES).await {
                Ok(Bounded::Bytes(bytes)) => bytes,
                Ok(Bounded::NoFile) => continue,
                Ok(Bounded::Outside) => {
                    return Err(format!(
                        "{} leads outside the root of the project, or is broken: nothing is read through it",
                        file.display()
                    ));
                }
                Ok(Bounded::TooLarge) => {
                    return Err(format!(
                        "{} is larger than {MAX_SETTINGS_BYTES} bytes, and is not read",
                        file.display()
                    ));
                }
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

/// The domain's reason of a text this adapter wrote.
fn reason(why: String) -> Result<NonBlank, RustItemError> {
    NonBlank::new(why).map_err(RustItemError::Reason)
}

/// The reading of an item that is there: the one place it is made. The text of its
/// promise is hashed as each comparison asked counts it, on a blocking thread (a
/// text may be as large as its page); a promise that cannot be read is there for
/// presence only, and any other comparison cannot be told, for that reason.
async fn reading_of(
    promise: Result<Promise, Unreadable>,
    comparisons: &BTreeSet<Comparison>,
) -> Result<ArtifactReading, RustItemError> {
    match promise {
        Ok(promise) => {
            let text = promise.text();
            let asked = comparisons.clone();
            match super::on_blocking(text.len(), move || {
                ArtifactReading::of_bytes(text.as_bytes(), &asked, None)
            })
            .await
            {
                Ok(reading) => Ok(reading),
                Err(why) => Ok(ArtifactReading::there(reason(format!(
                    "the hash of what the item promises stopped: {why}"
                ))?)),
            }
        }
        Err(unreadable) => Ok(ArtifactReading::there(reason(unreadable.to_string())?)),
    }
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

impl RustPath {
    /// The segments, the crate first: what a prefix is compared on.
    fn segments(&self) -> Vec<&str> {
        std::iter::once(self.krate.as_str())
            .chain(self.rest.iter().map(String::as_str))
            .collect()
    }
}

/// What this adapter says of a move of a prefix, for the `names` of its space asked
/// about: a prefix is a Rust path ([`RustPath::parse`]) of one segment or more, and a
/// name is under it when its segments start with all of the prefix's, the same name
/// under `to` being `to`'s segments and the rest of the name's. `from` that is no Rust
/// path (a directory, say) is no prefix of the space's names; a `to` that is none
/// makes each name under `from` unbuildable. A name that is no Rust path is not under
/// it. Nothing is looked for in the text of a name.
pub(super) fn renamed(
    names: &BTreeSet<ArtifactName>,
    of: &PrefixMove,
) -> Result<(SpacePrefixes, SpaceRenamed), ValueError> {
    let from = match RustPath::parse(of.from().as_str()) {
        Ok(from) => from,
        Err(why) => {
            let why = NonBlank::new(format!(
                "`{}` is no prefix of a Rust path: {why}",
                of.from()
            ))?;
            return Ok((SpacePrefixes::NotUnder(why), SpaceRenamed::new()));
        }
    };
    let to = RustPath::parse(of.to().as_str());
    let from = from.segments();
    let mut answers = SpaceRenamed::new();
    for name in names {
        let answer = match RustPath::parse(name.as_str()) {
            Err(_) => Renamed::Outside,
            Ok(path) => {
                let segments = path.segments();
                match (segments.strip_prefix(from.as_slice()), &to) {
                    (None, _) => Renamed::Outside,
                    (Some(_), Err(why)) => super::unbuildable(format!(
                        "`{}` is no prefix of a Rust path: {why}",
                        of.to()
                    ))?,
                    (Some(rest), Ok(to)) => {
                        let joined: Vec<&str> = to
                            .segments()
                            .into_iter()
                            .chain(rest.iter().copied())
                            .collect();
                        super::built(&joined.join("::"))?
                    }
                }
            }
        };
        answers.insert(name.clone(), answer);
    }
    Ok((SpacePrefixes::Answered, answers))
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
    /// It is there, at this place.
    There(Place),
    /// It is there, and there is no page to read what it promises from, and why (a
    /// re-export into docs that are not here).
    Unread(String),
    Absent,
    CannotTell(String),
}

/// Where in the docs the thing a path names is told of: the one place
/// `Lookup::find` hands over, for what its promise is read from. Only a member has
/// an anchor, so it is the one variant that has it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// A module, or a crate: the index of its directory ([`MODULE_KIND`]).
    Module {
        /// The `index.html` of the module.
        page: PathBuf,
    },
    /// An item: its page.
    Item {
        page: PathBuf,
        /// The kind of the page: a kind of [`ITEM_KINDS`].
        kind: String,
    },
    /// A member of a type: the page of the type, the kind of the member and the
    /// `id` of its element on the page (`method.new`).
    Member {
        page: PathBuf,
        kind: MemberKind,
        anchor: String,
    },
}

impl Place {
    /// The page the promise is read from.
    fn page(&self) -> &Path {
        match self {
            Self::Module { page } | Self::Item { page, .. } | Self::Member { page, .. } => page,
        }
    }
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

/// The docs, read for one resolution: each directory listed once, each page read
/// once.
struct Lookup {
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
}

impl Lookup {
    async fn new(root: &Path, docs: PathBuf, max_page: u64) -> Self {
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
        }
    }

    /// Why nothing is read at `path`, when it leads outside the root of the project
    /// and the build directory once symbolic links are resolved, or is a link that
    /// cannot be resolved.
    async fn outside(&self, path: &Path) -> Option<String> {
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

    /// The text of a page, or why it cannot be read (one outside the root and the
    /// build directory is not).
    async fn page(&mut self, page: &Path) -> Result<Arc<str>, String> {
        if !self.pages.contains_key(page) {
            let text = match self.outside(page).await {
                Some(why) => Err(why),
                None => read(page, self.max_page).await.map(Arc::from),
            };
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
        match super::on_blocking(html.len(), move || html.contains(GENERATOR_MARK)).await {
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

    /// What the thing at `place` promises, read from its page (parsed on a blocking
    /// thread, as a page may be as large as its bound), or why it cannot be.
    async fn promise(&mut self, place: &Place) -> Result<Promise, Unreadable> {
        let html = self.page(place.page()).await.map_err(Unreadable::Page)?;
        let at = place.clone();
        super::on_blocking(html.len(), move || promise_of(&html, &at))
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
                let named = super::on_blocking(html.len(), move || {
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
        let Some(link) = super::on_blocking(html.len(), move || reexport_link(&html, &wanted))
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

/// Why a page cannot be told, when the blocking thread it was read on stopped (a bug
/// if it ever happens, or a runtime shutting down).
fn stopped(page: &Path, why: &str) -> String {
    format!("the reading of {} stopped: {why}", page.display())
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
    // A page may be as large as the bound: a large one is decoded on a blocking
    // thread.
    super::on_blocking(bytes.len(), move || {
        String::from_utf8_lossy(&bytes).into_owned()
    })
    .await
    .map_err(|why| stopped(page, &why))
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
    //! The pages of these tests are written by hand, in the layout rustdoc 1.95
    //! writes (its classes and the order of its elements), with words of this
    //! repository only: no page rustdoc wrote, and no text of the standard library,
    //! is copied into one. Most of them are only as much of a page as finding a name
    //! needs (so what they promise cannot be read, and each says so); the pages of
    //! the tests of `promise` and of what an item promises below are laid out whole.
    //! The end-to-end tests of the command line run `cargo doc` on a crate and watch
    //! the layout of the real thing.

    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Comparison, NamePrefix, Untold};

    use super::super::conformance;
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

    /// The names asked under presence and under the comparisons of a text.
    fn asks_text(names: &[&str]) -> SpaceAsks {
        let comparisons = BTreeSet::from([Comparison::Presence, Comparison::Bytes, lines()]);
        names.iter().map(|t| (n(t), comparisons.clone())).collect()
    }

    /// The items whose docs are under `target`, given as `CARGO_TARGET_DIR`.
    fn at_target(target: &Path) -> RustItemSpace {
        RustItemSpace::new(PathBuf::from("/nowhere"), Some(target.to_path_buf()))
    }

    /// Whether the reading tells the thing is there under presence, with the one
    /// hash presence has.
    fn is_there(reading: &ArtifactReading) -> bool {
        reading.state(&Comparison::Presence)
            == ArtifactState::Present(Comparison::Presence.fingerprint(&[]))
    }

    /// Why the reading cannot tell the thing under `comparison`, when it cannot.
    fn untold_why(reading: &ArtifactReading, comparison: &Comparison) -> Option<String> {
        match reading.state(comparison) {
            ArtifactState::Unknown(Untold::Said(why)) => Some(why.as_str().to_owned()),
            _ => None,
        }
    }

    fn lines() -> Comparison {
        Comparison::Lines {
            ignore: BTreeSet::new(),
        }
    }

    /// The hash of `text` under `comparison`.
    fn hash_of(text: &[u8], comparison: &Comparison) -> ArtifactState {
        ArtifactState::Present(comparison.fingerprint(text))
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
    fn the_adapter_offers_the_presets_rustdoc_and_rustdoc_promise_for_its_space() {
        let values = |compare: &str| PresetValues {
            space: Some("rust-item".to_owned()),
            compare: Some(compare.to_owned()),
            ignore: None,
            means: Some(Propagation::Depends),
        };
        assert_eq!(
            presets(),
            BuiltinPresets::from([
                (PresetName::new(RUSTDOC_PRESET).unwrap(), values("presence")),
                (
                    PresetName::new(RUSTDOC_PROMISE_PRESET).unwrap(),
                    values("lines")
                ),
            ])
        );
        assert_eq!(RUSTDOC_PRESET, "rustdoc");
        assert_eq!(RUSTDOC_PROMISE_PRESET, "rustdoc-promise");
    }

    #[tokio::test]
    async fn a_path_found_in_the_docs_is_there_and_tells_presence_whatever_its_page_is() {
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
            assert!(is_there(&found[&n(name)]), "{name}");
        }
        // Asked of presence only, no page is read for what the item promises.
        let why = untold_why(&found[&n("demo::Shelf")], &Comparison::Bytes).unwrap();
        assert!(why.contains("only presence was asked"), "{why}");
        // The pages of this tree are not laid out as rustdoc's (no main content, no
        // declaration): what the items promise cannot be read, so any comparison but
        // presence cannot be told, and says why.
        let found = source.resolve(&asks_text(&present)).await.unwrap();
        for name in present {
            assert!(is_there(&found[&n(name)]), "{name}");
        }
        for name in ["demo::Shelf", "demo::Shelf::build", "demo::inner"] {
            let why = untold_why(&found[&n(name)], &Comparison::Bytes).unwrap();
            assert!(
                why.contains("not laid out as this adapter reads"),
                "{name}: {why}"
            );
            assert!(untold_why(&found[&n(name)], &lines()).is_some(), "{name}");
        }
        // A re-export into docs that are not here has no page to read.
        let why = untold_why(&found[&n("demo::ext")], &Comparison::Bytes).unwrap();
        assert!(why.contains("from docs that are not here"), "{why}");
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
            .resolve(&asks_text(&["tiny::Big", "tiny::Big::x", "demo::Shelf"]))
            .await
            .unwrap();
        // The page is listed, so the item is there: only what the page would tell
        // cannot be told (its promise too), and neither can anything of a crate whose
        // index is large.
        assert!(is_there(&found[&n("tiny::Big")]));
        let why = untold_why(&found[&n("tiny::Big")], &Comparison::Bytes).unwrap();
        assert!(why.contains("larger than 200 bytes"), "{why}");
        assert_eq!(source.current(&n("tiny::Big")).await.unwrap(), None);
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
        assert!(is_there(&found[&n("demo::Esc")]));
        let why = untold_why(&found[&n("demo::Esc")], &Comparison::Bytes).unwrap();
        assert!(why.contains("from docs that are not here"), "{why}");
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
            // A text that is no name has no promise now; so has no build directory,
            // and so has a page that is not laid out as rustdoc's.
            for name in ["demo::Vec<T>", "demo::Shelf"] {
                assert_eq!(source.current(&n(name)).await.unwrap(), None, "{name}");
            }
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

    /// A page laid out whole, as rustdoc lays out that of an item: a sidebar with a
    /// heading, then the main content.
    fn whole(main: &str) -> String {
        format!(
            "<!DOCTYPE html><html><head>{GENERATOR_MARK}</head><body><nav class=\"sidebar\"><h2 class=\"location\">Sidebar</h2></nav><main><section id=\"main-content\" class=\"content\"><div class=\"main-heading\"><h1>Item</h1><span class=\"sub-heading\"><a class=\"src\" href=\"s.html#1\">Source</a> </span></div>{main}</section></main></body></html>"
        )
    }

    /// The top doc of an item, in one paragraph.
    fn top_doc(text: &str) -> String {
        format!(
            "<details class=\"toggle top-doc\" open><summary class=\"hideme\"><span>Expand description</span></summary><div class=\"docblock\"><p>{text}</p>\n</div></details>"
        )
    }

    /// A method as the page of its type lays it out: in a toggle when it has docs
    /// (`<p>` after `docs`, with the lines `Source` going to `at`).
    fn method(name: &str, signature: &str, docs: Option<&str>, at: u32) -> String {
        let section = format!(
            "<section id=\"method.{name}\" class=\"method\"><a class=\"src rightside\" href=\"s.html#{at}\">Source</a><h4 class=\"code-header\">{signature}</h4></section>"
        );
        match docs {
            Some(docs) => format!(
                "<details class=\"toggle method-toggle\" open><summary>{section}</summary><div class=\"docblock\"><p>{docs}</p>\n</div></details>"
            ),
            None => section,
        }
    }

    const SHELF_DECL: &str = "<pre class=\"rust item-decl\"><code>pub struct Shelf { <span class=\"comment\">/* private fields */</span> }</code></pre>";

    /// The page of the struct `Shelf`: its docs (none: no top doc), the methods
    /// of its impl, and what comes after them.
    fn shelf_page(docs: Option<&str>, methods: &str, tail: &str) -> String {
        let docs = docs.map(top_doc).unwrap_or_default();
        whole(&format!(
            "{SHELF_DECL}{docs}<h2 id=\"implementations\" class=\"section-header\">Implementations<a href=\"#implementations\" class=\"anchor\">§</a></h2><div id=\"implementations-list\"><details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Shelf\" class=\"impl\"><h3 class=\"code-header\">impl Shelf</h3></section></summary><div class=\"impl-items\">{methods}</div></details></div>{tail}"
        ))
    }

    /// The methods of `Shelf`: `put` has docs, `bare` has none and is the last.
    fn shelf_methods(put_signature: &str, put_docs: &str, at: u32) -> String {
        format!(
            "{}{}",
            method("put", put_signature, Some(put_docs), at),
            method(
                "bare",
                "pub fn <a href=\"#method.bare\">bare</a>(&amp;self)",
                None,
                at + 9
            )
        )
    }

    const PUT: &str = "pub fn <a href=\"#method.put\" class=\"fn\">put</a>(&amp;mut self)";

    /// A trait impl after the methods, as rustdoc writes it.
    const TRAIT_IMPL: &str = "<h2 id=\"trait-implementations\" class=\"section-header\">Trait Implementations</h2><div id=\"trait-implementations-list\"><details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-Shelf\" class=\"impl\"><h3 class=\"code-header\">impl Clone for Shelf</h3></section></summary><div class='docblock'>Not a member's.</div></details></div>";

    /// The docs of the crate `demo`, with the module `inner` (no docs), the function
    /// `free` (no docs) and the struct `Shelf` as this page has it.
    async fn promising(shelf: &str) -> tempfile::TempDir {
        let target = tempfile::tempdir().unwrap();
        let d = target.path();
        let index = whole(&format!(
            "{}<h2 id=\"modules\" class=\"section-header\">Modules</h2>",
            top_doc("The demo crate.")
        ));
        write(d, "doc/demo/index.html", &index).await;
        write(d, "doc/demo/struct.Shelf.html", shelf).await;
        write(
            d,
            "doc/demo/fn.free.html",
            &whole("<pre class=\"rust item-decl\"><code>pub fn free()</code></pre>"),
        )
        .await;
        write(
            d,
            "doc/demo/inner/index.html",
            &whole("<h2 id=\"structs\" class=\"section-header\">Structs</h2>"),
        )
        .await;
        target
    }

    const PROMISED: [&str; 6] = [
        "demo",
        "demo::Shelf",
        "demo::Shelf::put",
        "demo::Shelf::bare",
        "demo::free",
        "demo::inner",
    ];

    /// The readings of the names in `PROMISED`, each asked under all three
    /// comparisons.
    async fn promised(target: &Path) -> SpaceReadings {
        let comparisons = BTreeSet::from([Comparison::Presence, Comparison::Bytes, lines()]);
        let asks: SpaceAsks = PROMISED
            .iter()
            .map(|name| (n(name), comparisons.clone()))
            .collect();
        at_target(target).resolve(&asks).await.unwrap()
    }

    #[tokio::test]
    async fn an_item_is_there_with_the_hash_of_what_it_promises() {
        let shelf = shelf_page(
            Some("A shelf of books."),
            &shelf_methods(PUT, "Puts a book.", 20),
            "",
        );
        let target = promising(&shelf).await;
        let found = promised(target.path()).await;
        let texts = [
            ("demo", "doc The demo crate.\n"),
            (
                "demo::Shelf",
                "decl pub struct Shelf { /* private fields */ }\ndoc A shelf of books.\n",
            ),
            (
                "demo::Shelf::put",
                "decl pub fn put(&mut self)\ndoc Puts a book.\n",
            ),
            ("demo::Shelf::bare", "decl pub fn bare(&self)\n"),
            ("demo::free", "decl pub fn free()\n"),
            // A module with no docs promises that.
            ("demo::inner", ""),
        ];
        let source = at_target(target.path());
        for (name, text) in texts {
            let reading = &found[&n(name)];
            assert!(is_there(reading), "{name}");
            for comparison in [Comparison::Bytes, lines()] {
                assert_eq!(
                    reading.state(&comparison),
                    hash_of(text.as_bytes(), &comparison),
                    "{name}"
                );
            }
            // What `current` returns, for a link of any comparison.
            assert_eq!(
                source.current(&n(name)).await.unwrap(),
                Some(text.as_bytes().to_vec()),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn an_edit_of_a_body_changes_no_hash_and_one_of_docs_or_a_declaration_does() {
        let hashes = |found: &SpaceReadings, comparison: &Comparison| -> Vec<ArtifactState> {
            PROMISED
                .iter()
                .map(|name| found[&n(name)].state(comparison))
                .collect()
        };
        let base_page = shelf_page(
            Some("A shelf of books."),
            &shelf_methods(PUT, "Puts a book.", 20),
            "",
        );
        let target = promising(&base_page).await;
        let base = promised(target.path()).await;
        let page_at = "doc/demo/struct.Shelf.html";

        // A body that grew: every line `Source` goes to moves, nothing else does.
        let grown = shelf_page(
            Some("A shelf of books."),
            &shelf_methods(PUT, "Puts a book.", 40),
            "",
        );
        write(target.path(), page_at, &grown).await;
        let after = promised(target.path()).await;
        for comparison in [Comparison::Bytes, lines()] {
            assert_eq!(hashes(&base, &comparison), hashes(&after, &comparison));
        }

        // An impl added after the last method leaves the text of every method, and
        // of the type, as it was.
        let with_impl = shelf_page(
            Some("A shelf of books."),
            &shelf_methods(PUT, "Puts a book.", 20),
            TRAIT_IMPL,
        );
        write(target.path(), page_at, &with_impl).await;
        let after = promised(target.path()).await;
        for comparison in [Comparison::Bytes, lines()] {
            assert_eq!(hashes(&base, &comparison), hashes(&after, &comparison));
        }

        // Each of these changes the promise of what it names, for both comparisons,
        // and of nothing else.
        let changed = [
            (
                "the docs of the type",
                shelf_page(
                    Some("A shelf of old books."),
                    &shelf_methods(PUT, "Puts a book.", 20),
                    "",
                ),
                "demo::Shelf",
            ),
            (
                "no docs of the type",
                shelf_page(None, &shelf_methods(PUT, "Puts a book.", 20), ""),
                "demo::Shelf",
            ),
            (
                "the docs of a method",
                shelf_page(
                    Some("A shelf of books."),
                    &shelf_methods(PUT, "Puts a book away.", 20),
                    "",
                ),
                "demo::Shelf::put",
            ),
            (
                "the signature of a method",
                shelf_page(
                    Some("A shelf of books."),
                    &shelf_methods(
                        "pub fn <a href=\"#method.put\" class=\"fn\">put</a>(&amp;mut self, n: u8)",
                        "Puts a book.",
                        20,
                    ),
                    "",
                ),
                "demo::Shelf::put",
            ),
            (
                "docs given to a method that had none",
                shelf_page(
                    Some("A shelf of books."),
                    &format!(
                        "{}{}",
                        method("put", PUT, Some("Puts a book."), 20),
                        method(
                            "bare",
                            "pub fn <a href=\"#method.bare\">bare</a>(&amp;self)",
                            Some("Now said."),
                            29
                        )
                    ),
                    "",
                ),
                "demo::Shelf::bare",
            ),
            (
                "the declaration of the type",
                shelf_page(
                    Some("A shelf of books."),
                    &shelf_methods(PUT, "Puts a book.", 20),
                    "",
                )
                .replace("pub struct Shelf", "pub struct Shelf2"),
                "demo::Shelf",
            ),
        ];
        for (what, html, name) in changed {
            write(target.path(), page_at, &html).await;
            let after = promised(target.path()).await;
            for comparison in [Comparison::Bytes, lines()] {
                for other in PROMISED {
                    let (was, is) = (
                        base[&n(other)].state(&comparison),
                        after[&n(other)].state(&comparison),
                    );
                    assert_eq!(was != is, other == name, "{what}: {other}");
                }
            }
        }
    }

    #[tokio::test]
    async fn a_blank_line_added_to_the_docs_is_a_change_for_bytes_and_none_for_lines() {
        let one = shelf_page(
            Some("A shelf of books.</p>\n<p>It has a root."),
            &shelf_methods(PUT, "Puts a book.", 20),
            "",
        );
        let two = one.replace("</p>\n<p>It", "</p>\n\n<p>It");
        assert_ne!(one, two);
        let target = promising(&one).await;
        let before = promised(target.path()).await;
        write(target.path(), "doc/demo/struct.Shelf.html", &two).await;
        let after = promised(target.path()).await;
        let shelf = n("demo::Shelf");
        assert_eq!(
            before[&shelf].state(&lines()),
            after[&shelf].state(&lines())
        );
        assert_ne!(
            before[&shelf].state(&Comparison::Bytes),
            after[&shelf].state(&Comparison::Bytes)
        );
    }

    #[tokio::test]
    async fn a_kind_with_a_declaration_and_a_page_without_one_is_there_and_cannot_be_told() {
        // No `item-decl` in the page of a struct and of a method.
        let shelf = whole(&format!(
            "{}{}",
            top_doc("A shelf of books."),
            "<section id=\"method.put\" class=\"method\"></section>"
        ));
        let target = promising(&shelf).await;
        let found = promised(target.path()).await;
        for name in ["demo::Shelf", "demo::Shelf::put"] {
            let reading = &found[&n(name)];
            assert!(is_there(reading), "{name}");
            for comparison in [Comparison::Bytes, lines()] {
                let why = untold_why(reading, &comparison).unwrap();
                assert!(why.contains("has no declaration"), "{name}: {why}");
            }
        }
        // The kinds with none read the same way as before.
        for name in ["demo", "demo::inner"] {
            assert!(matches!(
                found[&n(name)].state(&lines()),
                ArtifactState::Present(_)
            ));
        }
        let source = at_target(target.path());
        assert_eq!(source.current(&n("demo::Shelf")).await.unwrap(), None);
        assert_eq!(source.current(&n("demo::Shelf::put")).await.unwrap(), None);
        // A module with no docs has the empty text.
        assert_eq!(
            source.current(&n("demo::inner")).await.unwrap(),
            Some(Vec::new())
        );
    }

    #[tokio::test]
    async fn what_is_absent_or_cannot_be_told_has_no_text_now() {
        let target = promising(&shelf_page(None, "", "")).await;
        let source = at_target(target.path());
        for name in ["demo::Missing", "demo::Shelf::missing", "other::Item"] {
            assert_eq!(source.current(&n(name)).await.unwrap(), None, "{name}");
        }
        assert_eq!(
            source.current(&n("demo::Shelf")).await.unwrap(),
            Some(b"decl pub struct Shelf { /* private fields */ }\n".to_vec())
        );
    }

    /// The bound of a page of the conformance test.
    const MAX_TEST_PAGE: u64 = 8192;

    /// A project at `root` with the docs of the crate `demo` beside its `Cargo.toml`:
    /// a re-export of `inner::Thing`, and two methods `put` on `Shelf` (`method.put`
    /// and `method.put-1`).
    async fn conformance_docs(root: &Path) {
        write(root, "Cargo.toml", "[workspace]\n").await;
        let d = root.join(TARGET_DIR_NAME);
        let index = whole(&format!(
            "{}<h2 id=\"reexports\" class=\"section-header\">Re-exports</h2><dl><dt id=\"reexport.Thing\"><code>pub use inner::<a class=\"struct\" href=\"inner/struct.Thing.html\" title=\"x\">Thing</a>;</code></dt></dl>",
            top_doc("The demo crate.")
        ));
        write(&d, "doc/demo/index.html", &index).await;
        let methods = format!(
            "{}{}",
            shelf_methods(PUT, "Puts a book.", 20),
            method(
                "put-1",
                "pub fn <a href=\"#method.put-1\">put</a>(&amp;self, n: u8)",
                Some("Puts many."),
                40
            )
        );
        write(
            &d,
            "doc/demo/struct.Shelf.html",
            &shelf_page(Some("A shelf of books."), &methods, ""),
        )
        .await;
        write(
            &d,
            "doc/demo/fn.free.html",
            &whole("<pre class=\"rust item-decl\"><code>pub fn free()</code></pre>"),
        )
        .await;
        write(
            &d,
            "doc/demo/inner/index.html",
            &whole("<h2 id=\"structs\" class=\"section-header\">Structs</h2>"),
        )
        .await;
        write(
            &d,
            "doc/demo/inner/struct.Thing.html",
            &whole(&format!(
                "<pre class=\"rust item-decl\"><code>pub struct Thing;</code></pre>{}",
                top_doc("A thing.")
            )),
        )
        .await;
    }

    /// The hook of the conformance test: the docs above, a page past the bound, one
    /// that cannot be read, one linked from outside the root and the build directory,
    /// and a root whose cargo settings are past their bound.
    struct Items;

    impl conformance::Hook for Items {
        fn space(&self) -> &'static str {
            RUST_ITEM_SPACE
        }

        fn source(&self) -> &'static str {
            include_str!("rust_item.rs")
        }

        fn bounds(&self) -> Vec<&'static str> {
            vec!["a page", "the cargo settings"]
        }

        /// DEC-54: an item whose page cannot be read is there for presence.
        fn unread(&self) -> conformance::Unread {
            conformance::Unread::ThereForPresence
        }

        fn walks_nesting(&self) -> bool {
            false
        }

        fn lists(&self) -> bool {
            false
        }

        fn adapter_at(&self, dir: &Path) -> super::super::Adapter {
            super::super::Adapter::RustItem(RustItemSpace {
                max_page: MAX_TEST_PAGE,
                ..RustItemSpace::new(dir.to_path_buf(), None)
            })
        }

        async fn tree(&self, dir: &Path) -> conformance::Tree {
            let root = dir.join("root");
            conformance_docs(&root).await;
            let d = root.join(TARGET_DIR_NAME);
            let over = usize::try_from(MAX_TEST_PAGE).unwrap() + 1;
            write(&d, "doc/demo/struct.Big.html", &whole(&"x".repeat(over))).await;
            let locked = "<pre class=\"rust item-decl\"><code>pub struct Locked;</code></pre>";
            write(&d, "doc/demo/struct.Locked.html", &whole(locked)).await;
            // The docs are there: only the settings, past their bound, keep them from
            // being found.
            let settings = dir.join("settings");
            conformance_docs(&settings).await;
            let past = usize::try_from(MAX_SETTINGS_BYTES).unwrap() + 1;
            write(&settings, ".cargo/config.toml", &"#".repeat(past)).await;

            let mut tree = conformance::Tree::of(self.adapter_at(&root));
            tree.present = vec![
                n("demo"),
                n("demo::Shelf"),
                n("demo::Shelf::bare"),
                n("demo::free"),
                n("demo::inner"),
            ];
            tree.absent = vec![n("demo::Missing"), n("demo::Shelf::missing")];
            tree.not_names = vec![n("demo::r#type"), n("demo::"), n("crate::demo")];
            tree.over_bytes = vec![
                conformance::OverBytes::Name {
                    bound: "a page",
                    name: n("demo::Big"),
                },
                conformance::OverBytes::Root {
                    bound: "the cargo settings",
                    adapter: self.adapter_at(&settings),
                    names: vec![n("demo"), n("demo::Shelf")],
                },
            ];
            tree.two_things = Some(n("demo::Shelf::put"));
            tree.spellings = Some((n("demo::Thing"), n("demo::inner::Thing")));
            tree.prefix = Some((
                PrefixMove::new(
                    NamePrefix::new("demo::inner").unwrap(),
                    NamePrefix::new("demo::outer").unwrap(),
                )
                .unwrap(),
                n("demo::inner::Thing"),
                n("demo::outer::Thing"),
            ));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let decl = "<pre class=\"rust item-decl\"><code>pub struct Esc;</code></pre>";
                write(dir, "outside/struct.Esc.html", &whole(decl)).await;
                tokio::fs::symlink(
                    dir.join("outside/struct.Esc.html"),
                    d.join("doc/demo/struct.Esc.html"),
                )
                .await
                .unwrap();
                tree.outside = Some(n("demo::Esc"));
                let page = d.join("doc/demo/struct.Locked.html");
                tokio::fs::set_permissions(&page, std::fs::Permissions::from_mode(0o000))
                    .await
                    .unwrap();
                // Run as a user every file opens to (root), it is no name to test.
                if tokio::fs::File::open(&page).await.is_err() {
                    tree.unreadable = Some(n("demo::Locked"));
                }
            }
            tree
        }
    }

    #[tokio::test]
    async fn the_adapter_keeps_the_contract_of_an_adapter() {
        conformance::check(&Items).await;
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
