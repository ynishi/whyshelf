//! The adapter of the space `rust-test`: the test functions of a project's Rust
//! source, by the file they are in and their path in it, read from the source
//! without running any program.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`RustTestSpace::resolve`] | reads each file named, by the rules of the space `file`, lexes it once, and finds each test named in it by a shallow scan of its items; the text of a test is what is compared | [`file`](mod@file), `proc-macro2` |
//! | [`RustTestSpace::current`] | the text of a named test now | the same |
//! | [`presets`] | the preset `test`: the space, `compare = "bytes"`, `means = "depends"` | values |
//!
//! ```text
//!   name ──<file>.rs::<module>::…::<function>?── no ──► not a name (why)
//!     │ yes
//!   the file, by the rules of the space `file`
//!     ├─ no file there ──► absent
//!     ├─ a link out of the root, too large, cannot be read, not UTF-8 ──► cannot tell
//!     └─ lexed once (proc-macro2), nesting held to MAX_DEPTH
//!          ├─ does not lex, or nests too deep ──► cannot tell
//!          └─ its items scanned, then those of each inline module named (every twin)
//!               ├─ found: the function's tokens, doc attributes left out ──► there (its text)
//!               ├─ not found, and a twin on the way is out of line (`mod x;`) ──► cannot tell
//!               ├─ not found, and a macro on the way may make it ──► cannot tell
//!               └─ not found ──► absent
//! ```
//!
//! Resolving runs no program: it reads the file and lexes it. `cargo test --
//! --list` is not used, since a repository's toolchain file would choose the program
//! a reading command runs.
//!
//! # The names of the space
//!
//! A name is `<file>::<segment>(::<segment>)*`:
//!
//! - `<file>` is everything before the first `::`: a path of the space `file`
//!   (relative to the root, `/` between parts, no empty, `.` or `..` part), ending
//!   in `.rs`. A file whose path holds `::` cannot be named.
//! - Each segment is a Rust identifier as written, a raw one (`r#type`) included,
//!   with no white space or comment around it; a keyword, `_`, or an empty segment
//!   is none. (`gen`, reserved only from the edition 2024, is taken as an
//!   identifier, since a file of an earlier edition may name a test so.)
//! - The last segment is the function, the ones before it the inline modules it is
//!   in, outermost first: `crates/x/tests/actor.rs::the_actor_comes_from_the_variable`,
//!   `crates/x/src/git.rs::tests::a_call_past_its_bound`.
//!
//! A raw identifier and its plain spelling name the same item (`r#try` and a
//! function `try` written raw). Identifiers are compared as written, code point by
//! code point, not normalized (NFC): a name and a source that spell a non-ASCII
//! identifier in two forms do not meet. Any other text is not a name of the space,
//! and the adapter says so with why.
//!
//! # Finding the test
//!
//! The file is read as the space `file` reads one ([`file`](mod@file)): its
//! path is compared as written, ASCII case included; a symbolic link on it is
//! followed only inside the root; and a file larger than [`MAX_FILE_BYTES`] (the
//! largest file the space `file` reads when it searches for moves) is not read.
//!
//! It is lexed once into token trees (`proc-macro2`, whose lexer and whose dropping
//! of a tree recurse on nothing), held to [`MAX_DEPTH`] brackets of nesting, and
//! then scanned item by item; no expression, type or pattern is parsed, so no
//! nesting of them (a long chain of prefix operators, of generic types, of
//! closures) can exhaust the stack. The scan of one list of items reads:
//!
//! - outer attributes (`#[…]`), and inner ones (`#![…]`);
//! - `mod <name> { … }` and `mod <name>;`, after a visibility (`pub`, `pub(…)`)
//!   and `unsafe`;
//! - `fn <name>`, after a visibility and any of `async`, `unsafe`, `const`, `safe`,
//!   `default` and `extern "<abi>"`, up to its body: the first brace group at the
//!   top of the item outside every `<` … `>` (one inside them is a const generic
//!   argument; the `>` of `->` and `=>` closes none), so a `where` clause that ends
//!   in `,` is no matter;
//! - an item macro call (`path! …`, `include!(…)`), to its group and a `;`;
//!   `macro_rules! name { … }` defines a macro and calls none, and a `!` after a
//!   keyword that starts an item (`impl !Send for S {}`) is no call;
//! - every other item, as tokens up to its end: the first `;` or brace group at the
//!   top of the item.
//!
//! Only the modules named are scanned below the file. Each module named is every
//! inline module (`mod tests { … }`) of the name in the items before it: twins
//! under their own `#[cfg]` are each searched, as twin functions are joined. A twin
//! declared out of line (`mod x;`) is not followed: a test not found in the inline
//! ones cannot be told.
//!
//! In them, the test is each function of the name that carries a test attribute:
//! an outer attribute whose path's last segment is `test`, or a `cfg_attr` whose
//! list of attributes holds one (`#[cfg_attr(feature = "x", test)]`). That accepts
//! `#[test]`, `#[tokio::test]`, `#[async_std::test]` and `#[test_log::test]`, each
//! of which makes the function a test; it does not accept `#[rstest]`,
//! `#[test_case(…)]` or `#[cfg(test)]`, whose last segment is another word. Two
//! functions of the name with a test attribute (each under its own `#[cfg]`) are
//! one test, their texts in the order of the file.
//!
//! # What may make a test
//!
//! The source is not expanded, so a test a macro makes is not in it. When a test is
//! not found, the answer is "cannot tell" rather than "absent" if, in the file or a
//! module searched (or on the way to it):
//!
//! - an item macro is called (`include!` among them, which reads another file);
//! - a module carries an attribute other than the inert ones below (an attribute
//!   macro may rewrite the module);
//! - a function of the name carries such an attribute (`#[rstest]` may make it a
//!   test).
//!
//! The inert attributes are [`INERT_ATTRIBUTES`], any attribute of a tool
//! (`rustfmt::…`, `clippy::…`, `rust_analyzer::…`, `diagnostic::…`), and a
//! `cfg_attr` whose attributes are all inert. The reason names the first
//! few that may make it, each once, and how many more there are.
//!
//! So where a module on the way holds an item macro or an attribute that is not
//! inert, a test removed (or renamed) reads as "cannot tell", not as gone: an
//! approval of it stays fresh, with a notice that it cannot be told.
//!
//! # What is compared
//!
//! The text of a test is the function item's own tokens as the source has them,
//! from its first outer attribute to the end of its body, each doc attribute
//! (`///`, `//!`, `#[doc …]`) left out where it stands as an attribute (on the
//! function, and on a statement or an item in its body; not in the tokens of a
//! macro call, where it is data), printed as `proc-macro2` prints a token stream:
//! white space and comments count for nothing, so an edit of them, or of the docs,
//! makes nothing stale, and any other edit of the function does. The text is what
//! [`ArtifactReading::of_bytes`] hashes, so every comparison works over it:
//!
//! - `bytes` (the preset `test`): every token.
//! - `lines`: the text is one line, save inside a string that spans lines, so it
//!   tells what `bytes` tells, under another hash; an `ignore` prefix is held
//!   against the start of that one line, and one the text starts with (`#`) leaves
//!   out the whole test, so that no edit is seen. It is allowed, since the domain
//!   offers every comparison over a content, and adds nothing.
//! - `presence`: only that the test is there.
//!
//! There is no address: the text is made, not stored, so nothing is read back.
//!
//! # The answers
//!
//! | answer | when |
//! |---|---|
//! | there | the test is found, with its text |
//! | absent | no file is there; or it is scanned, and has no module or no function of the name with a test attribute, and nothing on the way may make one |
//! | cannot tell | a symbolic link on the path leaves the root or is broken; the file cannot be read, is too large, is not UTF-8, does not lex, or nests too deep; or the test is not found, and a twin of a module on the way is declared out of line or a macro on the way may make it |
//! | not a name | the text is not of the grammar above |
//!
//! # Known limits
//!
//! - The text is the function alone. An edit of a helper it calls, of a `use` at
//!   the top of the file, of a fixture it reads, or a `cfg` that stops its module
//!   from being compiled, leaves an approval of it fresh.
//! - The file is read whether or not a crate compiles it: a test in a file no
//!   crate includes (no `mod` names it), or under a `cfg` never set, reads as there.
//! - The file is lexed and scanned, not parsed: one whose tokens lex but which is no
//!   valid Rust is read as far as its items can be told, and an item in it may be
//!   missed.
//! - A module declared out of line (`mod x;`, `#[path] mod x;`) is in another file,
//!   which is not followed: a test under it cannot be told. Name the test by the
//!   file of the module instead.
//! - A test a macro writes is not in the source as a function: it cannot be told
//!   when a macro on the way is seen (above), and reads as absent when an attribute
//!   macro on another item (a `struct`, an `impl`) makes it.
//! - A text is the tokens as `proc-macro2` prints them; an update of it that prints
//!   them otherwise would make every approval of a test stale once.
//! - Moves are not suggested ([`MoveSource`](whyshelf_core::app::shelf::MoveSource)).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use proc_macro2::{Delimiter, Group, Ident, Spacing, TokenStream, TokenTree};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, Comparison, Space, SpaceAsks, SpaceReadings,
};
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::file::{self, Bounded};

/// The name of the space this adapter resolves names in.
pub const RUST_TEST_SPACE: &str = "rust-test";

/// The name of the preset this adapter offers.
pub const TEST_PRESET: &str = "test";

/// The most bytes of a file read: the largest file the space `file` reads when it
/// searches for moves ([`ScanLimits::file_bytes`](super::file::ScanLimits::file_bytes));
/// a larger one cannot be told.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The deepest nesting of brackets a file is scanned with; a deeper one cannot be
/// told. Printing a text and leaving out its docs go down the groups of a function
/// by recursion, so the bound keeps a file from exhausting the stack; code people
/// write nests far less.
pub const MAX_DEPTH: usize = 128;

/// The attributes that make no item and rewrite none (with those of a tool, and a
/// `cfg_attr` of them): one of them on a module, or on a function of the name, does
/// not make a test that is not found one that cannot be told.
pub const INERT_ATTRIBUTES: &[&str] = &[
    "cfg",
    "cfg_attr",
    "allow",
    "deny",
    "warn",
    "expect",
    "forbid",
    "doc",
    "path",
    "macro_use",
    "inline",
    "cold",
    "must_use",
    "deprecated",
    "track_caller",
    "ignore",
    "should_panic",
    "no_mangle",
    "coverage",
    "export_name",
    "link_section",
    "target_feature",
    "feature",
    "no_std",
    "no_main",
    "no_implicit_prelude",
    "recursion_limit",
    "type_length_limit",
    "crate_name",
    "crate_type",
    "windows_subsystem",
];

/// The keywords that start an item: a `!` after one is no macro call
/// (`impl !Send for S {}`).
const ITEM_KEYWORDS: &[&str] = &[
    "impl", "unsafe", "fn", "mod", "struct", "enum", "union", "trait", "type", "const", "static",
    "use", "extern", "pub", "async", "auto", "default", "safe",
];

/// The first segments of the attributes of a tool, which are inert.
const TOOLS: &[&str] = &["rustfmt", "clippy", "rust_analyzer", "diagnostic"];

/// The words that are no identifier in any edition from 2018 (strict and reserved
/// keywords; `gen`, reserved from 2024 only, is left out).
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "_",
];

/// The presets this adapter offers when it resolves the names of `space`: one,
/// [`TEST_PRESET`], for a link to the tests that show a document holds, that stands
/// on each token of them (`space` that space, `compare = "bytes"`,
/// `means = "depends"`): an edit of a test makes the approvals that name it stale,
/// one of another test in the file does not, and a test that goes away breaks them.
pub fn presets(space: &Space) -> BuiltinPresets {
    let values = PresetValues {
        space: Some(space.as_str().to_owned()),
        compare: Some("bytes".to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    match PresetName::new(TEST_PRESET) {
        Ok(name) => BuiltinPresets::from([(name, values)]),
        // Not a name: no preset. The test of this module holds it to be one.
        Err(_) => BuiltinPresets::new(),
    }
}

/// Why the tests could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum RustTestError {
    /// The domain refused a reason this adapter gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// Why a text is no name of the space.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum NotAName {
    #[error("no `::` follows the path of the file, so no function is named")]
    NoFunction,
    #[error("the path `{0}` does not end in `.rs`")]
    NotRust(String),
    #[error("`{path}` is no path under the root of the project: {why}")]
    NoPath { path: String, why: String },
    #[error("a segment between `::` is empty")]
    EmptySegment,
    #[error("`{0}` is no Rust identifier")]
    NoIdentifier(String),
}

impl NotAName {
    /// The reason a reading gives.
    fn reason(&self) -> String {
        format!("no test name (`<file>.rs::<module>::<function>`): {self}")
    }
}

/// Why a test cannot be told.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum Untellable {
    #[error(
        "a symbolic link on the path of {0} leads outside the root of the project, or is broken: nothing is read through it"
    )]
    Outside(RelPath),
    #[error("{path} is larger than {max} bytes, and is not read")]
    TooLarge { path: RelPath, max: u64 },
    #[error("{path} cannot be read: {why}")]
    Unreadable { path: RelPath, why: String },
    #[error("{0} is not UTF-8, so it is no Rust source")]
    NotUtf8(RelPath),
    #[error("{path} does not parse as Rust: its tokens do not lex{at}")]
    Lex { path: RelPath, at: Place },
    #[error("{path} nests brackets deeper than {max}, and is not read further")]
    TooDeep { path: RelPath, max: usize },
    #[error(
        "the module `{module}` in {path} is declared out of line (`mod …;`): its file is not followed, so the test cannot be told (name it by the file of the module)"
    )]
    OutOfLine { path: RelPath, module: String },
    #[error(
        "{path} has no such test as written, but {by} may make one, and macros are not expanded"
    )]
    MayBeMade { path: RelPath, by: String },
    #[error("the reading of {path} stopped: {why}")]
    Stopped { path: RelPath, why: String },
}

/// Where in a file the lexer stopped, when it can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place(Option<(usize, usize)>);

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some((line, column)) => write!(f, " (at line {line}, column {column})"),
            None => Ok(()),
        }
    }
}

/// The test functions of the Rust source of a project.
#[derive(Debug, Clone)]
pub struct RustTestSpace {
    root: PathBuf,
    /// The most bytes of a file read.
    max_file: u64,
}

impl RustTestSpace {
    /// The tests in the files under `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            max_file: MAX_FILE_BYTES,
        }
    }

    /// For each name: not a name, the hash of the test's text under each comparison
    /// asked, absent, or cannot tell (with why). Every name asked about has an
    /// answer; each file is read and lexed once.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, RustTestError> {
        let mut readings = SpaceReadings::new();
        let mut by_file: BTreeMap<RelPath, Vec<Asked<'_>>> = BTreeMap::new();
        for (name, comparisons) in asks {
            match TestName::parse(name.as_str()) {
                Ok(test) => {
                    by_file
                        .entry(test.file.clone())
                        .or_default()
                        .push((name, comparisons, test))
                }
                Err(why) => {
                    readings.insert(
                        name.clone(),
                        ArtifactReading::not_a_name(reason(why.reason())?),
                    );
                }
            }
        }
        for (path, tests) in by_file {
            let wanted: Vec<Wanted> = tests.iter().map(|(_, _, t)| t.wanted()).collect();
            let found = self.find_in(&path, wanted).await;
            for ((name, comparisons, _), found) in tests.into_iter().zip(found) {
                let reading = match found {
                    Found::Text(text) => {
                        ArtifactReading::of_bytes(text.as_bytes(), comparisons, None)
                    }
                    Found::Absent => ArtifactReading::absent(),
                    Found::CannotTell(why) => {
                        ArtifactReading::cannot_tell(reason(why.to_string())?)
                    }
                };
                readings.insert(name.clone(), reading);
            }
        }
        Ok(readings)
    }

    /// The text of a test now (what is compared), or none when the name is none, or
    /// the test is not there or cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, RustTestError> {
        let Ok(test) = TestName::parse(name.as_str()) else {
            return Ok(None);
        };
        let found = self.find_in(&test.file, vec![test.wanted()]).await;
        Ok(match found.into_iter().next() {
            Some(Found::Text(text)) => Some(text.into_bytes()),
            Some(Found::Absent | Found::CannotTell(_)) | None => None,
        })
    }

    /// What each of `wanted` comes to in the file at `path`, in their order.
    async fn find_in(&self, path: &RelPath, wanted: Vec<Wanted>) -> Vec<Found> {
        let every = |found: Found| vec![found; wanted.len()];
        let untold = |why: Untellable| every(Found::CannotTell(why));
        let bytes = match file::read_within(&self.root, path, self.max_file).await {
            Ok(Bounded::Bytes(bytes)) => bytes,
            Ok(Bounded::NoFile) => return every(Found::Absent),
            Ok(Bounded::Outside) => return untold(Untellable::Outside(path.clone())),
            Ok(Bounded::TooLarge) => {
                return untold(Untellable::TooLarge {
                    path: path.clone(),
                    max: self.max_file,
                });
            }
            Err(e) => {
                return untold(Untellable::Unreadable {
                    path: path.clone(),
                    why: e.to_string(),
                });
            }
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return untold(Untellable::NotUtf8(path.clone()));
        };
        let count = wanted.len();
        let at = path.clone();
        match tokio::task::spawn_blocking(move || look_up(&at, &text, &wanted)).await {
            Ok(found) => found,
            Err(e) => vec![
                Found::CannotTell(Untellable::Stopped {
                    path: path.clone(),
                    why: e.to_string(),
                });
                count
            ],
        }
    }
}

/// The domain's reason of a text this adapter wrote.
fn reason(why: String) -> Result<NonBlank, RustTestError> {
    NonBlank::new(why).map_err(RustTestError::Reason)
}

/// Drops the record of the source text `proc-macro2` keeps for the spans of the
/// thread when it goes out of scope, so that no reading keeps the text of a file
/// (spans carry locations, which a lex error reports). Nothing lexed on the
/// thread is used after it.
struct Spans;

impl Drop for Spans {
    fn drop(&mut self) {
        proc_macro2::extra::invalidate_current_thread_spans();
    }
}

/// A module or function of a name: an identifier, unraw, compared as written.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment(String);

impl Segment {
    /// The segment as written in a name, or why it is no identifier.
    fn new(text: &str) -> Result<Self, NotAName> {
        if text.is_empty() {
            return Err(NotAName::EmptySegment);
        }
        let no = || NotAName::NoIdentifier(text.to_owned());
        let lexed = {
            let _spans = Spans;
            let tokens: TokenStream = text.parse().map_err(|_| no())?;
            let mut tokens = tokens.into_iter();
            match (tokens.next(), tokens.next()) {
                (Some(TokenTree::Ident(ident)), None) => ident.to_string(),
                _ => String::new(),
            }
        };
        if lexed != text || KEYWORDS.contains(&text) {
            return Err(no());
        }
        Ok(Self::of(&lexed))
    }

    /// The segment an identifier of the source is.
    fn of(written: &str) -> Self {
        Self(written.strip_prefix("r#").unwrap_or(written).to_owned())
    }

    /// Whether `ident` is this identifier.
    fn names(&self, ident: &Ident) -> bool {
        Self::of(&ident.to_string()) == *self
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A name of the space, read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TestName {
    file: RelPath,
    /// The inline modules, outermost first.
    modules: Vec<Segment>,
    function: Segment,
}

/// A name asked about, the comparisons asked of it, and the name read.
type Asked<'a> = (&'a ArtifactName, &'a BTreeSet<Comparison>, TestName);

/// What is looked for in one file: the modules and the function of a name.
#[derive(Debug, Clone)]
struct Wanted {
    modules: Vec<Segment>,
    function: Segment,
}

impl TestName {
    /// The one reader of the grammar of the names (on the page of the module).
    fn parse(text: &str) -> Result<Self, NotAName> {
        let Some((file, rest)) = text.split_once("::") else {
            return Err(NotAName::NoFunction);
        };
        if !file.ends_with(".rs") {
            return Err(NotAName::NotRust(file.to_owned()));
        }
        let path: RelPath = file.parse().map_err(|e: ValueError| NotAName::NoPath {
            path: file.to_owned(),
            why: e.to_string(),
        })?;
        let mut segments = rest
            .split("::")
            .map(Segment::new)
            .collect::<Result<Vec<_>, _>>()?;
        let Some(function) = segments.pop() else {
            return Err(NotAName::NoFunction);
        };
        Ok(Self {
            file: path,
            modules: segments,
            function,
        })
    }

    fn wanted(&self) -> Wanted {
        Wanted {
            modules: self.modules.clone(),
            function: self.function.clone(),
        }
    }
}

/// What a name comes to in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    Text(String),
    Absent,
    CannotTell(Untellable),
}

/// What each of `wanted` comes to in the source `text` of the file at `path`: the
/// file is lexed once, and its items scanned for each.
fn look_up(path: &RelPath, text: &str, wanted: &[Wanted]) -> Vec<Found> {
    let _spans = Spans;
    let every = |why: Untellable| vec![Found::CannotTell(why); wanted.len()];
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // A first line `#!` that starts no inner attribute is the line of an
    // interpreter, which rustc skips too; its line end is kept, so lines count
    // as in the file.
    let text = match text.strip_prefix("#!") {
        Some(rest) if !rest.trim_start().starts_with('[') => {
            rest.find('\n').map_or("", |end| &rest[end..])
        }
        _ => text,
    };
    let tokens: TokenStream = match text.parse() {
        Ok(tokens) => tokens,
        Err(e) => {
            let start = e.span().start();
            let at = (start.line > 0).then_some((start.line, start.column + 1));
            return every(Untellable::Lex {
                path: path.clone(),
                at: Place(at),
            });
        }
    };
    if nests_deeper_than(&tokens, MAX_DEPTH) {
        return every(Untellable::TooDeep {
            path: path.clone(),
            max: MAX_DEPTH,
        });
    }
    let top = Items::scan(tokens);
    wanted
        .iter()
        .map(|w| find(path, &top, &w.modules, &w.function))
        .collect()
}

/// One list of items (of a file or of an inline module), scanned: its inner
/// attributes and its items, with the tokens they were scanned from.
struct Items {
    tokens: Vec<TokenTree>,
    inner: Vec<Group>,
    items: Vec<Scanned>,
}

/// One item of a list: its outer attributes, what it is, and where its tokens are
/// in the list, from its first outer attribute to its end.
struct Scanned {
    attrs: Vec<Group>,
    kind: Kind,
    start: usize,
    end: usize,
}

/// What an item is, as far as the scan tells.
enum Kind {
    Fn(Ident),
    Mod {
        name: Ident,
        body: Option<Group>,
    },
    /// An item macro call, by the last segment of its path.
    Macro(Ident),
    Other,
}

impl Items {
    /// The items of `tokens`, by the shape on the page of the module. A list of
    /// tokens has no recursion here: a group is one token.
    fn scan(tokens: TokenStream) -> Self {
        let t: Vec<TokenTree> = tokens.into_iter().collect();
        let mut inner = Vec::new();
        let mut items = Vec::new();
        let mut i = 0;
        while i < t.len() {
            if punct(&t, i, '#')
                && punct(&t, i + 1, '!')
                && let Some(group) = bracket(&t, i + 2)
            {
                inner.push(group.clone());
                i += 3;
                continue;
            }
            if punct(&t, i, ';') {
                i += 1;
                continue;
            }
            let start = i;
            let mut attrs = Vec::new();
            while punct(&t, i, '#')
                && let Some(group) = bracket(&t, i + 1)
            {
                attrs.push(group.clone());
                i += 2;
            }
            if i >= t.len() {
                break;
            }
            let mut j = i;
            if word(&t, j) == Some("pub") {
                j += 1;
                if matches!(t.get(j), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                {
                    j += 1;
                }
            }
            let (kind, end) = if let Some((name, after)) = macro_call(&t, j) {
                let mut k = after;
                let defines = name == "macro_rules";
                if defines && matches!(t.get(k), Some(TokenTree::Ident(_))) {
                    k += 1;
                }
                if matches!(t.get(k), Some(TokenTree::Group(_))) {
                    k += 1;
                }
                if punct(&t, k, ';') {
                    k += 1;
                }
                let kind = if defines {
                    Kind::Other
                } else {
                    Kind::Macro(name)
                };
                (kind, k)
            } else {
                loop {
                    match word(&t, j) {
                        Some("async" | "unsafe" | "const" | "safe" | "default") => j += 1,
                        Some("extern") => {
                            j += 1;
                            if matches!(t.get(j), Some(TokenTree::Literal(_))) {
                                j += 1;
                            }
                        }
                        _ => break,
                    }
                }
                match (word(&t, j), t.get(j + 1)) {
                    (Some("fn"), Some(TokenTree::Ident(name))) => match body_of(&t, j + 2) {
                        Some(body) => (Kind::Fn(name.clone()), body + 1),
                        None => (Kind::Other, to_end(&t, j)),
                    },
                    (Some("mod"), Some(TokenTree::Ident(name))) => match t.get(j + 2) {
                        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => (
                            Kind::Mod {
                                name: name.clone(),
                                body: Some(g.clone()),
                            },
                            j + 3,
                        ),
                        _ if punct(&t, j + 2, ';') => (
                            Kind::Mod {
                                name: name.clone(),
                                body: None,
                            },
                            j + 3,
                        ),
                        _ => (Kind::Other, to_end(&t, j)),
                    },
                    _ => (Kind::Other, to_end(&t, j)),
                }
            };
            items.push(Scanned {
                attrs,
                kind,
                start,
                end,
            });
            i = end.max(i + 1);
        }
        Self {
            tokens: t,
            inner,
            items,
        }
    }

    /// The tokens of an item, as the source has them.
    fn tokens_of(&self, item: &Scanned) -> TokenStream {
        self.tokens[item.start..item.end.min(self.tokens.len())]
            .iter()
            .cloned()
            .collect()
    }

    /// What in this list may make items: its inner attributes that are not inert,
    /// and its item macro calls.
    fn makers(&self, makers: &mut Makers) {
        for attr in &self.inner {
            if let Some(path) = not_inert(attr) {
                makers.add(format!("the attribute `#![{path}]`"));
            }
        }
        for item in &self.items {
            match &item.kind {
                Kind::Macro(name) if name == "include" => {
                    makers.add("`include!`, which reads another file,".to_owned());
                }
                Kind::Macro(name) => makers.add(format!("the macro call `{name}!`")),
                _ => {}
            }
        }
    }
}

/// What may make a test, as a reason says it: the first [`Makers::SHOWN`]
/// different ones, and how many more were seen, so that a reason stays small
/// whatever a file holds.
#[derive(Debug, Default)]
struct Makers {
    shown: Vec<String>,
    more: usize,
}

impl Makers {
    /// How many are named in a reason.
    const SHOWN: usize = 5;

    fn add(&mut self, maker: String) {
        if self.shown.contains(&maker) {
            return;
        }
        if self.shown.len() < Self::SHOWN {
            self.shown.push(maker);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }

    fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }
}

impl fmt::Display for Makers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.shown.join(", "))?;
        if self.more > 0 {
            write!(f, ", and {} more", self.more)?;
        }
        Ok(())
    }
}

/// The test `function` under the inline `modules` of the items `top` of the file at
/// `path` (the rules on the page of the module).
fn find(path: &RelPath, top: &Items, modules: &[Segment], function: &Segment) -> Found {
    let mut makers = Makers::default();
    top.makers(&mut makers);
    // The first module of the name on the way that is declared out of line.
    let mut out_of_line: Option<String> = None;
    // The items of the modules the walk is in, once it is below the file.
    let mut owned: Vec<Items>;
    let mut levels: Vec<&Items> = vec![top];
    let mut named: Vec<String> = Vec::new();
    for module in modules {
        named.push(module.to_string());
        let mut next = Vec::new();
        for level in &levels {
            for item in &level.items {
                let Kind::Mod { name, body } = &item.kind else {
                    continue;
                };
                if !module.names(name) {
                    continue;
                }
                let Some(body) = body else {
                    out_of_line.get_or_insert_with(|| named.join("::"));
                    continue;
                };
                for attr in &item.attrs {
                    if let Some(p) = not_inert(attr) {
                        makers.add(format!(
                            "the attribute `#[{p}]` on the module `{}`",
                            named.join("::")
                        ));
                    }
                }
                let items = Items::scan(body.stream());
                items.makers(&mut makers);
                next.push(items);
            }
        }
        if next.is_empty() {
            return not_found(path, out_of_line, &makers);
        }
        owned = next;
        levels = owned.iter().collect();
    }
    let mut texts = Vec::new();
    for level in &levels {
        for item in &level.items {
            let Kind::Fn(name) = &item.kind else {
                continue;
            };
            if !function.names(name) {
                continue;
            }
            if item.attrs.iter().any(is_test) {
                texts.push(without_docs(level.tokens_of(item)).to_string());
            } else {
                for attr in &item.attrs {
                    if let Some(p) = not_inert(attr) {
                        makers.add(format!(
                            "the attribute `#[{p}]` on the function `{function}`"
                        ));
                    }
                }
            }
        }
    }
    if texts.is_empty() {
        not_found(path, out_of_line, &makers)
    } else {
        Found::Text(texts.join("\n"))
    }
}

/// A test not found in the inline modules: cannot tell when a twin of a module on
/// the way is declared out of line, or something on the way may make it; else
/// absent.
fn not_found(path: &RelPath, out_of_line: Option<String>, makers: &Makers) -> Found {
    if let Some(module) = out_of_line {
        return Found::CannotTell(Untellable::OutOfLine {
            path: path.clone(),
            module,
        });
    }
    if makers.is_empty() {
        Found::Absent
    } else {
        Found::CannotTell(Untellable::MayBeMade {
            path: path.clone(),
            by: makers.to_string(),
        })
    }
}

/// Whether `t[i]` is the punctuation `c`.
fn punct(t: &[TokenTree], i: usize, c: char) -> bool {
    matches!(t.get(i), Some(TokenTree::Punct(p)) if p.as_char() == c)
}

/// The bracket group at `t[i]`.
fn bracket(t: &[TokenTree], i: usize) -> Option<&Group> {
    match t.get(i) {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket => Some(g),
        _ => None,
    }
}

/// The word at `t[i]`, when it is an identifier (a keyword is one to the lexer).
fn word(t: &[TokenTree], i: usize) -> Option<&str> {
    // An identifier as a string costs an allocation; the words looked for are
    // keywords, never raw, so they are told by their text.
    const WORDS: &[&str] = &[
        "pub", "async", "unsafe", "const", "safe", "default", "extern", "fn", "mod",
    ];
    match t.get(i) {
        Some(TokenTree::Ident(ident)) => WORDS.iter().copied().find(|w| ident == w),
        _ => None,
    }
}

/// An item macro call at `t[j]` (a path, then `!`): the last segment of its path,
/// and where the tokens after the `!` start.
fn macro_call(t: &[TokenTree], j: usize) -> Option<(Ident, usize)> {
    let mut k = j;
    if punct(t, k, ':') && punct(t, k + 1, ':') {
        k += 2;
    }
    let Some(TokenTree::Ident(mut last)) = t.get(k).cloned() else {
        return None;
    };
    // A keyword that starts an item is no macro: `impl !Send for S {}`.
    if k == j && ITEM_KEYWORDS.iter().any(|w| last == w) {
        return None;
    }
    k += 1;
    while punct(t, k, ':')
        && punct(t, k + 1, ':')
        && let Some(TokenTree::Ident(next)) = t.get(k + 2)
    {
        last = next.clone();
        k += 3;
    }
    punct(t, k, '!').then_some((last, k + 1))
}

/// Where the body of a function starts after its name, at `from`: the first brace
/// group at the top of the item outside every `<` … `>` (a brace inside them is a
/// const generic argument; the `>` of `->` and `=>` closes none); none when a `;`
/// comes first.
fn body_of(t: &[TokenTree], from: usize) -> Option<usize> {
    let mut angles: usize = 0;
    for (k, token) in t.iter().enumerate().skip(from) {
        match token {
            TokenTree::Punct(p) if p.as_char() == ';' => return None,
            TokenTree::Punct(p) if p.as_char() == '<' => angles += 1,
            TokenTree::Punct(p) if p.as_char() == '>' => {
                let arrow = matches!(
                    k.checked_sub(1).and_then(|before| t.get(before)),
                    Some(TokenTree::Punct(q))
                        if matches!(q.as_char(), '-' | '=') && q.spacing() == Spacing::Joint
                );
                if !arrow {
                    angles = angles.saturating_sub(1);
                }
            }
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace && angles == 0 => {
                return Some(k);
            }
            _ => {}
        }
    }
    None
}

/// The end of an item whose shape is not read, from `from`: after the first `;` or
/// brace group at its top.
fn to_end(t: &[TokenTree], from: usize) -> usize {
    let mut k = from;
    while k < t.len() {
        match &t[k] {
            TokenTree::Punct(p) if p.as_char() == ';' => return k + 1,
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => return k + 1,
            _ => k += 1,
        }
    }
    k
}

/// The paths of the attributes an attribute is: its own, or, for a `cfg_attr`,
/// those of its list (after the condition), each in turn.
fn attribute_paths(attr: &Group) -> Vec<Vec<String>> {
    let tokens: Vec<TokenTree> = attr.stream().into_iter().collect();
    let mut out = Vec::new();
    paths_in(&tokens, &mut out);
    out
}

fn paths_in(tokens: &[TokenTree], out: &mut Vec<Vec<String>>) {
    let path = path_at(tokens);
    if path.len() == 1 && path[0] == "cfg_attr" {
        if let Some(TokenTree::Group(list)) = tokens.get(1) {
            let inside: Vec<TokenTree> = list.stream().into_iter().collect();
            for part in inside
                .split(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ','))
                .skip(1)
            {
                if !part.is_empty() {
                    paths_in(part, out);
                }
            }
        }
        return;
    }
    out.push(path);
}

/// The path at the start of the tokens of an attribute (`a::b::c`), each segment
/// unraw.
fn path_at(tokens: &[TokenTree]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut k = 0;
    if punct(tokens, 0, ':') && punct(tokens, 1, ':') {
        k = 2;
    }
    while let Some(TokenTree::Ident(ident)) = tokens.get(k) {
        segments.push(Segment::of(&ident.to_string()).0);
        if punct(tokens, k + 1, ':') && punct(tokens, k + 2, ':') {
            k += 3;
        } else {
            break;
        }
    }
    segments
}

/// Whether an attribute makes the function a test: one whose path's last segment
/// is `test`, itself or in a `cfg_attr` (the rule on the page of the module).
fn is_test(attr: &Group) -> bool {
    attribute_paths(attr)
        .iter()
        .any(|path| path.last().is_some_and(|s| s == "test"))
}

/// The first attribute of `attr` that is not inert, written as a path; none when
/// all are.
fn not_inert(attr: &Group) -> Option<String> {
    attribute_paths(attr)
        .into_iter()
        .find(|path| {
            let inert = match path.as_slice() {
                [] => true,
                [one] => INERT_ATTRIBUTES.contains(&one.as_str()),
                [tool, ..] => TOOLS.contains(&tool.as_str()),
            };
            !inert
        })
        .map(|path| path.join("::"))
}

/// The tokens without the doc attributes (`#[doc …]`, `#![doc …]`, which `///` and
/// `//!` are) that stand as attributes, at any depth but inside the tokens of a
/// macro call (`name! group`, `macro_rules! name group`), which are kept as written.
fn without_docs(tokens: TokenStream) -> TokenStream {
    let t: Vec<TokenTree> = tokens.into_iter().collect();
    let mut kept = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if punct(&t, i, '#') {
            let bang = punct(&t, i + 1, '!');
            let body = i + 1 + usize::from(bang);
            if let Some(group) = bracket(&t, body)
                && matches!(group.stream().into_iter().next(), Some(TokenTree::Ident(ident)) if ident == "doc")
            {
                i = body + 1;
                continue;
            }
        }
        if matches!(t[i], TokenTree::Ident(_))
            && let Some(TokenTree::Punct(bang)) = t.get(i + 1)
            && bang.as_char() == '!'
            && bang.spacing() == Spacing::Alone
        {
            let data = match (t.get(i + 2), t.get(i + 3)) {
                (Some(TokenTree::Group(_)), _) => 1,
                (Some(TokenTree::Ident(_)), Some(TokenTree::Group(_))) => 2,
                _ => 0,
            };
            if data > 0 {
                kept.extend(t[i..i + 2 + data].iter().cloned());
                i += 2 + data;
                continue;
            }
        }
        kept.push(match &t[i] {
            TokenTree::Group(group) => {
                let mut inner = Group::new(group.delimiter(), without_docs(group.stream()));
                inner.set_span(group.span());
                TokenTree::Group(inner)
            }
            other => other.clone(),
        });
        i += 1;
    }
    kept.into_iter().collect()
}

/// Whether the tokens nest groups deeper than `max`, looked at without recursion.
fn nests_deeper_than(tokens: &TokenStream, max: usize) -> bool {
    let mut stack = vec![(tokens.clone().into_iter(), 0_usize)];
    while let Some((iter, depth)) = stack.last_mut() {
        let depth = *depth;
        match iter.next() {
            Some(TokenTree::Group(group)) => {
                if depth + 1 > max {
                    return true;
                }
                stack.push((group.stream().into_iter(), depth + 1));
            }
            Some(_) => {}
            None => {
                stack.pop();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Untold};

    use super::super::file::ScanLimits;
    use super::*;

    fn n(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn asks(names: &[&str]) -> SpaceAsks {
        names
            .iter()
            .map(|t| (n(t), BTreeSet::from([Comparison::Bytes])))
            .collect()
    }

    async fn write(dir: &Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(full, text).await.unwrap();
    }

    /// The text a name reads as, under `root`.
    async fn text(root: &Path, name: &str) -> Option<String> {
        RustTestSpace::new(root.to_path_buf())
            .current(&n(name))
            .await
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    }

    async fn reading(root: &Path, name: &str) -> ArtifactReading {
        let found = RustTestSpace::new(root.to_path_buf())
            .resolve(&asks(&[name]))
            .await
            .unwrap();
        found[&n(name)].clone()
    }

    async fn unknown_why(root: &Path, name: &str) -> String {
        match reading(root, name).await.state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => why.as_str().to_owned(),
            other => panic!("{name}: {other:?}"),
        }
    }

    /// What `function` under `modules` comes to in the source `text`.
    fn in_source(text: &str, modules: &[&str], function: &str) -> Found {
        let wanted = [Wanted {
            modules: modules.iter().map(|m| Segment::new(m).unwrap()).collect(),
            function: Segment::new(function).unwrap(),
        }];
        let path: RelPath = "t.rs".parse().unwrap();
        look_up(&path, text, &wanted).remove(0)
    }

    fn may_be_made(found: &Found) -> Option<String> {
        match found {
            Found::CannotTell(why @ Untellable::MayBeMade { .. }) => Some(why.to_string()),
            _ => None,
        }
    }

    const SOURCE: &str = r#"//! The crate.

/// Not a test.
fn helper() -> u8 { 1 }

#[test]
fn top() {
    assert_eq!(helper(), 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// It holds.
    #[test]
    fn holds() {
        // A comment.
        assert!(true);
    }

    fn not_a_test() {}

    #[tokio::test(flavor = "current_thread")]
    async fn waits() {}

    #[test]
    fn r#try() {}

    pub(crate) mod deeper {
        mod deepest {
            #[test]
            pub(super) unsafe extern "C" fn far<T, const N: usize>() -> [u8; { N }] where T: Into<{ 1 }> { let _ = 1; }
        }
    }

    mod elsewhere;
}
"#;

    #[test]
    fn the_adapter_offers_the_preset_test_for_its_space() {
        let space = Space::new(RUST_TEST_SPACE).unwrap();
        assert_eq!(
            presets(&space),
            BuiltinPresets::from([(
                PresetName::new(TEST_PRESET).unwrap(),
                PresetValues {
                    space: Some("rust-test".to_owned()),
                    compare: Some("bytes".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                }
            )])
        );
        assert_eq!(MAX_FILE_BYTES, ScanLimits::default().file_bytes);
    }

    #[tokio::test]
    async fn a_test_found_is_there_with_the_hash_of_its_text() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "src/lib.rs", SOURCE).await;
        let present = [
            "src/lib.rs::top",
            "src/lib.rs::tests::holds",
            "src/lib.rs::tests::waits",
            "src/lib.rs::tests::r#try",
            "src/lib.rs::tests::deeper::deepest::far",
            "src/lib.rs::r#tests::holds",
        ];
        let found = RustTestSpace::new(r.to_path_buf())
            .resolve(&asks(&present))
            .await
            .unwrap();
        for name in present {
            let text = text(r, name).await.unwrap();
            assert_eq!(
                found[&n(name)],
                ArtifactReading::of_bytes(
                    text.as_bytes(),
                    &BTreeSet::from([Comparison::Bytes]),
                    None
                ),
                "{name}"
            );
        }
        // The text is the tokens of the item: its attributes and its body, the
        // docs and the comments left out.
        assert_eq!(
            text(r, "src/lib.rs::tests::holds").await.unwrap(),
            "# [test] fn holds () { assert ! (true) ; }"
        );
        assert_eq!(
            text(r, "src/lib.rs::tests::waits").await.unwrap(),
            "# [tokio :: test (flavor = \"current_thread\")] async fn waits () { }"
        );
        // The body is the brace group after the const generic arguments.
        assert!(
            text(r, "src/lib.rs::tests::deeper::deepest::far")
                .await
                .unwrap()
                .ends_with("{ let _ = 1 ; }")
        );
    }

    #[tokio::test]
    async fn an_edit_of_white_space_comments_or_docs_keeps_the_text_and_one_of_the_body_does_not() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let name = "src/lib.rs::tests::holds";
        write(r, "src/lib.rs", SOURCE).await;
        let before = text(r, name).await.unwrap();

        let respaced = SOURCE.replace(
            "    fn holds() {\n        // A comment.\n        assert!(true);\n    }",
            "    fn   holds ( )\n{ /* another */ assert ! ( true ) ;\n\n}",
        );
        assert_ne!(respaced, SOURCE);
        write(r, "src/lib.rs", &respaced).await;
        assert_eq!(text(r, name).await.unwrap(), before);

        let redocumented = SOURCE
            .replace(
                "/// It holds.",
                "/// It holds, said otherwise.\n    #[doc = \"More.\"]",
            )
            .replace("// A comment.", "//! Inside.\n        /// On a statement.");
        write(r, "src/lib.rs", &redocumented).await;
        assert_eq!(text(r, name).await.unwrap(), before);

        // Another test in the file is edited: this one's text is the same.
        write(
            r,
            "src/lib.rs",
            &SOURCE.replace("fn top() {", "fn top() { let _ = 2;"),
        )
        .await;
        assert_eq!(text(r, name).await.unwrap(), before);

        let edited = SOURCE.replace("assert!(true);", "assert!(!false);");
        write(r, "src/lib.rs", &edited).await;
        assert_ne!(text(r, name).await.unwrap(), before);
        // So is one of its attributes.
        let ignored = SOURCE.replace(
            "    #[test]\n    fn holds",
            "    #[test]\n    #[ignore]\n    fn holds",
        );
        write(r, "src/lib.rs", &ignored).await;
        assert_ne!(text(r, name).await.unwrap(), before);
    }

    #[test]
    fn a_doc_inside_a_macro_call_is_data_and_is_kept() {
        let source = "#[test] fn a() { quote! { /// kept\n struct S; } }\n";
        let Found::Text(text) = in_source(source, &[], "a") else {
            panic!()
        };
        assert!(text.contains("doc = \" kept\""), "{text}");
        let other = in_source(&source.replace("/// kept", "/// changed"), &[], "a");
        assert_ne!(other, Found::Text(text));
    }

    #[tokio::test]
    async fn a_test_that_is_not_there_is_absent() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "src/lib.rs", SOURCE).await;
        let absent = [
            // No such function, and one with no test attribute.
            "src/lib.rs::tests::missing",
            "src/lib.rs::helper",
            "src/lib.rs::tests::not_a_test",
            "src/lib.rs::tests::try_",
            // In another module than its own, or a module that is not there.
            "src/lib.rs::holds",
            "src/lib.rs::tests::far",
            "src/lib.rs::nowhere::holds",
            // A file that is not there, and one named in another case.
            "src/gone.rs::top",
            "src/LIB.rs::top",
        ];
        let found = RustTestSpace::new(r.to_path_buf())
            .resolve(&asks(&absent))
            .await
            .unwrap();
        for name in absent {
            assert_eq!(found[&n(name)], ArtifactReading::absent(), "{name}");
            assert_eq!(text(r, name).await, None, "{name}");
        }
        // `#[cfg(test)]` is no test attribute.
        assert_eq!(
            in_source("#[cfg(test)] fn a() {}\n", &[], "a"),
            Found::Absent
        );
    }

    #[test]
    fn a_cfg_attr_that_holds_a_test_attribute_makes_a_test() {
        for attr in [
            "#[cfg_attr(feature = \"x\", test)]",
            "#[cfg_attr(all(a, b), allow(dead_code), tokio::test)]",
            "#[cfg_attr(a, cfg_attr(b, test))]",
        ] {
            let found = in_source(&format!("{attr} fn a() {{}}\n"), &[], "a");
            assert!(matches!(found, Found::Text(_)), "{attr}: {found:?}");
        }
        let inert = in_source("#[cfg_attr(test, allow(dead_code))] fn a() {}\n", &[], "a");
        assert_eq!(inert, Found::Absent);
    }

    #[test]
    fn twin_modules_are_each_searched_and_twin_functions_are_one_test() {
        let source = "#[cfg(a)] mod dup { #[test] fn x() {} }\n#[cfg(not(a))] mod dup { #[test] fn y() {} }\n";
        assert_eq!(
            in_source(source, &["dup"], "y"),
            Found::Text("# [test] fn y () { }".to_owned())
        );
        assert_eq!(
            in_source(source, &["dup"], "x"),
            Found::Text("# [test] fn x () { }".to_owned())
        );
        let source = "#[cfg(unix)] #[test] fn a() { 1; }\n#[cfg(windows)] #[test] fn a() { 2; }\n";
        assert_eq!(
            in_source(source, &[], "a"),
            Found::Text(
                "# [cfg (unix)] # [test] fn a () { 1 ; }\n# [cfg (windows)] # [test] fn a () { 2 ; }"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_test_not_found_where_a_macro_may_make_it_cannot_be_told() {
        let cases = [
            (
                "make_tests!(a);\n",
                &[][..],
                "a",
                "the macro call `make_tests!`",
            ),
            ("::m::make! { a }\n", &[], "a", "the macro call `make!`"),
            ("include!(\"more.rs\");\n", &[], "a", "`include!`"),
            (
                "mod t { other!{} }\n",
                &["t"],
                "a",
                "the macro call `other!`",
            ),
            (
                "#[my_macro] mod t {}\n",
                &["t"],
                "a",
                "`#[my_macro]` on the module `t`",
            ),
            ("mod t { #![my::inner] }\n", &["t"], "a", "`#![my::inner]`"),
            (
                "#[rstest] fn a() {}\n",
                &[],
                "a",
                "`#[rstest]` on the function `a`",
            ),
            (
                "#[test_case(1)] fn a(x: u8) {}\n",
                &[],
                "a",
                "`#[test_case]` on the function `a`",
            ),
            // A macro at the top may make the module itself.
            ("make!(t);\n", &["t"], "a", "the macro call `make!`"),
        ];
        for (source, modules, function, says) in cases {
            let found = in_source(source, modules, function);
            let why = may_be_made(&found);
            assert!(
                why.as_deref().is_some_and(|w| w.contains(says)),
                "{source}: {found:?}"
            );
        }
        // Inert attributes and the definition of a macro make nothing.
        for source in [
            "#![allow(unused)]\n#![cfg_attr(docsrs, feature(doc_cfg))]\n#[cfg(test)] #[allow(x)] #[rustfmt::skip] mod t {}\n",
            "macro_rules! m { () => {} }\nmod t {}\n",
            "#[path = \"x.rs\"] #[doc = \"x\"] mod u {}\nmod t {}\n",
        ] {
            assert_eq!(in_source(source, &["t"], "a"), Found::Absent, "{source}");
        }
        // A test found stands, whatever else is there.
        assert!(matches!(
            in_source("make!();\n#[test] fn a() {}\n", &[], "a"),
            Found::Text(_)
        ));
    }

    #[test]
    fn a_where_clause_that_ends_in_a_comma_ends_before_the_body() {
        // As rustfmt writes it, a helper before a test, then the test itself.
        let helper = "fn helper<T>(x: T) -> Option<T>\nwhere\n    T: Copy,\n{\n    Some(x)\n}\n\n#[test]\nfn after() {}\n";
        assert_eq!(
            in_source(helper, &[], "after"),
            Found::Text("# [test] fn after () { }".to_owned())
        );
        let test = "#[test]\nfn a<T>()\nwhere\n    T: Copy,\n    Vec<T>: Clone,\n{\n    let _ = 1;\n}\n#[test] fn b() {}\n";
        let Found::Text(text) = in_source(test, &[], "a") else {
            panic!()
        };
        assert!(text.ends_with("{ let _ = 1 ; }"), "{text}");
        assert!(matches!(in_source(test, &[], "b"), Found::Text(_)));
        // Const generic braces are still no body, and `->` and `=>` close nothing.
        let generic = "#[test] fn c<const N: usize>() -> Foo<{ N }> where [(); { N }]:, Bar<{ 1 }, { 2 }>: X { match 1 { _ => 2 }; }\n#[test] fn d() {}\n";
        let Found::Text(text) = in_source(generic, &[], "c") else {
            panic!()
        };
        assert!(text.ends_with("{ match 1 { _ => 2 } ; }"), "{text}");
        assert!(matches!(in_source(generic, &[], "d"), Found::Text(_)));
    }

    #[test]
    fn the_reason_names_the_first_few_makers_once() {
        let mut source = String::new();
        for i in 0..20_000 {
            source.push_str(&format!("m!(); n{i}!();\n"));
        }
        let why = may_be_made(&in_source(&source, &[], "a")).unwrap();
        assert!(why.len() < 400, "{} bytes: {why}", why.len());
        assert_eq!(why.matches("`m!`").count(), 1, "{why}");
        assert!(why.contains("and 19996 more"), "{why}");
    }

    #[test]
    fn an_out_of_line_twin_leaves_the_inline_ones_searched() {
        let source = "mod mixed;\n#[cfg(x)] mod mixed { #[test] fn z() {} }\n";
        assert_eq!(
            in_source(source, &["mixed"], "z"),
            Found::Text("# [test] fn z () { }".to_owned())
        );
        let why = match in_source(source, &["mixed"], "w") {
            Found::CannotTell(why) => why.to_string(),
            other => panic!("{other:?}"),
        };
        assert!(
            why.contains("`mixed` in t.rs is declared out of line"),
            "{why}"
        );
    }

    #[test]
    fn a_negative_impl_is_no_macro_call() {
        let source = "impl !Send for S {}\nunsafe impl !Sync for S {}\n#[coverage(off)] mod t {}\n";
        assert_eq!(in_source(source, &[], "a"), Found::Absent);
        assert_eq!(in_source(source, &["t"], "a"), Found::Absent);
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_read_as_rust_cannot_be_told() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "src/lib.rs", SOURCE).await;
        // Through a module declared out of line.
        let why = unknown_why(r, "src/lib.rs::tests::elsewhere::inner").await;
        assert!(
            why.contains("`tests::elsewhere` in src/lib.rs is declared out of line"),
            "{why}"
        );
        // A file that does not lex, with where it stopped.
        write(r, "src/broken.rs", "#[test]\nfn top() {\n").await;
        let why = unknown_why(r, "src/broken.rs::top").await;
        assert!(
            why.starts_with("src/broken.rs does not parse as Rust"),
            "{why}"
        );
        write(r, "src/broken.rs", "#[test] fn top() {}\nfn a() { ) }\n").await;
        let why = unknown_why(r, "src/broken.rs::top").await;
        assert!(why.contains("(at line 2, column"), "{why}");
        // One that is not UTF-8.
        tokio::fs::write(r.join("src/bytes.rs"), b"fn a() {}\xff")
            .await
            .unwrap();
        let why = unknown_why(r, "src/bytes.rs::a").await;
        assert!(why.contains("not UTF-8"), "{why}");
        // One that nests too deep.
        let deep = format!(
            "#[test] fn a() {{ {}{} }}",
            "(".repeat(200),
            ")".repeat(200)
        );
        write(r, "src/deep.rs", &deep).await;
        let why = unknown_why(r, "src/deep.rs::a").await;
        assert!(why.contains("deeper than 128"), "{why}");
        // One that is too large.
        let small = RustTestSpace {
            max_file: 100,
            ..RustTestSpace::new(r.to_path_buf())
        };
        let found = small.resolve(&asks(&["src/lib.rs::top"])).await.unwrap();
        match found[&n("src/lib.rs::top")].state(&Comparison::Bytes) {
            ArtifactState::Unknown(Untold::Said(why)) => {
                assert!(why.as_str().contains("larger than 100 bytes"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(small.current(&n("src/lib.rs::top")).await.unwrap(), None);
    }

    /// Sources that nest without brackets, each answered without a crash: the
    /// scan parses no expression or type, so the nesting is a flat list of tokens.
    #[tokio::test]
    async fn a_hostile_file_is_answered_without_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let hostile = [
            format!("#[test] fn a() {{ let x = {}1; }}", "-".repeat(200_000)),
            format!("#[test] fn a() {{ let x = {}1; }}", "&".repeat(200_000)),
            format!(
                "type T = {}u8{};\n#[test] fn a() {{}}",
                "Vec<".repeat(100_000),
                ">".repeat(100_000)
            ),
            format!("#[test] fn a() {{ let f = {}1; }}", "|| ".repeat(100_000)),
            format!("#[test] fn a() {{ {}1; }}", "return ".repeat(100_000)),
            format!("#[test] fn a() {{ {}1; }}", "x = ".repeat(200_000)),
            format!(
                "#[test] fn a() {{ {}1{} }}",
                "(-!&".repeat(126),
                ")".repeat(126)
            ),
        ];
        for (i, source) in hostile.iter().enumerate() {
            let file = format!("src/h{i}.rs");
            write(r, &file, source).await;
            assert!(reading(r, &format!("{file}::a")).await.is_present(), "{i}");
            assert_eq!(
                reading(r, &format!("{file}::b")).await,
                ArtifactReading::absent(),
                "{i}"
            );
        }
        // A big file under the bound of bytes.
        let mut big = String::from("#[test] fn a() {}\n");
        while big.len() < 4 * 1024 * 1024 {
            big.push_str("fn filler() { let x = - - - & & | | y = 1; }\n");
        }
        write(r, "src/big.rs", &big).await;
        assert!(reading(r, "src/big.rs::a").await.is_present());
        assert_eq!(reading(r, "src/big.rs::b").await, ArtifactReading::absent());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_file_through_a_link_out_of_the_root_cannot_be_told() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "lib.rs", SOURCE).await;
        tokio::fs::symlink(outside.path(), root.path().join("out"))
            .await
            .unwrap();
        let why = unknown_why(root.path(), "out/lib.rs::top").await;
        assert!(why.contains("leads outside the root"), "{why}");
        // A link inside the root is read through.
        write(root.path(), "src/lib.rs", SOURCE).await;
        tokio::fs::symlink(root.path().join("src"), root.path().join("in"))
            .await
            .unwrap();
        assert!(reading(root.path(), "in/lib.rs::top").await.is_present());
    }

    #[tokio::test]
    async fn a_text_out_of_the_grammar_is_not_a_name() {
        // No file is read for a text that is no name.
        let nowhere = tempfile::tempdir().unwrap();
        let wrongs = [
            ("src/lib.rs", "no `::` follows"),
            ("src/lib::top", "does not end in `.rs`"),
            ("src/lib.txt::top", "does not end in `.rs`"),
            ("/src/lib.rs::top", "no path under the root"),
            ("src/../lib.rs::top", "no path under the root"),
            ("./lib.rs::top", "no path under the root"),
            ("src\\lib.rs::top", "no path under the root"),
            ("src/lib.rs::", "empty"),
            ("src/lib.rs::tests::", "empty"),
            ("src/lib.rs::tests:::top", "no Rust identifier"),
            ("src/lib.rs::1st", "no Rust identifier"),
            ("src/lib.rs::_", "no Rust identifier"),
            ("src/lib.rs::fn", "no Rust identifier"),
            ("src/lib.rs::self::top", "no Rust identifier"),
            ("src/lib.rs::r#self", "no Rust identifier"),
            ("src/lib.rs::a b", "no Rust identifier"),
            ("src/lib.rs:: top", "no Rust identifier"),
            ("src/lib.rs::top/**/", "no Rust identifier"),
            ("src/lib.rs::top()", "no Rust identifier"),
            ("src/lib.rs::my-test", "no Rust identifier"),
            ("src/lib.rs::\"a\"", "no Rust identifier"),
        ];
        let names: Vec<&str> = wrongs.iter().map(|(w, _)| *w).collect();
        let source = RustTestSpace::new(nowhere.path().to_path_buf());
        let found = source.resolve(&asks(&names)).await.unwrap();
        for (wrong, says) in wrongs {
            let why = found[&n(wrong)].not_a_name_why().map(|w| w.as_str());
            assert!(
                why.is_some_and(|w| w.starts_with("no test name") && w.contains(says)),
                "{wrong}: {why:?}"
            );
            assert_eq!(source.current(&n(wrong)).await.unwrap(), None);
        }
    }

    #[test]
    fn a_name_reads_as_its_file_modules_and_function() {
        assert_eq!(
            TestName::parse("crates/x/src/git.rs::tests::r#a_call").unwrap(),
            TestName {
                file: "crates/x/src/git.rs".parse().unwrap(),
                modules: vec![Segment("tests".to_owned())],
                function: Segment("a_call".to_owned()),
            }
        );
        assert_eq!(TestName::parse("t.rs::über").unwrap().function.0, "über");
        assert_eq!(TestName::parse("t.rs::gen").unwrap().function.0, "gen");
    }

    #[test]
    fn a_line_of_an_interpreter_and_a_mark_of_order_are_skipped() {
        for source in [
            "#!/usr/bin/env run-cargo-script\n#[test] fn a() {}\n",
            "\u{feff}#[test] fn a() {}\n",
            "#![allow(unused)]\n#[test] fn a() {}\n",
        ] {
            assert_eq!(
                in_source(source, &[], "a"),
                Found::Text("# [test] fn a () { }".to_owned()),
                "{source:?}"
            );
        }
    }
}
