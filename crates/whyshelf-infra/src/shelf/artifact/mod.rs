//! The artifacts of a project: one adapter for each space, each keeping the contract
//! of [`SpaceAdapter`], in a list ([`Registry`]) that the dispatch of the core
//! ([`Spaces`]) hands the asks to.
//!
//! The domain knows a space only by its name. Each adapter names its own space and
//! the presets it offers, and turns the words of the outside (a path, a blob) into
//! the values of the domain (a name, an [`ArtifactReading`]). The dispatch, in the
//! core, keys the adapters by the spaces they name, refuses two adapters of one space
//! or two built-in presets of one name, and answers for a space with no adapter; the
//! registry here holds the list and nothing else.
//!
//! ```text
//!   ArtifactAsks ──Spaces (core)──per space──► Registry ──► the adapter that names it ──► SpaceReadings ─┐
//!                └─ a space no adapter names ──► cannot tell, each name                                  ┤
//!                                                                                          ArtifactReadings
//!   (space, ContentAddress) ──Spaces──► the adapter of the space ──► content, or not readable
//! ```
//!
//! | space | adapter | its names | the presets it offers | suggested moves ([`SpaceSuggest`]) | a prefix of its names ([`NamePrefixes`]) |
//! |---|---|---|---|---|---|
//! | `file` | [`file::FileSpace`] | paths under the root of the project | `file`: `space = "file"`, `compare = "bytes"`, `means = "depends"`; `code`: `space = "file"`, `compare = "presence"`, `means = "depends"` | the same content, then Git's renames ([`file`](mod@file#suggesting-moves)) | the parts of a path ([`file`](mod@file#prefixes-of-the-names)) |
//! | `rust-item` | [`rust_item::RustItemSpace`] | Rust paths (`crate::module::Item`, `Type::member`), looked up in the docs `cargo doc` wrote | `rustdoc`: `space = "rust-item"`, `compare = "presence"`, `means = "depends"`; `rustdoc-promise`: `space = "rust-item"`, `compare = "lines"`, `means = "depends"` (the declaration and the docs of an item) | the names in the docs that end as the gone one does, found by their content when an approval pinned some ([`rust_item`](mod@rust_item#suggesting-moves)) | the segments of a Rust path ([`rust_item`](mod@rust_item#prefixes-of-the-names)) |
//! | `rust-test` | [`rust_test::RustTestSpace`] | test functions by their file and path in it (`<file>.rs::<module>::<function>`), parsed from the source | `test`: `space = "rust-test"`, `compare = "bytes"`, `means = "depends"` | cannot suggest | the parts of a file's path, or a file and the first segments of a path in it ([`rust_test`](mod@rust_test#prefixes-of-the-names)) |
//! | `js-test` | [`js_test::JsTestSpace`] | JavaScript and TypeScript tests by their file and titles (`<file>.ts::<describe>::<test>`), lexed from the source | `js-test`: `space = "js-test"`, `compare = "bytes"`, `means = "depends"` | cannot suggest | the parts of a file's path, or a file and the first titles of a path in it, titles with their escapes ([`js_test`](mod@js_test#prefixes-of-the-names)) |
//!
//! An adapter that cannot suggest names [`CannotSuggest`], one that reads nothing
//! back [`NoReadBack`], and one that lists no names [`CannotList`]: the core gives
//! their answers. Each answers a move of a prefix with its own grammar of names (the
//! domain knows no boundary of a name, and builds none). `file` and `rust-test` list
//! the names a selection takes ([`NameList`],
//! [`file`](mod@file#listing-the-names-of-a-selection),
//! [`rust_test`](mod@rust_test#listing-the-names-of-a-selection)), choosing the files
//! they take in one place; the other spaces name [`CannotList`].
//!
//! Each adapter offers its presets of links to artifacts for its own space, as
//! values ([`SpaceAdapter::presets`]); the domain knows none. A project may write
//! `[defaults] artifact = "file"` so that a link to artifacts that names neither a
//! preset nor a space is one to files.
//!
//! # The contract, in these adapters
//!
//! Every adapter here keeps the promises of [`SpaceAdapter`] (identifying,
//! reproducible, content only), and these rules, whatever its names are:
//!
//! - A text is no name by the grammar of the space alone, with no I/O.
//! - What the adapter is not sure of cannot be told: absent and there are said only
//!   of what it looked at.
//! - A name whose thing cannot be read (refused, failing part way, past a bound of
//!   that name) cannot be told, and every other name asked with it is answered; an
//!   error is for what stops the whole ask (a Git that cannot be run, or a call of it
//!   that takes longer than its bound), and then no name is answered. One space
//!   tells such a name otherwise, as DEC-54 decides: in `rust-item`, an item whose
//!   own page cannot be read or is past its bound is there under `presence` (its page
//!   is listed), and cannot be told under a comparison of its content.
//! - Each thing read for a name (a file, a page, a file of settings) is held to a
//!   bound of bytes, and so are, of what Git gives, its listing of the work tree, its
//!   answer of the renames and a blob read back; nesting walked by recursion or a
//!   stack, to a depth; a program run, to a time. Nothing is cut or passed over to
//!   answer another thing. In suggestions, Git's renames past their time or their
//!   bound are a note of the name, and not an error (DEC-60).
//! - It reads and writes nothing else, and reads nothing outside the root of the
//!   project or the build directory it declares.
//! - Of these, what may be larger than 1 MiB (a content, a page, a file of tests,
//!   Git's listing of the work tree and its answer of the renames) is parsed, scanned
//!   and hashed on a blocking thread when it is, never on the async ones; a smaller
//!   one is worked where it is asked for.
//! - Only an adapter that reads content back gives an address, and it reads back,
//!   by an address it gave, what its store holds: an address of a content the store
//!   does not hold (a file Git never added) reads back none.
//!
//! The domain fixes the shape of the answers ([`ArtifactReading`]) and how a content
//! is hashed; what a name is stays the adapter's. Each adapter writes, on its module
//! page, its space's definition under these seven headings:
//!
//! | heading | what it says |
//! |---|---|
//! | `The names of the space` | the grammar: the form, the separators and escapes, the characters allowed |
//! | `Positions` | for names that hold a position (a line, a range), how it is counted |
//! | `Equal names` | two ways of writing a name of one thing, and whether one is the form written |
//! | `One thing for a name` | what a name that matches two things comes to |
//! | `The answers` | when a name is there, absent, cannot be told, or no name |
//! | `What is compared` | what of the thing is the content, and the bounds of reading it |
//! | `Where it reads` | the root, and the build directory it declares |
//!
//! A test of this module's (`conformance`, private), through a hook each adapter
//! gives for its tests, holds every adapter, asked through the dispatch as a project
//! asks it, to the promises
//! of identifying and of reproducibility over one machine, to the answers above, to
//! each bound of bytes and of depth its hook declares, to reading back, and to the
//! seven headings; its page lists what it checks, and what it does not (the blocking
//! threads, reading and writing nothing else, the bound of time, and the time and the
//! locale).
//!
//! [`SpaceAdapter`]: whyshelf_core::app::shelf::SpaceAdapter
//! [`SpaceAdapter::presets`]: whyshelf_core::app::shelf::SpaceAdapter::presets
//! [`SpaceSuggest`]: whyshelf_core::app::shelf::SpaceSuggest
//! [`NamePrefixes`]: whyshelf_core::app::shelf::NamePrefixes
//! [`CannotSuggest`]: whyshelf_core::app::shelf::CannotSuggest
//! [`NoReadBack`]: whyshelf_core::app::shelf::NoReadBack
//! [`CannotList`]: whyshelf_core::app::shelf::CannotList
//! [`NameList`]: whyshelf_core::app::shelf::NameList
//! [`Spaces`]: whyshelf_core::app::shelf::Spaces
//! [`ArtifactReading`]: whyshelf_core::domain::shelf::artifact::ArtifactReading

pub mod file;
pub mod js_test;
mod names;
mod registry;
pub mod rust_item;
pub mod rust_test;

use whyshelf_core::app::shelf::Spaces;

use names::{Rename, Spelled, after, built, renamed_tests, unbuildable};
pub use registry::{Adapter, Registry, SpacesError};

/// The artifacts of a project: the dispatch of the core over the adapters of its
/// spaces.
pub type ArtifactSpaces = Spaces<Registry>;

/// The most bytes of an input worked on where it is asked for; a larger one is worked
/// on a blocking thread (the contract, on the module page).
const BLOCKING_PAST: usize = 1024 * 1024;

/// The result of `work` over an input of `len` bytes: run where it is asked for when
/// the input is at most [`BLOCKING_PAST`] bytes, and on a blocking thread when it is
/// larger (the contract, on the module page); or why the thread stopped (a bug if it
/// ever happens, or a runtime shutting down), which the adapter tells as cannot tell.
async fn on_blocking<T: Send + 'static>(
    len: usize,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    if len <= BLOCKING_PAST {
        return Ok(work());
    }
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod conformance;

#[cfg(test)]
mod prefix_tests;
