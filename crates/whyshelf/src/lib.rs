//! Books of documents whose kinds a project declares, linked to the files they
//! stand on, and whether each decision about them still holds.
//!
//! The umbrella crate: the one name users depend on. The code lives in
//! `whyshelf-core`, re-exported here; the adapters of `whyshelf-infra` are
//! [`infra`], behind the `infra` feature (on by default).
//!
//! This documentation is the manual: the model, the files, the command line, its
//! output and exit status. The pages it links to document each part next to its
//! code.
//!
//! - [Getting started](#getting-started)
//! - [Books: kinds of document you declare](#books-kinds-of-document-you-declare),
//!   with the [commands](#commands) and their [output and exit
//!   status](#output-and-exit-status)
//! - [Crates](#crates)
//!
//! # Getting started
//!
//! In the root of a project:
//!
//! ```text
//!   whyshelf init --template sample
//!                             a declaration of four books, and one document of each
//!   whyshelf check            nothing wrong: exit 0
//!   whyshelf map              the books and their links, with a drawing of them in `diagram`
//!   whyshelf trace V-1        the chain, from the verification up to the finding
//!   whyshelf judge approve --pending F-1 V-1 --reason "read, and it holds"
//!                             the four, upstream first: F-1, REQ-1, DEC-1, V-1
//!   whyshelf status           each document, approved and fresh
//! ```
//!
//! `--pending F-1 V-1` approves F-1 and V-1 and the pending documents V-1 stands on
//! through `depends` links, DEC-1 and REQ-1, and nothing else, so a document of your
//! own already in a book's directory is left as it is. `judge approve --pending V-1
//! --reason …` alone would leave F-1 pending: REQ-1 only `grounds` on it, an
//! `informs` link, which an approval does not stand on. `init` answers this
//! command in its `next`.
//!
//! The template `sample` writes `.whyshelf/books.toml` with four books, `finding`,
//! `requirement`, `decision` and `verification`, and one pending document of each
//! under `books/`, linked in one chain about a small command that must not lose a
//! user's file:
//!
//! ```text
//!   F-1    finding        A save that fails can empty the user's file
//!    ^ grounds (informs)
//!   REQ-1  requirement    Saving never loses what was saved before
//!    ^ serves (depends)
//!   DEC-1  decision       Write a new file, then rename it over the old one
//!    ^ checks (depends)
//!   V-1    verification   Review every write of a user's file   (method: review)
//! ```
//!
//! Once they are approved, edit `books/requirement/REQ-1.md` and run `whyshelf
//! stale`: REQ-1 no longer holds, nor do DEC-1 and V-1, which depend on it, and
//! `stale` says where that starts. F-1 still holds: a change makes stale what
//! depends on the document changed, downstream, and never what it stands on. Change the declaration and the documents to the project's
//! own, or remove them and start again with another template.
//!
//! `init --template <name>` chooses the template (`--sample` is `--template
//! sample`), and `init --help` lists them, each with a line on what it is. In
//! short:
//!
//! ```text
//!   minimal     the default: one book of notes, with comments that show how to add
//!               a field, a link and a link to files; no document
//!   sample      the four books above, and one pending document of each
//!   qa-first    the four books, with two rules loosened for checks written before
//!               decisions; a finding, a requirement and a verification
//! ```
//!
//! Each is a template of `whyshelf_infra::shelf::init`, a whole set of files;
//! whyshelf itself knows no kind of document.
//!
//! ## Checks first: `qa-first`
//!
//! Where the checks are fixed first and the decisions written at release,
//! `whyshelf init --template qa-first` writes the four books with two rules
//! loosened, each marked `qa-first` in `.whyshelf/books.toml`: a verification's
//! `checks` is `count = "many"`, so a verification may check no decision yet, and
//! `serves` has `cover = "warn"`, so a requirement no decision serves is listed,
//! not failed. Beside it are F-1, REQ-1, which F-1 grounds, and V-1, a `review`
//! that checks what REQ-1 asks; it links nothing yet, and gets its link
//! (`checks`) when the decision is written. `check` is clean, with a warning:
//!
//! ```text
//!   warning: requirement:REQ-1 is reached by no decision.serves (cover = "warn")
//! ```
//!
//! and `next` approves the three with `judge approve --pending F-1 REQ-1 V-1`, as no
//! `depends` link stands on any of them. At release, tighten the rules:
//!
//! 1. In `.whyshelf/books.toml`, set `checks` back to `count = "at-least-one"` and
//!    the cover of `serves` back to `cover = true`. `check` now fails: V-1 checks
//!    no decision, and no decision serves REQ-1.
//! 2. Write the decisions, and link them from the verifications:
//!    `whyshelf new decision --title "Write a new file, then rename it over the
//!    old one" serves=REQ-1` makes DEC-1, and `checks: ["DEC-1"]` in the front
//!    matter of `books/verification/V-1.md` links it.
//! 3. `whyshelf judge approve --pending DEC-1 --reason "read, and it holds"`
//!    approves DEC-1, then `whyshelf judge approve V-1 --reason "checks DEC-1
//!    now"` approves V-1 again: its new link made its approval stale. A bare
//!    `judge approve --pending` (or `--stale`) approves every pending (or stale)
//!    document of the project that would hold, not only these.
//! 4. `whyshelf check`: clean, with nothing uncovered.
//!
//! # Books: kinds of document you declare
//!
//! `whyshelf ...` knows no kind of document of its own. A project declares its
//! *books* in `.whyshelf/books.toml`: for each, a directory, how its keys are
//! written, what it is for (`description`, one line, optional), its fields, and
//! the kinds of link it may hold to other books; `map` answers what it declares,
//! with a drawing. People decide about documents (approve, deny, deprecate,
//! retire, or revoke an earlier decision), each decision is a record of its own,
//! and whether an approval still holds is computed on every read. The model is
//! [`domain::shelf`], the use cases [`app::shelf`], the files and their formats
//! `whyshelf_infra::shelf`.
//!
//! | kind of link | what it means for an approval |
//! |---|---|
//! | `depends` | the approval pins the target; a change of it makes the approval stale, and a target that is gone or decided against makes it broken |
//! | `informs` | the target is worth reading: one that does not hold is a notice, never a staleness |
//! | `replaces` | the document takes the place of the target, and is its successor when the target is deprecated |
//!
//! A link into a book may also say `cover`: every pending or approved document of
//! the target book is expected to be reached by it, from a pending or approved
//! document. A project uses it to see a hole in its chain of reasons (a requirement
//! no decision serves, a decision no verification checks). `cover` has a strength:
//! `"error"` (or `true`) or `"warn"`; `false`, or no `cover`, expects nothing, and
//! any other value is a problem of the declaration. A document no covering link
//! reaches has a `not_covered` notice and is listed in `check`'s `uncovered`, each
//! with the strength; one of `error` makes `check` not clean, and one of `warn`
//! leaves it clean, for a hole that may stand a while, and is written by `check` as
//! a line on stderr, `warning: <key> is reached by no <book>.<link> (cover =
//! "warn")` (its identity when it has no key), so that a run which shows the answer only when it fails still shows
//! it. Its freshness is unchanged either way. A link to artifacts takes no
//! expectation, and a link into its own book is met only by another document of
//! that book.
//!
//! The rule stays declared for the whole book; one document can be exempt from it.
//! `judge exempt <ref>... --from <book>.<link> --reason <text>` records, by an
//! actor, at a time, for a reason, that the document is not expected to be reached
//! by that link (a finding kept on purpose that grounds no requirement). It is
//! accepted only when it would be needed the moment it is written: the document is
//! valid now, the link is declared with a cover (of either strength) and points
//! into the document's book, the document is not denied, deprecated or retired (no
//! cover expects it), and the link does not reach it now (the refusal names the
//! documents it is reached from); and when no exemption that stands already exempts
//! the document from the link (a second is refused, not repeated: withdraw the
//! first to say it again). An exemption is accepted and read by the one rule, so
//! whatever would read as unneeded at once is refused; anything else is refused
//! too, and the run writes nothing. An exempt document has no `not_covered` notice for that link
//! but an `exempt` one, is not in `uncovered`, and does not fail `check` under a
//! `true` cover; `check` lists the exemptions apart in `exempt`, with who, when and
//! why. When the link reaches the document again, or no longer expects to (it is
//! no longer declared, or declared with no cover, or into another book), or the
//! document is not valid or decided against, the exemption is no longer needed, as
//! Rust's `#[expect]` tells an expectation no longer met: the notice
//! `exemption_unneeded`, with why, listed by `check` in `exemption_unneeded` and
//! warned of on stderr, one line each, every document by its key (by its identity
//! when it has none, as a document that is not valid):
//!
//! ```text
//!   warning: <record id> exempts <key> from `<book>.<link>` and is not needed: <why>; withdraw it with `judge withdraw <record id>`
//! ```
//!
//! where `<why>` is one of `` `<book>.<link>` reaches it now, from <key>, <key> ``,
//! `` no link `<book>.<link>` into a book is declared now ``, `` `<book>.<link>` is
//! declared with no cover now ``, `` `<book>.<link>` points into `<book>` now, and
//! the document is of `<book>` ``, `the document is not valid now: its files are
//! invalid` (or `no file in the books' ranges carries it`, for a document whose file
//! is gone or that a book's range leaves out, or `whether a file carries it cannot be
//! told`), and `the document is decided against, and no covering link expects to
//! reach it`. It does not fail `check`. `judge withdraw <record id>` takes an
//! exemption back. Two exemptions that stand of one document from one link (branches
//! merged, each with one) are the notice `exemption_repeated`, listed by `check` in
//! `exemption_repeated` and warned of on stderr:
//!
//! ```text
//!   warning: 2 exemptions exempt <key> from `<book>.<link>` (<record id>, <record id>): one is enough, withdraw the others, with `judge withdraw <id>`
//! ```
//!
//! `unlinked` asks nothing of a cover, and lists an exempt document as it lists any
//! other.
//!
//! The files:
//!
//! ```text
//!   .whyshelf/books.toml         the declaration of the books
//!   <book dir>/**/*.md           the documents: Markdown with a flat YAML front matter
//!   .whyshelf/ledger/<id>.json   one decision per file (two branches merge without a conflict)
//! ```
//!
//! A chain of four books, finding -> requirement -> decision -> verification, is
//! what `whyshelf init --template sample` writes (a test keeps it clean); change what you
//! need. A document of its `requirement` book:
//!
//! ```text
//!   ---
//!   id: "019a6f3e-0000-7000-8000-000000000002"   a UUID v7: the identity, never changed
//!   key: "REQ-1"                                 how people name it; may change
//!   grounds: ["F-1"]                             a link: keys in the target book
//!   title: "Saving never loses what was saved before"
//!   ---
//!   The body.
//! ```
//!
//! A document's hash covers its book, its fields and its body (not its key, its
//! place or its links), so it can be renamed or moved and stay approved.
//!
//! ## A book in an existing directory
//!
//! By default every `.md` file under a book's `dir` is a document of it, and a file
//! with no front matter is an invalid one. A project that brings a book into a
//! directory that already holds other files says which are the book's:
//!
//! ```text
//!   [[book]]
//!   name = "spec"
//!   dir = "docs"
//!   key = { prefix = "S" }
//!   include = ["**/*.md"]                    # globs relative to dir; default: every .md below it
//!   exclude = ["archive/**", "**/README.md"] # default: none
//!   without_front_matter = "skip"            # "invalid" (the default) or "skip"
//! ```
//!
//! A file is a document of the book when it is a `.md` file under `dir` whose path
//! below `dir` (with `/` between its segments) matches some `include` and no
//! `exclude`. A pattern is matched against that path segment by segment, and has
//! three wildcards and nothing else:
//!
//! | written | matches |
//! |---|---|
//! | `**`, a whole segment | any number of segments, none included |
//! | `*` in a segment | any characters of one segment, none included |
//! | `?` in a segment | one character of one segment |
//! | any other character | itself, case and bytes as written |
//!
//! So `*.md` is the files of `dir` itself, `**/*.md` every file below it, and
//! `archive/**` everything under `archive`. Unicode is compared byte for byte: a
//! pattern written in NFC does not take a name stored in NFD (as macOS may store
//! one), so write the name as the file system holds it.
//!
//! A problem of the declaration, which names the book and the pattern, is: a
//! pattern that is empty; that leaves `dir` (absolute, or with a `..` segment); that
//! has an empty or `.` segment; that begins with `!` (write what to leave out in
//! `exclude`); that holds a `\` (`/` is the one separator), or a `[`, `]`, `{` or
//! `}` (there is no class and no alternative); that writes `**` inside a segment
//! (`sub**`); or whose last segment has no `*` or `?` and does not end in `.md`, so
//! that it takes no file (an `exclude = ["archive"]` is told to write
//! `archive/**`). So are an `include = []`, which would take no file, and a
//! `without_front_matter` that is neither word. Two books whose directories are the
//! same or nest are refused as before, whatever their ranges.
//!
//! A file the range leaves out is not read, and a directory an `exclude` takes
//! whole (`archive/**`) is not even opened, so a name there that is no path, or a
//! directory there that cannot be read, is no error: `check` does not mention it,
//! `fmt` does not touch it, a link to a key only it writes is a link to no
//! document, and the key it writes is not taken. A document that was approved and
//! that the range comes to leave out is gone, as if its file were deleted: `stale`
//! lists its approval as broken, because no file carries the document. `new`
//! refuses a document whose file, `<dir>/<key>.md`, the range leaves out, for a
//! `--key` and for the next key alike (exit 2, naming the book, the path and the
//! patterns, before anything is written).
//!
//! A file in the range that has *no front matter at all* is invalid, or, with
//! `without_front_matter = "skip"`, is not a document: it is skipped, and not
//! silently. `check` gives, for each book, `skipped` (the count) and `skipped_files`
//! (`{"path", "why": "no_front_matter"}`, in the order they were read), beside
//! `valid` and `invalid`, and writes a line on stderr per book that skipped any,
//! `note: book spec skipped 3 files with no front matter`, so that a run which
//! shows the answer only when it fails still says that files were left out. A
//! skipped file changes no exit and is no row of `status`. A file has *no front
//! matter* when its first line with anything on it (after a leading byte order mark
//! and blank lines) does not begin with `---`: the empty file, a file that opens
//! with a heading or a paragraph, and one whose only `---` is a thematic break in
//! the middle are all such files. A file that tries to open a front matter and
//! does not is *broken*, never skipped, so a mistyped delimiter is not passed over:
//! an opening line `---` never closed, a front matter that is not YAML or not a
//! mapping, a byte order mark or blank line before the opening `---`, and a first
//! line that is `----`, `---x` or `--- x`. Under `skip`, such an opening is told as
//! "the front matter must open with a line that is exactly `---` at the start of
//! the file"; a book that does not skip tells every file as it did before it could
//! (a file that does not open with the line `---` has no front matter). A Markdown
//! file whose first line is a thematic break `---` is read as opening a front
//! matter, so under `skip` it is invalid, not skipped: exclude it, or begin it
//! otherwise.
//!
//! `map` gives, for each book, `include`, `exclude` and `without_front_matter` as
//! written, each null when not written.
//!
//! ## Links to artifacts: things outside the documents
//!
//! A link may point at *artifacts*, things of the project outside the documents
//! (code, tests, data), with `target = "artifact"`; it `depends` or `informs`, never
//! `replaces` (and no book can be called `artifact`). Every such link names the
//! *space* its names are in, with `space`, itself or through a preset (below): there
//! is no space a link has when it names none, and a space no adapter offers is
//! refused, as a declaration that cannot be used. The spaces offered, and what the
//! names of each are, are listed on the page of `whyshelf_infra::shelf::artifact`.
//! The sample's `verification` book has one such
//! link, `tests` in the space `file` (the files of the project, by their path), so a
//! verification names the tests that show it holds:
//!
//! ```text
//!   tests: [crates/cache/tests/expiry.rs]      paths under the root, with `/`
//! ```
//!
//! The space `rust-item` holds the items of the project's Rust crates, by their
//! path (`cache::Expiry`, `cache::Expiry::check`, `cache::Kind::Soft`), as the docs
//! `cargo doc` builds know them: they are looked up in the HTML rustdoc writes in
//! the project's build directory (`CARGO_TARGET_DIR`, else `build.target-dir` of
//! the cargo settings at the root, else the one beside the root's `Cargo.toml`).
//! Finding them runs no program. A path is there or absent only when the docs of
//! its crate are built and are rustdoc's; before that, or when the build directory
//! cannot be told, it cannot be told. Only that an item is there is known of it, so
//! a link to items compares `presence` (the preset `rustdoc`, below; another
//! comparison cannot be told): an edit of an item changes nothing, and an item
//! renamed, moved or removed is gone. The docs are read as they are, so docs left
//! by an earlier build, of an item or a crate since gone, read as there: build them
//! afresh before you check or approve (`cargo doc --no-deps`; in this repository
//! `just rust-doc`, which removes the old ones first). A name reached only through
//! a glob re-export of a public module, a member reached only through a type alias,
//! and an item hidden from the docs (`#[doc(hidden)]`) read as absent; a member a
//! type has through `Deref` or a blanket impl reads as there; a raw identifier
//! (`r#type`) is no name. The rules and the limits are on the page of
//! `whyshelf_infra::shelf::artifact::rust_item`. A link to items:
//!
//! ```text
//!   [[book.link]]
//!   name = "rustdoc"
//!   target = "artifact"
//!   preset = "rustdoc"       # space = "rust-item", compare = "presence", means = "depends"
//! ```
//!
//! The space `rust-test` holds the test functions of the project's Rust source,
//! each by its file and its path in the file: `<file>.rs::<module>::<function>`,
//! the file a path of the space `file` ending in `.rs` (as written, case included,
//! never out of the root), then the inline modules it is in, outermost first, then
//! the function, each a Rust identifier (a raw one, `r#type`, included), compared
//! as written (not normalized). A function is a test when it carries an attribute
//! whose path ends in `test` (`#[test]`, `#[tokio::test]`), itself or in a
//! `cfg_attr`; `#[rstest]`, `#[test_case]` and `#[cfg(test)]` are not such an
//! attribute. Twin modules or functions of the name (each under its own `#[cfg]`)
//! are all searched, and twin tests are one. Finding them runs no program: the file
//! is read (at most 16 MiB), lexed, and its items scanned; no expression or type is
//! parsed. What is compared is the function item's own tokens, from its first
//! attribute to the end of its body, with its doc attributes left out (not those in
//! a macro call's tokens, which are data). So an edit of white space, comments or
//! doc comments changes nothing, an edit of another test in the file changes
//! nothing, any other edit of the function makes the approvals that name it stale,
//! and a test renamed or removed breaks them, save where a module on the way
//! holds an item macro or an attribute that is not inert (below): there a test
//! removed cannot be told, and the approval stays fresh, with a notice that it
//! cannot be told. Nothing outside the function counts:
//! an edit of a helper it calls, of a `use` at the top of the file, or a `cfg` that
//! stops its module from being compiled leaves it fresh, and a test in a file no
//! crate compiles reads as there. A file that cannot be read, is too large or does
//! not lex cannot be told, nor can a test under a module declared out of line
//! (`mod x;`), whose file is not followed: name such a test by the file of its
//! module. Macros are not expanded: a test not found where a macro may make it (an
//! item macro call, `include!`, an attribute macro on a module on the way or on a
//! function of the name) cannot be told; one an attribute macro on another item
//! makes reads as absent. A twin of a module declared out of line is not
//! followed: a test not found in its inline twins cannot be told. The text
//! is made, not stored, so `diff` has nothing to set beside it. The rules and the
//! limits are on the page of `whyshelf_infra::shelf::artifact::rust_test`. A link
//! to tests, and a name in it:
//!
//! ```text
//!   [[book.link]]
//!   name = "tests"
//!   target = "artifact"
//!   preset = "test"          # space = "rust-test", compare = "bytes", means = "depends"
//!
//!   tests: ["crates/cache/tests/expiry.rs::an_entry_past_its_time_expires",
//!           "crates/cache/src/lib.rs::tests::a_fresh_entry_is_kept"]
//! ```
//!
//! A text that is no name of the link's space makes the document invalid, with a
//! problem that names the link, the text and why. For each name, its adapter answers
//! one of four: the thing is there (with its hash), not there, it cannot tell now
//! (with why), or the text is not a name. An approval pins, for each name of a
//! `depends` link, the hash of what is there (of its bytes, unless the link compares
//! otherwise, below), or that nothing was there: a name with nothing there yet is the
//! evidence not written yet, not a broken link. A path names a file only as written,
//! ASCII case included, on every system: where the file system does not tell case
//! apart, `justfile` still names no file `Justfile`, as for Git. Every name of a
//! pending or approved document with nothing there is told, since a wrong name is
//! pinned as absent too: `check` lists it under `absent` (the check stays clean),
//! `judge approve` under each judgement's `absent`, and its row has an
//! `artifact_absent` notice. A name its adapter cannot tell about now is listed under
//! `check`'s `unknown` (the check stays clean) with an `artifact_unknown` notice; an
//! approval that would pin one is refused, and one already pinned changes nothing of
//! the approval while it lasts. `check` also writes each such name on stderr, as it
//! does a gap of `warn`: `warning: <key> names <name> in <space>, which cannot be
//! told: <why>`, or `…, which has nothing behind it`. A gate cannot say an approval
//! holds that stands on a name that cannot be told, so `stale` lists each name an
//! approved document pins through a `depends` link that cannot be told now in its
//! `unknown` (`doc`, `link`, `name`, `space`, `why`, as `check`'s), and
//! `stale --exit-code` exits 1 for one too, with `warning: <key> pins <name> in
//! <space>, which cannot be told now, …` on stderr; its freshness is still
//! unchanged. Afterwards:
//!
//! | the pinned artifact | now | the approval is |
//! |---|---|---|
//! | there, with hash A | there, with hash A | unchanged by it |
//! | there, with hash A | there, with hash B | `stale` (`artifact_changed`) |
//! | there | gone | `broken` (`artifact_gone`) |
//! | not there | not there | unchanged by it |
//! | not there | there | `stale` (`artifact_arrived`): the evidence came, look again |
//! | there, under one comparison | there, and the link now declares another | `stale` (`artifact_comparison_changed`): the hashes are not compared |
//! | there or not | cannot be told now (its adapter says so, gives no answer, or was not asked for the link's comparison) | unchanged by it (`artifact_unknown` notice); in `diff`, `now: "unknown"` and `changed: true`, since it is not known to be what was approved |
//!
//! "Cannot be told" is never read as "not there": only the adapter's answer says
//! that nothing is there. `diff` gives each artifact's `now` (`present`, `absent` or
//! `unknown`), and so does the artifact target of `trace`.
//!
//! A name added to or taken from the link since is `links_changed` (`stale`). An
//! `informs` link to an artifact changes no freshness. The same name in two spaces is
//! two artifacts. Every answer that names an artifact gives its `space` beside the
//! name. The name is in `artifact` in a cause, a notice and the declarations of
//! `judge same`; in `path` and, the same, `name` in `check`'s `absent`, `judge
//! approve`'s `absent`, a link target of `get` and `trace`, an origin of `stale` and
//! of `stats`, and an item of `get --approved` and `diff`; and in `name` alone in the `unknown` of
//! `check` and `stale`. A `links_changed` cause of a link to artifacts gives `space` when every
//! name added and removed is in one space (none when the link moved to another).
//!
//! What of an artifact counts is declared per link, with `compare`:
//!
//! ```text
//!   [[book.link]]
//!   name = "tests"
//!   target = "artifact"
//!   space = "file"
//!   means = "depends"
//!   compare = "lines"        # default "bytes": every byte counts; "presence": only that it is there
//!   ignore = ["//"]          # lines that start with these (after white space) do not count
//! ```
//!
//! `lines` compares the lines of the file with their white space collapsed and the
//! blank lines left out, so indentation, spacing and blank lines change nothing, and
//! neither do the lines `ignore` names (`//` takes `//`, `///` and `//!` comments; `#`
//! takes those of TOML, shell or Python). It knows no language: a comment at the end
//! of a line of code still counts, and so does every byte that is not UTF-8 (a file
//! that is no text is better compared as `bytes`). `presence` counts no content: an
//! edit changes nothing, and only an artifact that comes or goes is told. Each pin
//! keeps the comparison its hash was made with. A link whose `compare` is changed
//! afterwards makes the approvals that pinned an artifact through it `stale`
//! (`artifact_comparison_changed`), and the hashes are not compared, since they were
//! made differently: look again and approve. An artifact gone or come is told as
//! such, whatever the comparison.
//!
//! ### Presets and a default
//!
//! A *preset* names values of a link to artifacts (`space`, `compare`, `ignore`,
//! `means`), so that a link writes `preset = "<name>"` and only what differs:
//!
//! ```text
//!   [defaults]
//!   artifact = "file"        # the preset of a link that names neither a preset nor a space
//!
//!   [[preset]]
//!   name = "rust"            # a preset of the project
//!   from = "file"            # optional: the preset it starts from
//!   compare = "lines"
//!   ignore = ["//"]
//!
//!   [[book.link]]
//!   name = "code"
//!   target = "artifact"
//!   preset = "rust"
//!   means = "informs"        # a value the link writes wins over the preset's
//! ```
//!
//! Each adapter offers presets for its own space (the file adapter offers `file`:
//! `space = "file"`, `compare = "bytes"`, `means = "depends"`; the adapter of Rust
//! items offers `rustdoc`: `space = "rust-item"`, `compare = "presence"`,
//! `means = "depends"`; the adapter of Rust tests offers `test`:
//! `space = "rust-test"`, `compare = "bytes"`, `means = "depends"`; the page of
//! `whyshelf_infra::shelf::artifact` lists them).
//! A project declares its own with `[[preset]]`. One of the name of a built-in
//! preset and with no `from` replaces
//! the built-in one whole: it takes none of its values. With `from` of its own name
//! it refines the built-in one instead (`name = "file", from = "file"`); any other
//! `from` names a preset of the project first, then a built-in one. `compare` and
//! `ignore` go together: a link or preset that writes `compare` takes no `ignore`
//! from its preset, and one that writes only `ignore` keeps its preset's `compare`.
//! Without `[defaults]` there is no default, and a link that names no space is
//! refused as above. How the values are taken is stated on
//! [`expand`](domain::shelf::schema::expand): the declaration is expanded before it
//! is checked, and only the expanded values are checked, answered and pinned, so
//! editing a preset changes the next approval and never how an old pin reads (a
//! comparison that changes is `artifact_comparison_changed`, as above). A preset that
//! is not there, a cycle of `from`, a preset named twice, a preset that `replaces`,
//! a preset's `space` that is not offered or `compare` that is none (checked on the
//! preset, whether a link uses it or not), `preset` on a link into a book, and a link
//! left with no `means` are problems of the declaration, told with the others.
//! `map` gives each link to artifacts its `preset` (null when none)
//! and `preset_by_default`, and draws `preset=<name>` (`preset=<name>(default)`).
//!
//! In a Git repository an approval also records, for the approved document, each
//! upstream it pinned and each file, the Git blob its content is in, and the commit
//! `HEAD` names (a hint). Freshness never reads either: it is decided by hashes, so a
//! directory outside Git works the same, with nothing recorded to read back. The
//! records of the ledger written before these were kept read back unchanged, and so
//! do those written before spaces were declared: a pin that names its file by `path`
//! alone is a name in the space `file`.
//!
//! ## An edit that keeps the meaning
//!
//! Any change of a hash makes the approvals that pinned it stale, a typo fixed as
//! much as a rule rewritten: whyshelf does not guess what an edit means. When a person
//! has read an edit and it keeps the meaning, `judge same <ref>` (or `--artifact
//! <name>` for an artifact, in the one space the documents link the name in; a name
//! linked in two spaces is refused) records that, with a reason. The declaration is of the edit
//! from one earlier hash to the hash now: the one the document's own approval saw, or
//! the only one approvals pinned, or the one `--from <hash>` names when they pinned
//! several that do not mean the same (then, unnamed, it is refused with the list). It
//! covers every approval that pinned that hash (its own approval, and every downstream
//! at once); an older pin, from before a change no one declared, stays a change. The
//! approvals it covers hold again: each tells the edit as a notice
//! (`self_same_meaning`, `upstream_same_meaning`, `artifact_same_meaning`) naming the
//! declaration, instead of a cause. Declarations chain (A to B, then B to C), and a
//! later edit is a change again until it is declared too. A declaration is a record of
//! the ledger like a decision: it is read on every run, never stored as a result, and
//! is refused when the document is not valid, the file is not there, or nothing
//! pinned another hash of it. One made by mistake is taken back with `judge withdraw
//! <record id>` (the id each notice names), and the edit is a change again.
//!
//! ## An artifact that moved
//!
//! A file moved (or a Rust item renamed) is, to its approvals, a file gone: the pin
//! of the old name has nothing behind it. When a person knows the two names are one
//! thing, `judge moved <old> <new> --reason <text>` records it, one move per run. It
//! is accepted when the old name is the name some pin of an approval that stands,
//! one that pinned something there, is read as now (through the moves already
//! declared), in one space; when nothing is
//! behind it now; and when the new name has something, as the space's adapter says.
//! A move is of something that was there: a name only a document's link writes, or
//! only pinned as absent, is not known; a name
//! already moved away cannot be moved again (no pin reads as it), so a copy in two
//! steps is refused as a copy in one is (both there), and a move back is accepted.
//! Anything else is refused and writes nothing. A move onto a name that was there
//! all along is not refused: the approvals that pinned that name see what replaced
//! it as a change.
//!
//! From then on, a pin of the old name made at or before the declaration is read as a
//! pin of the new name, with the hash and the state it pinned, and a chain of moves
//! (A to B, then B to C) is followed in order of time; of two moves of one name at
//! one time, the larger record id, as for decisions. So a move that changed nothing
//! leaves the approval as it was; a move with an edit is `artifact_changed` at the new
//! name (stale, not broken, and the change is the new name's, which `stale` names as
//! the origin); a pin of a name that had nothing behind it when it was approved held
//! no content to move, and is read as written, as is a pin made after the
//! declaration. A link that still writes a name a pin was moved
//! away from (the old name, or one a chain passed) is one cause, `artifact_moved`
//! (stale): its `artifact` is the name written and its `read_as` the name to write,
//! and it is no `links_changed`. `check` lists it in `moved` and warns of it on
//! stderr. Write the name it is read as in the link and the approval holds again. A
//! cause, `get --approved` and `diff` show the name pinned and, beside it as
//! `read_as`, the name it is read as; what it is now is asked of that name. A
//! declaration is a record like the others, and `judge withdraw <record id>` takes it
//! back.
//!
//! `moves` finds the names to declare, and declares nothing. It lists every name a
//! `judge moved` could be declared of now (a name some pin of something there, of an
//! approval that stands, is read as through the moves declared) that has nothing
//! behind it, and asks the adapter of its space where it may have gone. For `file`,
//! a file there now with the content pinned comes first (`same_content`, in the
//! hash the pin was made with, among the files Git lists, or outside Git the files
//! under the root but those in a directory or of a name that starts with `.`), then
//! the renames Git's rename detection pairs between the commit the approval was made
//! at and the index (`renamed`, with Git's similarity). The index holds what `git mv`
//! and `git add` put there, so a file moved and edited is found once it is added,
//! and nothing of the work tree is read through a filter. The search for the same
//! content reads only regular files, through a symbolic link only while it stays
//! inside the root (as every reading of a file does), each of at most
//! 16 MiB, and stops at 20,000 files or 512 MiB; a name pins are read as in two
//! spaces is listed with no candidate, as `judge moved` refuses it. What could not be
//! looked at is a `note` of the name. A space that has no way to suggest
//! (`rust-item` and `rust-test`, for now) is listed in `cannot_suggest`, and its names with no
//! candidate.
//!
//! ## What an approval saw
//!
//! `get <ref> --approved` lists what the approval that stands saw (the
//! document, its upstreams and its files), each with its content as it was approved,
//! read back from Git, or why it cannot be: `no_address` (made outside Git, by an
//! older version, or no file was there) or `address_not_readable` (the content was
//! never committed, or is not in this clone). `diff <ref>` sets each beside
//! the file now: the approved and current hash, whether it changed, and a unified
//! line diff when both are text. Both only read; putting a file back as it was is
//! Git's work (`git checkout`) and yours. A release is a Git tag: check it out and
//! `status` says what held then, because freshness is derived, never stored.
//!
//! ## Commands
//!
//! ```text
//!   whyshelf [--root <dir>] <command>
//!
//!   init [--template <name>]  start a project: the declaration and the documents of a
//!        [--sample]           template, `minimal` (one book of notes; the default), `sample`
//!                             (four books and one pending document of each; --sample is a
//!                             shorthand) or `qa-first` (the four, loosened for checks before
//!                             decisions); it writes nothing when one of those files is there
//!                             already
//!   check                     the declaration, the documents and the ledger, what no covering
//!                             link reaches, the names with nothing there and those that cannot
//!                             be told now; exit 1 if anything is wrong (neither of those is;
//!                             for what a covering link does not reach, see `cover` above)
//!   map                       what the declaration says: every book and every link, in its order,
//!                             and a drawing of them; it reads no document and no ledger
//!                             (exit 1, as check, when the declaration cannot be used)
//!   status                    every document: what was decided, and where it stands now
//!   get <ref>                 one document, with where its approval starts to fail
//!   get <ref> --approved      what the approval that stands saw, with its content read back
//!   trace <ref>               what it links to and what links to it, transitively
//!   unlinked [--book <name>] [--no-incoming | --no-outgoing]
//!                             the documents linked neither way (look there first), those no
//!                             link reaches and those that hold none, those decided against
//!                             apart (exit 1, as map does, when the declaration cannot be used;
//!                             otherwise 0, whatever it lists)
//!   stale [--exit-code]       the approvals that no longer hold, strongest first, with their roots,
//!                             and their origins: each document, artifact, cycle or the declaration where
//!                             they start, with the approvals it degrades, and the names
//!                             approvals stand on that cannot be told now (`unknown`)
//!                             (exit 1 with --exit-code when there is any, or when an
//!                             approval stands on a name that cannot be told now)
//!   stats [--since <time>]    how the work went, from the ledger: approvals made again, the time
//!                             documents stayed pending, the runs of judge, and what changed
//!                             under approvals made again
//!   moves                     the pinned names with nothing behind them now, each with the
//!                             documents that pin it and where it may have moved (for files:
//!                             the same content, then Git's renames); it declares nothing
//!   diff <ref>                each item of that approval, as approved and now, with a line diff
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>] [--again]
//!                             approve each; one whose standing approval a new one would
//!                             repeat exactly is left out, unless --again
//!   judge approve --stale --reason <text> [--until <time>] [--actor <name>]
//!                             approve again, in one run and upstream first, every approval
//!                             that is expired or stale and would hold; the rest are left out
//!   judge approve --pending [<ref>...] [--stale] --reason <text> [--until <time>] [--actor <name>]
//!                             approve, in one run and upstream first, every pending document,
//!                             or those named and the pending ones they stand on, that would
//!                             hold; the rest are left out (with --stale, both in one run)
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
//!   judge same <ref>... [--artifact <name>]... [--from <hash>] --reason <text>
//!                             the edits named keep the meaning: the approvals that pinned the
//!                             hashes before them hold again, each with a notice
//!   judge moved <old> <new> --reason <text>
//!                             the artifact at the old name is now at the new one: a pin of the
//!                             old name made before is read as a pin of the new one
//!   judge exempt <ref>... --from <book>.<link> --reason <text>
//!                             the documents are not expected to be reached by that covering
//!                             link: no `not_covered` for it, and `check` lists them apart
//!   judge withdraw <record id>... --reason <text>
//!                             take back decisions or declarations by the id of their record
//!   new <book> [--title <text>] [--key <key>] [<entry>=<value> | <entry>+=<item>]... [--body <text>]
//!                             one new document, valid for its book, at <book dir>/<key>.md
//!   fmt [--check]             every document's file in the one form `new` writes, where that
//!                             changes no reading of it; --check writes nothing, and exits 1
//!                             when a file is not in the form, or would be left alone
//!   schema                    the JSON Schema of the answer of every command
//! ```
//!
//! - A `<ref>` is a key (`REQ-1`), a key in a book (`requirement:REQ-1`), an identity, or the
//!   end of one. It is read once, into one type, before anything runs: a reference
//!   that is not one, a book that is not declared, or a key the book's grammar
//!   refuses is told as such, not as "not found".
//! - `judge` takes several references and decides them in order on one reading of
//!   the project, each on what the ones before it left (so `judge approve REQ-1 DEC-1`
//!   frames DEC-1 on REQ-1's new approval); it writes all of them, or none when one is
//!   refused. The answer is `{"judged": [...], "left_out": [...]}`.
//! - `judge approve` leaves out a document whose approval would only repeat the
//!   one that stands (the rule is on
//!   [`repeated`](domain::shelf::ledger::repeated)): the approval it would make pins
//!   exactly what the standing one pins (the document's hash; each upstream's link,
//!   identity, key and hash; each artifact's link, space, name, comparison, and
//!   whether it was there and its hash), and `--until` gives the expiry it has (none
//!   and none, or the same time, which must still be after now: an expiry already
//!   past is refused, exit 1, as for any approval). Addresses, the revision, who approves and why are
//!   not compared, and freshness is not asked: an approval stale only through an
//!   upstream (one pending, say) is left out too, as a new one would be as stale.
//!   An edit kept by `judge same` is a new hash, so that document is approved. No
//!   record is written for it; it is listed under `left_out` with `"reason":
//!   "repeats"` (it is the repeat that is told, not a freshness), its `detail` and the id of
//!   the approval that stands as `approval`, in the shape `--stale` uses, and
//!   written on stderr as `warning: <key> was not approved again: <detail>; pass
//!   --again to approve it anyway`. Leaving out is no refusal: the others of the run
//!   are judged and written as before, and a run that leaves out every document
//!   writes nothing, exits 0 and asks for no actor (`WHYSHELF_ACTOR` and Git's
//!   `user.name` are read only for a record to write). One whose `depends` link
//!   names an artifact that cannot be told now is refused as before. A denied,
//!   deprecated or retired document, a pending one, and one whose approval was
//!   withdrawn with nothing before it are approved as before; withdrawing a later
//!   approval leaves the earlier one standing, and it is that one that is compared.
//!   `--again` makes the approval anyway, for a person who read the document again
//!   and wants that recorded (it is counted as an approval made again by `stats`).
//!   `--again` cannot be given with `--stale` or `--pending`. In the library,
//!   [`ShelfApp::judge`](app::shelf::ShelfApp::judge) and `judge_all` return a
//!   [`JudgedAll`](app::shelf::JudgedAll), the records written and those left out.
//! - `judge approve --stale` takes no reference (one given beside it is a request
//!   error, exit 2) and needs `--reason`. It approves again, in one such run, every
//!   approval whose standing is `expired` or `stale` and that would hold once
//!   approved, each after every one of them it stands on (the rule is on
//!   [`upstream_first`](domain::shelf::order::upstream_first)); no order of books is
//!   read. A pending document is not touched (it was never approved) unless
//!   `--pending` is given too, below. Every other
//!   approval that does not hold is left out, never in silence, with a `reason`:
//!   `broken` (it is broken now: an artifact pinned there that is gone, an upstream
//!   that is not valid, a cycle), `refused` (the approval would be refused now, as
//!   below: an upstream decided against, an artifact that cannot be told), or
//!   `stands_on` (a `depends` link names a document that is broken, pending, or
//!   itself left out for any reason but `repeats`; a pending one is approved first, with `judge approve`
//!   or in the same run with `--pending`), or
//!   `repeats` (it was stale only through its upstream's upstream, whose hash is new,
//!   and the run approved its upstream again: nothing it pinned changed, so it holds
//!   again with no new approval, as `judge approve` leaves out one that would
//!   repeat; such a document stands, and holds back nothing on it). Each is
//!   listed under `left_out` as `{"doc", "reason", "detail", "upstream", "link",
//!   "upstream_is", "approval", "kinds", "causes"}` (`kinds` and `causes` for
//!   `broken`, `upstream`, `link` and `upstream_is` for `stands_on`, `approval`, the
//!   id of the approval that stands, for `repeats`), and written as a line on
//!   stderr: `warning: <key> is broken and was not approved again (<kinds>)`, or
//!   `warning: <key> was not approved again: <detail>`. A name with nothing there is
//!   pinned as absent, as by any approval, listed in the judgement's `absent`, and
//!   written on stderr as `warning: <key> pins <name> in <space>, which has nothing
//!   behind it`; what is never approved again is an artifact pinned there and gone.
//!   Leaving out is no failure. One `--until` applies to every approval of the run
//!   (an expiry an approval had before is not carried over), and one not after now
//!   is refused, exit 1, even when nothing is stale. The answer is `{"judged":
//!   [...], "left_out": [...]}`, each of `judged` as `judge approve` answers it and
//!   each of `left_out` as above, both with `"rule": "stale"`, and `judged` empty,
//!   with exit 0, when nothing can be approved. Otherwise the exits are those of
//!   `judge`.
//! - `judge approve --pending` needs `--reason`, and approves in the same way, in one
//!   run upstream first, every pending document: a valid document no decision stands
//!   about, never approved or with every decision about it withdrawn (`status` says
//!   `"declared": "pending"` of it). An invalid one (its `presence` there has
//!   `"state": "invalid"`, though it is pending in the ledger) is not taken, nor one
//!   approved, denied, deprecated or retired. The ledger knows no standing
//!   "superseded": a pending document another replaces is pending, and is approved
//!   with its replacement. With references, it takes the documents named, each
//!   pending, and every pending document they stand on through `depends` links,
//!   transitively, as the documents link now (through documents that are not
//!   pending too), and nothing else; a named one that is not pending is a request
//!   error, exit 2, that names it and says what it is. Nothing that is not pending
//!   is approved. Each document is decided on the ledger the earlier ones of the run
//!   left, and so is what holds it back: a document whose `depends` link names one
//!   that is broken, or pending or stale and not in the run, or left out, as it
//!   stands after the approvals made before it in the run, is left out as
//!   `stands_on` (`upstream_is` `broken`, `pending`, `stale` or `left_out`); an
//!   upstream stale only through one the run approved holds nothing back. One whose
//!   approval the model refuses is left out as `refused`. Each is told on stderr as
//!   `warning: <key> was not approved: <detail>`. A pending document has no approval
//!   to repeat, so `repeats` comes only from a stale document in a run with
//!   `--stale`. With `--stale` too, both are one run on one reading, upstream first,
//!   so a pending upstream is approved before the stale document on it, and a stale
//!   upstream again before the pending document on it; each entry of `judged` and
//!   of `left_out` says which rule took it, `"rule": "stale"` or `"pending"`.
//!   `--until` and `--actor` reach every approval of the run, and the answer, the
//!   exits and the time of the records are those of `--stale`; nothing pending is an
//!   empty run.
//! - An approval is refused when the document is not valid, a `depends` link cannot
//!   be followed, it would close a cycle, a `depends` link names an artifact its
//!   adapter cannot tell about now, or a `depends` link names a document people
//!   decided does not stand (denied, deprecated or retired: pin the successor). An
//!   upstream that is only pending is accepted, and the approval is stale from the
//!   start.
//! - `new` takes the book's next key (`V-4` after `V-3`, counting every key a file
//!   of the book writes, valid or not; a file whose front matter cannot be read is
//!   not counted) unless `--key` gives one; a book
//!   of slugs needs `--key`, and a key that is taken is refused. Each `<entry>` is
//!   read by the book's declaration: a list field or a link takes `a,b` (links are
//!   keys), or one item at a time with `<entry>+=<item>`, commas kept and white space
//!   around it left out (after the items of `<entry>=`, in the order given; an empty
//!   item is refused); a truth value
//!   `true` or `false`, a whole number its digits. The document
//!   is checked by the same rules as every file before anything is written (fields,
//!   required fields, enum words, link names and counts, types), with every problem
//!   listed; a link to a key no document has yet is allowed, and listed in
//!   `unresolved_links`. The identity is a new UUID v7.
//! - `init` writes `.whyshelf/books.toml` as its template is written, comments
//!   included, and each document of the template as `new` makes
//!   one: a new identity, the key the template gives, in the one form, pending. The
//!   declaration is read by the reader of every command and checked, and each
//!   document by the rules of `new`, before anything is written. A template that
//!   cannot be read (a declaration that is no TOML declaration, a document whose
//!   front matter cannot be read) is a failure of this program, exit 3; a
//!   declaration template that reads and is not sound is refused as every
//!   declaration that cannot be used is, exit 2, with its problems. Then everything
//!   in the way is looked for: anything at the declaration's path or a document's
//!   (a file, a directory, a link even to nothing), or at a parent of one, inside
//!   the root, that is no directory (a file, a link to nothing or to what is no
//!   directory; a link to a directory is a way through, as for `new`); and, when no
//!   path is in the way, each file of a book that already writes a key the
//!   template gives. Any of it is exit 2, nothing written,
//!   and each is named in `problems` (`<path> is already there`, `<file> already
//!   writes the key <book>:<key>`). It writes no approval, and never over a file.
//!   The declaration is written first, then the documents, and a write that fails
//!   removes the file it made (on Unix only while the file at the path is still
//!   the one it made; elsewhere the file at the path): a run that fails part way leaves, at worst, the
//!   declaration with fewer documents, each whole, and its error lists in
//!   `problems` each file written (`<path> was written`) and the one that was not,
//!   with why; it exits 3 when a write failed, and 2 when a file came to be at a
//!   path after it was looked at. `new` removes a file it made and could not write
//!   too. The answer is `{"declaration": <file>, "documents": [{"key", "path"}],
//!   "next": [...]}`, `next` the commands to run after it, as they are typed, each
//!   with `--root=<dir>` when it ran with one (in single quotes when a shell would
//!   read anything in it); for a root that is no UTF-8 text, `next` is empty and a
//!   line on stderr says why.
//! - `fmt` writes each document's file in the one form `new` writes: a flat front
//!   matter with `id` and `key` first and the other entries by name, every text in
//!   double quotes, and then the body, the whole text normalized as every file is
//!   read (line endings `\n`, no white space at the end of a line, no blank lines
//!   at the end, one final newline), so a body's line endings and trailing white
//!   space are normalized too; the hash is taken after the same rule. A file is
//!   written only when its text in the form reads back as exactly what the file
//!   reads as now (the same entries, the same body), so no hash and no approval
//!   changes; the check is made on every file, on every run. A file already in the
//!   form is not written; `new` writes this form, so what it writes is in it. A
//!   file whose front matter cannot be read, one whose front matter holds a YAML
//!   comment (the form would lose it; the rule is on
//!   `whyshelf_infra::shelf::docs::comment_line`), one whose text in the form would
//!   read otherwise, one the user cannot write, one with more than one link (a hard
//!   link, which a file written in its place would not keep), and one that changed
//!   between the reading and the write are left as they are, each listed with why
//!   and written as a line on stderr, `warning: <file> is left as it is: <why>`;
//!   the others are still written, and `fmt` exits 0. A file is compared with what
//!   was read just before it is replaced, and is replaced whole; an edit saved in
//!   the short time between the two is overwritten, and a file removed in it is
//!   written again. The file written in its place keeps the permissions, not the
//!   owner or extended attributes, and a directory the user cannot write is
//!   found only when the write fails. A run that fails part way leaves the first
//!   files written, each a sound file of its own. `fmt --check` writes nothing, lists the files `fmt`
//!   would write and, apart, those it would leave alone, and exits 1 when there is
//!   any of either: a file left alone is not in the form either.
//! - Only `judge`, `new`, `init` and `fmt` write: `judge` one new file per decision
//!   or declaration, `new` one new document's file, never in place of a file that is
//!   there, `init` the declaration and its documents, never in place of a file that
//!   is there either, and `fmt` over documents' files that are there, as above; every other
//!   command, `fmt --check` among them, only reads and creates nothing. Who judges
//!   is `--actor`, else `WHYSHELF_ACTOR`, else Git's `user.name`; never a committed
//!   file.
//! - None of these commands needs a Git repository. Git is used, when it is there,
//!   to name and read back the content an approval saw and to keep `HEAD` as a hint
//!   of the revision, and for `moves` to list the files of the work tree and the
//!   renames since an approval's commit; its `user.name`, read even outside a
//!   repository, says who judges when nothing else does. No freshness depends on any
//!   of it.
//!
//! ## Output and exit status
//!
//! Every command prints one JSON document on stdout, with a `version` field (now
//! `1`; within a version the shape only grows: new fields, new words in a field of
//! words); `schema` prints the JSON Schema of each, made from the same types.
//! Arguments that are no command (an unknown command, a missing value, none at all)
//! are answered too, with the error of a request and exit 2, and their text on
//! stderr; only `--help` and `--version` answer in text. A
//! document is
//! `{"id": ..., "key": ...}`; its `standing` is `pending`, `denied`, `deprecated`,
//! `retired`, or for an approval `fresh`, `expired`, `stale` or `broken`; its
//! `decision` is the decision that stands (the record's `judgement` id, `actor`, `at`
//! and `reason`, null when none was given), or null while it is pending; a cause has
//! a `kind`, a `severity`, the `upstream` it names (or the `artifact`, a name, with
//! its `space`) and a `detail` sentence; every answer that reads the documents (`check` and
//! `unlinked` when the declaration can be used, `status`, `get`, `trace`, `stale`, `stats`, `moves`, `diff`) carries
//! `health` (files that are no document, cycles, records that contradict one another
//! or cannot be read). The answers of the writes, of `map` and of `schema` carry
//! none.
//!
//! | status | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `check` found something wrong, `map` or `unlinked` was given a declaration that cannot be used, `stale --exit-code` found an approval that does not hold or one that stands on a name that cannot be told now, `fmt --check` found a file not in the form (one it would write, or one it would leave alone), or a rule of the model refused the judgement (nothing was written; for `judge moved`, something still at the old name, nothing at the new one, or a name that cannot be told now; for `judge exempt`, a document whose file is invalid; a document no file carries is a reference that names no document, exit 2, even when an exemption of it stands) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book (in a reference or `unlinked --book`) or a key its grammar refuses, a declaration that cannot be used, `get --approved` / `diff` of a document no approval stands about, a `--successor` that does not take the place of the document deprecated, for `new` a key that is taken, a book of slugs with no `--key`, entries the book refuses, a file already at the path, or a path the range of the book does not take (`include`, `exclude`), for `init` anything at the declaration's path or a document's, a file that already writes a key the template gives, a declaration template that is not sound, or a file that came to be at a path between the look and the write, for `judge same` a name no document links or one linked in two spaces, a `--from` hash nothing pinned, several earlier hashes and none named, or nothing to declare, for `judge moved` two names that are one, an old name no pin of an approval that stands is read as now (one only a link writes, or one already moved away), or pins read as it in two spaces, for `judge exempt` a `--from` that is no `<book>.<link>` or no covering link into the document's book (not declared, declared with no cover, or into another book), a document the link reaches now or one decided against (an exemption unneeded at once), or a document an exemption already exempts from it, and for `judge withdraw` an id that is no decision or declaration, or one already withdrawn |
//! | 3 | the run failed: a file could not be read or written (for `init`, part way too: the files written are listed), or a template `init` ships cannot be read |
//!
//! `check` answers `clean` (whether it exits 0) beside what it found; each of its
//! `books` is `{"name", "dir", "valid", "invalid", "skipped", "skipped_files"}`; its
//! `uncovered` lists each document a covering link does not reach as `{"doc",
//! "book", "link", "severity"}`, the book and link expected to reach it and their
//! strength (`warn` or `error`), and the `not_covered` notice carries the same
//! `severity` and `book`. Its `exempt` lists each exemption that stands in the place
//! of a gap as `{"doc", "book", "link", "severity", "exemption", "actor", "at",
//! "reason"}` (`exemption` is the record id `judge withdraw` takes); its
//! `exemption_unneeded` each exemption no longer needed as `{"doc", "book", "link",
//! "exemption", "actor", "at", "reason", "why", "detail"}`, with `why` one of
//! `reached`, `not_declared`, `no_cover`, `other_book`, `not_valid` or
//! `decided_against` and `detail` the sentence of the stderr line, after
//! `warning: ` and before `; withdraw it`; and its `exemption_repeated` each
//! document two or more exemptions exempt from one link as `{"doc", "book",
//! "link", "exemptions", "detail"}`. A `doc` is `{"id", "key"}`, with a null key
//! for a document that is not valid. The notices `not_covered`, `exempt`,
//! `exemption_unneeded` and `exemption_repeated` carry the `book` and the `link`;
//! `exempt` and `exemption_unneeded` carry the exemption in `declared`, and
//! `exemption_repeated` every one of them; `exemption_unneeded` carries the same
//! `why`, and its `detail` names the document `it`.
//!
//! `map` answers `{"schema_problems": [...], "books": [...], "links": [...],
//! "diagram": ...}`: each book with its `name`, `dir`, `key` (`{"kind": "prefix",
//! "prefix"}` or `{"kind": "slug"}`), `include`, `exclude` and
//! `without_front_matter` as written, or null (see "A book in an existing
//! directory"),
//! `description` and `fields` (`name`, `type`,
//! `words` of an enum, `required`); each link with the book it is `from`, its
//! `name`, its `target` (`{"kind": "book", "book"}` or `{"kind": "artifact",
//! "space", "compare", "ignore"}`), `means`, `count`, `cover` (true for either
//! strength), `cover_severity` (`warn`, `error`, or null when `cover` is false),
//! `preset` (the preset a link to artifacts was expanded from, or null) and
//! `preset_by_default` (whether that preset is the default), as the declaration
//! writes them, its presets expanded, and in its order, with one exception: the
//! `words` of an enum and the prefixes of `ignore` are sets, so they are answered
//! sorted. `diagram` is the same as text, one block per book: a line of its name,
//! key and description, then a line per link with its meaning, `cover` (`cover`, or
//! `cover(warn)` for `warn`), count (`1`, `0..1`, `0..`, `1..`), preset
//! (`preset=<name>`, or `preset=<name>(default)`) and target, every column aligned
//! across the drawing. It
//! is null, and `books` and `links` are empty, when `schema_problems` lists why
//! the declaration cannot be used, as `check` lists it.
//!
//! `unlinked` answers `{"isolated": [...], "schema_problems": [...], "no_incoming":
//! [...], "no_outgoing": [...], "decided_against": {"isolated": [...],
//! "no_incoming": [...], "no_outgoing": [...]}, "health": ...}`, each entry
//! `{"book", "row"}` with the document as `status` shows it. The three lists are
//! disjoint. `isolated` is the place to look first: the documents that hold no link
//! and that no link from a document that stands reaches. `no_incoming`
//! holds those no link reaches, and `no_outgoing` those that hold none. What the
//! declaration makes inevitable is left out, and a document decided against reaches
//! nothing; the rule is on [`Linkage`](domain::shelf::trace::Linkage). The documents
//! decided against (denied, deprecated, retired) are listed in `decided_against`,
//! the pending and approved ones at the top. Lists are in the order the declaration
//! writes the books, then by identity. `--no-incoming` or `--no-outgoing` asks for
//! one of the two one-way lists, and the other is null; `isolated` is always
//! answered. Files that are no document are in `health`, not in a list. When
//! `schema_problems` lists why the declaration cannot be used, as `check` lists it,
//! the lists asked for are empty and `health` is null.
//!
//! `new` answers `{"doc": {"id", "key"}, "file": ..., "unresolved_links": [{"link",
//! "written", "why"}]}`. On 1 (a refusal), 2 and 3 the JSON is `{"error": "refused" |
//! "request" | "failed", "messages": [...], "candidates": [...], "problems": [...]}`
//! (`problems`: each problem of a declaration that cannot be used, or of a new
//! document that would be invalid) and the same chain goes to stderr.
//!
//! `stats` answers `{"since", "approvals": {"docs": [{"doc", "approvals", "again",
//! "revoked"}], "books": [{"book", "docs", "approvals", "again", "revoked"}]},
//! "pending": [{"book", "count", "median_ms", "p90_ms", "max_ms"}], "runs": {"count",
//! "median", "p90", "max", "largest": [{"at", "actor", "size"}]}, "origins":
//! [{"origin", "count"}], "health", "drawing"}`: `since` as given (as `--until`: an
//! RFC 3339 time with an offset or `Z`, or a date `YYYY-MM-DD`, the start of that
//! day in UTC; a time with no offset is refused, exit 2; null without it), an origin as `stale` names one (`{"kind": "doc",
//! "doc"}` or `{"kind": "artifact", "path", "name", "space"}`). `drawing` is the
//! same as text for a person (`jq -r .drawing`), made from the answer alone: the
//! approvals per book, the documents approved again most often, the pending time
//! per book in hours and minutes, the runs and the largest (their times RFC 3339
//! in UTC), and the origins most often under an approval made again, each section
//! in aligned columns and saying so when it has nothing. Each measure, what
//! `--since` keeps and the order of every list are defined on
//! [`stats`](domain::shelf::stats). No measure reads a clock: the times are those of
//! the records and of the identities, so the same ledger and documents always give
//! the same measures. `health` beside them is read as every answer's is.
//!
//! `moves` answers `{"absent": [{"space", "name", "pinned_by": [{"id", "key"}],
//! "candidates": [{"name", "found", "similarity", "present"}], "notes": [{"kind",
//! "detail"}]}], "cannot_suggest": [...], "health", "drawing"}`: the gone names by
//! space and then by name, each with the documents whose approvals pin it; its
//! candidates `same_content` first, then `renamed` from the most alike
//! (`similarity`, a percent, null when none was given), then by name, a candidate
//! that is the name itself or another gone name left out, each a name as written on
//! disk (a move in case only is one); `present` whether something is at the
//! candidate now; `notes` what could not be looked at, each `kind` one of
//! `in_two_spaces`, `no_repository`, `no_revision`, `unknown_revision`,
//! `history_unreadable`, `scan_truncated`, `unreadable_skipped`, `too_large_skipped`,
//! in that order, each once, with a `detail` sentence; `cannot_suggest` the spaces
//! of gone names whose adapter cannot suggest, sorted. `drawing` is the same as text
//! (`jq -r .drawing`). A file or a directory that cannot be read, a history that
//! cannot be read and a bound reached are notes, and the answer is given: exit 0. A
//! declaration that cannot be used is exit 2, as for every reading command, and a
//! port that fails (a ledger or a document that cannot be read, Git that cannot be
//! run or takes longer than 30 s for a listing) exit 3.
//!
//! What Git may and may not do, for `moves` and every command: it runs as the `git`
//! on the path, with no shell and no prompt; it reads the repository's settings and
//! ignore rules; it runs no program a repository's settings name (no file system
//! monitor, no attributes file of the settings or the system, no filter driver, text
//! conversion or external diff: every driver the settings define is shut out on the
//! command line, while Git's own line-ending conversion, which runs no program, is
//! kept, so the address of a text file of a repository that converts line endings
//! reads back); it
//! writes nothing (no object, no refreshed index, no object fetched by a partial
//! clone); and each call is stopped after 30 s.
//!
//! `fmt` answers `{"rewritten": [...], "in_form": <count>, "not_in_form": null,
//! "left_alone": [{"file", "reason", "line", "detail"}]}`, the files by their path
//! under the root; `fmt --check` answers the same with `rewritten` empty and
//! `not_in_form` the files `fmt` would write. `in_form` is a count, not a list: it
//! is most files, and nothing is to be done about them. A `reason` is `unparsed`,
//! `comment` (with its `line` in the file), `reads_otherwise`, `read_only`,
//! `linked` or `changed`. A
//! declaration that cannot be used is exit 2, as for every reading command, and a
//! file that cannot be read or written exit 3.
//!
//! ## For implementers of the ports
//!
//! `ShelfRead` has a port more, `type Moves: MoveSource` with `fn moves()`, which
//! `moves` asks. A type of your own that implements `ShelfRead` does not build
//! until it names one: a breaking change of the library, before 1.0. A
//! `MoveSource` that has no way to suggest answers `SpaceSuggestions::CannotSuggest`
//! for every space.
//!
//! ## Not yet in books
//!
//! Books has no gates, no private documents, and no `find` or tags yet. Every read
//! reads every document.
//!
//! # Crates
//!
//! `whyshelf-core` holds the rules ([`domain`], pure) and the use cases with the
//! ports they need ([`app`]); it depends on no I/O crate. `whyshelf-infra`
//! implements the ports with the file system, Git and the clock. The `whyshelf`
//! command is the `whyshelf-cli` crate, a thin adapter over the same use cases. The
//! picture of the layers is in the documentation of the core crate.
#![warn(missing_docs)]

pub use whyshelf_core::*;

#[cfg(feature = "infra")]
pub use whyshelf_infra as infra;
