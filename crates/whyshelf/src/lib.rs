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
//!   experiment  the four books, for measurements planned before they are run, and
//!               one chain ending in a plan, with the table it expects
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
//! ## Measurements planned first: `experiment`
//!
//! Where a check is a measurement, what it expects is worth fixing before it is
//! run, and the ledger's times of the approvals let a reader see that it was.
//! `whyshelf init --template
//! experiment` writes the four books with the changes for that, each marked
//! `experiment` in `.whyshelf/books.toml`: a decision names the files of its code
//! in `code` (the preset `code`, below), and a verification's `method` may be
//! `plan` or `result` as well as `test` and `review`. A plan names its table of
//! what is expected in `sheet`; a result names the record of its run in `log`,
//! and its plan in `plans`, a `depends` link. `sheet` and `log` take the preset
//! `file`, so every byte of them is pinned, and neither is required: a plan names
//! no log, and a result no sheet. A result checks the decision its plan checks,
//! and names it in `checks` as well. Beside the declaration are F-1, REQ-1 and
//! DEC-1, a chain about a save that is too slow, the plan V-1, whose `sheet` names
//! `measurements/save-time.csv`, and that table, written too, so `check` is clean
//! and names nothing with nothing behind it. There is no result: the run has not
//! happened.
//!
//! The plan and the result are two verifications, approved in order:
//!
//! 1. Before the run, approve the plan: `whyshelf judge approve --pending F-1 V-1
//!    --reason "read, and it holds"`, which `init` gives in its `next`, approves it
//!    with what it stands on, and the ledger records its time.
//! 2. Run the measurement, and write what it gave to a file, say
//!    `measurements/run-1.txt`.
//! 3. Write the result, linking its decision, its plan and the log: `whyshelf new
//!    verification --title "Result: the four sizes timed" method=result
//!    checks+=DEC-1 plans+=V-1 log+=measurements/run-1.txt` makes V-2.
//! 4. Approve it: `whyshelf judge approve V-2 --reason "the run is as planned"`.
//!
//! Edit the table after the plan is approved, and `whyshelf stale` lists V-1 as
//! stale (`artifact_changed`, through `sheet`), and V-2 with it, as it stands on
//! its plan.
//!
//! Which half names what, and the order of the two approvals, are conventions of
//! the template, and `check` does not enforce them: a result may name a `sheet`, a
//! plan a `log`, and `plans` a verification that is no plan, and a result may be
//! approved before its plan, as `plans` pins the plan's document, not its
//! approval. The ledger records the time of each approval, so a reader can see
//! the order; whyshelf does not refuse the other order, and the ledger does not
//! record when the run happened.
//!
//! One verification cannot be both halves: one approval stands per document. The
//! log added to the plan and approved after the run would stand in the place of
//! the approval made before it, and the plan approved again unchanged is left out
//! as `repeats` (see `judge approve`, below).
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
//! A link into a book may also say `cover`: every document of the target book that no
//! decision went against ([`NotStanding`](domain::shelf::ledger::NotStanding)) is
//! expected to be reached by it, from such a document. A project uses it to see a hole in its chain of reasons (a requirement
//! no decision serves, a decision no verification checks). `cover` has a strength:
//! `"error"` (or `true`) or `"warn"`; `false`, or no `cover`, expects nothing, and
//! any other value is a problem of the declaration. A document no covering link
//! reaches has a `not_covered` notice and is listed in `check`'s `uncovered`, each
//! with the strength; one of `error` makes `check` not clean, and one of `warn`
//! leaves it clean, for a hole that may stand a while, and is written by `check` as
//! a line on stderr, `warning: <key> is reached by no <book>.<link> (cover =
//! "warn")` (its identity when it has no key), so that a run which shows the answer only when it fails still shows
//! it. Its freshness is unchanged either way. A link into its own book is met only
//! by another document of that book.
//!
//! A link to artifacts may declare a `cover` over a *selection* of the names of its
//! space: `cover = { strength = "error", include = ["src/**/*.rs"], exclude =
//! ["src/gen/**"] }` (`strength` is `"error"` when left out, `include` holds at least
//! one text, `exclude` is optional). Every name the selection takes that is there
//! now is expected to be reached by a document no decision went against that writes
//! it in that link, and one that none does is a gap of the strength, as a document is.
//! Only a space whose adapter lists its names takes one (each adapter's page says
//! whether it does, and how it reads a selection); for `file`, the regular files Git
//! lists (tracked, or untracked and not ignored), named by their path under the
//! root, with `include` and `exclude` as globs over the path (`*` and `?` within a
//! segment, `**` across segments). A `cover` on a link to a space whose adapter
//! lists nothing, a strength alone (`cover = true`, `cover = "warn"`) and a table on
//! a link into a book are problems of the declaration. It fails closed: a listing
//! that is not the whole (more names than the adapter lists, a name it cannot tell
//! or cannot write, or no listing at all) proves no gap, so `check` of an `error`
//! cover is not clean for it, and for a selection that takes no name; a `warn` cover
//! says either on stderr and leaves `check` clean. `check` answers the gaps in
//! `artifact_uncovered` and, for each covering link to artifacts, its selection and
//! whether every name is listed in `artifact_cover`; `cover` tells it link by link
//! and fails nothing (see its entry below).
//!
//! The rule stays declared for the whole book; one document can be exempt from it.
//! `judge exempt <ref>... --from <book>.<link> --reason <text>` records, by an
//! actor, at a time, for a reason, that the document is not expected to be reached
//! by that link (a finding kept on purpose that grounds no requirement). It is
//! accepted only when it would be needed the moment it is written: the document is
//! valid now, the link is declared with a cover (of either strength) and points
//! into the document's book, the document is not decided against (no cover expects
//! it), and the link does not reach it now (the refusal names the
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
//! the document is of `<book>` ``, `the document is invalid` (or `no file in the
//! books' ranges carries the document`, for a document whose file is gone or that a
//! book's range leaves out, or `whether a file carries the document cannot be
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
//! `judge exempt --artifact <name>... --from <book>.<link> --reason <text>` does the
//! same for a name of a link to artifacts (the space is the link's; not given with
//! document references). It is accepted by the same rule, for an artifact: the link
//! declares a `cover` over the names of a space, the selection takes the name now, no
//! document reaches it, and the names the selection takes can all be listed (one
//! that cannot is refused, as it cannot be told to be needed). The exemption does
//! not move with the file: after `judge moved`, the exemption of the old name is no
//! longer needed (`why` is `not_listed`: the selection does not list it now) and the
//! new name is a gap to reach or exempt. `check` lists them apart in
//! `artifact_exempt`, `artifact_exemption_unneeded` (`why` is `reached`,
//! `not_declared`, `no_cover`, `other_space` or `not_listed`) and
//! `artifact_exemption_repeated`, and warns on stderr as for a document, naming the
//! artifact by its name and space.
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
//! lists its approval as broken, because no file in the books' ranges carries the
//! document. `new`
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
//! cannot be told, it cannot be told. A link to items compares what it says:
//! `presence` (the preset `rustdoc`, below) stands on an item being there, so an
//! edit of an item changes nothing and an item renamed, moved or removed is gone;
//! `lines` or `bytes` (the preset `rustdoc-promise` is `lines`) stand on what the
//! item promises, its declaration and its docs as its page says them (for a
//! variant, its fields too; for a module, a crate, a primitive type or a keyword,
//! its docs only): an edit of a function's body, of an inherent method or of an
//! impl changes nothing of a type, while its declaration as rustdoc writes it lists
//! its public fields, its variants and, for a trait, its items' signatures (a
//! constant's declaration holds its value), so those are part of its promise, and a
//! member named on its own is its own promise; an edit of the declaration or of the
//! docs, docs written or taken away, makes the approval stale (`artifact_changed`).
//! An item whose page cannot be read as that cannot be told for such a comparison,
//! with why, and is still there for `presence`; so is an item whose own page is too
//! large (a member is found by reading its type's page, so a too-large page of the
//! type makes its members cannot-tell for presence too). The docs are read as they are, so docs left
//! by an earlier build, of an item or a crate since gone, read as there: build them
//! afresh before you check or approve (`cargo doc --no-deps`; in this repository
//! `just rust-doc`, which removes the old ones first). A name reached only through
//! a glob re-export of a public module, a member reached only through a type alias,
//! and an item hidden from the docs (`#[doc(hidden)]`) read as absent; a member a
//! type has through `Deref` or a blanket impl reads as there; a raw identifier
//! (`r#type`) is no name. rustdoc's HTML is no contract: after the toolchain is
//! raised, a declaration written another way can make an approval stale. The rules
//! and the limits are on the page of `whyshelf_infra::shelf::artifact::rust_item`.
//! A link to items, and one to what they promise:
//!
//! ```text
//!   [[book.link]]
//!   name = "rustdoc"
//!   target = "artifact"
//!   preset = "rustdoc"       # space = "rust-item", compare = "presence", means = "depends"
//!
//!   [[book.link]]
//!   name = "promised"
//!   target = "artifact"
//!   preset = "rustdoc-promise"   # space = "rust-item", compare = "lines", means = "depends"
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
//! attribute to the end of its body, then every other item of its test part, one
//! on each line, in the order of the file: the part is the outermost inline module
//! on the way that carries a `cfg` holding only under test (`#[cfg(test)]`,
//! `#![cfg(test)]`), or the file when none on the way does, and its items are its
//! helpers, `use`s, constants, types and macro calls, those of the inline modules
//! in it included, every function read as a test, wherever it stands, left out. A
//! module in the part is read through: its name and braces are in no line, and a
//! mark of its depth (`>` a module) and its attributes stand before each of its
//! items, so renaming it, or adding a module of tests only, changes no text and
//! `judge moved --prefix` follows the test, while an item moved into or out of it
//! changes the text. Doc attributes are left out (not those in a macro call's
//! tokens, which are data). So an edit of
//! white space, comments or doc comments changes nothing, an edit of another test
//! in the file changes nothing, any other edit of the function or of an item beside
//! it in its part makes the approvals that name it stale (those of every test of
//! the part), and a test renamed or removed breaks them, save where a module on the
//! way holds an item macro or an attribute that is not inert (below): there a test
//! removed cannot be told, and the approval stays fresh, with a notice that it
//! cannot be told. Nothing outside the test part counts: an edit of the code under
//! test beside inline tests, of a module declared out of line (`mod common;`, whose
//! declaration alone is in the part), of a file `include!` reads, of a fixture, or
//! of an outer attribute of the part or of a module above it (a `cfg` there that
//! stops the part from being compiled among them) leaves it fresh, while an inner
//! `#![cfg(..)]` inside the part is an item beside; and a test in a file no crate
//! compiles reads as there. A file that cannot be read, is too large
//! or does not lex cannot be told, nor can a test under a module declared out of line
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
//! The space `js-test` holds the tests of the project's JavaScript and TypeScript
//! source, each by its file and its titles: `<file>::<title>(::<title>)*`, the
//! file a path of the space `file` ending in `.ts`, `.tsx`, `.mts`, `.cts`, `.js`,
//! `.jsx`, `.mjs` or `.cjs` (everything before the first `::`), then the titles of
//! the `describe`s it is in, outermost first, then the title of the `it` or `test`.
//! A title is any non-empty text, compared with the value of the literal in the
//! source, case included; in it `\:` stands for `:` and `\\` for `\` (so a title
//! holding `::` can be written), and any other `\` is no name. A test call is a
//! call of a `describe` (`describe`, `suite`, `context`, `xcontext`, `xdescribe`,
//! `fdescribe`, or Playwright's `test.describe` with `only`, `skip`, `fixme`,
//! `serial` or `parallel`) or of a test (`it`, `test`, `specify`, `xspecify`, `xit`,
//! `fit`, `xtest`), with
//! any of the members `only`, `skip`, `todo`, `concurrent` and `sequential` (for a
//! test also `fails`, `failing`, `fixme`, `fail` and `slow`), `skipIf(…)` or
//! `runIf(…)` applied, and type arguments (`it<Ctx>(…)`); its title is its first
//! argument when that is a string literal or a template literal with no `${}`.
//! Playwright's annotations (`test.skip()`, `test.slow()`, `test.skip(cond, "why")`,
//! `test.skip(({ browserName }) => …)`, of `test` alone) and settings (`test.use`,
//! `test.describe.configure`) make no test; `test.skip(name)` or `it.skip(name)`
//! is a test whose title is not literal. Type arguments longer than 256 tokens
//! are not stepped over: such a call is not read, and a test not found beside it
//! cannot be told. The tests of
//! a `describe` are those in the brace body of its callback (a function or an
//! arrow), with the blocks of its statements (`if`, `for`, `switch` and its cases,
//! a label, a `namespace`), never the body of a function. Twin `describe`s or tests
//! of a title are all searched, and twin tests are one. Finding them runs no
//! program: the file is read (at most 16 MiB) and lexed by whyshelf's own lexer
//! (comments, strings, templates with `${}`, regular expressions told from division
//! by the token before, JSX in the files that may hold it, brackets nested at most
//! 128 deep), and indexed once however many of its tests are named; no expression
//! is parsed. What is compared is the test call alone: its tokens from the callee
//! to the `)` that closes it, comments left out, one space between tokens (a line
//! end where JavaScript ends a statement there whatever follows: after `return`,
//! `throw`, `break`, `continue`, `yield`, `async`, and before `++` and `--`),
//! strings and templates kept exactly, and the text of a JSX element with its
//! white space at both ends left out and its inner white space kept. So an edit
//! of white space or comments, or of another test, changes nothing, but for the
//! white space inside a JSX text, which is compared (re-indenting a text of several
//! lines makes the call stale); an edit of the call's body, title or modifiers makes the
//! approvals that name it stale, and a test renamed or removed breaks them. Nothing
//! outside the call counts: an edit of a hook (`beforeEach`), a helper, an
//! `import` or a fixture leaves it fresh. A file that cannot be read, is too large
//! or does not lex cannot be told. Titles that are not literal (a variable, a
//! template with `${}`, `describe.each`, `it.each`, `test.each`) and tests made by
//! helpers cannot be read: a test not found where a level on the way (the top of
//! the file or the body of a `describe` named) has such a title, or a call of
//! another function that starts a statement there (`cases.forEach(…)`,
//! `sharedBehaviour()`; not a hook, `vi.…`, `jest.…`, `expect.…`, and not a call
//! inside a test's body), cannot be told, and an approval of it stays fresh with a
//! notice. The text is made,
//! not stored, so `diff` has nothing to set beside it. The rules and the limits are
//! on the page of `whyshelf_infra::shelf::artifact::js_test`. A link to such
//! tests, and a name in it:
//!
//! ```text
//!   [[book.link]]
//!   name = "tests"
//!   target = "artifact"
//!   preset = "js-test"       # space = "js-test", compare = "bytes", means = "depends"
//!
//!   tests: ["tests/cart.test.ts::Cart::adds an item"]
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
//! document no decision went against with nothing there is told, since a wrong name is
//! pinned as absent too: `check` lists it under `absent` (the check stays clean),
//! `judge approve` under each judgement's `absent`, and its row has an
//! `artifact_absent` notice. A name its adapter cannot tell about now is listed under
//! `check`'s `unknown` (the check stays clean) with an `artifact_unknown` notice; an
//! approval that would pin one is refused, and one already pinned changes nothing of
//! the approval while it lasts. `check` also writes such names on stderr, as it does
//! a gap of `warn`, one line for each cause, not for each name: what is mended is
//! the cause (the docs of a crate are built), and names of one space with an equal
//! reason share one (the adapter of `rust-item` gives one reason for each crate
//! whose docs are not built). A cause that holds back one name of one document is
//! `warning: <key> names <name> in <space>, which cannot be told: <why>`; one that
//! holds back more names of one document is `warning: <key> names <n> names in
//! <space>, which cannot be told, each for this reason: <why>; names: <names>`, and
//! of two or more documents `warning: <n> names in <space>, named by <d> documents
//! (<keys>), cannot be told, each for this reason: <why>; names: <names>` (`1 name`
//! when the documents write one name). Here
//! `<names>` is the first three names joined by `, `, followed by `, and <k> more`
//! when there are more, and `<keys>` the keys of the first three documents the same
//! way (a document with no key by its identity), so a line says which documents to
//! look at. The answer on stdout keeps every
//! name. A name with nothing there is a
//! line of its own, `…, which has nothing behind it`. A gate cannot say an approval
//! holds that stands on a name that cannot be told, so `stale` lists each name an
//! approved document pins through a `depends` link that cannot be told now in its
//! `unknown` (`doc`, `link`, `name`, `space`, `why`, as `check`'s), and
//! `stale --exit-code` exits 1 for one too, with the lines of that rule, worded as a
//! pin, on stderr (`warning: <key> pins <name> in <space>, which cannot be told now,
//! …`); its freshness is still unchanged. Afterwards:
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
//! `space = "file"`, `compare = "bytes"`, `means = "depends"`, and `code`:
//! `space = "file"`, `compare = "presence"`, `means = "depends"`; the adapter of Rust
//! items offers `rustdoc`: `space = "rust-item"`, `compare = "presence"`,
//! `means = "depends"`, and `rustdoc-promise`: `space = "rust-item"`,
//! `compare = "lines"`, `means = "depends"`; the adapter of Rust tests offers `test`:
//! `space = "rust-test"`, `compare = "bytes"`, `means = "depends"`; the adapter of
//! JavaScript and TypeScript tests offers `js-test`: `space = "js-test"`,
//! `compare = "bytes"`, `means = "depends"`; the page of
//! `whyshelf_infra::shelf::artifact` lists them).
//!
//! `code` suits a decision's link to the code that carries it out. Under `file`
//! every edit of the code would make the decision stale, though the decision is
//! the same; an `informs` link would notice nothing, even the file removed. Under
//! `code` an edit changes nothing, and a file removed or renamed breaks the
//! decision (`artifact_gone`). `judge moved` follows a rename, and the decision
//! holds again. `moves` can suggest the new name only from Git's renames (one
//! staged with `git mv`, or committed): a presence pin has no content to search
//! for, so after a plain `mv` it gives no candidate, with the note `no_content`.
//! A link of the preset:
//!
//! ```text
//!   [[book.link]]
//!   name = "code"
//!   target = "artifact"
//!   preset = "code"          # space = "file", compare = "presence", means = "depends"
//! ```
//!
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
//! <record id>` (the id each notice names), and the edit is a change again. `stale`
//! lists this declaration, typed, beside the other ways to settle an edit: see
//! [What settles an origin](#what-settles-an-origin).
//!
//! ## Retire a field
//!
//! Taking a field, a link or a link to artifacts out of a book's declaration leaves the
//! documents that wrote it with an entry the book does not declare: `check` tells each
//! as an unknown entry, and a document that tells one is not valid. `status --with` finds
//! the valid documents that hold a value for a field before (not those that write it
//! with nothing in it, nor invalid ones, which `check` tells after), and `fmt --drop`
//! takes the entry out of all of them after, in this order:
//!
//! 1. `status --with <field>` lists the documents that hold a value for the field
//!    (add `--book` for one book). A link is not a field to `--with`: look for the
//!    documents that write it with `check` once the declaration no longer has it.
//! 2. Take the declaration out of `.whyshelf/books.toml`. A document that still writes
//!    the entry (with a value or with nothing in it) is now not valid, and `check`
//!    tells the unknown entry: until the entry is taken out, its approval and those
//!    that stand on it are broken, not stale. Once it is out (step 4), a document
//!    that held a value reads without it, so its hash is another, and its approval
//!    is stale: this is the declaration's doing, not `fmt`'s, and those documents
//!    are approved again (`judge approve --stale`).
//! 3. `fmt --check --drop <name>` lists the files `fmt` would write and, for each, the
//!    names it would take out (`dropped` in the answer). Nothing is written.
//! 4. `fmt --drop <name>` writes them. A name every book still declares is a request
//!    error (exit 2), so a declaration not taken out of every book is noticed here;
//!    where only some books declared it, a declaration not taken out shows as an
//!    empty `dropped`. `id` and `key` cannot be dropped, and a book that still
//!    declares the name keeps its entries.
//! 5. `check`. A file `fmt` leaves alone (a YAML comment in its front matter, a
//!    read-only file, a hard link, a file that changed while `fmt` ran) still tells the
//!    unknown entry, and is edited by hand; `fmt --check --drop <name>` lists it under
//!    `left_alone`, with why.
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
//! back. `stale` lists `moves` and `judge moved` for a file that is gone, with the
//! old name filled in: see [What settles an origin](#what-settles-an-origin).
//!
//! A directory moved or a module renamed moves every name under it, and
//! `judge moved --prefix <old> <new> --reason <text>` declares them at once: the two
//! are prefixes (a directory, `src/a`; a Rust module, `app::m`; a test's file and a
//! module in it, `src/a.rs::m`; a script's file and a title, `tests/cart.test.ts::Cart`),
//! and each name an approval that stands pins, in any space, that is under `<old>` is
//! declared moved to the same name under `<new>`, one declaration for each, so that
//! `judge withdraw` takes one back. Which names are under a prefix, and what each is
//! under the new one, is each space's own grammar of names (a part of a path is never
//! cut, a Rust path breaks at `::`, a title of a test is read as in a name, and one `/`
//! at the end of a directory is as none): the adapter of the space says, and a prefix
//! that is no prefix of a space's names (a directory, for `rust-item`) has none in it.
//! A new name keeps the spelling of what follows the prefix in the old name, and is
//! `<new>` as it is typed. Every old and new name is then held to
//! the rule above in one reading, and the answer is all or none: a name under the
//! prefix that cannot be moved (something still at the old name, nothing at the new
//! one, a new name its space cannot write) is listed, each with why, in `refused`, and
//! nothing is written; so is a prefix nothing under which is pinned. The answer is
//! `{"moved": [...], "not_asked": [...]}`: `moved` as for `judge moved`, and
//! `not_asked` the spaces whose adapter takes no prefix, whose names may be under it
//! and were not moved.
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
//! looked at is a `note` of the name.
//!
//! For `rust-item`, the docs keep no history, so a candidate is a name the built docs
//! have now that ends as the gone one does: of the modules (a directory with an
//! `index.html`) and the items (a page `<kind>.<Name>.html`) of the crates rustdoc
//! wrote, those that end with the most segments in common with it, counted from the
//! last, in each crate (`app::old::m::Foo` has `app::new::m::Foo`, and not
//! `app::other::Foo` when it has the first; a dependency's `http::Error`, which ends alike in two
//! segments with `myapp::http::Error`, does not hide `myapp::net::Error`, which does in
//! one). A member `Type::m` has the members of that name of the types that
//! end as `Type` does and whose page has the anchor of `m` (the most alike in each
//! crate of those, so a type that ends alike in more segments and has no such member
//! does not hide one that has it), and the names that end as the member does,
//! as a whole (a function `m`, say), are candidates as any other, so that a member no
//! type has the anchor of has those only. When an approval pinned content (a
//! preset that compares more than presence, as `rustdoc-promise` does), a candidate
//! whose promise, hashed as the pin was, is the hash pinned is `same_content`; any
//! other is `same_end`, which is the form of a name and no evidence that it is the same
//! thing. At most 16 candidates are kept for a name: `same_content` before `same_end`,
//! and of one rank those in the crate of the gone name before the others, then by
//! name; that order decides only which are kept, for the answer is in the order of
//! candidates, then by name, and `candidates_cut` says how many were found. The bounds
//! below are shared by every gone name, searched one after another; for one, the pages
//! read for content are those of the candidates in its crate first, and a page read
//! already (the page of a member's type) is used whatever the bound, so that a bound
//! spent by the names before can leave a later one's candidate `same_end`, with
//! `scan_truncated`. The search stops at 20,000 names, 64 modules deep, 2,000 pages or
//! 512 MiB of them, and passes over a page of more than 32 MiB (`scan_truncated`,
//! `unreadable_skipped`, `too_large_skipped`); when where the docs are built cannot be
//! told, or they are not built, nothing is searched (`search_unavailable`). A name only
//! a public re-export reaches is not a candidate, a name renamed where it stands
//! (`m::Foo` to `m::Bar`) has none, and the docs are read as they are, so an item of
//! docs an earlier build left is a candidate too. The crates of the gone names are
//! walked first, and the others in the order of their names, so docs built with their
//! dependencies can reach the bound of names before another crate of the project is
//! walked: a candidate in a crate not walked is not found, and `scan_truncated` says
//! the search stopped. A gone name counts toward a move of a prefix (below) only
//! through the candidates kept, at most 16: one whose candidate was cut does not.
//!
//! A candidate carries every move of a prefix that leads from the gone name to it, the
//! widest first: a leading part of each of the two names, or the whole of it, with the
//! same segments after it in both. The two names end with some segments alike, and the
//! segments left out of the parts are some of that end, down to none, when the parts are
//! the whole of both names (`app::old::Foo` to `app::new::Foo` is moved by `app::old` to
//! `app::new` and by the whole name; a candidate that ends alike in no segment has the
//! whole name alone). For `app::x::Foo` and `domain::x::Foo` they are `app` to
//! `domain`, `app::x` to `domain::x` and the whole name, so that a module that keeps its
//! name under another parent is moved by the parent, by itself or by each name in it.
//! When one name ends with all of the other (`app::parser::Token` and `parser::Token`),
//! they are the leading parts that keep one segment of that end on each side
//! (`app::parser` to `parser`) and the whole name.
//! When two gone names or more have candidates carrying one such move, `moves` holds
//! it as `judge moved --prefix <old> <new>` would, against what is there now, and does
//! not try whether the actor is admitted to declare it. A move that stands is in
//! `prefixes`, with the names it moves and `not_asked`, the spaces whose adapter takes
//! no prefix (as for `judge moved --prefix`); one that does not stand (a name under it
//! is still where it was, say) is in `refused_prefixes`, with how many names it cannot
//! move. At most 32 moves are tried, and `untried_prefixes` counts the rest: the ones
//! tried are first the moves a candidate found the best way carries (`same_content`
//! before `same_end`), then those the most gone names have, then by the move. The moves
//! that stand are alternatives, each held alone against what is there now, and not
//! together: two of them may move the same names (`app` to `domain`, and `app::x` to
//! `domain::x` when nothing else is left under `app`; or one prefix to two places), and
//! after one is declared another that moves the same names has nothing under it (or
//! only what the first did not move), as `judge moved --prefix` then holds it.
//!
//! A space that has no way to suggest (`rust-test` and `js-test`) is listed in
//! `cannot_suggest`, and its names with no candidate.
//!
//! ## What settles an origin
//!
//! `stale` gives each origin (where degradations start) a `next`: the commands that
//! can settle it, as they are typed, in the order a person would try them: read
//! what changed (`diff`), then declare (`judge same`, `moves`, `judge moved`), then
//! approve. whyshelf does not guess what an edit means: `next` lists what can be
//! said and says none of it, and whyshelf runs none of it. What a person is to
//! write is a word in `<...>` (a reason, a time, a new name), which a shell reads
//! as one quoted word, so it is replaced and not left. A document is written as
//! `book:KEY`, which no other document has, or by its identity when it has no valid
//! key; an artifact by its name. Each command carries `--root=<dir>` when the run
//! had a `--root`, as the `next` of `init` does; a `--root` that is not UTF-8 text
//! gives an empty `next` and a warning on stderr.
//!
//! What can settle an origin depends on what happened there, and the last column is
//! whether `judge approve --stale` can: the rule is stated once, on the page of
//! [`domain::shelf::settle`], and this is its summary.
//!
//! | what happened | what `next` lists | `judge approve --stale` |
//! |---|---|---|
//! | a document or an artifact was edited | `diff` of an approval that holds the cause, `judge same` of the document or the artifact, and `--stale` | listed |
//! | an artifact came where there was none, or a link names something else, or compares otherwise | `diff`, and `--stale` | listed |
//! | an artifact is gone | `moves`, and `judge moved <name> '<new name>'` with the old name filled in | not listed: it is broken |
//! | an approval lapsed | `judge approve <ref> --until '<time>'` | not listed: it would approve without an expiry |
//! | an upstream has no decision | `judge approve <upstream>` | not listed: approving again changes nothing until it is decided |
//! | an upstream's approval awaits the approvals of other names (see "Who may decide") | `judge approve <upstream> --actor '<another name>'` | not listed: `--stale` holds back what stands on it until another name approves it |
//! | an upstream was replaced, or a link writes a name the artifact moved away from | nothing to type: write the link to the successor, or to the new name | not listed |
//! | an upstream was decided against, a document is not valid, or a cycle | nothing to type: mend the file, or look at the decision again | not listed |
//!
//! `--stale` is listed last, and only when it would take an approval under the
//! origin. Three things to know before using it:
//!
//! - it acts on the whole shelf: every approval that is expired or stale, not only
//!   those under this origin;
//! - it approves with no expiry unless `--until` is given, so an approval that had
//!   one loses it; when an origin holds both an expiry and an edit, `--until` and
//!   `--stale` are both listed, and the second drops what the first keeps;
//! - `judge same` may be refused when approvals pinned several earlier hashes that
//!   do not mean the same: `next` gives it without `--from`, and the refusal lists
//!   the hashes, from which a person picks one and adds `--from <hash>`.
//!
//! An origin that is the declaration (a link to artifacts that now declares another
//! comparison) is no one document or artifact, so nothing is declared of it.
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
//! ## Who may decide
//!
//! A project may name groups of actors and declare, for a book, which groups may
//! write each kind of judgement about its documents:
//!
//! ```toml
//! [[group]]
//! name = "maintainers"
//! members = ["ana", { name = "bo", until = "2026-12-01" }]
//!
//! [decide]                   # the records of no book: moves, the same meaning of an artifact
//! default = ["maintainers"]
//! approvals = 2              # at least the most a book asks for
//!
//! [[book]]
//! name = "requirement"
//! # ...
//! [book.decide]
//! approve = ["maintainers"]  # approve, deny, deprecate, retire, revoke, same, exempt, moved
//! since = "2026-12-01"       # the records made from then on
//! approvals = 2              # different names to approve the same content (1 to 10)
//! ```
//!
//! There is no authentication: the rule is about the name a record writes, taken
//! as declared (`--actor`, `WHYSHELF_ACTOR`, Git's `user.name`), as its time is; who
//! ran the command (`ran_by`) is read by no rule. It
//! checks the ledger against what contributors agreed, and stops no one who writes
//! another name. `judge` refuses a record whose name the rule does not admit, exit 1,
//! and writes nothing; `judge approve --stale` and `--pending` leave such a document
//! out as `not_admitted`. A record in the ledger the rule does not admit (one written
//! by hand, say) is not used, so it makes no decision stand, and `check` lists it
//! under `ledger_problems`, exit 1, until `judge withdraw <id>` by a name the rule
//! admits to `revoke` in its book takes it back. A record made from now on names the
//! book its document is in (its key `book`); one made before is read with the book
//! the document is in now, so a rule declared over an earlier ledger is best given a
//! `since`. A project that declares no rule takes any name, as before. The rules are
//! on [`Authority`](domain::shelf::schema::Authority)'s page, and how a record is
//! read with them on [`Admit`](domain::shelf::ledger::Admit)'s.
//!
//! A rule's `approvals` (1, the default, to 10) is how many different names approve
//! the same content before an approval of its book settles
//! ([`StandingApproval::quorum`](domain::shelf::ledger::StandingApproval::quorum)). A
//! name counts once,
//! and only while its own latest approval of that content has not lapsed, whoever
//! approved last, so a document waits again once any reading it needed lapses (with
//! every reading lapsed, the approval is `expired`, its last reader told alone). Until there
//! are enough, a fresh approval is `awaiting`, listed by `status`, `get` and `check`
//! with `"awaiting": {"readers", "need"}` (and by `check` under `awaiting`, with a
//! `note:` line on stderr, leaving it clean); a degraded one keeps its level and lists
//! the same. `stale` and `stale --exit-code` look at degraded approvals only, so an
//! awaiting document does not fail them, and `--stale` does not take it: another name
//! settles it, with `judge approve <ref>`. What stands on an approval that awaits, at
//! any freshness, an expired one included, is stale through it as on a pending
//! upstream (a cause `upstream_declared`, "unseconded"), and a run of `--stale` or
//! `--pending` holds it back as `stands_on` with `upstream_is` `awaiting`; so a CI
//! that runs `stale --exit-code` fails between the first approval and the last. An
//! edit declared to keep the meaning joins two hashes only once as many different
//! names declared that edit as one of its declarations needs (the `approvals` of the
//! record's book, of `[decide]` for an artifact, over every name of the artifact's
//! way through its moves), counted per edit, never per way; `check` lists one with
//! fewer that a pin of an approval that stands meets under `same_meaning_short`, with
//! a `note:` line, and leaves it clean. `[decide]` asks for at least the most a book
//! asks for, and reads every record a book that asks for more than one name reads (no
//! later `since`). A name that declared an edit already, under any name of the
//! artifact's way, is refused a second declaration of it (`judge same`, exit 2: that
//! name already declared the edit, and it awaits another name): one name counts once. The names
//! are as declared as every name is: one person who writes two names is two names.
//!
//! ## Working from several branches
//!
//! A key is a short name a branch takes at once; the identity is what a document is.
//! Two branches can take the same key, and nothing prevents it: it is found and
//! repaired. whyshelf reads only the shelf it is run on, never another branch or an
//! open pull request, so that `check` holds only what that shelf contains.
//!
//! - Push documents early and apart from the code they are about, so that they are
//!   reviewed and merged quickly. The unit is the documents the books' `cover` rules
//!   of strength `error` ask for together (in the `sample` template, a requirement,
//!   a decision that serves it and a verification that checks it), pending or
//!   approved: less than that fails `check`.
//! - Before starting and before merging, fetch and merge the main branch and run
//!   `check`. Run it in CI on pull requests too, on the result of their merge (the
//!   README, "In CI").
//! - When the main branch took a key the branch took too, renumber the branch's
//!   document, the one the main branch does not hold yet. Which one to renumber is a
//!   person's choice; this is the default. With two files of one key, `check` reports
//!   both as invalid, the main branch's too, until one is renumbered. `new` names a
//!   file after its key, so two runs of `new` that took one key wrote one file, and
//!   Git stops the merge on it: put the main branch's side back in that file and the
//!   branch's side in a file of its own, then renumber the branch's.
//! - Renumbering a document is `rekey <ref>` (name it by its identity or the end of it:
//!   its key is the one two files write). It gives the document the key `new` would take
//!   now (`new`, under Commands; `--key` gives another), puts its file where `new` would,
//!   rewrites its `key:` and the links that mean it, and removes its old file, all or
//!   none. Its identity stays, and its approvals hold, for the reason the document's
//!   hash gives under Books. A link writes a key as text, so after a merge nothing in a
//!   file says which of the two documents it meant. The approval of the document that
//!   wrote the link does (it pinned the identity it stood on), and a person does:
//!   `--follow <ref>`, `--stay <ref>` or `--both <ref>` for a document whose link means
//!   the one that is renumbered, the other, or both. `rekey` decides none by a default:
//!   it lists every document that has neither, and writes nothing. `--dry-run` says
//!   what it would write. The body of a document that names the old key is not changed
//!   (the hash covers it); `mentions` lists those that are valid, and the document
//!   renumbered.
//! - Link only keys the main branch or this branch holds. A link names a key, so a
//!   link to a document of another branch means another document once that one is
//!   renumbered: a pending document says nothing of it, and an approved one is stale
//!   through a `depends` link that changed.
//! - Two branches that approve one document each keep their record, and the later
//!   approval stands. A document whose text conflict was resolved is stale unless it
//!   is the text that approval saw: read it and approve it again.
//!
//! ## Commands
//!
//! ```text
//!   whyshelf [--root <dir>] <command>
//!
//!   init [--template <name>]  start a project: the declaration and the documents of a
//!        [--sample]           template, `minimal` (one book of notes; the default), `sample`
//!                             (four books and one pending document of each; --sample is a
//!                             shorthand), `qa-first` (the four, loosened for checks before
//!                             decisions) or `experiment` (the four, for measurements planned
//!                             first, with the table its plan names); it writes nothing when
//!                             one of those files is there already
//!   check                     the declaration, the documents and the ledger, what no covering
//!                             link reaches, the names with nothing there and those that cannot
//!                             be told now; exit 1 if anything is wrong (neither of those is;
//!                             for what a covering link does not reach, see "Books" above)
//!   cover [--link <book>.<link>]
//!                             what the covering links to artifacts expect and how each stands:
//!                             its selection, whether every name it takes is listed, and which
//!                             names a document reaches, an exemption keeps out of the gaps, or
//!                             are gaps; it fails nothing (`check` is the gate), and exits 1,
//!                             as `map`, only when the declaration cannot be used
//!   map                       what the declaration says: every book and every link, in its order,
//!                             and a drawing of them; it reads no document and no ledger
//!                             (exit 1, as check, when the declaration cannot be used)
//!   status [--book <name>]... [--standing <word>]... [--readers <word>]...
//!          [--where <field>=<value>]... [--without <field>]... [--with <field>]...
//!                             every document, or those that pass the filters: what was
//!                             decided, and where it stands now (the filters are under
//!                             "Output and exit status")
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
//!                             they start, with the approvals it degrades and the commands that can
//!                             settle it (`next`, see "What settles an origin"), and the names
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
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>] [--ran-by <name>] [--again]
//!                             approve each; one whose standing approval a new one would
//!                             repeat exactly is left out, unless --again
//!   judge approve --stale --reason <text> [--until <time>] [--actor <name>] [--ran-by <name>]
//!                             approve again, in one run and upstream first, every approval
//!                             that is expired or stale and would hold; the rest are left out
//!   judge approve --pending [<ref>...] [--stale] --reason <text> [--until <time>] [--actor <name>] [--ran-by <name>]
//!                             approve, in one run and upstream first, every pending document,
//!                             or those named and the pending ones they stand on, that would
//!                             hold; the rest are left out (with --stale, both in one run)
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
//!   judge same <ref>... [--artifact <name>]... [--from <hash>] --reason <text>
//!                             the edits named keep the meaning: the approvals that pinned the
//!                             hashes before them hold again, each with a notice
//!   judge moved [--prefix] <old> <new> --reason <text>
//!                             the artifact at the old name is now at the new one: a pin of the
//!                             old name made before is read as a pin of the new one (with
//!                             --prefix, every name under the old prefix, in any space, is
//!                             now at the same name under the new one)
//!   judge exempt <ref>... --from <book>.<link> --reason <text>
//!   judge exempt --artifact <name>... --from <book>.<link> --reason <text>
//!                             the documents, or the artifacts, are not expected to be reached
//!                             by that covering link: no `not_covered` for it, and `check`
//!                             lists them apart
//!   judge withdraw <record id>... --reason <text>
//!                             take back decisions or declarations by the id of their record
//!   new <book> [--title <text>] [--key <key>] [<entry>=<value> | <entry>+=<item>]... [--body <text>]
//!                             one new document, valid for its book, at <book dir>/<key>.md
//!   fmt [--check] [--drop <name>]...
//!                             every document's file in the one form `new` writes, where that
//!                             changes no reading of it; --check writes nothing, and exits 1
//!                             when a file is not in the form, or would be left alone;
//!                             --drop also takes out the entry of that name where the
//!                             file's book declares none (see "Retire a field")
//!   rekey <ref> [--key <key>] [--follow <ref>]... [--stay <ref>]... [--both <ref>]... [--dry-run]
//!                             give one document a new key (the book's next, or --key), put its
//!                             file where `new` would, and rewrite the links that mean it; each
//!                             document that writes the old key in a link is decided by its
//!                             approval or by a --follow, --stay or --both, and one that is
//!                             neither is refused with every such one listed (nothing is
//!                             written); --dry-run writes nothing
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
//!   reading of the one who approves (the rule is on
//!   [`ApprovalKind`](domain::shelf::ledger::ApprovalKind)): the approval it would
//!   make pins exactly what the standing one pins (the document's hash; each
//!   upstream's link, identity, key and hash; each artifact's link, space, name,
//!   comparison, and whether it was there and its hash), the actor is one of those
//!   who approved that content since anything else was decided about the document
//!   ([`Readers`](domain::shelf::ledger::Readers)), and `--until` gives the expiry
//!   both that actor's latest approval of it and the approval that stands have,
//!   which has not passed (none and none, or the same time, which must still be
//!   after now: an expiry already past is refused, exit 1, as for any approval).
//!   Once the approval that stands lapsed, anyone's approval of it approves it again. Addresses, the revision and why are not
//!   compared, and freshness is not asked: an approval stale only through an
//!   upstream (one pending, say) is left out too, as a new one would be as stale.
//!   An edit kept by `judge same` is a new hash, so that document is approved.
//!   Another actor's approval of the same content with the expiry of the approval
//!   that stands is a second reading, written in every project; one with another
//!   expiry moves when the document lapses and is an approval made again, as is an
//!   actor's whose own approval of it expired.
//!   No record is written for one that repeats; it is listed under `left_out` with
//!   `"reason": "repeats"` (it is the repeat that is told, not a freshness), its
//!   `detail` and the id of the approval that stands as `approval`, in the shape
//!   `--stale` uses, and written on stderr as `warning: <key> was not approved again:
//!   <detail>; pass --again to approve it anyway`. Leaving out is no refusal: the
//!   others of the run are judged and written as before, and a run that leaves out
//!   every document writes nothing and exits 0. The actor (`--actor`, else
//!   `WHYSHELF_ACTOR`, else Git's `user.name`), and who ran the command (`--ran-by`,
//!   else `WHYSHELF_RAN_BY`) with it, is asked once per run, after every
//!   reference resolves and every expiry is after now, and before any document is
//!   decided, since what an approval is depends on who makes it: with no name the
//!   run fails (exit 3), even one that would only leave out repeats, which exited 0
//!   before second readings were written. One whose `depends` link names an
//!   artifact that cannot be told now is refused as before. A document decided
//!   against, a pending one, and one whose approval was withdrawn with
//!   nothing before it are approved as before; withdrawing a later approval leaves
//!   the earlier one standing, and it is that one that is compared. `--again` makes
//!   the approval anyway, for a person who read the document again and wants that
//!   recorded (it is counted as an approval made again by `stats`; a second reading
//!   is not). `--again` cannot be given with `--stale` or `--pending`. In the library,
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
//!   `stands_on` (a `depends` link names a document that is broken, pending, whose
//!   approval awaits other names (see "Who may decide"), or
//!   itself left out for any reason but `repeats` or `second_reading`; a pending one is approved first, with `judge approve`
//!   or in the same run with `--pending`), or
//!   `repeats` (it was stale only through its upstream's upstream, whose hash is new,
//!   and the run approved its upstream again: nothing it pinned changed, so it holds
//!   again with no new approval, as `judge approve` leaves out one that would
//!   repeat; such a document stands, and holds back nothing on it), or
//!   `second_reading` (an approval now would be of the content that stands by an
//!   actor who has not approved it, as when another person's run takes one stale
//!   only through its upstream's upstream; an expired approval is approved again
//!   by anyone's run: a run that chooses
//!   its documents by a rule did not read them, so it writes no second reading,
//!   which is made by naming the document with `judge approve`; such a document
//!   stands too, and holds back nothing on it), or `not_admitted` (who may decide
//!   does not admit the actor to approve in the document's book, see "Who may
//!   decide"; the others are approved, and what stands on it is held back as
//!   `left_out`; `--pending` leaves one out the same way). Each is
//!   listed under `left_out` as `{"doc", "reason", "detail", "upstream", "link",
//!   "upstream_is", "approval", "kinds", "causes"}` (`kinds` and `causes` for
//!   `broken`, `upstream`, `link` and `upstream_is` for `stands_on`, `approval`, the
//!   id of the approval that stands, for `repeats` and `second_reading`), and written as a line on
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
//!   approved or decided against. The ledger knows no standing
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
//!   `stands_on` (`upstream_is` `broken`, `pending`, `stale`, `awaiting` or `left_out`); an
//!   upstream stale only through one the run approved holds nothing back. One whose
//!   approval the model refuses is left out as `refused`. Each is told on stderr as
//!   `warning: <key> was not approved: <detail>`. A pending document has no approval
//!   to repeat or read again, so `repeats` and `second_reading` come only from a
//!   stale document in a run with `--stale`. With `--stale` too, both are one run on one reading, upstream first,
//!   so a pending upstream is approved before the stale document on it, and a stale
//!   upstream again before the pending document on it; each entry of `judged` and
//!   of `left_out` says which rule took it, `"rule": "stale"` or `"pending"`.
//!   `--until`, `--actor` and `--ran-by` reach every approval of the run, and the answer, the
//!   exits and the time of the records are those of `--stale`; nothing pending is an
//!   empty run.
//! - An approval is refused when the document is not valid, a `depends` link cannot
//!   be followed, it would close a cycle, a `depends` link names an artifact its
//!   adapter cannot tell about now, or a `depends` link names a document people
//!   decided against (pin the successor of a deprecated one). An
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
//!   included, each other file of the template as it is (the table of
//!   `experiment`, which its plan names), and each document of the template as `new` makes
//!   one: a new identity, the key the template gives, in the one form, pending. The
//!   declaration is read by the reader of every command and checked, and each
//!   document by the rules of `new`, before anything is written. A template that
//!   cannot be read (a declaration that is no TOML declaration, a document whose
//!   front matter cannot be read) is a failure of this program, exit 3; a
//!   declaration template that reads and is not sound is refused as every
//!   declaration that cannot be used is, exit 2, with its problems. Then everything
//!   in the way is looked for: anything at the declaration's path, another file's
//!   or a document's (a file, a directory, a link even to nothing), or at a parent of one, inside
//!   the root, that is no directory (a file, a link to nothing or to what is no
//!   directory; a link to a directory is a way through, as for `new`); and, when no
//!   path is in the way, each file of a book that already writes a key the
//!   template gives. Any of it is exit 2, nothing written,
//!   and each is named in `problems` (`<path> is already there`, `<file> already
//!   writes the key <book>:<key>`). It writes no approval, and never over a file.
//!   The declaration is written first, then the other files, then the documents, and a write that fails
//!   removes the file it made (on Unix only while the file at the path is still
//!   the one it made; elsewhere the file at the path): a run that fails part way leaves, at worst, the
//!   declaration with fewer files, each whole, and its error lists in
//!   `problems` each file written (`<path> was written`) and the one that was not,
//!   with why; it exits 3 when a write failed, and 2 when a file came to be at a
//!   path after it was looked at, but 3 whatever stopped it when the files written
//!   could not be put on disk either (a further line in `problems` says why). The
//!   files a run of `init` or `new` writes are put on disk together when it ends;
//!   when that fails, they are left whole and each is listed, and it exits 3. `new`
//!   removes a file it made and could not write
//!   too. The answer is `{"declaration": <file>, "files": [<file>], "documents":
//!   [{"key", "path"}], "next": [...]}`, `files` the other files written (none but
//!   for `experiment`), `next` the commands to run after it, as they are typed, each
//!   with `--root=<dir>` when it ran with one (in single quotes when a shell would
//!   read anything in it); for a root that is no UTF-8 text, `next` is empty and a
//!   line on stderr says why.
//! - `fmt` writes each document's file in the one form `new` writes: a flat front
//!   matter with `id` and `key` first and the other entries by name, every text in
//!   double quotes, and then the body, the whole text normalized as every file is
//!   read (line endings `\n`, no white space at the end of a line, no blank lines
//!   at the end, one final newline), so a body's line endings and trailing white
//!   space are normalized too; the hash is taken after the same rule. The form
//!   leaves out an entry written with nothing in it that the build reads as not
//!   written (an optional field, or a link whose count admits none), by the rule
//!   `new` writes with ([`domain::shelf::build::Shelf::form_entries`]); every other
//!   entry is kept as it is written, except an entry of a name given to `--drop`
//!   (below). A file is written only when its text in the
//!   form reads back as what the file reads as now without those entries (the
//!   same other entries, the same body), so no hash or approval changes, and
//!   no problem does but the unknown entry of a name given to `--drop`, which goes
//!   (a document that told only those is valid afterwards); the check is made on
//!   every file, on every run. A file already in the
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
//! - `fmt --drop <name>`, repeated for each name, is the second half of retiring a
//!   field, a link or a link to artifacts (see "Retire a field" below): it takes the
//!   entry of that name out of every file whose book declares nothing of that name
//!   (no field, link or link to artifacts), whatever the entry holds; an entry of the
//!   name in a book that declares it stays, and so does every entry no `--drop` names. It
//!   is also the dry run when given with `--check`, which lists, for each file it
//!   would write, the names it would take out. Every other rule of `fmt` holds: a file
//!   with a comment, a read-only file and a hard link are left alone, and the entry
//!   stays in them. The names are checked against the declaration before anything is
//!   read or written: an empty name is an argument error, and `id`, `key` and a name
//!   every book declares are request errors (exit 2), each told, nothing written.
//!   The last is so that a name still declared everywhere is noticed.
//! - `rekey <ref>` gives one document a new key (the reasons and how a link is read are
//!   under "Working from several branches"). The document is one file with an identity
//!   and a key that read; the new key is `--key` (free, as for `new`) or the book's
//!   next, and its file `<book dir>/<key>.md`. The documents that write the old key are
//!   every file whose `key:` reads in the book's grammar, valid or not and whatever its
//!   `id:`; the documents that write it in a link are every file that has it in a link,
//!   valid or not. A file that could not be read at all and may write the key (any file
//!   of the book of the key), or link to it (any file of a book that declares a link
//!   into that book, and a link entry that is no list), stops `rekey` before it writes
//!   anything, and is listed: a file that still holds a merge's conflict markers is one.
//!   The document's file and the files whose links are rewritten are written in the one
//!   form `fmt` writes, by the rule of `fmt`: one it would leave alone (a comment in its
//!   front matter, read-only, a hard link) is refused with why. Before anything is
//!   written the files are read back as the adapter says they read, built as the books
//!   are built, and held against the plan: the document is one file with the new key,
//!   each link is where its word puts it, and no file written has a problem it did not
//!   have (a `--both` on a link whose count admits one key is one); any difference is
//!   listed and nothing is written. Then the document's file is made, the others written
//!   over (compared with what was read, as `fmt` does, and whole), those put on disk, the
//!   old file removed, and that put on disk. A stop before the removal (a file in the
//!   way, one no longer what was read, a port that failed) puts back what was written, in
//!   reverse, and gives its cause as it is; when that cannot be done, or cannot be put on
//!   disk, the files not put back are listed and the exit is 3. A removal that cannot be
//!   put on disk leaves what was written, each file whole, lists it, and exits 3. The
//!   ledger is not written: an approval pinned the identity of what it stood on, and an
//!   identity stays. A power loss between the write and the removal leaves one identity
//!   in two files, which `check` tells.
//! - Only `judge`, `new`, `init`, `fmt` and `rekey` write: `judge` one new file per decision
//!   or declaration, `new` one new document's file, never in place of a file that is
//!   there, `init` the declaration, its other files and its documents, never in place of a file that
//!   is there either, `fmt` over documents' files that are there, as above, and `rekey`
//!   one new file, documents' files that are there written over, and one removed, as
//!   above; every other
//!   command, `fmt --check` and `rekey --dry-run` among them, only reads and creates nothing. Who judges
//!   is `--actor`, else `WHYSHELF_ACTOR`, else Git's `user.name`; never a committed
//!   file.
//! - Who ran the command is told beside who judges, when it is another: a program (a
//!   coding agent, a script) may run `judge` for a person who answers for it, and the
//!   record then names both, the person as `actor` and the program as `ran_by`. Every
//!   `judge` verb takes `--ran-by <name>`; without it `WHYSHELF_RAN_BY` names one (a
//!   blank value is none), else no one: never Git's `user.name`, which names the person
//!   whose checkout it is whoever runs the command in it, and never a committed file.
//!   A name that is the actor's is one name: no `ran_by` is written, and the answers
//!   say `null`. A record has the key only when there is one, so every other record
//!   keeps the form, and the hash, it has always had; a whyshelf that does not know
//!   the key refuses such a record, and its `check` lists it under
//!   `unreadable_records`; its other commands act as if the record were not there,
//!   so its `judge` can write over that decision with no warning. A stored record whose `ran_by` is its `actor` is refused
//!   in the same way, never read as one name. It is told and nothing more: who may decide, who has read a
//!   document, what an approval would repeat, what an actor declared and the runs
//!   `stats` counts are all read from the actor alone. Like the actor, it is as
//!   declared, and whyshelf cannot tell a variable a committed file set (a direnv
//!   `.envrc`, say) from one a person set: such a file can name either.
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
//! `retired`, or for an approval `fresh`, `awaiting`, `expired`, `stale` or `broken`; its
//! `decision` is the decision that stands (the record's `judgement` id, `actor`,
//! `ran_by` (null when the actor ran the command), `at` and `reason`, null when none
//! was given), or null while it is pending; `get --approved` shows `actor` and
//! `ran_by` of the approval in the same way; a cause has
//! a `kind`, a `severity`, the `upstream` it names (or the `artifact`, a name, with
//! its `space`) and a `detail` sentence; every answer that reads the documents (`check`, `cover` and
//! `unlinked` when the declaration can be used, `status`, `get`, `trace`, `stale`, `stats`, `moves`, `diff`) carries
//! `health` (files that are no document, cycles, records that contradict one another
//! or cannot be read). The answers of the writes, of `map` and of `schema` carry
//! none.
//!
//! | status | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `check` found something wrong, `map`, `cover` or `unlinked` was given a declaration that cannot be used, `stale --exit-code` found an approval that does not hold or one that stands on a name that cannot be told now, `fmt --check` found a file not in the form (one it would write, or one it would leave alone), or a rule of the model refused the judgement (nothing was written; for `judge moved`, something still at the old name, nothing at the new one, or a name that cannot be told now (with `--prefix`, any name under the prefix of that kind, each listed in `refused`, and a name an adapter gave no answer for); for `judge exempt`, a document whose file is invalid, or an artifact when the names its link's selection takes cannot all be listed; a document no file carries is a reference that names no document, exit 2, even when an exemption of it stands; for `rekey`, files that could not be read and may write the key or link to it, a file whose identity cannot be told, a file `fmt` would leave alone, or files that would not read as planned, a file changed since the first reading among them, each listed in `problems`) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book (in a reference, `unlinked --book` or `status --book`) or a key its grammar refuses, a `status` filter that is no request (a `--standing` that is no standing, a `--readers` that is no count of readers, a `--where` with no `=` or with an empty value, a field no book declares, a value no book that declares the field reads, a field asked with `--where` or `--with` and with `--without`, or a `--without` of a field that every book declaring it requires, of the books asked when any are and one of them declares it), an `fmt --drop` with an empty name, with `id` or `key`, or with a name every book declares, a declaration that cannot be used, `get --approved` / `diff` of a document no approval stands about, a `--successor` that does not take the place of the document deprecated, for `new` a key that is taken, a book of slugs with no `--key`, entries the book refuses, a file already at the path, or a path the range of the book does not take (`include`, `exclude`), for `init` anything at the declaration's path, another file's of the template or a document's, a file that already writes a key the template gives, a declaration template that is not sound, or a file that came to be at a path between the look and the write, for `judge same` a name no document links or one linked in two spaces, a `--from` hash nothing pinned, several earlier hashes and none named, nothing to declare, or an edit the actor already declared, which awaits another name, for `judge moved` two names that are one, an old name no pin of an approval that stands is read as now (one only a link writes, or one already moved away), or pins read as it in two spaces (with `--prefix`, two prefixes that are one, a prefix no pinned name of any space is under, or only names no space can write under the new prefix, each listed in `refused`), for `judge exempt` a `--from` that is no `<book>.<link>` or no covering link into the document's book (not declared, declared with no cover, or into another book), a document the link reaches now or one decided against (an exemption unneeded at once), or a document an exemption already exempts from it (with `--artifact`: a `--from` that is no link to artifacts with a `cover`, a name its selection does not take now, or one a document reaches or already exempt, and, with `--artifact` together with document references, bad arguments), for `cover` a `--link` that is no covering link to artifacts, for `judge withdraw` an id that is no decision or declaration, or one already withdrawn, and for `rekey` a document no file carries or that is not one file with an identity and a key that read, a new key that is taken or that the book refuses, a book with no next key, a new file the range of the book does not take, a word for a document that writes no key in a link or two words for one, a document that writes the old key in a link and that no approval and no word decides, or whose word is against its approval (every one is listed in `problems`), a file already at the new file, or a file that is no longer what was read when it is written over (one changed before, between the two readings, is exit 1) |
//! | 3 | the run failed: a file could not be read, written or put on disk (for `new`, `init` and `rekey`, part way too: the files written, or for `rekey` those not put back, are listed), or a template `init` ships cannot be read |
//!
//! `status` answers `{"docs", "filters", "health"}`: the documents that pass the
//! filters (all of them when none is given), the filters as given (`books` by
//! name and each once, `standings` in a fixed order, `where` as `{"field",
//! "value"}` in the order given, and `without` and `with` as the field names in the
//! order given, each empty when none was; `readers` by word, in a fixed order), and
//! the `health` of the whole project, not of the documents listed. A row of an
//! approved document
//! also carries `readers`: `{"count", "counted", "lapsed"}`, the names that approved
//! the content it stands on now, in every project whether or not it declares a rule
//! (`count` is `nobody`, `one` or `several`; `counted` are the names whose own latest
//! approval of that content has not lapsed, `lapsed` those whose has, so that an
//! approval every reading of which lapsed has none counted); a row with no approval
//! has none. The readers are those of the content the approval stands on, so a
//! stale row keeps the readers of what was approved (`--standing fresh` with
//! `--readers` asks what is read now), and `awaiting.readers` still names the last
//! reader when `readers` counts nobody. The filters are:
//!
//! - `--book <name>`: the documents of this book;
//! - `--standing <word>`: the documents at this standing, in the words of a
//!   document's `standing` in the answers (above);
//! - `--readers <word>`: the approved documents read by this many names now, in the
//!   words of `readers.count` (`nobody`, `one`, `several`), as the answers show them;
//!   `--readers one` lists what no second name reads now (one whose reading lapsed
//!   is `lapsed`, not counted), and does not list an approval
//!   every reading of which lapsed (that is `--readers nobody`). A document with no
//!   approval passes none. A name is the actor declared, so one person who gives two
//!   names reads as two;
//! - `--where <field>=<value>`: the documents whose book declares the field and
//!   whose value equals this one; for a list field, whose list holds it as an item.
//!   The text after the first `=` is the value, read as the type of the field.
//! - `--without <field>`: the documents whose book declares the field and that
//!   hold no value for it (left out, or written with nothing in it).
//! - `--with <field>`: the documents whose book declares the field and that hold a
//!   value for it, whichever (written, with something in it). A field every book
//!   that declares it requires is answered: every valid document of those books has
//!   a value.
//!
//! Each may be repeated. A document must pass every kind given; of one kind
//! repeated, it passes by any one (`--book a --book b --standing stale` is the stale
//! documents of `a` or `b`), except that each `--where`, each `--without` and each
//! `--with` is a condition it must hold. A book or a field that is not declared, a
//! value no book that declares the field reads as its type, an empty value, a field
//! asked with `--where` or `--with` and with `--without`, a `--without` of a
//! field that every book declaring it requires (of the books asked, when any are and
//! one of them declares it), and a `--standing` or a `--readers` that is no word of
//! theirs are a request error
//! (exit 2) that lists every problem. A document that is not valid passes the
//! `--book` of any of its files and never a `--where`, a `--without` or a `--with`. The rule is on the page of [`domain::shelf::filter`].
//!
//! `check` answers `clean` (whether it exits 0) beside what it found; each of its
//! `books` is `{"name", "dir", "valid", "invalid", "skipped", "skipped_files"}`; its
//! `uncovered` lists each document a covering link does not reach as `{"doc",
//! "book", "link", "severity"}`, the book and link expected to reach it and their
//! strength (`warn` or `error`), and the `not_covered` notice carries the same
//! `severity` and `book`. Its `exempt` lists each exemption that stands in the place
//! of a gap as `{"doc", "book", "link", "severity", "exemption", "actor", "ran_by",
//! "at", "reason"}` (`exemption` is the record id `judge withdraw` takes); its
//! `exemption_unneeded` each exemption no longer needed as `{"doc", "book", "link",
//! "exemption", "actor", "ran_by", "at", "reason", "why", "detail"}`, with `why` one of
//! `reached`, `not_declared`, `no_cover`, `other_book`, `not_valid` or
//! `decided_against` and `detail` the sentence of the stderr line, after
//! `warning: ` and before `; withdraw it`; and its `exemption_repeated` each
//! document two or more exemptions exempt from one link as `{"doc", "book",
//! "link", "exemptions", "detail"}`. A `doc` is `{"id", "key"}`, with a null key
//! for a document that is not valid. The notices `not_covered`, `exempt`,
//! `exemption_unneeded` and `exemption_repeated` carry the `book` and the `link`;
//! `exempt` and `exemption_unneeded` carry the exemption in `declared`, and
//! `exemption_repeated` every one of them; `exemption_unneeded` carries the same
//! `why`, and its `detail` names the document `it`. `check`'s `collisions` (left
//! out when there is none) lists each key that two or more files write, once, as
//! `{"key", "files": [{"path", "id"}], "detail"}`, every file and its identity
//! (null when it cannot be read), without saying which to renumber.
//!
//! Beside them `check` answers, for the covering links to artifacts, five more lists
//! (the answer's version stays 1: fields are only added). `artifact_uncovered` lists
//! each name nobody reaches as `{"book", "link", "name", "space", "severity"}`;
//! `artifact_exempt`, `artifact_exemption_unneeded` and `artifact_exemption_repeated`
//! list the exemptions of artifacts as `exempt`, `exemption_unneeded` and
//! `exemption_repeated` list those of documents, with `name` and `space` in the place
//! of `doc` (and, for `artifact_exemption_unneeded`, `why` one of `reached`,
//! `not_declared`, `no_cover`, `other_space` or `not_listed`); and `artifact_cover`
//! lists each covering link as `{"book", "link", "space", "severity", "selection":
//! {"include", "exclude"}, "completeness", "why", "selects_nothing", "selected",
//! "reached", "exempt", "gaps", "detail"}`, with `completeness` `all`, `partial`,
//! `cannot_list` or `not_asked` (`why` says why when it is not `all`) and the counts
//! null unless it is `all`. A gap is told only of a listing that is `all`. `clean` is
//! false for a gap of an `error` link, for such a link whose `completeness` is not
//! `all`, and for such a link that `selects_nothing`. On stderr, for a `warn` link,
//! `check` writes a line for each name nobody reaches (`warning: <name> in <space> is
//! reached by no <book>.<link> (cover = "warn")`, the name and the space each in
//! backticks), one for each such link whose listing is not `all` or that selects
//! nothing, and one for each exemption of an artifact no longer needed (with its
//! `judge withdraw`) or repeated, as it does for documents.
//!
//! `cover` answers `{"schema_problems": [...], "links": [...], "uncovered": [...],
//! "exempt": [...], "exemption_unneeded": [...], "exemption_repeated": [...],
//! "health": ...}`: each link is the entry of `artifact_cover` with `names`, each
//! name the link takes (when `completeness` is `all`) as `{"name", "stands",
//! "exemption"}` with `stands` `reached`, `exempt` (and the record id) or `gap`; the
//! lists are those of `check`, of the link asked for with `--link` or of every one.
//! A `--link` that is no covering link to artifacts is a request error (exit 2).
//! `judge exempt --artifact` answers each exemption with `artifact` and `space` in
//! the place of `doc`, null for the one that is not.
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
//! strength, of a link into a book or to artifacts), `cover_severity` (`warn`,
//! `error`, or null when `cover` is false), `selection` (`{"include", "exclude"}` of
//! a link to artifacts that covers, as written; null for any other),
//! `preset` (the preset a link to artifacts was expanded from, or null) and
//! `preset_by_default` (whether that preset is the default), as the declaration
//! writes them, its presets expanded, and in its order, with one exception: the
//! `words` of an enum and the prefixes of `ignore` are sets, so they are answered
//! sorted. `diagram` is the same as text, one block per book: a line of its name,
//! key and description, then a line per link with its meaning, `cover` (`cover`, or
//! `cover(warn)` for `warn`), count (`1`, `0..1`, `0..`, `1..`), preset
//! (`preset=<name>`, or `preset=<name>(default)`) and target (for a covering link to
//! artifacts, ending in `include="..."` and `exclude="..."`), every column aligned
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
//! decided against ([`NotStanding`](domain::shelf::ledger::NotStanding)) are listed
//! in `decided_against`, the pending and approved ones at the top. Lists are in the order the declaration
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
//! "detail"}]}], "cannot_suggest": [...], "prefixes": [{"from", "to", "moved":
//! [{"space", "name"}], "not_asked": [...]}], "refused_prefixes": [{"from", "to",
//! "refused"}],
//! "untried_prefixes", "health", "drawing"}`: the gone names by
//! space and then by name, each with the documents whose approvals pin it; its
//! candidates `same_content` first, then `renamed` from the most alike
//! (`similarity`, a percent, null when none was given), then `same_end` (the name
//! ends as the gone one does, and nothing more is known; `similarity` null), then by
//! name, a candidate
//! that is the name itself or another gone name left out, each a name as written on
//! disk (a move in case only is one); `present` whether something is at the
//! candidate now; `notes` what could not be looked at, each `kind` one of
//! `in_two_spaces`, `no_repository`, `no_revision`, `unknown_revision`,
//! `history_unreadable`, `search_unavailable`, `scan_truncated`,
//! `unreadable_skipped`, `too_large_skipped`, `candidates_cut`,
//! `no_content` (every pin of it compares presence only, as the preset `code` does:
//! there is no content to search for, and only Git's renames can suggest), in that
//! order, each once, with a `detail` sentence; `cannot_suggest` the spaces
//! of gone names whose adapter cannot suggest, sorted; `prefixes` and
//! `refused_prefixes` the moves of a prefix that gone names share, those that stand
//! against what is there now (with the names each moves, as they are now, and
//! `not_asked`, the spaces whose adapter takes no prefix, as `judge moved --prefix`
//! gives them) and those that do not (with how many names cannot be moved), each in
//! the order the moves were tried: first a move that a candidate found the best way
//! carries (`same_content` before `renamed` before `same_end`), then the one the most
//! gone names share, then by the move (whether the actor is admitted to declare a move
//! is not tried); `untried_prefixes` how many were not tried. `drawing` is the same as text
//! (`jq -r .drawing`). A file or a directory or a page that cannot be read, a history
//! that cannot be read and a bound reached are notes, and the answer is given: exit 0. A
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
//! "left_alone": [{"file", "reason", "line", "detail"}], "dropped": [{"file",
//! "names"}]}`, the files by their path under the root; `fmt --check` answers the
//! same with `rewritten` empty and `not_in_form` the files `fmt` would write.
//! `dropped` is, for each file in `rewritten` (for `--check`, in `not_in_form`) that
//! loses an entry of a name given to `--drop`, the names it loses, in the order of
//! the names; it is empty without `--drop`. `in_form` is a count, not a list: it
//! is most files, and nothing is to be done about them. A `reason` is `unparsed`,
//! `comment` (with its `line` in the file), `reads_otherwise`, `read_only`,
//! `linked` or `changed`. A
//! declaration that cannot be used is exit 2, as for every reading command, and a
//! file that cannot be read or written exit 3.
//!
//! `rekey` answers `{"dry_run", "moved", "rewritten", "stayed", "both", "mentions",
//! "approvals", "written"}`:
//!
//! | field | holds |
//! |---|---|
//! | `dry_run` | whether nothing was written (`--dry-run`) |
//! | `moved` | the document that took the new key: `{"doc": {"id", "key"}, "from_key", "to_key", "from_file", "to_file"}`, the keys as `book:KEY` |
//! | `rewritten` | each document whose links that wrote the old key now write the new one |
//! | `stayed` | each document whose links keep the old key, which another document has now |
//! | `both` | each document whose links keep the old key and gained the new one |
//! | `mentions` | the valid documents `{"id", "key"}`, and the document that took the new key, whose body names the old key, not changed (the hash covers a body) |
//! | `approvals` | the document that took the new key and each document of the three lists above, as `status` shows one, read from the files after the write; empty with `--dry-run` |
//! | `written` | `{"created", "replaced": [...], "removed"}`, the files, or null with `--dry-run` |
//!
//! Each of `rewritten`, `stayed` and `both` is `{"doc", "file", "why", "items": [{"link",
//! "written"}], "detail"}`: `items` the keys it wrote for the old key, `why` one of
//! `only_writer` (no other file writes the old key), `pinned` (its approval pinned the
//! document) or `asked` (a word said so), and `detail` a sentence that names `--both`
//! where a document can have meant both. A document with no identity that can be read has
//! a null `doc`. A refusal lists in `problems` each document that is not decided, each
//! word against an approval, each file that could not be read, and each difference
//! between the files and the plan; a refusal that lists none says all in `messages`.
//!
//! ## For implementers of the ports
//!
//! `ShelfRead` has a port more, `type Moves: MoveSource` with `fn moves()`, which
//! `moves` asks. A type of your own that implements `ShelfRead` does not build
//! until it names one: a breaking change of the library, before 1.0. A
//! `MoveSource` that has no way to suggest answers `SpaceSuggestions::CannotSuggest`
//! for every space.
//!
//! `rekey` needs three ports more: `ShelfWrite` has `type DocRemover: DocRemove` with
//! `fn doc_remover()` (a file removed after a comparison with what was read),
//! `DocReplace` has `restore` (the bytes a file had put back over the text written,
//! whether or not they are in the form), and `FileSync` has `persist_gone` (a removal put
//! on disk). `DocSource::render` must give the text `DocWrite::create` writes, byte for
//! byte: `rekey` removes the file it made again, when it stops, by comparing it with the
//! text it took before. A type of your own that implements these traits does not build
//! until it implements them: a breaking change of the library, before 1.0.
//!
//! ## Not yet in books
//!
//! Books has no gates, no private documents, no tags and no search of the text of
//! the documents. Every read reads every document; `status` can show only some.
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
