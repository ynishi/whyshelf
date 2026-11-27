//! The adapter of the space `rust-test`: the test functions of a project's Rust
//! source, by the file they are in and their path in it, read from the source
//! without running any program.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`RustTestSpace::resolve`] | reads each file named, by the rules of the space `file`, lexes it once, and finds each test named in it by a shallow scan of its items; the text of a test, with what it holds of the modules of its crate, is what is compared | [`file`](mod@file), the manifests (`Cargo.toml`) of the project, `proc-macro2`, `sha2` |
//! | [`RustTestSpace::current`] | the text of a named test now | the same |
//! | [`NameList::list`] | the tests of the `.rs` files a selection takes, the files being those the state of the project lists, chosen as the space `file` chooses them | [`file`](mod@file), the state of the project, `proc-macro2` |
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
//!               ├─ found: the function's tokens, then the items beside it in its
//!               │  test part, doc attributes left out, then what it holds
//!               │    ├─ what it holds cannot be told (R5) ──► cannot tell
//!               │    └─ ──► there (its text)
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
//! Any other text is not a name of the space, and the adapter says so with why.
//!
//! # Positions
//!
//! A name holds no position in a file: it names a test function whole.
//!
//! # Equal names
//!
//! A raw identifier and its plain spelling name the same item (`r#try` and a
//! function `try` written raw). Identifiers are compared as written, code point by
//! code point, not normalized (NFC): a name and a source that spell a non-ASCII
//! identifier in two forms do not meet. The path of the file is compared as the space
//! `file` compares one, case included. No name is rewritten into another.
//!
//! # One thing for a name
//!
//! Every inline module of a name on the way is searched (twins under their own
//! `#[cfg]`), and two functions of the name with a test attribute are one test, their
//! texts in the order of the file (finding the test, below).
//!
//! # Prefixes of the names
//!
//! `judge moved --prefix` moves every name under a prefix at once, and this adapter
//! says which names are under it and what each becomes
//! ([`NamePrefixes`]).
//! A prefix is of one of two forms, and a text is of one only:
//!
//! - the parts of the path of a file, when the text has no `::` (`src/a.rs` is the
//!   parts `src` and `a.rs`, `src/a` the parts `src` and `a`; one `/` at its end is
//!   dropped, so `src/a/` is `src/a`): a name is under it when the parts of its file's
//!   path start with the prefix's, so `src/a` holds `src/a/x.rs::t` and not
//!   `src/a.rs::t`, and `src/a.rs` holds each test of that file;
//! - a file and the first segments of a path in it, `<file>.rs::<segment>(::<segment>)*`
//!   (a module, with its tests and its modules): a name is under it when it is in that
//!   file and the segments of its path in it start with the prefix's. A segment is the
//!   same one however it is written (`r#foo` and `foo`).
//!
//! The same name under `to` is built of the parts as they were written: the parts of
//! `to` as the person typed them, then the rest of the name's as the name has them
//! (`src/a/x.rs::r#type` under `src/b` is `src/b/x.rs::r#type`: a spelling is kept,
//! not rewritten as `src/b/x.rs::type`, which is no name). Where a part ends is what
//! the grammar of a name found in reading it, and the name built is read back by it. A
//! `to` of the other form than `from`, one that cannot be read, or a name built that is
//! none (a file that does not end in `.rs`) or that reads as other parts than were
//! joined, makes each name under `from` unbuildable, with why. A `from` of neither form
//! (a module path with no file, say) is no prefix of the space's names. The adapter
//! looks for no text in a name.
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
//! To find the test, only the modules named are scanned below the file; once it is
//! found, every inline module of its test part is scanned too, for the items beside
//! it (what is compared, below). Each module named is every inline module
//! (`mod tests { … }`) of the name in the items before it: twins
//! under their own `#[cfg]` are each searched, as twin functions are joined. To find
//! the test, a twin declared out of line (`mod x;`) is not followed: a test not found
//! in the inline ones cannot be told. (What a test holds follows such a module to its
//! file: what a test holds, below.)
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
//! - an item macro is called (`include!` among them, whose file's items are not
//!   scanned for a test);
//! - a module carries an attribute other than the inert ones below (an attribute
//!   macro may rewrite the module);
//! - a function of the name carries such an attribute (`#[rstest]` may make it a
//!   test).
//!
//! The inert attributes are [`INERT_ATTRIBUTES`], any attribute of a tool
//! (`rustfmt::…`, `clippy::…`, `rust_analyzer::…`, `diagnostic::…`), a
//! `cfg_attr` whose attributes are all inert, an `unsafe(…)` of an inert one
//! (`#[unsafe(no_mangle)]`), and a `derive` of [`INERT_DERIVES`] only (those of the
//! standard library; a derive of a crate may write items). The reason names the first
//! few that may make it, each once, and how many more there are.
//!
//! So where a module on the way holds an item macro or an attribute that is not
//! inert, a test removed (or renamed) reads as "cannot tell", not as gone: an
//! approval of it stays fresh, with a notice that it cannot be told.
//!
//! # What is compared
//!
//! A test means what the helpers beside it do, so its text is its own text, then
//! the text of the items beside it in its test part, then what it holds:
//!
//! - Its own text is the function item's tokens as the source has them, from its
//!   first outer attribute to the end of its body (twins joined, in the order of
//!   the file, one on each line).
//! - Its test part is the outermost inline module on the way to it that carries a
//!   `cfg` holding only under test, outer or inner (`#[cfg(test)]`, `#![cfg(test)]`;
//!   an `all` of which one holds only under test, such as
//!   `#[cfg(all(test, unix))]`; an `any` of which each does), or the file when no
//!   module on the way does. Twins of a module on the way may be two parts: each is
//!   taken, in the order of the file, two apart by an empty line, and one inside
//!   another is taken once.
//! - The items beside it are every item of the part, in the order of the file, each
//!   on a line of its own after its own text: the inner attributes of the part, its
//!   helpers, `use`s, constants, types, `impl`s, macro definitions and item macro
//!   calls, and the items of each inline module in it, at any depth. A module in
//!   the part is read through, not printed: its name, its visibility and its braces
//!   are left out. Each line of an item inside it starts with a mark of how many
//!   modules of the part it is in, as many `>` (`> fn h () { }` one deep, `>>` two);
//!   no item starts with `>`, so a mark is never taken for an item, and a line of the
//!   part's own level has none. Then come the attributes of those modules (outer,
//!   then inner, outermost first), so that they stay tied to the items they apply
//!   to, then the item. So a module renamed, made public, or of tests only added
//!   changes no line, and a move of a prefix (`judge moved --prefix`) follows the
//!   test; a module with no item but tests is no line; and an item moved into or out
//!   of a module changes its mark. Every function read as a test, wherever it stands
//!   in the part, is left out, the test itself among them. So each line beside is
//!   one item, with its depth and every attribute over it, none of them a test, and
//!   the text tells where the test ends.
//! - A module declared out of line in the part (`mod common;`) and an `include!` are
//!   the tokens that declare them, beside; what of their files the test holds comes
//!   after (what a test holds, below). The code compiled always outside the part (the
//!   code under test, beside inline tests) is not compared.
//! - Nothing is parsed or resolved: an item is its tokens, and no call is followed.
//!
//! Each piece is printed as `proc-macro2` prints a token stream, each doc attribute
//! (`///`, `//!`, `#[doc …]`) left out where it stands as an attribute (on an item,
//! and on a statement or an item in a body; not in the tokens of a macro call,
//! where it is data): white space and comments count for nothing. So an edit of
//! them or of the docs, of another test, or of the code outside the part makes
//! nothing stale; any other edit of the function, or of an item beside it, does,
//! for every test of the part. The text is what [`ArtifactReading::of_bytes`]
//! hashes, so every comparison works over it:
//!
//! - `bytes` (the preset `test`): every token.
//! - `lines`: the text is a line for the test (one for each twin) and one for each
//!   item beside, save inside a string that spans lines, so it tells what `bytes`
//!   tells, under another hash; an `ignore` prefix is held against the start of
//!   each line, and one a line starts with (`#`) leaves out that line, so that no
//!   edit of it is seen. It is allowed, since the domain offers every comparison
//!   over a content, and adds nothing.
//! - `presence`: only that the test is there.
//!
//! There is no address: the text is made, not stored, so nothing is read back. The
//! file is read up to [`MAX_FILE_BYTES`] and scanned to [`MAX_DEPTH`] brackets
//! (finding the test, above).
//!
//! # What a test holds
//!
//! Some of the helpers a test reaches are in other files: a module compiled only under
//! test that its text names (`#[cfg(test)] mod fixtures_tests;`, `tests/common/mod.rs`),
//! and a file an include reads. Its text holds them, so that an edit of a helper makes
//! stale the tests that use it, and those only where a name says which. The crates are
//! the targets of the manifests of the project (each library, binary, integration test,
//! bench and example of the package at the root and of each member of its workspace),
//! and the modules of a target are read from its root file: the file of a module
//! declared out of line is found by the rules of the language (`x.rs` or `x/mod.rs`,
//! under the directory of the stem of a file that is neither a root nor a `mod.rs`, an
//! inline module a step of the directory, and `#[path]`). An item is compiled only under
//! test when a `cfg` that holds only under test (as for a test part, above) is on it,
//! outer or inner, or on a module around it, or when it is of an integration test; any
//! other item is compiled always. A module compiled only under test counts from the
//! outermost one of its file: the modules inline in it are its text, and one in another
//! file is another.
//!
//! - **R1** A test holds such a module of a target its file is in when a path names it:
//!   a path in the test's text (its own, and the items beside), in the items of the
//!   modules compiled only under test of its file outside its parts and outside the
//!   modules of its file it may hold, or in the held text of a module or residue item
//!   it already holds; each once. A path is a run of two or more identifiers joined by
//!   `::`, or a leaf of a `use` tree (`use a::{b, c::d as e}` is `a::b` and `a::c::d`),
//!   and is resolved from the module it is written in: `crate` is the root of the
//!   target, `super` the parent, `self` that module, and a first segment that is a
//!   module of it is that module. A segment that is no module (a name a `use` brought
//!   in, an alias of a module of code, a glob, the prelude) leaves the segments from it
//!   as names that hold a module of the same name; `::x` (another crate) and a `super`
//!   above the root name nothing. So `use crate::fixtures_tests as fx;` names
//!   `fixtures_tests`, and `fx::seed()` after it the same. The body of a `macro_rules!`
//!   is resolved where the macro is called, so all its segments are such names, and so
//!   is a lone identifier in the arguments of a macro that is not of the standard
//!   library or a logging one (`suite!(fixtures_tests)`). A lone identifier anywhere
//!   else (a local, a parameter, a field) names no module. A test never holds a module
//!   of its file that it is in, nor one in its test part, which its text already is.
//! - **R2** Such a module is held by every test of its target when a name of it can be
//!   used without the module's name: it defines a `macro_rules!` with
//!   `#[macro_export]`, it stands under `#[macro_use]`, a public glob `use` of a test
//!   residue brings its names in (`pub use M::*`, `pub(…)` too), it holds an `impl`
//!   whose self type it does not declare, or it is the root of the crate (the file of
//!   an integration test, which its modules reach as `crate` and `super`).
//! - **R2b** The items compiled only under test of a module of code, but modules
//!   (`#[cfg(test)] impl Shelf { fn seeded() … }`, `#[cfg(test)] use …`), are its test
//!   residue, and each is held by name (an identifier, where R1 holds a module by a
//!   path): a `fn`, `const`,
//!   `static`, `struct`, `enum`, `union`, `type` or `macro_rules!` when an identifier of
//!   its name stands in the test's text or in what it holds; an inherent `impl` when
//!   one of the names of its associated items is a member name there, and a `trait`
//!   when its name stands there as an identifier or one of its items' is a member name
//!   there. A member name is an identifier right after `.` or `::` (`.seeded(`,
//!   `Shelf::EMPTY`, `<X as Tr>::m`), since an associated item is reached only so, or a
//!   lone identifier in the arguments of a macro that is not of the standard library or
//!   a logging one (`check!(shelf, seeded)`); a local of the same word is none. Held by every test of the
//!   target, since a test uses them with no name it writes: a trait `impl` (`==`,
//!   `format!`, `.into()`, `?` reach it), a derive that holds only under test on an item
//!   of code (`#[cfg_attr(test, derive(…))]`), an `impl` with no associated item, and an
//!   item macro call. A residue `use` in a module is held by every test in that
//!   module's subtree of the modules of the target, whatever file it is in (`use
//!   super::*` reaches it), and a public one also by a test that names what it brings
//!   in (its last segment or its alias). What a held item names is followed as R1
//!   says. The items held of one module are one part, those items only, in the order
//!   of the file. R2b held every residue item by every test of the target once; on
//!   this repository that made nearly every approval stale at once, and an edit of a
//!   residue item stale a whole crate, so the residue is held by name. A `mod` item
//!   declares a module and is no use of it: `#[cfg(test)] mod fixtures_tests;` is held
//!   by the tests that name it, not by every test.
//! - **R3** An item compiled always, the code under test, is never held: `use
//!   super::*` reaching it changes nothing.
//! - **R4** An include (`include!`, `include_str!`, `include_bytes!`) in the text of
//!   the test or of what it holds, with a literal path, relative to the file of the
//!   call, is held as the SHA-256 of the bytes of the file it reads.
//! - **R5** What a test holds cannot be told, with why, only where it would reach what
//!   cannot be read: a module out of line whose file is not there or cannot be read, or
//!   whose `#[path]` is not one literal, reached by its name or standing under
//!   `#[macro_use]`; an include whose path is no literal (`concat!`, `env!`), leaves the
//!   root, or names a file that is not there; for the tests of a file in no target,
//!   while something of the project could not be read (a manifest, a member written as
//!   a pattern that is not read, a directory not listed whole, a module of a target
//!   whose file cannot be read, which the file may be under); and for every test of a
//!   target whose modules are past [`MAX_TREE_FILES`] files or [`MAX_TREE_BYTES`]
//!   bytes. Cannot tell leaves an approval as it is, with a notice; it is not toward
//!   stale, so its forms are few. A file in no target, while all was read, holds
//!   nothing.
//!
//! A module held is printed as the items beside a test are, doc attributes left out
//! and its tests too, so a module of tests only is not held. After the test's own text
//! and the items beside it come, when it holds anything, an empty line, then each
//! module held and each part of the residue items held, as a line `@ <file> <module>` (the module's
//! path in its target, from `crate`) and its held text, in the order of the file and
//! then of the module, then each include as a line `@@ <path> <64 hex>`, in the order
//! of the paths. No item starts with `@`, and a path holds no line break. A test that
//! holds nothing has the text it had before tests held anything, so its approval stays
//! fresh.
//!
//! Each file is read as the file of a test is (up to [`MAX_FILE_BYTES`], lexed to
//! [`MAX_DEPTH`]), once in a reading, whatever tests ask for it.
//!
//! # The answers
//!
//! | answer | when |
//! |---|---|
//! | there | the test is found, with its text |
//! | absent | no file is there; or it is scanned, and has no module or no function of the name with a test attribute, and nothing on the way may make one |
//! | cannot tell | a symbolic link on the path leaves the root or is broken; the file cannot be read, is too large, is not UTF-8, does not lex, or nests too deep; the test is not found, and a twin of a module on the way is declared out of line or a macro on the way may make it; or the test is found, and what it holds cannot be told (R5) |
//! | not a name | the text is not of the grammar above |
//!
//! # Listing the names of a selection
//!
//! A selection of this space is one of the space `file`: patterns over the paths
//! under the root, read by the same grammar ([`PathGlob`]), a path taken when some
//! `include` matches it and no `exclude` does; a text that is no pattern makes it no
//! selection, and its listing [`NameListing::CannotList`] with why.
//!
//! The files it takes are those the state of the project lists at the side, chosen
//! as the space `file` chooses the files of its own listing (the same code: through
//! a link only inside the root, within [`MAX_FILE_BYTES`], named as written); of
//! them, those whose path ends in `.rs`. Each is read and lexed as for a name
//! (finding the test, above), and its items are walked into every inline module, at
//! any depth: each function that carries a test attribute is a test, named by the
//! file, the modules on the way and the function, each segment as the source spells
//! it (`fn r#type` is `<file>::r#type`), and read back by the grammar of the names.
//! Twins (a function or a module of one name under two `#[cfg]`s, or spelled raw and
//! plain) are one name, the first in the order of the file. A module declared out of
//! line is in another file, whose tests are listed when the selection takes that
//! file. A test listed lives where "Where a name lives" (below) says of the file it
//! is listed of.
//!
//! Nothing is passed over in silence. A file taken that cannot be read, is not UTF-8,
//! does not lex or nests too deep, and one in which something may make a test the
//! scan does not see, is a part of the listing ([`NameListing::Parted`]), never
//! listed whole: the file whole, in the space `file` (its path as written, the name
//! the space `file` gives it), with why (the first thing found that may make a test,
//! or why it cannot be read), and the tests found in it, or that it could not be read
//! at all ([`PartRead`]). A file read with no test found in it, beside something that
//! may make one, is a part as any other: whether it holds a test cannot be told. A
//! cover that takes each part whole reaches it through the link it names for parts,
//! one that could not be read as well (a document names the file as for any file, and
//! the space `file` reads it, whatever its bytes are); to any other the listing is not
//! the whole. The listing is
//! [`NameListing::Some`] with why, and never all of them, when what keeps the whole
//! selection from being listed is so, which says nothing of which file is missing:
//! what the space `file` would not list whole (more files than it lists, a listing of
//! the state left short, a file a reading cannot tell), a test's name that cannot be
//! written as a name of the space, a file to be a part whose path is no name of the
//! space `file`, more tests than the bound of names of a listing
//! ([`ScanLimits::names`](super::file::ScanLimits::names), the tests of the parts
//! counted with the others), and more files, or more bytes in all, than one listing
//! reads ([`ScanLimits::files`](super::file::ScanLimits::files),
//! [`ScanLimits::total_bytes`](super::file::ScanLimits::total_bytes)): it stops
//! there, and says so, the tests found in the parts among the names. The size of a
//! file is looked at before it is opened, so a file past the bytes left is not read,
//! and no listing reads past the bound. A file with nothing that may make a test, and
//! with no test, adds nothing. When the state cannot list the files (outside a Git
//! repository, a Git that cannot be run), no listing is made:
//! [`NameListing::CannotList`] with why, and no error. A side the run does not read is
//! an error of the whole ask.
//!
//! What may make a test the scan does not see, for a listing, is wider than on the
//! way to one named test (what may make a test, above), since a listing answers for
//! every item of the file: an item macro call; an inner attribute that is not inert;
//! an attribute that is not inert on any item at all (a module, a function that is no
//! test, an `impl`, a `struct`, an `enum`, a `trait`, a `const`, a `static`, a module
//! out of line, a macro call), an attribute macro there being free to write test
//! functions; and on a test, any such attribute beside its test attribute
//! (`#[test] #[gen_variants]`), which may write more tests. A test attribute that is
//! a path (`#[tokio::test]`, `#[test_log::test]`) is a macro of a crate: the function
//! is listed as the test of its name, and the file is not whole, since such a macro
//! may write more than one test. A `derive` is inert only when every derive it names
//! is one of [`INERT_DERIVES`], under `std` or `core`, or by its bare name when, in
//! the module the derive is written in, no `use` names an item so (as the last
//! segment of a path or an alias) and no glob `use` (`use x::*;`) is written: a `use`
//! may bring a crate's derive in under a name of the standard library's, so a
//! spelling alone is not trusted. The scope is the module's own, as Rust resolves a
//! name: a `use` of a module around it, or of a module in it, does not reach it, so
//! `use super::*;` in `mod tests` makes the bare derives of `mod tests` makers, and
//! none of the file's others. Any other
//! derive is a macro of a crate, and may. A module out of line read by `#[path]` from
//! a file that does not end in `.rs` (`#[path = "x.inc"] mod tests;`), or by a
//! `path` the scan cannot read as one literal, is never taken by a listing, so its
//! tests would be passed over: the file that declares it is not whole. Such an
//! attribute beside a test does not change how the test itself is resolved: the
//! test named is there, with its text, while the listing of its file is not whole.
//!
//! # Where it reads
//!
//! The files under the root of the project, by the rules of the space `file`
//! (through a symbolic link only while it stays inside the root); for a listing, the
//! files a selection takes are those the state of the project lists. It declares no
//! build directory, and runs no program.
//!
//! # Where a name lives
//!
//! A test lives, whole ([`Placement::Only`]), in the files its text is read from: the
//! file its name writes before `::`, the file of each module it holds and each file its
//! includes read (what a test holds, above), each as the space `file` places a path:
//! the path as written when no link is on it; with links inside the root on its way,
//! each link entry and the name they lead to. A test that is not there lives where it
//! would be, in its file alone, and one whose file cannot be read where that file is
//! placed. A link that leaves the root or is broken, or a kind of file that cannot be
//! told, on the way to any of the files, places it nowhere ([`Placement::Unplaced`]),
//! with the reason `resolve` gives; so does what it holds when it cannot be told, the
//! reason naming the files that were told; and so does a text that is not of the
//! grammar, by the grammar alone, with nothing read.
//!
//! # Known limits
//!
//! - A space reads the trees of the project's crates once for each side, when its first
//!   test is found (a reading that finds no test reads none), and keeps them for its
//!   life. A space is built for one run, so a source edited while one space lives is not
//!   seen by it; build another space to read the project again.
//! - The text holds the items beside the test in its test part, not only those it
//!   calls: an edit of a helper there makes every test of the part stale, the tests
//!   that do not call it among them. Split the part into modules that hold only
//!   under test to keep them apart.
//! - A function made a test by an attribute rust-test does not read as one
//!   (`#[rstest]`, `#[test_case(…)]`), and a test written inside an item macro call
//!   (`proptest! { … }`), is an item beside: an edit of it makes the other tests of
//!   the part stale. The error is toward stale.
//! - A module in the part counts for its depth, its attributes and its items, not
//!   for its name: where two sibling modules at one depth, with the same attributes,
//!   meet, the boundary can shift over their items in their order and change no line
//!   (`mod a { fn x() {} } mod b { fn y() {} }` read as
//!   `mod a { fn x() {} fn y() {} } mod b {}`). Which module an item is in, and so
//!   what a test calls by a path or through a glob `use`, can then change while an
//!   approval of the test stays fresh. This is accepted only there: items at one
//!   depth, in one order, across sibling modules; any other move, a swap of two
//!   items of one name across a module's boundary among them, changes the text.
//! - What is outside the test part and not held is not compared: a fixture the test
//!   reads as it runs (`std::fs::read`), a file an include in the definition of a
//!   macro reads (`include_str!($path)`), and the outer attributes of the module that
//!   is the part and of the modules on the way to it (a `cfg` that stops one from
//!   being compiled among them; an inner `#![cfg(..)]` of the part is an item beside).
//!   An edit of any of them leaves an approval of the test fresh; name such a file
//!   whole beside the test.
//! - A module is held by a path that resolves to it, not by an identifier of its name:
//!   a local or a parameter named `tests` holds nothing. A name that no module
//!   resolves (a `use` alias, a glob, a macro's body) falls back to the module of that
//!   name anywhere in the target, which is toward stale; a module reached by `super`
//!   alone, from a module in another file of it, is not held. What a module of its file
//!   outside its parts names counts only once that module is held.
//! - The file is read whether or not a crate compiles it: a test in a file no
//!   crate includes (no `mod` names it), or under a `cfg` never set, reads as there,
//!   and, in no target, holds nothing.
//! - The file is lexed and scanned, not parsed: one whose tokens lex but which is no
//!   valid Rust is read as far as its items can be told, and an item in it may be
//!   missed.
//! - To find a test, a module declared out of line (`mod x;`, `#[path] mod x;`) is
//!   not followed: a test under it cannot be told. Name the test by the file of the
//!   module instead.
//! - A test a macro writes is not in the source as a function: it cannot be told
//!   when a macro on the way is seen (above), and reads as absent when an attribute
//!   macro on another item (a `struct`, an `impl`) makes it.
//! - A text is the tokens as `proc-macro2` prints them; an update of it that prints
//!   them otherwise would make every approval of a test stale once.
//! - Moves are not suggested: the adapter names [`CannotSuggest`].
//! - A cover compares names as written, so a document that spells a test otherwise
//!   than the listing does (`foo` for a function written `r#foo`) does not reach it,
//!   though `resolve` reads both as the one test: write the name as the listing
//!   spells it.
//!
//! [`PathGlob`]: whyshelf_core::domain::location::PathGlob

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use proc_macro2::{Delimiter, Group, Ident, Punct, Spacing, TokenStream, TokenTree};
use whyshelf_core::app::shelf::{
    Answers, CannotSuggest, NameList, NamePlaces, NamePrefixes, NoReadBack, Side, SpaceAdapter,
};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, Comparison, ListingPart, NameListing, NotASelection, PartRead,
    PrefixMove, Selection, SpaceAsks, SpacePrefixes, SpaceReadings, SpaceRenamed,
};
use whyshelf_core::domain::shelf::placement::Placement;
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::super::state::ProjectTree;
use super::super::state::StateError;
use super::super::state::files::FileStates;
use super::Spelled;
use super::file::{self, Bounded, Cause, FileSelection, Lister, Take, Taken, Unit};

mod tree;

use tree::{Held, ModuleTrees, Reached, Refs, TreeBounds};

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

/// The most module files the modules of one crate are read from, for what a test
/// holds; past it, what a test of the crate holds cannot be told. Each file is still
/// read within [`MAX_FILE_BYTES`] and [`MAX_DEPTH`].
pub const MAX_TREE_FILES: usize = 4096;

/// The most bytes of the module files of one crate read, for what a test holds; past
/// it, what a test of the crate holds cannot be told.
pub const MAX_TREE_BYTES: u64 = 256 * 1024 * 1024;

/// The attributes that make no item and rewrite none (with those of a tool, and a
/// `cfg_attr` of them): one of them on an item does not make a test that is not found
/// one that cannot be told, nor a listing that is not whole.
///
/// They are the built-in attributes the language defines, which are no macros: the
/// Rust Reference, "Attributes", "Built-in attributes index"
/// (<https://doc.rust-lang.org/reference/attributes.html#built-in-attributes-index>),
/// with `coverage`, which the compiler reads the same way. Left out of it: `test`, a
/// test attribute, read on its own; `derive`, read derive by derive
/// ([`INERT_DERIVES`]). `macro_use` and `macro_export` are in it: they say where a
/// macro's name is seen and write no item, and a call of a macro is a maker of its
/// own. An attribute of a crate (a derive's helper `serde`, `error`, `command`) is
/// none of these, and is no inert attribute.
pub const INERT_ATTRIBUTES: &[&str] = &[
    // Conditional compilation.
    "cfg",
    "cfg_attr",
    // Testing.
    "ignore",
    "should_panic",
    // Derive.
    "automatically_derived",
    // Macros.
    "macro_export",
    "macro_use",
    "proc_macro",
    "proc_macro_derive",
    "proc_macro_attribute",
    // Diagnostics.
    "allow",
    "expect",
    "warn",
    "deny",
    "forbid",
    "deprecated",
    "must_use",
    // ABI, linking, symbols and library handling.
    "link",
    "link_name",
    "link_ordinal",
    "no_link",
    "repr",
    "crate_type",
    "no_main",
    "export_name",
    "link_section",
    "no_mangle",
    "used",
    "crate_name",
    // Code generation.
    "inline",
    "cold",
    "naked",
    "no_builtins",
    "target_feature",
    "track_caller",
    "instruction_set",
    "coverage",
    // Documentation.
    "doc",
    // Preludes.
    "no_std",
    "no_implicit_prelude",
    // Modules.
    "path",
    // Limits.
    "recursion_limit",
    "type_length_limit",
    // Runtime.
    "panic_handler",
    "global_allocator",
    "windows_subsystem",
    // Features.
    "feature",
    // Type system.
    "non_exhaustive",
    // Debugger.
    "debugger_visualizer",
    "collapse_debuginfo",
];

/// The derives whose output is known to add no item that can be a test: those of the
/// standard library. Any other derive (a derive macro of a crate) may, and is no
/// inert attribute.
pub const INERT_DERIVES: &[&str] = &[
    "Debug",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "Default",
];

/// The mark of a path of a derive among the paths of an attribute.
const DERIVE: &str = "derive";

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

/// The presets this adapter offers: one, [`TEST_PRESET`], for a link to the tests
/// that show a document holds, that stands on each token of them
/// (`space` [`RUST_TEST_SPACE`], `compare = "bytes"`, `means = "depends"`): an edit
/// of a test, of an item beside it in its test part, or of what it holds, makes the
/// approvals that name it stale, one of another test in the file does not, and a test
/// that goes away breaks them.
pub fn presets() -> BuiltinPresets {
    let values = PresetValues {
        space: Some(RUST_TEST_SPACE.to_owned()),
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
    /// The state of the project could not be read at the side asked.
    #[error(transparent)]
    State(#[from] StateError),
}

/// The space [`RUST_TEST_SPACE`]: it answers a move of a prefix, cannot suggest where
/// a test moved, keeps nothing by address, lists the tests of the files a selection
/// takes, and says where a test lives.
impl SpaceAdapter for RustTestSpace {
    type Error = RustTestError;
    type Prefixes = Answers;
    type Suggest = CannotSuggest;
    type ReadBack = NoReadBack;
    type List = Answers;
    type Places = Answers;

    fn space(&self) -> &str {
        RUST_TEST_SPACE
    }

    fn presets(&self) -> BuiltinPresets {
        presets()
    }

    async fn resolve(&self, side: Side, asks: &SpaceAsks) -> Result<SpaceReadings, RustTestError> {
        self.resolve_at(side, asks).await
    }

    async fn current(
        &self,
        side: Side,
        name: &ArtifactName,
    ) -> Result<Option<Vec<u8>>, RustTestError> {
        self.current_at(side, name).await
    }
}

impl NamePrefixes for RustTestSpace {
    fn renamed(
        &self,
        names: &BTreeSet<ArtifactName>,
        of: &PrefixMove,
    ) -> Result<(SpacePrefixes, SpaceRenamed), RustTestError> {
        renamed(names, of).map_err(RustTestError::Reason)
    }
}

impl NameList for RustTestSpace {
    /// A selection is one of the space `file`: patterns over the paths of files.
    fn select(&self, selection: &Selection) -> Result<(), NotASelection> {
        FileSelection::read(selection).map(|_| ())
    }

    /// The tests of the `.rs` files the selection takes at `side`: the module page,
    /// "Listing the names of a selection".
    async fn list(&self, side: Side, selection: &Selection) -> Result<NameListing, RustTestError> {
        self.list_at(side, selection).await
    }
}

impl NamePlaces for RustTestSpace {
    /// A test lives in the files of the file its name writes before `::` and of what
    /// it holds, whole: the module page, "Where a name lives".
    async fn places(
        &self,
        side: Side,
        names: &BTreeSet<ArtifactName>,
    ) -> Result<BTreeMap<ArtifactName, Placement>, RustTestError> {
        self.state.reads(side)?;
        let mut placed = BTreeMap::new();
        let mut by_file: BTreeMap<RelPath, Vec<(&ArtifactName, TestName)>> = BTreeMap::new();
        for name in names {
            match TestName::parse(name.as_str()) {
                Ok(test) => by_file
                    .entry(test.file.clone())
                    .or_default()
                    .push((name, test)),
                Err(why) => {
                    placed.insert(name.clone(), Placement::Unplaced(reason(why.reason())?));
                }
            }
        }
        for (path, tests) in by_file {
            let wanted: Vec<Wanted> = tests.iter().map(|(_, t)| t.wanted()).collect();
            let found = self.find_in(side, &path, wanted).await;
            let found = self.holding(side, &path, found).await;
            for ((name, _), found) in tests.into_iter().zip(found) {
                let placement = match &found {
                    Found::Text(text) => self.placed_holding(side, &path, &text.held).await?,
                    // What it holds cannot be told: nowhere, the files told named.
                    Found::CannotTell(why @ Untellable::Helper { told, .. }) => {
                        let told: Vec<&str> = told.iter().map(RelPath::as_str).collect();
                        Placement::Unplaced(reason(format!(
                            "{why}; the files told are {}",
                            told.join(", ")
                        ))?)
                    }
                    // A test not there lives where it would be; one whose file cannot
                    // be told, where its file is placed.
                    Found::Absent | Found::CannotTell(_) => {
                        file::placed_only(&self.state, side, &path)
                            .await
                            .map_err(RustTestError::Reason)?
                    }
                };
                placed.insert(name.clone(), placement);
            }
        }
        Ok(placed)
    }
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
    #[error("`{0}` does not read back as the file and the segments it was written of")]
    NotReadBack(String),
    #[error("`{name}` is refused as a name: {why}")]
    Refused { name: String, why: String },
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
    /// What the test holds cannot be told: where, why, and the files that were told
    /// (the test's own, and those of what it was found to hold before).
    #[error("what the test holds cannot be told, at {at}: {why}")]
    Helper {
        at: RelPath,
        why: String,
        told: Vec<RelPath>,
    },
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
    /// The tree of the project the files are read from.
    state: ProjectTree,
    /// The files a selection takes, the bounds of a listing, and the most bytes of a
    /// file read ([`MAX_FILE_BYTES`]): one value for each, for the listing and the
    /// reading alike.
    lister: Lister,
    /// The bounds the modules of one crate are read within ([`MAX_TREE_FILES`],
    /// [`MAX_TREE_BYTES`]).
    tree: TreeBounds,
    /// The trees of the crates of the project, read once for each side, when a first
    /// test is found, and shared by every call and every clone of the space. A space is
    /// built for one run (`shelf/mod.rs` builds one per run), so a source edited while
    /// one space lives is not seen by it: build another to read the project again.
    trees: TreeCache,
}

/// The trees of the crates of a project, one for each [`Side`], read when first asked.
#[derive(Clone, Default)]
struct TreeCache(Arc<[tokio::sync::OnceCell<ModuleTrees>; 2]>);

impl fmt::Debug for TreeCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TreeCache")
    }
}

impl TreeCache {
    /// The trees at `side`, read now when they are not yet. The one place a side is
    /// mapped to its cell.
    async fn at(&self, space: &RustTestSpace, side: Side) -> &ModuleTrees {
        let cell = match side {
            Side::Before => &self.0[0],
            Side::After => &self.0[1],
        };
        cell.get_or_init(|| ModuleTrees::of(space, side)).await
    }
}

impl RustTestSpace {
    /// The tests in the files under `root`, read at one side (the root as it is).
    pub fn new(root: PathBuf) -> Self {
        Self::on(ProjectTree::files(FileStates::new(root)))
    }

    /// The tests in the files of the project `state` reads.
    pub fn on(state: ProjectTree) -> Self {
        Self {
            lister: Lister::on(state.clone(), MAX_FILE_BYTES),
            state,
            tree: TreeBounds::default(),
            trees: TreeCache::default(),
        }
    }

    /// The same, the modules of a crate read within `files` files and `bytes` bytes.
    #[cfg(test)]
    fn with_tree_bounds(mut self, files: usize, bytes: u64) -> Self {
        self.tree = TreeBounds { files, bytes };
        self
    }

    /// The same, listing within `limits`.
    #[cfg(test)]
    fn with_limits(mut self, limits: super::file::ScanLimits) -> Self {
        self.lister = self.lister.with_limits(limits);
        self
    }

    /// The same, a file being read up to `max_file` bytes.
    #[cfg(test)]
    fn with_max_file(mut self, max_file: u64) -> Self {
        self.lister = self.lister.with_max_file(max_file);
        self
    }

    /// For each name, as the project is ([`Side::After`]): not a name, the hash of the
    /// test's text under each comparison asked, absent, or cannot tell (with why).
    /// Every name asked about has an answer; each file is read and lexed once.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, RustTestError> {
        self.resolve_at(Side::After, asks).await
    }

    /// The text of a test as the project is ([`Side::After`], what is compared), or
    /// none when the name is none, or the test is not there or cannot be told.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, RustTestError> {
        self.current_at(Side::After, name).await
    }

    /// [`resolve`](Self::resolve), at `side`.
    async fn resolve_at(
        &self,
        side: Side,
        asks: &SpaceAsks,
    ) -> Result<SpaceReadings, RustTestError> {
        self.state.reads(side)?;
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
            let found = self.find_in(side, &path, wanted).await;
            let found = self.holding(side, &path, found).await;
            // The texts may be as large as the file: large ones are hashed on a
            // blocking thread.
            let asked: Vec<BTreeSet<Comparison>> =
                tests.iter().map(|(_, c, _)| (*c).clone()).collect();
            let len = found.iter().fold(0_usize, |len, found| match found {
                Found::Text(text) => len.saturating_add(text.len()),
                Found::Absent | Found::CannotTell(_) => len,
            });
            let told = super::on_blocking(len, move || {
                found
                    .into_iter()
                    .zip(asked)
                    .map(|(found, asked)| {
                        Ok(match found {
                            // Joined one at a time, so that the texts of the items
                            // beside, which every test of a part shares, are held once.
                            Found::Text(text) => {
                                ArtifactReading::of_bytes(text.joined().as_bytes(), &asked, None)
                            }
                            Found::Absent => ArtifactReading::absent(),
                            Found::CannotTell(why) => {
                                ArtifactReading::cannot_tell(reason(why.to_string())?)
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, RustTestError>>()
            })
            .await;
            let told = match told {
                Ok(told) => told?,
                Err(why) => {
                    let why = Untellable::Stopped {
                        path: path.clone(),
                        why,
                    };
                    vec![ArtifactReading::cannot_tell(reason(why.to_string())?); tests.len()]
                }
            };
            for ((name, _, _), reading) in tests.into_iter().zip(told) {
                readings.insert(name.clone(), reading);
            }
        }
        Ok(readings)
    }

    /// [`current`](Self::current), at `side`.
    async fn current_at(
        &self,
        side: Side,
        name: &ArtifactName,
    ) -> Result<Option<Vec<u8>>, RustTestError> {
        self.state.reads(side)?;
        let Ok(test) = TestName::parse(name.as_str()) else {
            return Ok(None);
        };
        let found = self.find_in(side, &test.file, vec![test.wanted()]).await;
        let found = self.holding(side, &test.file, found).await;
        Ok(match found.into_iter().next() {
            Some(Found::Text(text)) => Some(text.joined().into_bytes()),
            Some(Found::Absent | Found::CannotTell(_)) | None => None,
        })
    }

    /// Each of `found` in the file at `path` at `side`, a test found with what it holds
    /// ([`ModuleTrees::held_by`]), one whose holding cannot be told as cannot tell. The
    /// trees are read when a first test is found, once for the life of the space; a
    /// reading that finds no test reads none.
    async fn holding(&self, side: Side, path: &RelPath, found: Vec<Found>) -> Vec<Found> {
        let mut holding = Vec::with_capacity(found.len());
        for found in found {
            holding.push(match found {
                Found::Text(mut text) => {
                    let trees = self.trees.at(self, side).await;
                    match trees.held_by(self, side, &text, path).await {
                        Ok(held) => {
                            text.held = held;
                            Found::Text(text)
                        }
                        Err(why) => Found::CannotTell(why),
                    }
                }
                other => other,
            });
        }
        holding
    }

    /// Where a test of the file at `path` that holds `held` lives: the files of its
    /// own file, of each module it holds and of each file its includes read, each as
    /// the space `file` places a path; nowhere, with why, when one of them is.
    async fn placed_holding(
        &self,
        side: Side,
        path: &RelPath,
        held: &Held,
    ) -> Result<Placement, RustTestError> {
        let mut files = BTreeSet::new();
        for file in held.files(path) {
            match file::placed_only(&self.state, side, &file)
                .await
                .map_err(RustTestError::Reason)?
            {
                Placement::Only(placed) => files.extend(placed.iter().cloned()),
                other @ (Placement::Partly(_) | Placement::Unplaced(_)) => return Ok(other),
            }
        }
        match NonEmptySet::from_set(files) {
            Ok(files) => Ok(Placement::Only(files)),
            // The test's own file is among them, so there is one.
            Err(_) => file::placed_only(&self.state, side, path)
                .await
                .map_err(RustTestError::Reason),
        }
    }

    /// The tests of the `.rs` files `selection` takes at `side` (the rules on the
    /// page of the module, "Listing the names of a selection"): the files are those
    /// the state lists, taken as the space `file` takes them ([`Lister::taken`]); each
    /// is read as `resolve` reads one and its items walked into every inline module;
    /// and the listing is all of them only when nothing kept it from being so
    /// ([`file::listing`]). A selection that is none, and a state that cannot list,
    /// give no listing, and no error.
    async fn list_at(
        &self,
        side: Side,
        selection: &Selection,
    ) -> Result<NameListing, RustTestError> {
        self.state.reads(side)?;
        let chosen = match FileSelection::read(selection) {
            Ok(chosen) => chosen,
            Err(not) => return Ok(NameListing::CannotList(not.why().clone())),
        };
        let takes = |path: &RelPath| {
            if path.as_str().ends_with(".rs") && chosen.takes(path.as_str()) {
                Take::Named(path.clone())
            } else {
                Take::Skip
            }
        };
        // The bound of names is of the tests listed, counted where their names are
        // made (below), not of the files.
        let (files, mut causes) = match self.lister.taken(side, takes, None).await? {
            Taken::CannotList(why) => return Ok(NameListing::CannotList(why)),
            Taken::All(files) => (files, Vec::new()),
            Taken::Some { found, short } => (
                found,
                short.into_vec().into_iter().map(Cause::from).collect(),
            ),
        };
        let limits = *self.lister.limits();
        let limit = limits.names;
        let mut found = BTreeSet::new();
        // The names listed, of the files listed whole and of the parts alike: the
        // bound of names is of all of them.
        let mut listed = 0usize;
        let mut unwritten = 0usize;
        let mut unlisted = Unlisted::default();
        // The files and the bytes the listing has read, held to the bounds of what
        // one listing reads ([`ScanLimits::files`](super::file::ScanLimits::files),
        // [`ScanLimits::total_bytes`](super::file::ScanLimits::total_bytes)).
        // The size of a file is looked at before it is opened, so no file is read past
        // the bound of bytes in all.
        let mut read_bytes = 0u64;
        'files: for (read_files, path) in files.iter().enumerate() {
            let left = limits.total_bytes.saturating_sub(read_bytes);
            let stop = |read_bytes: u64| {
                reason(format!(
                    "the listing stops after reading {read_files} file(s) of {read_bytes} bytes in all, the most one listing reads"
                ))
                .map(Cause::Own)
            };
            if read_files >= limits.files || left == 0 {
                causes.push(stop(read_bytes)?);
                break;
            }
            let walked = match self.tests_in(side, path, left, &mut read_bytes).await {
                Ok(Some(walked)) => walked,
                Ok(None) => {
                    causes.push(stop(read_bytes)?);
                    break;
                }
                Err(why) => {
                    unlisted.unread(path, &why)?;
                    continue;
                }
            };
            let mut here = BTreeSet::new();
            let mut past = false;
            for name in walked.names {
                let Ok(name) = name else {
                    unwritten += 1;
                    continue;
                };
                if found.contains(&name) || here.contains(&name) {
                    continue;
                }
                if listed == limit {
                    causes.push(Cause::PastNames { limit });
                    past = true;
                    break;
                }
                listed += 1;
                here.insert(name);
            }
            match walked.maker {
                Some(maker) => unlisted.made(path, &maker, here)?,
                None => found.extend(here),
            }
            if past {
                break 'files;
            }
        }
        if let Some(count) = NonZeroUsize::new(unwritten) {
            causes.push(Cause::Unnameable {
                count,
                of: Unit::Tests,
            });
        }
        let (parts, unnamed) = unlisted.into_parts();
        if let Some(count) = NonZeroUsize::new(unnamed) {
            causes.push(Cause::Unnameable {
                count,
                of: Unit::Files,
            });
        }
        file::listing(found, causes, parts).map_err(RustTestError::Reason)
    }

    /// The tests of the file at `path` at `side`, read as `resolve` reads it, and the
    /// first thing in it that may make a test the scan cannot see; or why it cannot
    /// be read; or none, unread, when it is larger than the `left` bytes the listing
    /// may still read. The bytes read are added to `read`.
    async fn tests_in(
        &self,
        side: Side,
        path: &RelPath,
        left: u64,
        read: &mut u64,
    ) -> Result<Option<Walked>, Untellable> {
        let max = self.lister.max_file();
        let bytes = match self.source_at(side, path, max.min(left)).await {
            Source::Bytes(bytes) => {
                *read = read.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                bytes
            }
            // Past what the listing may still read, and within what a file may be.
            Source::Untold(Untellable::TooLarge { .. }) if left < max => return Ok(None),
            // The listing saw it there: what is not there now cannot be passed over.
            Source::NoFile => {
                return Err(Untellable::Unreadable {
                    path: path.clone(),
                    why: "no file is there now".to_owned(),
                });
            }
            Source::Untold(why) => return Err(why),
        };
        let at = path.clone();
        // The file may be as large as the bound: it is decoded and lexed on a
        // blocking thread.
        let walk = move || match String::from_utf8(bytes) {
            Ok(text) => tests_of(&at, &text),
            Err(_) => Err(Untellable::NotUtf8(at.clone())),
        };
        match tokio::task::spawn_blocking(walk).await {
            Ok(walked) => walked.map(Some),
            Err(e) => Err(Untellable::Stopped {
                path: path.clone(),
                why: e.to_string(),
            }),
        }
    }

    /// The bytes of the file at `path` at `side`, read by the rules of the space
    /// `file` up to `max` bytes (its size looked at before it is opened), or what
    /// keeps them from being read.
    async fn source_at(&self, side: Side, path: &RelPath, max: u64) -> Source {
        let unreadable = |why: String| {
            Source::Untold(Untellable::Unreadable {
                path: path.clone(),
                why,
            })
        };
        match file::read_within(&self.state, side, path, max).await {
            Ok(Bounded::Bytes(bytes)) => Source::Bytes(bytes),
            Ok(Bounded::NoFile) => Source::NoFile,
            Ok(Bounded::Outside) => Source::Untold(Untellable::Outside(path.clone())),
            Ok(Bounded::TooLarge) => Source::Untold(Untellable::TooLarge {
                path: path.clone(),
                max,
            }),
            Ok(Bounded::Untold(why)) => unreadable(why.as_str().to_owned()),
            Err(e) => unreadable(e.to_string()),
        }
    }

    /// What each of `wanted` comes to in the file at `path`, in their order.
    async fn find_in(&self, side: Side, path: &RelPath, wanted: Vec<Wanted>) -> Vec<Found> {
        let every = |found: Found| vec![found; wanted.len()];
        let bytes = match self.source_at(side, path, self.lister.max_file()).await {
            Source::Bytes(bytes) => bytes,
            Source::NoFile => return every(Found::Absent),
            Source::Untold(why) => return every(Found::CannotTell(why)),
        };
        let count = wanted.len();
        let at = path.clone();
        // The file may be as large as the bound: it is decoded and lexed on a
        // blocking thread.
        let look = move || match String::from_utf8(bytes) {
            Ok(text) => look_up(&at, &text, &wanted),
            Err(_) => vec![Found::CannotTell(Untellable::NotUtf8(at.clone())); wanted.len()],
        };
        match tokio::task::spawn_blocking(look).await {
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

/// What a file is for a reading of its tests: its bytes, no file there, or why it
/// cannot be told.
enum Source {
    Bytes(Vec<u8>),
    NoFile,
    Untold(Untellable),
}

/// The tests of one file, walked: the name of each (or why it cannot be written), in
/// the order of the file, twins once, and the first thing in the file that may make a
/// test the scan cannot see.
struct Walked {
    names: Vec<Result<ArtifactName, NotAName>>,
    maker: Option<String>,
}

/// The files a listing of tests could not list whole, as the adapter says them: the
/// one place a part of its listing and its why are made. Each is a part, the file
/// whole in the space `file` ([`file::name_of`]), in the order of the files: one that
/// may hold a test the scan cannot see, with the first thing in it that may make one
/// and the tests found in it, or one that cannot be read, with why. A file whose path
/// is no name of the space `file` cannot be a part, and is counted.
#[derive(Debug, Default)]
struct Unlisted {
    parts: Vec<ListingPart>,
    unnamed: usize,
}

impl Unlisted {
    /// The file at `path` holds `maker`, which may make a test the scan does not see,
    /// beside the tests `found` in it.
    fn made(
        &mut self,
        path: &RelPath,
        maker: &str,
        found: BTreeSet<ArtifactName>,
    ) -> Result<(), RustTestError> {
        let why = format!("{maker} may make a test, and macros are not expanded");
        self.add(path, why, PartRead::Found(found))
    }

    /// The file at `path` cannot be read, for `why`.
    fn unread(&mut self, path: &RelPath, why: &Untellable) -> Result<(), RustTestError> {
        self.add(path, why.to_string(), PartRead::Unread)
    }

    fn add(&mut self, path: &RelPath, why: String, read: PartRead) -> Result<(), RustTestError> {
        let why = reason(why)?;
        match file::name_of(path) {
            Some(key) => self.parts.push(ListingPart::new(key, why, read)),
            None => self.unnamed = self.unnamed.saturating_add(1),
        }
        Ok(())
    }

    /// The parts, and how many files could be none.
    fn into_parts(self) -> (Vec<ListingPart>, usize) {
        (self.parts, self.unnamed)
    }
}

/// The tests in the source `text` of the file at `path`: the file is lexed once (as
/// `resolve` lexes it), and its items walked into every inline module, a function
/// that carries a test attribute ([`Scanned::is_test`]) being a test, named as the
/// source spells it ([`TestName::of`]). Twins (under their own `#[cfg]`, or spelled
/// raw and plain) are one test, the first in the order of the file. What may make a
/// test is gathered over every item, by the rule on the page of the module ("Listing
/// the names of a selection"): an item macro call, an inner attribute that is not
/// inert, an attribute that is not inert on any item, and on a test, one beside its
/// test attribute.
fn tests_of(path: &RelPath, text: &str) -> Result<Walked, Untellable> {
    let mut written: Vec<(Vec<String>, String)> = Vec::new();
    let mut makers = Makers::default();
    {
        let _spans = Spans;
        let top = Items::scan(lexed(path, text)?);
        let mut twins = BTreeSet::new();
        let mut walk = Walk {
            makers: &mut makers,
            twins: &mut twins,
            written: &mut written,
        };
        walk_tests(&top, &mut Vec::new(), &mut walk);
    }
    // Read back once the spans of the file are dropped: reading a name lexes it.
    let names = written
        .iter()
        .map(|(modules, function)| TestName::of(path, modules, function))
        .collect();
    Ok(Walked {
        names,
        maker: makers.first().map(str::to_owned),
    })
}

/// What a walk of the tests of one file gathers.
struct Walk<'a> {
    makers: &'a mut Makers,
    twins: &'a mut BTreeSet<Vec<String>>,
    written: &'a mut Vec<(Vec<String>, String)>,
}

/// The tests of `items` (inside the inline `modules`, as written, outermost first),
/// and of every inline module among them, pushed onto `written` as spelled, a twin of
/// one already in `twins` (its segments unraw) left out; and what may make a test,
/// added to `makers`. A bare derive is read in the scope of the module it is written
/// in (`items`), as Rust resolves it: the `use` items of that module, not of a module
/// in it nor of one around it.
fn walk_tests(items: &Items, modules: &mut Vec<String>, walk: &mut Walk<'_>) {
    items.makers(walk.makers);
    let scope = Scope::of(items);
    // A bare derive of a name of the standard library's that the scope shadows: why.
    let note = |p: &str| match p
        .strip_prefix("derive(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        Some(name) if INERT_DERIVES.contains(&name) && scope.names.contains(name) => {
            format!(", where a `use` of the module brings in an item named `{name}`")
        }
        Some(name) if INERT_DERIVES.contains(&name) => {
            format!(", where a glob `use` of the module may bring in a derive named `{name}`")
        }
        _ => String::new(),
    };
    let path_of = |modules: &[String], last: &Ident| {
        modules
            .iter()
            .map(|m| Segment::of(m).to_string())
            .chain(std::iter::once(Segment::of(&last.to_string()).to_string()))
            .collect::<Vec<_>>()
    };
    for item in &items.items {
        match &item.kind {
            Kind::Fn(name) if item.is_test() => {
                let test = path_of(modules, name).join("::");
                // A test attribute of a crate (`tokio::test`) is a macro, which may
                // write more than the one test it is read as.
                for p in item.test_macros() {
                    walk.makers.add(format!(
                        "the attribute `#[{p}]` on the test `{test}`, a macro that may write more than one test"
                    ));
                }
                // An attribute macro beside the test attribute may make more tests.
                for p in item.maker_beside_test() {
                    walk.makers
                        .add(format!("the attribute `#[{p}]` on the test `{test}`"));
                }
                // A twin is the test already written.
                let first = walk.twins.insert(path_of(modules, name));
                if first {
                    walk.written.push((modules.clone(), name.to_string()));
                }
            }
            Kind::Fn(name) => {
                for p in item.maker_in(&scope) {
                    walk.makers.add(format!(
                        "the attribute `#[{p}]` on the function `{}`{}",
                        path_of(modules, name).join("::"),
                        note(&p)
                    ));
                }
            }
            Kind::Mod {
                name,
                body: Some(body),
            } => {
                for p in item.maker_in(&scope) {
                    walk.makers.add(format!(
                        "the attribute `#[{p}]` on the module `{}`{}",
                        path_of(modules, name).join("::"),
                        note(&p)
                    ));
                }
                modules.push(name.to_string());
                walk_tests(&Items::scan(body.stream()), modules, walk);
                modules.pop();
            }
            // Any other item (an impl, a struct, a trait, a const, a module out of
            // line, a macro call): an attribute macro or a derive on it may make
            // tests too.
            Kind::Mod { body: None, .. } | Kind::Macro(_) | Kind::Other => {
                let at = if modules.is_empty() {
                    "at the top of the file".to_owned()
                } else {
                    format!(
                        "in the module `{}`",
                        modules
                            .iter()
                            .map(|m| Segment::of(m).to_string())
                            .collect::<Vec<_>>()
                            .join("::")
                    )
                };
                for p in item.maker_in(&scope) {
                    walk.makers.add(format!(
                        "the attribute `#[{p}]` on an item {at}{}",
                        note(&p)
                    ));
                }
                // A module out of line read from a file that is no `.rs` file: the
                // listing never takes that file, so its tests would be passed over.
                if let Kind::Mod { name, body: None } = &item.kind
                    && item.path_off_listing()
                {
                    walk.makers.add(format!(
                        "the module `{name}` {at}, read by `#[path]` from a file that is no `.rs` file a listing takes"
                    ));
                }
            }
        }
    }
}

/// The tokens of the source `text` of the file at `path`, as the scan reads them (a
/// byte order mark and a first line `#!` of an interpreter left out), held to
/// [`MAX_DEPTH`]; or why they cannot be read.
fn lexed(path: &RelPath, text: &str) -> Result<TokenStream, Untellable> {
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
            return Err(Untellable::Lex {
                path: path.clone(),
                at: Place(at),
            });
        }
    };
    if nests_deeper_than(&tokens, MAX_DEPTH) {
        return Err(Untellable::TooDeep {
            path: path.clone(),
            max: MAX_DEPTH,
        });
    }
    Ok(tokens)
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
        Self::parse_spelled(text).map(|(name, _)| name)
    }

    /// The name read, and its segments (the modules, then the function) as the text
    /// wrote each: `r#type` stays so, though it reads as the segment `type`.
    fn parse_spelled(text: &str) -> Result<(Self, Vec<Spelled<Segment>>), NotAName> {
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
        let written = rest
            .split("::")
            .map(|part| Segment::new(part).map(|segment| Spelled::new(segment, part)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut segments: Vec<Segment> = written.iter().map(|s| s.value.clone()).collect();
        let Some(function) = segments.pop() else {
            return Err(NotAName::NoFunction);
        };
        Ok((
            Self {
                file: path,
                modules: segments,
                function,
            },
            written,
        ))
    }

    /// The name of the test the source writes as `function` under the inline
    /// `modules` of the file at `file`, each segment as the source spells it (the
    /// printing of its identifier: `r#type` stays so), so that a document written in
    /// that spelling meets it (a cover compares names as written). The one place a
    /// name is built from the source; it is read back by the one reader of the
    /// grammar ([`TestName::parse_spelled`]) to the same file and the same segments
    /// as written, or it is none, with why (a file whose path holds `::`, an
    /// identifier the grammar refuses, a name the domain refuses).
    fn of(file: &RelPath, modules: &[String], function: &str) -> Result<ArtifactName, NotAName> {
        let segments: Vec<&str> = modules
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(function))
            .collect();
        let text = std::iter::once(file.as_str())
            .chain(segments.iter().copied())
            .collect::<Vec<_>>()
            .join("::");
        let (read, spelled) = Self::parse_spelled(&text)?;
        if read.file != *file || spelled.iter().map(|s| s.text.as_str()).ne(segments) {
            return Err(NotAName::NotReadBack(text));
        }
        ArtifactName::new(text.as_str()).map_err(|why| NotAName::Refused {
            name: text.clone(),
            why: why.to_string(),
        })
    }

    fn wanted(&self) -> Wanted {
        Wanted {
            modules: self.modules.clone(),
            function: self.function.clone(),
        }
    }
}

/// A prefix of the names of the space, read: of one of two forms, and a text is of one
/// only.
#[derive(Debug, Clone)]
enum TestPrefix {
    /// The parts of the path of a file: a text with no `::` (so `src/a.rs` is this,
    /// the parts `src` and `a.rs`, and a prefix of the names of a file only as far as
    /// its path goes).
    FileParts(file::FilePrefix),
    /// A file and the first segments of the path in it: `<file>.rs::<segment>` and
    /// more segments after it, each `::` before, each segment as written.
    Items {
        file: RelPath,
        segments: NonEmptyVec<Spelled<Segment>>,
    },
}

impl TestPrefix {
    /// The one reader of the prefixes: a text with a `::` is read as a name is
    /// ([`TestName::parse_spelled`]), its segments all of the prefix's; one without, as
    /// a path ([`file::FilePrefix::parse`], which drops one `/` at its end).
    fn parse(text: &str) -> Result<Self, NotAName> {
        if !text.contains("::") {
            let path = file::FilePrefix::parse(text).map_err(|e: ValueError| NotAName::NoPath {
                path: text.to_owned(),
                why: e.to_string(),
            })?;
            return Ok(Self::FileParts(path));
        }
        let (TestName { file, .. }, segments) = TestName::parse_spelled(text)?;
        Ok(Self::Items {
            file,
            segments: NonEmptyVec::from_vec(segments).map_err(|_| NotAName::NoFunction)?,
        })
    }

    /// The name of the same test under `to`, when the test (its `file` and its
    /// `items`, the modules and then the function, as written) is under this prefix: of
    /// the two forms, the form of this prefix only. None when it is not under it. The
    /// name is built of the parts of `to` as it writes them, then the rest of the
    /// test's as it writes them.
    fn rename(
        &self,
        file: &RelPath,
        items: &[Spelled<Segment>],
        to: &Result<Self, NotAName>,
    ) -> Option<super::Rename> {
        use super::Rename;
        match self {
            Self::FileParts(from) => {
                let parts = file::parts_of(file);
                let rest = from.rest(&parts)?;
                Some(match to {
                    Err(why) => Rename::NoPrefix(why.to_string()),
                    Ok(Self::Items { .. }) => Rename::OtherForm,
                    Ok(Self::FileParts(to)) => {
                        let joined: Vec<file::Part> =
                            to.parts().iter().chain(rest).cloned().collect();
                        match file::path_from(&joined) {
                            Ok(path) => built(path.as_str(), items.iter()),
                            Err(e) => Rename::NoPrefix(e.to_string()),
                        }
                    }
                })
            }
            Self::Items {
                file: from_file,
                segments: from,
            } => {
                if file != from_file {
                    return None;
                }
                let rest = super::after(items, from.iter())?;
                Some(match to {
                    Err(why) => Rename::NoPrefix(why.to_string()),
                    Ok(Self::FileParts(_)) => Rename::OtherForm,
                    Ok(Self::Items { file, segments }) => {
                        built(file.as_str(), segments.iter().chain(rest))
                    }
                })
            }
        }
    }
}

/// The name of the file and the segments, each as written, joined with `::`, and the
/// parts it is to be read back as.
fn built<'a>(file: &str, items: impl Iterator<Item = &'a Spelled<Segment>>) -> super::Rename {
    let mut text = file.to_owned();
    let mut parts = vec![file.to_owned()];
    for item in items {
        text.push_str("::");
        text.push_str(&item.text);
        parts.push(item.text.clone());
    }
    super::Rename::Built { text, parts }
}

/// What this adapter says of a move of a prefix, for the `names` of its space asked
/// about. A prefix is the path of a file, in parts (`src/a.rs` is that and nothing
/// more), or a file and the first segments of a path in it (`src/a.rs::m`); a name is
/// under one when its file's parts, or its file and the first segments of its path in
/// it, are the prefix's (a segment is the same one however it is written, `r#type` and
/// `type`), the same name under `to` being built of `to`'s parts and the rest of the
/// name's, each as it was written. A `from` that cannot be read as either is no prefix
/// of the space's names; a `to` that cannot, or that is of the other form, makes each
/// name under `from` unbuildable, and so does a name built that is no name of the
/// space (a file that does not end in `.rs`). A text that is no name is not under any
/// prefix. Nothing is looked for in the text of a name.
pub(super) fn renamed(
    names: &BTreeSet<ArtifactName>,
    of: &PrefixMove,
) -> Result<(SpacePrefixes, SpaceRenamed), ValueError> {
    super::renamed_tests(
        names,
        of,
        "a path of a file, or a file and a path in it",
        TestPrefix::parse,
        |from, name, to| {
            let (test, items) = TestName::parse_spelled(name).ok()?;
            from.rename(&test.file, &items, to)
        },
        |text| {
            let (test, items) = TestName::parse_spelled(text)?;
            Ok(std::iter::once(test.file.as_str().to_owned())
                .chain(items.into_iter().map(|item| item.text))
                .collect())
        },
    )
}

/// What a name comes to in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    Text(Text),
    Absent,
    CannotTell(Untellable),
}

/// The text of a test found, in parts: its own text, then the text of the items
/// beside it in each of its test parts, then what it holds (what is compared, and
/// what a test holds, on the page of the module). The text of a part is made once and
/// shared by every test of it, since it may be as large as the file, and so is the
/// held text of a module. A part with no item beside is empty, and there are none
/// when no part has an item beside. What its own text and the items beside name is
/// kept apart from the text (`reached`, boxed so that what a name comes to stays
/// small), for what it holds to be found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Text {
    own: String,
    beside: Vec<Arc<str>>,
    held: Held,
    reached: Box<Reached>,
}

impl Text {
    /// The lines of what it holds, each module held a line `@ <file> <module>` then
    /// its held text, each include a line `@@ <path> <64 hex>`; none when it holds
    /// nothing.
    fn held_lines(&self) -> impl Iterator<Item = String> + '_ {
        let modules = self
            .held
            .modules
            .iter()
            .map(|held| format!("@ {} {}\n{}", held.file, held.module, held.text));
        let includes = self
            .held
            .includes
            .iter()
            .map(|include| format!("@@ {} {}", include.path, include.hex()));
        modules.chain(includes)
    }

    /// The bytes of the text joined.
    fn len(&self) -> usize {
        let len = self
            .beside
            .iter()
            .enumerate()
            .fold(self.own.len(), |len, (at, part)| {
                len.saturating_add(1 + usize::from(at > 0))
                    .saturating_add(part.len())
            });
        match self.held.is_empty() {
            true => len,
            false => self.held_lines().fold(len.saturating_add(1), |len, line| {
                len.saturating_add(1).saturating_add(line.len())
            }),
        }
    }

    /// The text: its own, then each part beside it on the lines after, two parts
    /// apart by an empty line (no line of a part is empty), so that an item moved
    /// from one twin part to the other changes the text; then, when it holds
    /// anything, an empty line and the lines of what it holds. A test that holds
    /// nothing is its own text and the parts beside it alone.
    fn joined(&self) -> String {
        let mut text = String::with_capacity(self.len());
        text.push_str(&self.own);
        for (at, part) in self.beside.iter().enumerate() {
            text.push('\n');
            if at > 0 {
                text.push('\n');
            }
            text.push_str(part);
        }
        if !self.held.is_empty() {
            text.push('\n');
            for line in self.held_lines() {
                text.push('\n');
                text.push_str(&line);
            }
        }
        text
    }
}

/// What each of `wanted` comes to in the source `text` of the file at `path`: the
/// file is lexed once, and its items scanned for each.
fn look_up(path: &RelPath, text: &str, wanted: &[Wanted]) -> Vec<Found> {
    let _spans = Spans;
    let tokens = match lexed(path, text) {
        Ok(tokens) => tokens,
        Err(why) => return vec![Found::CannotTell(why); wanted.len()],
    };
    let top = Items::scan(tokens);
    // The text of the items beside of each test part, by its place, made once.
    let mut parts = BTreeMap::new();
    wanted
        .iter()
        .map(|w| match find(path, &top, &w.modules, &w.function) {
            Ok(test) => {
                let mut text = test.text(&top, &mut parts);
                text.reached.module = w.modules.iter().map(ToString::to_string).collect();
                Found::Text(text)
            }
            Err(found) => found,
        })
        .collect()
}

/// One list of items (of a file or of an inline module), scanned: its inner
/// attributes (each with where its `#` is in the tokens) and its items, with the
/// tokens they were scanned from.
struct Items {
    tokens: Vec<TokenTree>,
    inner: Vec<(usize, Group)>,
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

impl Scanned {
    /// Whether the item is a test: a function that carries a test attribute
    /// ([`is_test`], the rule on the page of the module). The one place it is told,
    /// for finding a test, leaving the tests out of the items beside, and listing.
    fn is_test(&self) -> bool {
        matches!(self.kind, Kind::Fn(_)) && self.attrs.iter().any(is_test)
    }

    /// What of its outer attributes may make an item: each that is not inert
    /// ([`not_inert`]), as a path, in the order written; none when all are.
    fn maker(&self) -> Vec<String> {
        self.attrs.iter().filter_map(not_inert).collect()
    }

    /// What of its outer attributes, besides a test attribute, may make an item:
    /// on a test, an attribute macro beside `#[test]` may make more tests.
    fn maker_beside_test(&self) -> Vec<String> {
        self.attrs.iter().flat_map(beside_test).collect()
    }

    /// [`maker`](Self::maker) in a module whose `use` items make `scope`: a bare
    /// derive of a name one brings in, or of any name where a glob is, may be a
    /// crate's.
    fn maker_in(&self, scope: &Scope) -> Vec<String> {
        self.attrs
            .iter()
            .filter_map(|attr| not_inert_in(attr, scope))
            .collect()
    }

    /// Its test attributes that are a path of more than one segment (`tokio::test`):
    /// a crate's macro, which may write more tests than the one it is read as.
    fn test_macros(&self) -> Vec<String> {
        self.attrs.iter().flat_map(test_macros).collect()
    }

    /// Whether a `#[path]` on it names a file a listing cannot read as Rust: a
    /// literal that does not end in `.rs`, or a `path` in another form.
    fn path_off_listing(&self) -> bool {
        self.attrs
            .iter()
            .any(|attr| path_is_rust(attr) == Some(false))
    }
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
                inner.push((i, group.clone()));
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
        for (_, attr) in &self.inner {
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

    /// The first one seen.
    fn first(&self) -> Option<&str> {
        self.shown.first().map(String::as_str)
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

/// A test found: its own text (twins joined in the order of the file), what it names,
/// and the places of its test parts, in the order of the file, each with the inline
/// modules it is (unraw).
struct Test {
    own: String,
    refs: Refs,
    parts: BTreeMap<Vec<usize>, Vec<String>>,
}

/// The text of the items beside in one test part, and what they name, made once for
/// every test of the part.
type Part = (Option<Arc<str>>, Arc<Refs>);

impl Test {
    /// Its text: its own, then the items beside it in each test part that is in no
    /// other of them, the text of a part taken from `made` or made into it; a part
    /// with no item beside is kept empty, unless no part has one. It holds nothing
    /// yet.
    fn text(self, top: &Items, made: &mut BTreeMap<Vec<usize>, Part>) -> Text {
        let mut beside: Vec<Option<Arc<str>>> = Vec::new();
        let mut reached = Reached {
            own: self.refs,
            parts: Vec::new(),
            module: Vec::new(),
        };
        let mut kept: Option<&Vec<usize>> = None;
        // A place sorts before every place inside it, so a part inside one kept
        // follows it at once.
        for (part, inline) in &self.parts {
            if kept.is_some_and(|outer| part.starts_with(outer)) {
                continue;
            }
            kept = Some(part);
            let (text, refs) = made.entry(part.clone()).or_insert_with(|| {
                let (text, refs) = printed_part(top, part);
                (text, Arc::new(refs))
            });
            beside.push(text.clone());
            reached.parts.push((inline.clone(), refs.clone()));
        }
        let beside = if beside.iter().all(Option::is_none) {
            Vec::new()
        } else {
            beside
                .into_iter()
                .map(|part| part.unwrap_or_else(|| Arc::from("")))
                .collect()
        };
        Text {
            own: self.own,
            beside,
            held: Held::default(),
            reached: Box::new(reached),
        }
    }
}

/// Where a list of items the walk is in stands: the index of each module item on
/// the way from the file, its name (unraw), and how many of them lead to the test
/// part, when a module on the way is one (none: the part is the file).
#[derive(Debug, Clone, Default)]
struct At {
    path: Vec<usize>,
    names: Vec<String>,
    part: Option<usize>,
}

impl At {
    /// The place of the module `name` at `index` in this list; it starts the test
    /// part when none is on the way yet and it carries a `cfg` that holds only under
    /// test (`under_test`, an outer or an inner attribute of it).
    fn in_module(&self, index: usize, name: &Segment, under_test: bool) -> Self {
        let mut path = self.path.clone();
        path.push(index);
        let mut names = self.names.clone();
        names.push(name.to_string());
        let part = self.part.or_else(|| under_test.then_some(path.len()));
        Self { path, names, part }
    }

    /// The place of the test part: the modules on the way to it (none for the file),
    /// by index and by name.
    fn part(&self) -> (Vec<usize>, Vec<String>) {
        let depth = self.part.unwrap_or(0);
        (self.path[..depth].to_vec(), self.names[..depth].to_vec())
    }
}

/// The test `function` under the inline `modules` of the items `top` of the file at
/// `path` (the rules on the page of the module), or what the name comes to when it
/// is not found.
fn find(
    path: &RelPath,
    top: &Items,
    modules: &[Segment],
    function: &Segment,
) -> Result<Test, Found> {
    let mut makers = Makers::default();
    top.makers(&mut makers);
    // The first module of the name on the way that is declared out of line.
    let mut out_of_line: Option<String> = None;
    // The items of the modules the walk is in, once it is below the file, and
    // where each stands.
    let mut owned: Vec<Items>;
    let mut levels: Vec<&Items> = vec![top];
    let mut places: Vec<At> = vec![At::default()];
    let mut named: Vec<String> = Vec::new();
    for module in modules {
        named.push(module.to_string());
        let mut next = Vec::new();
        let mut next_places = Vec::new();
        for (level, at) in levels.iter().zip(&places) {
            for (index, item) in level.items.iter().enumerate() {
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
                for p in item.maker() {
                    makers.add(format!(
                        "the attribute `#[{p}]` on the module `{}`",
                        named.join("::")
                    ));
                }
                let items = Items::scan(body.stream());
                items.makers(&mut makers);
                let under_test = item
                    .attrs
                    .iter()
                    .chain(items.inner.iter().map(|(_, attr)| attr))
                    .any(only_under_test);
                next_places.push(at.in_module(index, module, under_test));
                next.push(items);
            }
        }
        if next.is_empty() {
            return Err(not_found(path, out_of_line, &makers));
        }
        owned = next;
        levels = owned.iter().collect();
        places = next_places;
    }
    let mut texts = Vec::new();
    let mut refs = Refs::default();
    let mut parts = BTreeMap::new();
    for (level, at) in levels.iter().zip(&places) {
        for item in &level.items {
            let Kind::Fn(name) = &item.kind else {
                continue;
            };
            if !function.names(name) {
                continue;
            }
            if item.is_test() {
                let tokens = without_docs(level.tokens_of(item));
                refs.add(tokens.clone());
                texts.push(tokens.to_string());
                let (part, inline) = at.part();
                parts.insert(part, inline);
            } else {
                for p in item.maker() {
                    makers.add(format!(
                        "the attribute `#[{p}]` on the function `{function}`"
                    ));
                }
            }
        }
    }
    if texts.is_empty() {
        Err(not_found(path, out_of_line, &makers))
    } else {
        Ok(Test {
            own: texts.join("\n"),
            refs,
            parts,
        })
    }
}

/// The text of the items beside in the test part at `at` (the index of each module
/// item on the way from the file): its inner attributes, then each item but the
/// tests on a line of its own, in the order of the file, doc attributes left out
/// (the rules on the page of the module); none when there is no such item. And what
/// those items name ([`tree::refs_beside`]).
fn printed_part(top: &Items, at: &[usize]) -> (Option<Arc<str>>, Refs) {
    let mut scanned: Option<Items> = None;
    for &index in at {
        let items = scanned.as_ref().unwrap_or(top);
        let body = match items.items.get(index) {
            Some(Scanned {
                kind: Kind::Mod {
                    body: Some(body), ..
                },
                ..
            }) => body.stream(),
            // The walk gave the place, so a module is there.
            _ => return (None, Refs::default()),
        };
        scanned = Some(Items::scan(body));
    }
    let items = scanned.as_ref().unwrap_or(top);
    let mut lines: Vec<TokenStream> = items
        .inner
        .iter()
        .map(|(at, _)| items.tokens[*at..*at + 3].iter().cloned().collect())
        .collect();
    beside(items, 0, &[], &mut lines);
    let lines: Vec<String> = lines
        .into_iter()
        .map(|tokens| without_docs(tokens).to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let mut refs = Refs::default();
    tree::refs_beside(items, &mut refs);
    (
        (!lines.is_empty()).then(|| Arc::from(lines.join("\n"))),
        refs,
    )
}

/// The items of a list but its tests, each one line of tokens, pushed onto `lines`
/// in their order: the mark of `depth` (how many modules of the part it is in, as
/// many `>`), then `over` (the attributes of those modules, outermost first), then
/// the item. A function read as a test is left out, and an inline module is read
/// through: its items are lines one deeper, after `over`, its own outer attributes
/// and its inner ones, and its name, its visibility and its braces are no token of
/// any line, so a module with no item but tests is none. No item starts with `>`,
/// so the mark is never taken for the item. A module declared out of line, an
/// `include!` and every other item are their own tokens: nothing is followed or
/// parsed.
fn beside(items: &Items, depth: usize, over: &[TokenTree], lines: &mut Vec<TokenStream>) {
    let mark = (0..depth).map(|at| {
        let spacing = if at + 1 < depth {
            Spacing::Joint
        } else {
            Spacing::Alone
        };
        TokenTree::Punct(Punct::new('>', spacing))
    });
    let mark: Vec<TokenTree> = mark.collect();
    for item in &items.items {
        match &item.kind {
            Kind::Fn(_) if item.is_test() => {}
            Kind::Mod {
                body: Some(body), ..
            } => {
                let inside = Items::scan(body.stream());
                let mut within = over.to_vec();
                // The outer attributes are the first tokens of an item, two each.
                within.extend(
                    items.tokens[item.start..item.start + 2 * item.attrs.len()]
                        .iter()
                        .cloned(),
                );
                for (at, _) in &inside.inner {
                    within.extend(inside.tokens[*at..*at + 3].iter().cloned());
                }
                beside(&inside, depth + 1, &within, lines);
            }
            _ => lines.push(
                mark.iter()
                    .chain(over)
                    .cloned()
                    .chain(items.tokens_of(item))
                    .collect(),
            ),
        }
    }
}

/// Whether an attribute is a `cfg` that holds only under test: `cfg(test)` (with a
/// comma after it or not, as rustc takes it), an `all` of which one holds only
/// under test, or an `any` of which each does.
fn only_under_test(attr: &Group) -> bool {
    let tokens: Vec<TokenTree> = attr.stream().into_iter().collect();
    match tokens.as_slice() {
        [TokenTree::Ident(cfg), TokenTree::Group(predicate)]
            if cfg == "cfg" && predicate.delimiter() == Delimiter::Parenthesis =>
        {
            let inside: Vec<TokenTree> = predicate.stream().into_iter().collect();
            matches!(predicates(&inside).as_slice(), [one] if holds_only_under_test(one))
        }
        _ => false,
    }
}

/// The predicates of a list of `cfg`, between its commas (one after the last is
/// none).
fn predicates(list: &[TokenTree]) -> Vec<&[TokenTree]> {
    let mut each: Vec<&[TokenTree]> = list
        .split(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ','))
        .collect();
    if each.last().is_some_and(|last| last.is_empty()) {
        each.pop();
    }
    each
}

/// Whether a predicate of `cfg` holds only under test (as [`only_under_test`]).
fn holds_only_under_test(predicate: &[TokenTree]) -> bool {
    match predicate {
        [TokenTree::Ident(word)] => word == "test",
        [TokenTree::Ident(word), TokenTree::Group(list)]
            if list.delimiter() == Delimiter::Parenthesis =>
        {
            let inside: Vec<TokenTree> = list.stream().into_iter().collect();
            let each = predicates(&inside);
            if word == "all" {
                each.iter().any(|part| holds_only_under_test(part))
            } else if word == "any" {
                !each.is_empty() && each.iter().all(|part| holds_only_under_test(part))
            } else {
                false
            }
        }
        _ => false,
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
    // `#[derive(A, b::C)]` is each derive it names, marked as one: `derive`, then the
    // path of the derive.
    if path.len() == 1
        && path[0] == "derive"
        && let Some(TokenTree::Group(list)) = tokens.get(1)
        && list.delimiter() == Delimiter::Parenthesis
    {
        let inside: Vec<TokenTree> = list.stream().into_iter().collect();
        for part in inside.split(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ',')) {
            if !part.is_empty() {
                let mut derive = vec![DERIVE.to_owned()];
                derive.extend(path_at(part));
                out.push(derive);
            }
        }
        return;
    }
    // `#[unsafe(no_mangle)]` (edition 2024) is the attribute inside it.
    if path.len() == 1
        && path[0] == "unsafe"
        && let Some(TokenTree::Group(inner)) = tokens.get(1)
        && inner.delimiter() == Delimiter::Parenthesis
    {
        let inside: Vec<TokenTree> = inner.stream().into_iter().collect();
        if !inside.is_empty() {
            paths_in(&inside, out);
            return;
        }
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
    attribute_paths(attr).iter().any(|path| is_test_path(path))
}

/// Whether the path of an attribute is a test attribute: its last segment is `test`
/// (a derive is none).
fn is_test_path(path: &[String]) -> bool {
    path.first().is_none_or(|first| first != DERIVE) && path.last().is_some_and(|s| s == "test")
}

/// Whether the path of an attribute is inert: one of [`INERT_ATTRIBUTES`], one of a
/// tool, or a derive of [`INERT_DERIVES`], under `std` or `core`, or by its bare name
/// when no `use` of the module it is written in names an item so and no glob `use` is
/// written there (`scope`: a `use` may bring a derive of a crate in under that name).
fn inert(path: &[String], scope: &Scope) -> bool {
    match path {
        [] => true,
        [derive, rest @ ..] if derive == DERIVE => match rest {
            [] => true,
            [name] => INERT_DERIVES.contains(&name.as_str()) && !scope.shadows(name),
            [root, .., name] => {
                matches!(root.as_str(), "std" | "core") && INERT_DERIVES.contains(&name.as_str())
            }
        },
        [one] => INERT_ATTRIBUTES.contains(&one.as_str()),
        [tool, ..] => TOOLS.contains(&tool.as_str()),
    }
}

/// An attribute's path as a reason writes it: `derive(serde::Serialize)` for a
/// derive, else its segments.
fn written(path: &[String]) -> String {
    match path {
        [derive, rest @ ..] if derive == DERIVE => format!("derive({})", rest.join("::")),
        _ => path.join("::"),
    }
}

/// The first attribute of `attr` that is not inert, written as a path; none when
/// all are.
fn not_inert(attr: &Group) -> Option<String> {
    not_inert_in(attr, &Scope::default())
}

/// [`not_inert`], in a module whose `use` items make `scope`.
fn not_inert_in(attr: &Group, scope: &Scope) -> Option<String> {
    attribute_paths(attr)
        .into_iter()
        .find(|path| !inert(path, scope))
        .map(|path| written(&path))
}

/// Each attribute of `attr` that is neither inert nor a test attribute, written as a
/// path: what, on a function that is a test, may make other tests.
fn beside_test(attr: &Group) -> Vec<String> {
    attribute_paths(attr)
        .into_iter()
        .filter(|path| !inert(path, &Scope::default()) && !is_test_path(path))
        .map(|path| written(&path))
        .collect()
}

/// Each test attribute of `attr` that is a path of more than one segment
/// (`tokio::test`, `test_log::test`), written as a path: a macro of a crate, which
/// may write more than the one test it is read as.
fn test_macros(attr: &Group) -> Vec<String> {
    attribute_paths(attr)
        .into_iter()
        .filter(|path| is_test_path(path) && path.len() > 1)
        .map(|path| written(&path))
        .collect()
}

/// When `attr` is a `path` attribute, whether its file is one a listing reads: the
/// literal of `#[path = "…"]` ends in `.rs` (raw or not). A `path` in any other form
/// (under a `cfg_attr`, or not a literal) is none it can tell. None when `attr` is no
/// `path` attribute.
fn path_is_rust(attr: &Group) -> Option<bool> {
    let tokens: Vec<TokenTree> = attr.stream().into_iter().collect();
    if let [
        TokenTree::Ident(name),
        TokenTree::Punct(eq),
        TokenTree::Literal(file),
    ] = tokens.as_slice()
        && name == "path"
        && eq.as_char() == '='
    {
        let file = file.to_string();
        return Some(file.ends_with(".rs\"") || file.ends_with(".rs\"#"));
    }
    attribute_paths(attr)
        .iter()
        .any(|path| path.len() == 1 && path[0] == "path")
        .then_some(false)
}

/// What the `use` items of one module (one list of items, not those of a module in
/// it nor of one around it) bring into its scope, as Rust resolves a name there: the
/// last segment of each path of a `use` tree, or its alias, and whether a glob
/// (`use x::*;`) is among them, which may bring in any name.
#[derive(Debug, Default)]
struct Scope {
    names: BTreeSet<String>,
    glob: bool,
}

impl Scope {
    /// The scope the `use` items of `items` make.
    fn of(items: &Items) -> Self {
        let mut scope = Self::default();
        for item in &items.items {
            if !matches!(item.kind, Kind::Other) {
                continue;
            }
            let tokens: Vec<TokenTree> = items.tokens_of(item).into_iter().collect();
            let mut k = 2 * item.attrs.len();
            if word(&tokens, k) == Some("pub") {
                k += 1;
                if matches!(tokens.get(k), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                {
                    k += 1;
                }
            }
            if matches!(tokens.get(k), Some(TokenTree::Ident(u)) if u == "use") {
                named_in_use(&tokens[k + 1..], &mut scope);
            }
        }
        scope
    }

    /// Whether a bare `name` may name an item a `use` of the module brings in.
    fn shadows(&self, name: &str) -> bool {
        self.glob || self.names.contains(name)
    }
}

/// The names a `use` tree brings in: each identifier that ends a path (before `,`,
/// `;` or the end of a group) and each one after `as`; and a `*`, a glob.
fn named_in_use(tokens: &[TokenTree], out: &mut Scope) {
    for (i, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => {
                let inside: Vec<TokenTree> = group.stream().into_iter().collect();
                named_in_use(&inside, out);
            }
            TokenTree::Punct(p) if p.as_char() == '*' => out.glob = true,
            TokenTree::Ident(ident) => {
                let ends = match tokens.get(i + 1) {
                    None => true,
                    Some(TokenTree::Punct(p)) => matches!(p.as_char(), ',' | ';'),
                    _ => false,
                };
                let aliased =
                    i > 0 && matches!(&tokens[i - 1], TokenTree::Ident(before) if before == "as");
                if ends || aliased {
                    out.names.insert(Segment::of(&ident.to_string()).0);
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
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

    use whyshelf_core::domain::shelf::artifact::{ArtifactState, NamePrefix, Untold};

    use whyshelf_core::domain::nonempty::NonEmptySet;

    use super::super::conformance;
    use super::super::file::ScanLimits;
    use super::*;

    fn n(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    /// The bound of bytes of the files of the conformance test.
    const MAX_TEST_FILE: u64 = 256;

    /// The tests of the conformance test: one beside a helper, twins under two
    /// `#[cfg]`s, and one in a module that holds only under test, beside a helper
    /// of its own, so that each text holds the items beside it.
    const TESTS: &str = "fn h() -> u8 { 1 }\n\n#[test]\nfn one() {\n    assert_eq!(h(), 1);\n}\n\n#[cfg(unix)]\n#[test]\nfn twin() {}\n\n#[cfg(not(unix))]\n#[test]\nfn twin() {}\n\n#[cfg(test)]\nmod tests {\n    fn g() {}\n    #[test]\n    fn inner() { g() }\n}\n";

    /// The hook of the conformance test: the tests above, a file that cannot be read,
    /// one past the bound, one that nests one past the depth, and a link out of the
    /// root.
    struct Tests;

    impl conformance::Hook for Tests {
        fn space(&self) -> &'static str {
            RUST_TEST_SPACE
        }

        fn source(&self) -> &'static str {
            include_str!("rust_test.rs")
        }

        fn bounds(&self) -> Vec<&'static str> {
            vec!["a file"]
        }

        fn unread(&self) -> conformance::Unread {
            conformance::Unread::CannotTell
        }

        fn walks_nesting(&self) -> bool {
            true
        }

        fn lists(&self) -> bool {
            true
        }

        fn places(&self) -> bool {
            true
        }

        fn adapter_at(&self, dir: &Path) -> super::super::Adapter {
            super::super::Adapter::RustTest(
                RustTestSpace::new(dir.to_path_buf()).with_max_file(MAX_TEST_FILE),
            )
        }

        async fn tree(&self, dir: &Path) -> conformance::Tree {
            let root = dir.join("root");
            write(&root, "src/a.rs", TESTS).await;
            write(&root, "src/locked.rs", "#[test]\nfn t() {}\n").await;
            let pad = "/".repeat(usize::try_from(MAX_TEST_FILE).unwrap());
            write(
                &root,
                "src/big.rs",
                &format!("#[test]\nfn t() {{}}\n{pad}\n"),
            )
            .await;
            let deep = format!(
                "#[test]\nfn t() {{ {}{} }}\n",
                "(".repeat(MAX_DEPTH),
                ")".repeat(MAX_DEPTH)
            );
            write(&root, "src/deep.rs", &deep).await;
            write(
                &root,
                "src/made.rs",
                "#[derive(thiserror::Error)]\nenum E {}\n#[test]\nfn t() {}\n",
            )
            .await;

            let mut tree = conformance::Tree::of(self.adapter_at(&root));
            tree.present = vec![n("src/a.rs::one"), n("src/a.rs::tests::inner")];
            tree.absent = vec![n("src/a.rs::missing"), n("src/none.rs::t")];
            tree.not_names = vec![n("src/a.rs"), n("src/a.txt::t"), n("src/a.rs::fn")];
            // A test lives in the file it names, there or not.
            let only = |file: &str| {
                Placement::Only(NonEmptySet::new(file.parse::<RelPath>().unwrap(), []))
            };
            tree.placed = vec![
                (n("src/a.rs::tests::inner"), only("src/a.rs")),
                (n("src/a.rs::missing"), only("src/a.rs")),
                (n("src/none.rs::t"), only("src/none.rs")),
            ];
            tree.over_bytes = vec![conformance::OverBytes::Name {
                bound: "a file",
                name: n("src/big.rs::t"),
            }];
            tree.too_deep = Some(n("src/deep.rs::t"));
            tree.two_things = Some(n("src/a.rs::twin"));
            tree.spellings = Some((n("src/a.rs::one"), n("src/a.rs::r#one")));
            tree.prefix = Some((
                PrefixMove::new(
                    NamePrefix::new("src/a.rs::tests").unwrap(),
                    NamePrefix::new("src/a.rs::checks").unwrap(),
                )
                .unwrap(),
                n("src/a.rs::tests::inner"),
                n("src/a.rs::checks::inner"),
            ));
            // The files are listed through Git: with none here, no listing is made.
            if conformance::git_runs().await {
                conformance::isolated_git(&root, &["init", "-q"]).await;
                tree.listing = Some(conformance::ListCase {
                    // The three tests of the file: one, the twins as one, and the one in
                    // its module of tests.
                    selection: select(&["src/a.rs"]),
                    bad: select(&["/a.rs"]),
                    bounded: super::super::Adapter::RustTest(
                        RustTestSpace::new(root.clone())
                            .with_max_file(MAX_TEST_FILE)
                            .with_limits(ScanLimits {
                                names: 1,
                                ..ScanLimits::default()
                            }),
                    ),
                    // A derive of a crate may make a test: the file is a part, the
                    // file whole in the space `file`.
                    parts: Some(conformance::PartsCase {
                        selection: select(&["src/made.rs"]),
                        of: super::super::Adapter::File(file::FileSpace::new(root.clone())),
                    }),
                });
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                write(dir, "outside/x.rs", "#[test]\nfn t() {}\n").await;
                // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
                tokio::fs::symlink(dir.join("outside"), root.join("out"))
                    .await
                    .unwrap();
                tree.outside = Some(n("out/x.rs::t"));
                // A test read through a link inside the root lives in the link and in
                // the file it leads to.
                // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
                tokio::fs::symlink("a.rs", root.join("src/link.rs"))
                    .await
                    .unwrap();
                tree.present.push(n("src/link.rs::one"));
                let both = NonEmptySet::new(
                    "src/a.rs".parse::<RelPath>().unwrap(),
                    ["src/link.rs".parse::<RelPath>().unwrap()],
                );
                tree.placed
                    .push((n("src/link.rs::one"), Placement::Only(both)));
                tree.linked = Some(n("src/link.rs::one"));
                let locked = root.join("src/locked.rs");
                // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
                tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
                    .await
                    .unwrap();
                // Run as a user every file opens to (root), it is no name to test.
                // arch-lint: allow(readers-read-through-the-state) reason="a test asks whether its tree locks a file"
                if tokio::fs::File::open(&locked).await.is_err() {
                    tree.unreadable = Some(n("src/locked.rs::t"));
                }
            }
            tree
        }
    }

    #[tokio::test]
    async fn the_adapter_keeps_the_contract_of_an_adapter() {
        conformance::check(&Tests).await;
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
            // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
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

    /// The text `function` under `modules` reads as in the source `text`; it is found.
    fn text_in(text: &str, modules: &[&str], function: &str) -> String {
        match in_source(text, modules, function) {
            Found::Text(found) => found.joined(),
            other => panic!("{modules:?} {function}: {other:?}"),
        }
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
        assert_eq!(
            presets(),
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
        // docs and the comments left out; then, a line each, the items beside it in
        // its test part, `mod tests` (each test of it left out, at any depth; the
        // modules `deeper` and `deepest` hold tests only, so they are no line).
        let beside = "use super ::*;\nfn not_a_test () { }\nmod elsewhere ;";
        assert_eq!(
            text(r, "src/lib.rs::tests::holds").await.unwrap(),
            format!("# [test] fn holds () {{ assert ! (true) ; }}\n{beside}")
        );
        assert_eq!(
            text(r, "src/lib.rs::tests::waits").await.unwrap(),
            format!(
                "# [tokio :: test (flavor = \"current_thread\")] async fn waits () {{ }}\n{beside}"
            )
        );
        // The body is the brace group after the const generic arguments.
        let far = text(r, "src/lib.rs::tests::deeper::deepest::far")
            .await
            .unwrap();
        assert!(
            far.ends_with(&format!("{{ let _ = 1 ; }}\n{beside}")),
            "{far}"
        );
        // A test at the top of a file with no module that holds only under test has
        // the file for its part: every item there but the tests, those of `mod tests`
        // after its attribute.
        assert_eq!(
            text(r, "src/lib.rs::top").await.unwrap(),
            "# [test] fn top () { assert_eq ! (helper () , 1) ; }\nfn helper () -> u8 { 1 }\n> # [cfg (test)] use super ::*;\n> # [cfg (test)] fn not_a_test () { }\n> # [cfg (test)] mod elsewhere ;"
        );
    }

    /// A file of code with inline tests: code under test, then a test part with a
    /// helper, a constant, an inner module with a helper of its own, a module out of
    /// line and two tests.
    const PART: &str = r#"fn code() -> u8 { 1 }

#[cfg(test)]
mod tests {
    use super::*;

    mod common;

    /// Two.
    const TWO: u8 = 2;

    fn helper() -> u8 { code() + 1 }

    #[test]
    fn one() { assert_eq!(helper(), TWO); }

    mod inner {
        fn deep() -> u8 { 3 }

        #[test]
        fn nested() { assert_eq!(deep(), 3); }
    }

    #[test]
    fn other() { assert_eq!(code(), 1); }
}
"#;

    #[test]
    fn an_edit_of_an_item_beside_a_test_in_its_part_changes_its_text() {
        let before = text_in(PART, &["tests"], "one");
        for (from, to) in [
            // A helper, a constant and a `use` of the part.
            ("{ code() + 1 }", "{ code() + 2 }"),
            ("const TWO: u8 = 2;", "const TWO: u8 = 3;"),
            ("use super::*;", "use super::code;"),
            // A helper in a module inside the part.
            ("fn deep() -> u8 { 3 }", "fn deep() -> u8 { 4 }"),
        ] {
            let edited = PART.replace(from, to);
            assert_ne!(edited, PART);
            assert_ne!(text_in(&edited, &["tests"], "one"), before, "{to}");
        }
        // The test in the inner module stands on the same part.
        assert_ne!(
            text_in(
                &PART.replace("{ 3 }", "{ 4 }"),
                &["tests", "inner"],
                "nested"
            ),
            text_in(PART, &["tests", "inner"], "nested")
        );
    }

    #[test]
    fn an_edit_of_another_test_or_of_the_code_under_test_keeps_the_text() {
        let before = text_in(PART, &["tests"], "one");
        for (from, to) in [
            // Another test of the part, at its top and in a module inside it.
            ("assert_eq!(code(), 1);", "assert_eq!(code(), 2);"),
            ("assert_eq!(deep(), 3);", "assert_eq!(deep(), 4);"),
            // The code outside the module that holds only under test.
            ("fn code() -> u8 { 1 }", "fn code() -> u8 { 5 }"),
            // A doc beside, and the spacing of an item beside.
            ("/// Two.", "/// The number two."),
            (
                "fn helper() -> u8 { code() + 1 }",
                "fn helper()->u8{code()+1}",
            ),
        ] {
            let edited = PART.replace(from, to);
            assert_ne!(edited, PART);
            assert_eq!(text_in(&edited, &["tests"], "one"), before, "{to}");
        }
        // No test is beside another, wherever it stands.
        assert!(!before.contains("fn other"), "{before}");
        assert!(!before.contains("fn nested"), "{before}");
        // The helper of the inner module is a line of its own, marked one deep, its
        // module left out.
        assert!(before.contains("\n> fn deep () -> u8 { 3 }"), "{before}");
        assert!(!before.contains("inner"), "{before}");
    }

    #[test]
    fn a_module_in_the_part_is_read_through_so_its_name_counts_for_nothing() {
        let names = [
            ("one", &["tests"][..]),
            ("other", &["tests"][..]),
            ("nested", &["tests", "inner"][..]),
        ];
        let texts = |source: &str, inner: &str| -> Vec<String> {
            names
                .iter()
                .map(|(function, modules)| {
                    let modules: Vec<&str> = modules
                        .iter()
                        .map(|m| if *m == "inner" { inner } else { m })
                        .collect();
                    text_in(source, &modules, function)
                })
                .collect()
        };
        let before = texts(PART, "inner");
        // The inner module renamed: no text changes, the moved test's among them.
        let renamed = PART.replace("mod inner {", "mod renamed {");
        assert_eq!(texts(&renamed, "renamed"), before);
        // So is its visibility, and a module that holds tests only added.
        let public = PART.replace("mod inner {", "pub(crate) mod inner {");
        assert_eq!(texts(&public, "inner"), before);
        let added = PART.replace(
            "    #[test]\n    fn other()",
            "    mod added {\n        #[test]\n        fn new() {}\n    }\n\n    #[test]\n    fn other()",
        );
        assert_ne!(added, PART);
        assert_eq!(texts(&added, "inner"), before);
        // A helper of the inner module edited still changes the text of each test.
        let edited = PART.replace("fn deep() -> u8 { 3 }", "fn deep() -> u8 { 4 }");
        for (now, then) in texts(&edited, "inner").iter().zip(&before) {
            assert_ne!(now, then);
        }
        // An attribute of the inner module stands before each of its items, and its
        // inner ones after it, so it stays tied to them: an item moved out of the
        // module and an attribute added change the text.
        let tied = "#[cfg(test)]\nmod tests {\n    #[cfg(unix)]\n    mod inner {\n        #![allow(dead_code)]\n        fn a() {}\n        fn b() {}\n    }\n    #[test]\n    fn t() {}\n}\n";
        let text = text_in(tied, &["tests"], "t");
        assert_eq!(
            text,
            "# [test] fn t () { }\n> # [cfg (unix)] #! [allow (dead_code)] fn a () { }\n> # [cfg (unix)] #! [allow (dead_code)] fn b () { }"
        );
        let moved = tied.replace("        fn b() {}\n    }\n", "    }\n    fn b() {}\n");
        assert_ne!(moved, tied);
        assert_ne!(text_in(&moved, &["tests"], "t"), text);
        let unattributed = tied.replace("    #[cfg(unix)]\n    mod inner", "    mod inner");
        assert_ne!(text_in(&unattributed, &["tests"], "t"), text);
        // Each line of an item in a module carries its depth below the part, so two
        // helpers of one name swapped across the boundary of a module change the
        // text, though no module's name is in it.
        let swap = "#[cfg(test)] mod tests { fn h() -> u8 { 1 } mod inner { fn h() -> u8 { 2 } } #[test] fn a() { assert_eq!(h(), 1); } }\n";
        let swapped = "#[cfg(test)] mod tests { mod inner { fn h() -> u8 { 1 } } fn h() -> u8 { 2 } #[test] fn a() { assert_eq!(h(), 1); } }\n";
        let text = text_in(swap, &["tests"], "a");
        assert_eq!(
            text,
            "# [test] fn a () { assert_eq ! (h () , 1) ; }\nfn h () -> u8 { 1 }\n> fn h () -> u8 { 2 }"
        );
        assert_ne!(text_in(swapped, &["tests"], "a"), text);
        // Two deep, the mark is two long.
        let deeper = "#[cfg(test)] mod tests { mod a { mod b { fn h() {} } } #[test] fn t() {} }\n";
        assert_eq!(
            text_in(deeper, &["tests"], "t"),
            "# [test] fn t () { }\n>> fn h () { }"
        );
    }

    #[test]
    fn a_module_declared_out_of_line_is_its_declaration_only() {
        let before = text_in(PART, &["tests"], "one");
        assert!(before.contains("\nmod common ;\n"), "{before}");
        // An attribute on the declaration is of its tokens.
        let pathed = PART.replace("mod common;", "#[path = \"c.rs\"] mod common;");
        assert_ne!(text_in(&pathed, &["tests"], "one"), before);
        // So is an `include!`, whose file is not read.
        let included = PART.replace("mod common;", "include!(\"c.rs\");");
        assert!(
            text_in(&included, &["tests"], "one").contains("\ninclude ! (\"c.rs\") ;\n"),
            "{included}"
        );
    }

    #[test]
    fn the_test_part_is_the_outermost_module_that_holds_only_under_test_else_the_file() {
        // A file of tests with no such module: a helper at its top is beside.
        let file = "use std::fmt;\n\nfn helper() -> u8 { 1 }\n\n#[test]\nfn a() { assert_eq!(helper(), 1); }\n\nmod more {\n    #[test]\n    fn b() {}\n}\n";
        let before = text_in(file, &[], "a");
        assert_eq!(
            before,
            "# [test] fn a () { assert_eq ! (helper () , 1) ; }\nuse std :: fmt ;\nfn helper () -> u8 { 1 }"
        );
        assert_ne!(text_in(&file.replace("{ 1 }", "{ 2 }"), &[], "a"), before);
        assert_eq!(
            text_in(file, &["more"], "b"),
            "# [test] fn b () { }\nuse std :: fmt ;\nfn helper () -> u8 { 1 }"
        );
        // A `cfg` that holds only under test starts the part; one that may hold
        // otherwise does not, and nor does one in a `cfg_attr`.
        for (cfg, part) in [
            ("#[cfg(test)]", true),
            ("#[cfg(test,)]", true),
            ("#[cfg(all(unix, test,))]", true),
            ("#[cfg(all(unix, test))]", true),
            ("#[cfg(any(test, all(test, unix)))]", true),
            ("#[cfg(any(test, feature = \"x\"))]", false),
            ("#[cfg(not(test))]", false),
            ("#[cfg(any())]", false),
            ("#[cfg_attr(unix, cfg(test))]", false),
        ] {
            let source = format!("fn code() {{}}\n{cfg} mod t {{ #[test] fn a() {{}} }}\n");
            let text = text_in(&source, &["t"], "a");
            assert_eq!(text == "# [test] fn a () { }", part, "{cfg}: {text}");
        }
        // An inner `cfg` of the module starts the part as an outer one does.
        assert_eq!(
            text_in(
                "fn code() {}\nmod t { #![cfg(test)] #[test] fn a() {} }\n",
                &["t"],
                "a"
            ),
            "# [test] fn a () { }\n#! [cfg (test)]"
        );
        // The outermost such module on the way.
        let nested = "fn code() {}\nmod m {\n    fn under() {}\n    #[cfg(test)]\n    mod t {\n        fn h() {}\n        #[cfg(test)]\n        mod u {\n            #[test]\n            fn a() {}\n        }\n    }\n}\n";
        assert_eq!(
            text_in(nested, &["m", "t", "u"], "a"),
            "# [test] fn a () { }\nfn h () { }"
        );
    }

    #[tokio::test]
    async fn a_name_asked_with_others_in_any_order_reads_the_same() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        write(r, "src/lib.rs", PART).await;
        let names = [
            "src/lib.rs::tests::one",
            "src/lib.rs::tests::other",
            "src/lib.rs::tests::inner::nested",
        ];
        let space = RustTestSpace::new(r.to_path_buf());
        let all = space.resolve(&asks(&names)).await.unwrap();
        for name in names {
            let alone = space.resolve(&asks(&[name])).await.unwrap();
            assert_eq!(alone[&n(name)], all[&n(name)], "{name}");
            let wanted: Vec<Wanted> = names
                .iter()
                .rev()
                .map(|t| TestName::parse(t).unwrap().wanted())
                .collect();
            let path: RelPath = "src/lib.rs".parse().unwrap();
            let reversed = look_up(&path, PART, &wanted);
            let at = names.iter().rev().position(|t| *t == name).unwrap();
            let Found::Text(found) = &reversed[at] else {
                panic!("{name}")
            };
            assert_eq!(Some(found.joined()), text(r, name).await, "{name}");
        }
        // The tests of one part hold its text once, shared.
        let wanted: Vec<Wanted> = names[..2]
            .iter()
            .map(|t| TestName::parse(t).unwrap().wanted())
            .collect();
        let path: RelPath = "src/lib.rs".parse().unwrap();
        match look_up(&path, PART, &wanted).as_slice() {
            [Found::Text(one), Found::Text(other)] => {
                assert!(Arc::ptr_eq(&one.beside[0], &other.beside[0]));
            }
            other => panic!("{other:?}"),
        }
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
        let text = text_in(source, &[], "a");
        assert!(text.contains("doc = \" kept\""), "{text}");
        let other = text_in(&source.replace("/// kept", "/// changed"), &[], "a");
        assert_ne!(other, text);
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
        let source = "#[cfg(a)] mod dup { fn h() {} #[test] fn x() {} }\n#[cfg(not(a))] mod dup { fn h() {} #[test] fn y() {} }\n";
        // No module on the way holds only under test, so the part is the file: the
        // items of each twin are beside, after its attribute, its test left out.
        let beside = "> # [cfg (a)] fn h () { }\n> # [cfg (not (a))] fn h () { }";
        assert_eq!(
            text_in(source, &["dup"], "y"),
            format!("# [test] fn y () {{ }}\n{beside}")
        );
        assert_eq!(
            text_in(source, &["dup"], "x"),
            format!("# [test] fn x () {{ }}\n{beside}")
        );
        let source = "#[cfg(unix)] #[test] fn a() { 1; }\n#[cfg(windows)] #[test] fn a() { 2; }\n";
        assert_eq!(
            text_in(source, &[], "a"),
            "# [cfg (unix)] # [test] fn a () { 1 ; }\n# [cfg (windows)] # [test] fn a () { 2 ; }"
        );
        // Twin test parts are each beside, in the order of the file, apart by an
        // empty line.
        let source = "#[cfg(all(test, unix))] mod t { fn u() {} #[test] fn a() {} }\n#[cfg(all(test, not(unix)))] mod t { fn w() {} #[test] fn a() {} }\n";
        let twins = text_in(source, &["t"], "a");
        assert_eq!(
            twins,
            "# [test] fn a () { }\n# [test] fn a () { }\nfn u () { }\n\nfn w () { }"
        );
        // So an item moved from one twin part to the other changes the text.
        let moved = "#[cfg(all(test, unix))] mod t { #[test] fn a() {} }\n#[cfg(all(test, not(unix)))] mod t { fn u() {} fn w() {} #[test] fn a() {} }\n";
        assert_ne!(text_in(moved, &["t"], "a"), twins);
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
        let text = text_in(helper, &[], "after");
        assert!(
            text.starts_with("# [test] fn after () { }\nfn helper"),
            "{text}"
        );
        assert!(text.ends_with("{ Some (x) }"), "{text}");
        let test = "#[test]\nfn a<T>()\nwhere\n    T: Copy,\n    Vec<T>: Clone,\n{\n    let _ = 1;\n}\n#[test] fn b() {}\n";
        let text = text_in(test, &[], "a");
        assert!(text.ends_with("{ let _ = 1 ; }"), "{text}");
        assert!(matches!(in_source(test, &[], "b"), Found::Text(_)));
        // Const generic braces are still no body, and `->` and `=>` close nothing.
        let generic = "#[test] fn c<const N: usize>() -> Foo<{ N }> where [(); { N }]:, Bar<{ 1 }, { 2 }>: X { match 1 { _ => 2 }; }\n#[test] fn d() {}\n";
        let text = text_in(generic, &[], "c");
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
            text_in(source, &["mixed"], "z"),
            "# [test] fn z () { }\nmod mixed ;"
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
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
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
        let small = RustTestSpace::new(r.to_path_buf()).with_max_file(100);
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
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::symlink(outside.path(), root.path().join("out"))
            .await
            .unwrap();
        let why = unknown_why(root.path(), "out/lib.rs::top").await;
        assert!(why.contains("leads outside the root"), "{why}");
        // A link inside the root is read through.
        write(root.path(), "src/lib.rs", SOURCE).await;
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
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
        ] {
            assert_eq!(
                text_in(source, &[], "a"),
                "# [test] fn a () { }",
                "{source:?}"
            );
        }
        // An inner attribute is no line of an interpreter: it is beside the test.
        assert_eq!(
            text_in("#![allow(unused)]\n#[test] fn a() {}\n", &[], "a"),
            "# [test] fn a () { }\n#! [allow (unused)]"
        );
    }

    // Listing the names of a selection.

    fn select(include: &[&str]) -> Selection {
        Selection::new(include.iter().map(|s| (*s).to_owned()).collect(), vec![]).unwrap()
    }

    fn names(texts: &[&str]) -> BTreeSet<ArtifactName> {
        texts.iter().map(|t| n(t)).collect()
    }

    async fn listing_of(space: &RustTestSpace, include: &[&str]) -> NameListing {
        space.list(Side::After, &select(include)).await.unwrap()
    }

    /// Why a listing is not all: its why, or each part's, after the part's name.
    fn why_of(listing: &NameListing) -> String {
        match listing {
            NameListing::Some { why, .. } => why.as_str().to_owned(),
            NameListing::Parted { parts, .. } => parts
                .iter()
                .map(|part| format!("{}: {}", part.key().name(), part.why()))
                .collect::<Vec<_>>()
                .join("; "),
            other => panic!("a listing that is not all: {other:?}"),
        }
    }

    /// The one part of the listing of `file` alone: the file whole in the space
    /// `file`, with what was read of it and why, beside no name listed whole.
    fn one_part(listing: &NameListing, file: &str) -> (PartRead, String) {
        let NameListing::Parted { found, parts } = listing else {
            panic!("{file}: a part: {listing:?}");
        };
        assert!(found.is_empty(), "{file}: listed whole: {found:?}");
        assert_eq!(parts.len(), 1, "{file}: {parts:?}");
        let part = parts.first();
        assert_eq!(
            part.key(),
            &file::name_of(&file.parse::<RelPath>().unwrap()).unwrap(),
            "{file}: the part is the file whole, in the space file"
        );
        (part.read().clone(), part.why().as_str().to_owned())
    }

    /// `files` under a new root, in a repository of Git (untracked, so listed as files
    /// not ignored); none when Git cannot be run here.
    async fn in_git(files: &[(&str, &str)]) -> Option<tempfile::TempDir> {
        if !conformance::git_runs().await {
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        for (at, text) in files {
            write(dir.path(), at, text).await;
        }
        conformance::isolated_git(dir.path(), &["init", "-q"]).await;
        Some(dir)
    }

    /// Tests at the top, in inline modules two deep, twins of a function and of a
    /// module, a raw identifier, raw and plain twins, a module out of line, and a
    /// `#[test]` in a string.
    const LISTED: &str = r##"#[test]
fn top() {}

fn helper() {}

#[cfg(test)]
mod tests {
    const S: &str = "#[test] fn in_a_string() {}";

    #[test]
    fn r#type() {}

    #[cfg(unix)]
    #[test]
    fn twin() {}

    #[cfg(not(unix))]
    #[test]
    fn twin() {}

    #[cfg(unix)]
    #[test]
    fn r#both() {}

    #[cfg(not(unix))]
    #[test]
    fn both() {}

    mod deeper {
        mod deepest {
            #[test]
            fn far() {}
        }
    }

    mod elsewhere;
}

#[cfg(not(test))]
mod tests {
    #[test]
    fn in_a_twin_module() {}
}
"##;

    #[tokio::test]
    async fn a_listing_names_every_test_of_the_rs_files_taken_as_the_source_spells_it() {
        let Some(dir) = in_git(&[
            ("src/lib.rs", LISTED),
            ("src/notes.txt", "#[test]\nfn not_rust() {}\n"),
            ("other/x.rs", "#[test]\nfn not_taken() {}\n"),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        let listed = [
            "src/lib.rs::top",
            "src/lib.rs::tests::r#type",
            "src/lib.rs::tests::twin",
            "src/lib.rs::tests::r#both",
            "src/lib.rs::tests::deeper::deepest::far",
            "src/lib.rs::tests::in_a_twin_module",
        ];
        let listing = listing_of(&space, &["src/**"]).await;
        assert_eq!(listing, NameListing::All(names(&listed)));
        // Each is a name of the grammar, which `resolve` answers as there.
        let found = space.resolve(&asks(&listed)).await.unwrap();
        for name in listed {
            assert!(TestName::parse(name).is_ok(), "{name}");
            assert!(
                matches!(
                    found[&n(name)].state(&Comparison::Bytes),
                    ArtifactState::Present(_)
                ),
                "{name}: {:?}",
                found[&n(name)]
            );
        }
        // The same content, the same names; a file with no test adds none.
        write(dir.path(), "src/empty.rs", "fn not_a_test() {}\n").await;
        assert_eq!(
            listing_of(&space, &["src/**"]).await,
            NameListing::All(names(&listed))
        );
    }

    #[tokio::test]
    async fn a_listing_is_not_all_where_something_may_make_a_test() {
        let deep = format!(
            "#[test] fn a() {{ {}{} }}",
            "(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        );
        let Some(dir) = in_git(&[
            ("call.rs", "#[test]\nfn a() {}\nproptest! { fn b() {} }\n"),
            ("only_call.rs", "opaque!(X);\n"),
            ("module.rs", "#[my_attr]\nmod m {\n    #[test]\n    fn a() {}\n}\n"),
            ("inner.rs", "#![my_inner]\n#[test]\nfn a() {}\n"),
            ("main.rs", "#[tokio::main]\nasync fn main() {}\n"),
            ("rstest.rs", "#[rstest]\nfn a() {}\n"),
            ("lex.rs", "fn a() { \"unclosed }\n"),
            ("deep.rs", &deep),
            (
                "inert.rs",
                "#[cfg(test)]\n#[allow(dead_code)]\nmod m {\n    #[inline]\n    fn h() {}\n    #[test]\n    #[ignore]\n    fn a() {}\n}\n",
            ),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        // A file read, with what may make a test, is a part with the tests found in
        // it (none, it may be); one that does not lex or nests too deep (past
        // `MAX_DEPTH`) is a part that could not be read, told apart from one read
        // with no test in it.
        let found = |names_found: &[&str]| PartRead::Found(names(names_found));
        for (file, read, maker) in [
            (
                "call.rs",
                found(&["call.rs::a"]),
                "the macro call `proptest!`",
            ),
            ("only_call.rs", found(&[]), "the macro call `opaque!`"),
            (
                "module.rs",
                found(&["module.rs::m::a"]),
                "`#[my_attr]` on the module `m`",
            ),
            ("inner.rs", found(&["inner.rs::a"]), "`#![my_inner]`"),
            (
                "main.rs",
                found(&[]),
                "`#[tokio::main]` on the function `main`",
            ),
            ("rstest.rs", found(&[]), "`#[rstest]` on the function `a`"),
            ("lex.rs", PartRead::Unread, "does not parse as Rust"),
            ("deep.rs", PartRead::Unread, "deeper than"),
        ] {
            let listing = listing_of(&space, &[file]).await;
            let (part_read, why) = one_part(&listing, file);
            assert_eq!(part_read, read, "{file}");
            assert!(why.contains(maker), "{file}: {why}");
        }
        // Attributes that are inert, on a module and on functions, keep it whole.
        assert_eq!(
            listing_of(&space, &["inert.rs"]).await,
            NameListing::All(names(&["inert.rs::m::a"]))
        );
        // Each file that is not whole is a part of its own, in the order of the files,
        // and the file that is whole is listed by name beside them.
        let NameListing::Parted { found, parts } = listing_of(&space, &["*.rs"]).await else {
            panic!("the files in parts");
        };
        assert_eq!(found, names(&["inert.rs::m::a"]));
        let keys: Vec<&str> = parts.iter().map(|p| p.key().name().as_str()).collect();
        assert_eq!(
            keys,
            [
                "call.rs",
                "deep.rs",
                "inner.rs",
                "lex.rs",
                "main.rs",
                "module.rs",
                "only_call.rs",
                "rstest.rs"
            ]
        );
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_read_keeps_a_listing_from_being_all() {
        let Some(dir) = in_git(&[
            ("a.rs", "#[test]\nfn a() {}\n"),
            ("big.rs", &"/".repeat(80)),
        ])
        .await
        else {
            return;
        };
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::write(dir.path().join("bytes.rs"), [0xff, 0xfe, b'\n'])
            .await
            .unwrap();
        let listing = listing_of(&RustTestSpace::new(dir.path().to_path_buf()), &["*.rs"]).await;
        let NameListing::Parted { found, parts } = &listing else {
            panic!("{listing:?}");
        };
        assert_eq!(found, &names(&["a.rs::a"]));
        // `big.rs` is read (a comment), and holds no test; `bytes.rs` cannot be read.
        assert_eq!(parts.len(), 1, "{parts:?}");
        assert_eq!(parts.first().key().name().as_str(), "bytes.rs");
        assert_eq!(parts.first().read(), &PartRead::Unread);
        assert!(
            parts
                .first()
                .why()
                .as_str()
                .contains("bytes.rs is not UTF-8"),
            "{parts:?}"
        );
        // A file past the bound of bytes is one a reading cannot tell, as for `file`.
        let small = RustTestSpace::new(dir.path().to_path_buf()).with_max_file(64);
        let why = why_of(&listing_of(&small, &["a.rs", "big.rs"]).await);
        assert_eq!(
            why,
            "1 file(s) the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable)"
        );
    }

    #[tokio::test]
    async fn a_listing_past_its_bound_of_names_or_with_a_name_it_cannot_write_is_some() {
        let long = format!("#[test]\nfn {}() {{}}\n", "a".repeat(5000));
        let Some(dir) = in_git(&[
            (
                "a.rs",
                "#[test]\nfn one() {}\n#[test]\nfn two() {}\n#[test]\nfn three() {}\n",
            ),
            ("long.rs", &long),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        let one = space.clone().with_limits(ScanLimits {
            names: 1,
            ..ScanLimits::default()
        });
        let listing = listing_of(&one, &["a.rs"]).await;
        let NameListing::Some { found, why } = &listing else {
            panic!("{listing:?}");
        };
        assert_eq!(found, &names(&["a.rs::one"]));
        assert_eq!(
            why.as_str(),
            "the selection takes more than 1 names, and the listing stops there"
        );
        // As many tests as the bound is all of them: the bound is of names, not files.
        let three = space.clone().with_limits(ScanLimits {
            names: 3,
            ..ScanLimits::default()
        });
        assert!(matches!(
            listing_of(&three, &["a.rs"]).await,
            NameListing::All(found) if found.len() == 3
        ));
        // Past the files, or the bytes in all, one listing reads: it stops, and says
        // so. The size of a file is looked at before it is opened, so a file past the
        // bytes left is not read at all.
        let a = u64::try_from(
            "#[test]\nfn one() {}\n#[test]\nfn two() {}\n#[test]\nfn three() {}\n".len(),
        )
        .unwrap();
        for (bounded, read, found_after) in [
            (
                ScanLimits {
                    files: 1,
                    ..ScanLimits::default()
                },
                format!("1 file(s) of {a} bytes"),
                names(&["a.rs::one", "a.rs::two", "a.rs::three"]),
            ),
            (
                ScanLimits {
                    total_bytes: a,
                    ..ScanLimits::default()
                },
                format!("1 file(s) of {a} bytes"),
                names(&["a.rs::one", "a.rs::two", "a.rs::three"]),
            ),
            (
                ScanLimits {
                    total_bytes: a - 1,
                    ..ScanLimits::default()
                },
                "0 file(s) of 0 bytes".to_owned(),
                BTreeSet::new(),
            ),
        ] {
            let listing = listing_of(&space.clone().with_limits(bounded), &["*.rs"]).await;
            let NameListing::Some { found, why } = &listing else {
                panic!("{bounded:?}: {listing:?}");
            };
            assert_eq!(found, &found_after, "{bounded:?}");
            assert!(
                why.as_str()
                    .starts_with(&format!("the listing stops after reading {read} in all")),
                "{bounded:?}: {why}"
            );
        }
        // A name longer than a name may be.
        let listing = listing_of(&space, &["long.rs"]).await;
        assert_eq!(
            why_of(&listing),
            "1 test(s) whose name cannot be written as a name of the space"
        );
    }

    #[tokio::test]
    async fn a_listing_that_cannot_be_made_is_no_error_and_a_side_not_read_is_one() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.rs", "#[test]\nfn a() {}\n").await;
        let space = RustTestSpace::new(dir.path().to_path_buf());
        // Outside a repository the state cannot list the files.
        assert!(matches!(
            listing_of(&space, &["*.rs"]).await,
            NameListing::CannotList(_)
        ));
        // A text that is no pattern: no selection, and no listing, with the same why.
        let bad = select(&["/a.rs"]);
        let why = space.select(&bad).unwrap_err().why().clone();
        assert_eq!(
            space.list(Side::After, &bad).await.unwrap(),
            NameListing::CannotList(why)
        );
        assert!(space.select(&select(&["src/**/*.rs"])).is_ok());
        // A run of one side reads no side before: the whole ask fails.
        assert!(matches!(
            space.list(Side::Before, &select(&["*.rs"])).await,
            Err(RustTestError::State(_))
        ));
    }

    #[tokio::test]
    async fn read_through_git_each_side_lists_the_tests_of_its_commit() {
        use super::super::super::state::GitAfter;
        use super::super::super::state::git::GitStates;
        let Some(dir) = in_git(&[("src/a.rs", "#[test]\nfn first() {}\n")]).await else {
            return;
        };
        let root = dir.path();
        conformance::isolated_git(root, &["add", "-A"]).await;
        conformance::isolated_git(root, &["commit", "-q", "-m", "one"]).await;
        write(root, "src/a.rs", "#[test]\nfn second() {}\n").await;
        conformance::isolated_git(root, &["add", "-A"]).await;
        conformance::isolated_git(root, &["commit", "-q", "-m", "two"]).await;
        // In the work tree only: no commit holds it.
        write(root, "src/loose.rs", "#[test]\nfn loose() {}\n").await;
        let Ok(states) = GitStates::open(
            root.to_path_buf(),
            "HEAD~1",
            &GitAfter::Commit("HEAD".to_owned()),
        )
        .await
        else {
            // A Git too old to read a commit as a checkout writes it.
            return;
        };
        let space = RustTestSpace::on(ProjectTree::git(states));
        let selection = select(&["src/**/*.rs"]);
        assert_eq!(
            space.list(Side::Before, &selection).await.unwrap(),
            NameListing::All(names(&["src/a.rs::first"]))
        );
        assert_eq!(
            space.list(Side::After, &selection).await.unwrap(),
            NameListing::All(names(&["src/a.rs::second"]))
        );
    }

    #[tokio::test]
    async fn a_test_listed_lives_where_the_file_it_is_listed_of_is_placed() {
        let Some(dir) = in_git(&[("src/a.rs", "#[test]\nfn one() {}\n")]).await else {
            return;
        };
        #[cfg(unix)]
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::symlink("a.rs", dir.path().join("src/link.rs"))
            .await
            .unwrap();
        let space = RustTestSpace::new(dir.path().to_path_buf());
        let NameListing::All(listed) = listing_of(&space, &["src/**/*.rs"]).await else {
            panic!("the files are listed whole");
        };
        assert!(listed.contains(&n("src/a.rs::one")), "{listed:?}");
        let placed = space.places(Side::After, &listed).await.unwrap();
        for name in &listed {
            let file = TestName::parse(name.as_str()).unwrap().file;
            let only = file::placed_only(&space.state, Side::After, &file)
                .await
                .unwrap();
            assert_eq!(placed[name], only, "{name}");
            assert!(
                matches!(&only, Placement::Only(files) if files.contains(&file)),
                "{name}: {only:?}"
            );
        }
        #[cfg(unix)]
        assert!(listed.contains(&n("src/link.rs::one")), "{listed:?}");
    }

    #[tokio::test]
    async fn an_attribute_macro_or_a_derive_on_any_item_keeps_a_listing_from_being_all() {
        const TEST: &str = "#[test]\nfn a() {}\n";
        let with = |item: &str| format!("{item}\n{TEST}");
        let files = [
            ("impl.rs", with("#[my_macro]\nimpl S {}")),
            (
                "struct.rs",
                with("#[derive(Debug, serde::Serialize)]\nstruct S;"),
            ),
            ("enum.rs", with("#[derive(thiserror::Error)]\nenum E {}")),
            (
                "derive_only.rs",
                "#[derive(thiserror::Error)]\nenum E {}\n".to_owned(),
            ),
            ("trait.rs", with("#[my_macro]\ntrait T {}")),
            ("const.rs", with("#[my_macro]\nconst X: u8 = 1;")),
            ("static.rs", with("#[my_macro]\nstatic Y: u8 = 1;")),
            ("outside.rs", with("#[my_macro]\nmod x;")),
            ("included.rs", with("#[path = \"x.inc\"]\nmod tests;")),
            (
                "unread_path.rs",
                with("#[cfg_attr(unix, path = \"x.rs\")]\nmod tests;"),
            ),
            (
                "inner.rs",
                "mod m {\n    #[my_macro]\n    struct S;\n    #[test]\n    fn a() {}\n}\n"
                    .to_owned(),
            ),
            (
                "std.rs",
                with(
                    "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]\n#[derive(std::fmt::Debug, core::hash::Hash)]\n#[repr(C)]\n#[cfg(unix)]\n#[allow(dead_code)]\nstruct S;\n#[path = \"y.rs\"]\nmod y;\n#[path = r\"z.rs\"]\nmod z;",
                ),
            ),
        ];
        let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
        let Some(dir) = in_git(&files).await else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        for (file, maker) in [
            ("impl.rs", "`#[my_macro]` on an item at the top of the file"),
            ("struct.rs", "`#[derive(serde::Serialize)]` on an item"),
            ("enum.rs", "`#[derive(thiserror::Error)]` on an item"),
            ("trait.rs", "`#[my_macro]` on an item"),
            ("const.rs", "`#[my_macro]` on an item"),
            ("static.rs", "`#[my_macro]` on an item"),
            ("outside.rs", "`#[my_macro]` on an item"),
            (
                "included.rs",
                "the module `tests` at the top of the file, read by `#[path]` from a file that is no `.rs` file",
            ),
            (
                "unread_path.rs",
                "the module `tests` at the top of the file, read by `#[path]`",
            ),
            ("inner.rs", "`#[my_macro]` on an item in the module `m`"),
        ] {
            let listing = listing_of(&space, &[file]).await;
            let (read, why) = one_part(&listing, file);
            assert!(
                matches!(&read, PartRead::Found(found) if found.len() == 1),
                "{file}: the test is still found: {read:?}"
            );
            assert!(why.contains(maker), "{file}: {why}");
        }
        // A derive of a crate in a file with no test: a part read, with no test found.
        let (read, why) = one_part(
            &listing_of(&space, &["derive_only.rs"]).await,
            "derive_only.rs",
        );
        assert_eq!(read, PartRead::Found(BTreeSet::new()));
        assert!(
            why.contains("`#[derive(thiserror::Error)]` on an item"),
            "{why}"
        );
        // The derives of the standard library and the built-in attributes keep it
        // whole.
        assert_eq!(
            listing_of(&space, &["std.rs"]).await,
            NameListing::All(names(&["std.rs::a"]))
        );
    }

    #[test]
    fn the_built_in_attributes_are_inert_and_those_of_a_crate_are_not() {
        let built_in = [
            "repr(C)",
            "non_exhaustive",
            "must_use",
            "deprecated(note = \"x\")",
            "doc = \"x\"",
            "doc(hidden)",
            "inline(always)",
            "cold",
            "track_caller",
            "allow(dead_code)",
            "warn(unused)",
            "deny(missing_docs)",
            "forbid(unsafe_code)",
            "expect(dead_code, reason = \"x\")",
            "automatically_derived",
            "no_mangle",
            "unsafe(no_mangle)",
            "export_name = \"x\"",
            "link_section = \".x\"",
            "used",
            "link(name = \"x\")",
            "link_name = \"x\"",
            "no_link",
            "path = \"x.rs\"",
            "ignore",
            "should_panic(expected = \"x\")",
            "macro_use",
            "macro_export",
            "target_feature(enable = \"avx2\")",
            "global_allocator",
            "panic_handler",
            "naked",
            "debugger_visualizer(natvis_file = \"x.natvis\")",
            "collapse_debuginfo(yes)",
            "cfg(unix)",
            "cfg_attr(unix, repr(C))",
            "derive(Debug, Clone, std::hash::Hash)",
            "clippy::cognitive_complexity",
            "diagnostic::on_unimplemented(message = \"x\")",
        ];
        for attr in built_in {
            let top = Items::scan(format!("#[{attr}]\nstruct S;\n").parse().unwrap());
            assert_eq!(top.items[0].maker(), Vec::<String>::new(), "#[{attr}]");
        }
        // A derive of a crate and its helper attributes: what it writes is not seen.
        for (attr, maker) in [
            ("serde(rename_all = \"snake_case\")", "serde"),
            ("error(\"x\")", "error"),
            ("command(name = \"x\")", "command"),
            (
                "derive(Debug, thiserror::Error)",
                "derive(thiserror::Error)",
            ),
            (
                "cfg_attr(unix, derive(serde::Serialize))",
                "derive(serde::Serialize)",
            ),
            ("my_macro", "my_macro"),
            ("test", "test"),
        ] {
            let top = Items::scan(format!("#[{attr}]\nstruct S;\n").parse().unwrap());
            assert_eq!(top.items[0].maker(), vec![maker.to_owned()], "#[{attr}]");
        }
    }

    #[tokio::test]
    async fn the_derives_of_the_standard_library_are_inert_and_no_other_is() {
        let Some(dir) = in_git(&[
            (
                "std.rs",
                "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]\n#[derive(std::fmt::Debug, core::hash::Hash)]\n#[cfg(unix)]\n#[allow(dead_code)]\nstruct S;\n#[test]\nfn a() {}\n",
            ),
            ("named.rs", "#[derive(Debug, Serialize)]\nstruct S;\n#[test]\nfn a() {}\n"),
            ("other.rs", "#[derive(my::Debug)]\nstruct S;\n#[test]\nfn a() {}\n"),
            (
                "aliased.rs",
                "use serde::Serialize as Debug;\n#[derive(Debug)]\nstruct S;\n#[test]\nfn a() {}\n",
            ),
            (
                "imported.rs",
                "mod m {\n    use my::{derives::Clone, Other};\n    #[derive(Clone)]\n    struct S;\n}\n#[test]\nfn a() {}\n",
            ),
            (
                "unused.rs",
                "use my::Other;\n#[derive(Debug, Clone)]\nstruct S;\n#[test]\nfn a() {}\n",
            ),
            (
                "glob.rs",
                "use my::*;\n#[derive(Debug)]\nstruct S;\n#[test]\nfn a() {}\n",
            ),
            (
                "child_glob.rs",
                "#[derive(Debug)]\nstruct S;\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn a() {}\n}\n",
            ),
            (
                "glob_inside.rs",
                "#[derive(Debug)]\nstruct S;\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[derive(Clone)]\n    struct T;\n    #[test]\n    fn a() {}\n}\n",
            ),
            (
                "parent_use.rs",
                "use my::Debug;\nmod m {\n    #[derive(Debug)]\n    struct S;\n    #[test]\n    fn a() {}\n}\n",
            ),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        assert_eq!(
            listing_of(&space, &["std.rs"]).await,
            NameListing::All(names(&["std.rs::a"]))
        );
        assert!(
            why_of(&listing_of(&space, &["named.rs"]).await).contains("`#[derive(Serialize)]`")
        );
        // A derive of another crate under a name the standard library uses.
        assert!(
            why_of(&listing_of(&space, &["other.rs"]).await).contains("`#[derive(my::Debug)]`")
        );
        // A bare name a `use` of the file brings in, as its last segment or by an
        // alias, at the top or in a module: what it names is not told by its spelling.
        assert!(
            why_of(&listing_of(&space, &["aliased.rs"]).await)
                .contains("`#[derive(Debug)]` on an item at the top of the file")
        );
        assert!(
            why_of(&listing_of(&space, &["imported.rs"]).await)
                .contains("`#[derive(Clone)]` on an item in the module `m`")
        );
        // A `use` of other names leaves them the standard library's.
        assert_eq!(
            listing_of(&space, &["unused.rs"]).await,
            NameListing::All(names(&["unused.rs::a"]))
        );
        // A glob `use` may bring in any name, in the module it is written in only.
        assert!(why_of(&listing_of(&space, &["glob.rs"]).await).contains(
            "`#[derive(Debug)]` on an item at the top of the file, where a glob `use` of the module may bring in a derive named `Debug`"
        ));
        assert_eq!(
            listing_of(&space, &["child_glob.rs"]).await,
            NameListing::All(names(&["child_glob.rs::tests::a"]))
        );
        let inside = why_of(&listing_of(&space, &["glob_inside.rs"]).await);
        assert!(
            inside.contains("`#[derive(Clone)]` on an item in the module `tests`, where a glob"),
            "{inside}"
        );
        assert!(!inside.contains("derive(Debug)"), "{inside}");
        // A `use` of the module around does not reach a derive in a module in it.
        assert_eq!(
            listing_of(&space, &["parent_use.rs"]).await,
            NameListing::All(names(&["parent_use.rs::m::a"]))
        );
    }

    #[tokio::test]
    async fn a_test_attribute_of_a_crate_is_a_test_and_keeps_a_listing_from_being_all() {
        let Some(dir) = in_git(&[
            (
                "tokio.rs",
                "#[tokio::test]\nasync fn a() {}\n#[test]\nfn b() {}\n",
            ),
            ("cfg.rs", "#[cfg_attr(unix, test_log::test)]\nfn a() {}\n"),
            (
                "plain.rs",
                "#[test]\nfn a() {}\n#[cfg_attr(unix, test)]\nfn b() {}\n",
            ),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        for (file, found, maker) in [
            (
                "tokio.rs",
                vec!["tokio.rs::a", "tokio.rs::b"],
                "`#[tokio::test]` on the test `a`, a macro that may write more than one test",
            ),
            (
                "cfg.rs",
                vec!["cfg.rs::a"],
                "`#[test_log::test]` on the test `a`",
            ),
        ] {
            let listing = listing_of(&space, &[file]).await;
            let (read, why) = one_part(&listing, file);
            assert_eq!(
                read,
                PartRead::Found(names(&found)),
                "{file}: each is a test of its name"
            );
            assert!(why.contains(maker), "{file}: {why}");
        }
        // The test attribute of the language alone keeps it whole.
        assert_eq!(
            listing_of(&space, &["plain.rs"]).await,
            NameListing::All(names(&["plain.rs::a", "plain.rs::b"]))
        );
    }

    #[tokio::test]
    async fn an_attribute_macro_beside_a_test_attribute_keeps_a_listing_from_being_all() {
        let Some(dir) = in_git(&[
            ("made.rs", "#[test]\n#[gen_variants]\nfn a() {}\n"),
            ("cfg.rs", "#[cfg_attr(unix, test)]\n#[serial]\nfn a() {}\n"),
            (
                "inert.rs",
                "#[test]\n#[ignore]\n#[should_panic]\nfn a() {}\n#[test]\n#[should_panic(expected = \"x\")]\nfn b() {}\n#[cfg_attr(unix, test)]\n#[allow(dead_code)]\nfn c() {}\n",
            ),
        ])
        .await
        else {
            return;
        };
        let space = RustTestSpace::new(dir.path().to_path_buf());
        for (file, maker) in [
            ("made.rs", "`#[gen_variants]` on the test `a`"),
            ("cfg.rs", "`#[serial]` on the test `a`"),
        ] {
            let listing = listing_of(&space, &[file]).await;
            let (read, why) = one_part(&listing, file);
            assert_eq!(
                read,
                PartRead::Found(names(&[format!("{file}::a").as_str()])),
                "{file}"
            );
            assert!(why.contains(maker), "{file}: {why}");
        }
        // The test named is still there to resolve: only the listing is not whole.
        let read = space.resolve(&asks(&["made.rs::a"])).await.unwrap();
        assert!(matches!(
            read[&n("made.rs::a")].state(&Comparison::Bytes),
            ArtifactState::Present(_)
        ));
        assert_eq!(
            listing_of(&space, &["inert.rs"]).await,
            NameListing::All(names(&["inert.rs::a", "inert.rs::b", "inert.rs::c"]))
        );
    }

    #[test]
    fn an_item_is_a_test_by_its_attribute_and_may_make_one_by_one_not_inert() {
        let top = Items::scan(
            "#[test] fn a() {}\n#[tokio::test] async fn b() {}\n#[cfg_attr(unix, test)] fn c() {}\n#[cfg(test)] fn d() {}\n#[rstest] fn e() {}\n#[test] mod f {}\n#[allow(x)] #[cfg(test)] #[clippy::y] fn g() {}\n#[a] #[b::c] #[inline] fn h() {}\n#[unsafe(no_mangle)] #[proc_macro] fn i() {}\n#[unsafe(a)] fn j() {}\n"
                .parse()
                .unwrap(),
        );
        let table: Vec<(bool, Vec<String>)> = top
            .items
            .iter()
            .map(|item| (item.is_test(), item.maker()))
            .collect();
        let strings = |paths: &[&str]| paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            table,
            // A test attribute is itself not inert: what a test carries is never
            // asked whether it may make one.
            vec![
                (true, strings(&["test"])),
                (true, strings(&["tokio::test"])),
                (true, strings(&["test"])),
                (false, strings(&[])),
                (false, strings(&["rstest"])),
                (false, strings(&["test"])),
                (false, strings(&[])),
                (false, strings(&["a", "b::c"])),
                (false, strings(&[])),
                (false, strings(&["a"])),
            ]
        );
    }

    #[test]
    fn a_name_is_built_as_the_source_spells_it_and_reads_back() {
        let file: RelPath = "x.rs".parse().unwrap();
        let of = |modules: &[&str], function: &str| {
            let modules: Vec<String> = modules.iter().map(|m| (*m).to_owned()).collect();
            TestName::of(&file, &modules, function).map(|name| name.as_str().to_owned())
        };
        assert_eq!(of(&[], "r#foo").unwrap(), "x.rs::r#foo");
        assert_eq!(of(&[], "r#type").unwrap(), "x.rs::r#type");
        assert_eq!(
            of(&["tests", "r#mod"], "a").unwrap(),
            "x.rs::tests::r#mod::a"
        );
        // What the grammar would not read back is no name.
        assert!(of(&[], "fn").is_err());
        let odd: RelPath = "a::b.rs".parse().unwrap();
        assert!(TestName::of(&odd, &[], "t").is_err());
    }
}
