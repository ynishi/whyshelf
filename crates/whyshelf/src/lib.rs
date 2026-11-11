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
//! - [Books: kinds of document you declare](#books-kinds-of-document-you-declare),
//!   with the [commands](#commands) and their [output and exit
//!   status](#output-and-exit-status)
//! - [Crates](#crates)
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
//! The files:
//!
//! ```text
//!   .whyshelf/books.toml         the declaration of the books
//!   <book dir>/**/*.md           the documents: Markdown with a flat YAML front matter
//!   .whyshelf/ledger/<id>.json   one decision per file (two branches merge without a conflict)
//! ```
//!
//! A Finding -> Spec -> Design chain, declared as books, is the sample
//! `whyshelf_infra::shelf::books::SAMPLE` (a test keeps it valid); copy it and change
//! what you need. A document of its `spec` book:
//!
//! ```text
//!   ---
//!   id: 019a6f3e-0000-7000-8000-000000000002   a UUID v7: the identity, never changed
//!   key: S-1                                   how people name it; may change
//!   title: "Sign-in answers within 200 ms"
//!   grounds: [F-1]                             a link: keys in the target book
//!   ---
//!   The body.
//! ```
//!
//! A document's hash covers its book, its fields and its body (not its key, its
//! place or its links), so it can be renamed or moved and stay approved.
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
//! The sample's `design` book has one such
//! link, `tests` in the space `file` (the files of the project, by their path), so a
//! design names the tests that show it holds:
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
//! `means = "depends"`; the page of `whyshelf_infra::shelf::artifact` lists them).
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
//!   diff <ref>                each item of that approval, as approved and now, with a line diff
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>]
//!   judge approve --stale --reason <text> [--until <time>] [--actor <name>]
//!                             approve again, in one run and upstream first, every approval
//!                             that is expired or stale and would hold; the rest are left out
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
//!   judge same <ref>... [--artifact <name>]... [--from <hash>] --reason <text>
//!                             the edits named keep the meaning: the approvals that pinned the
//!                             hashes before them hold again, each with a notice
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
//! - A `<ref>` is a key (`S-1`), a key in a book (`spec:S-1`), an identity, or the
//!   end of one. It is read once, into one type, before anything runs: a reference
//!   that is not one, a book that is not declared, or a key the book's grammar
//!   refuses is told as such, not as "not found".
//! - `judge` takes several references and decides them in order on one reading of
//!   the project, each on what the ones before it left (so `judge approve F-1 S-1`
//!   frames S-1 on F-1's new approval); it writes all of them, or none when one is
//!   refused.
//! - `judge approve --stale` takes no reference (one given beside it is a request
//!   error, exit 2) and needs `--reason`. It approves again, in one such run, every
//!   approval whose standing is `expired` or `stale` and that would hold once
//!   approved, each after every one of them it stands on (the rule is on
//!   [`upstream_first`](domain::shelf::order::upstream_first)); no order of books is
//!   read. A pending document is not touched: it was never approved. Every other
//!   approval that does not hold is left out, never in silence, with a `reason`:
//!   `broken` (it is broken now: an artifact pinned there that is gone, an upstream
//!   that is not valid, a cycle), `refused` (the approval would be refused now, as
//!   below: an upstream decided against, an artifact that cannot be told), or
//!   `stands_on` (a `depends` link names a document that is broken, pending, or
//!   itself left out; a pending one is approved first, with `judge approve`). Each is
//!   listed under `left_out` as `{"doc", "reason", "detail", "upstream", "link",
//!   "upstream_is", "kinds", "causes"}` (`kinds` and `causes` for `broken`,
//!   `upstream`, `link` and `upstream_is` for `stands_on`), and written as a line on
//!   stderr: `warning: <key> is broken and was not approved again (<kinds>)`, or
//!   `warning: <key> was not approved again: <detail>`. A name with nothing there is
//!   pinned as absent, as by any approval, listed in the judgement's `absent`, and
//!   written on stderr as `warning: <key> pins <name> in <space>, which has nothing
//!   behind it`; what is never approved again is an artifact pinned there and gone.
//!   Leaving out is no failure. One `--until` applies to every approval of the run
//!   (an expiry an approval had before is not carried over), and one not after now
//!   is refused, exit 1, even when nothing is stale. The answer is `{"judged":
//!   [...], "left_out": [...]}`, `judged` as `judge approve` answers it, and empty,
//!   with exit 0, when nothing can be approved. Otherwise the exits are those of
//!   `judge`.
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
//! - Only `judge`, `new` and `fmt` write: `judge` one new file per decision or
//!   declaration, `new` one new document's file, never in place of a file that is
//!   there, and `fmt` over documents' files that are there, as above; every other
//!   command, `fmt --check` among them, only reads and creates nothing. Who judges
//!   is `--actor`, else `WHYSHELF_ACTOR`, else Git's `user.name`; never a committed
//!   file.
//! - None of these commands needs a Git repository. Git is used, when it is there,
//!   to name and read back the content an approval saw and to keep `HEAD` as a hint
//!   of the revision; its `user.name`, read even outside a repository, says who
//!   judges when nothing else does. No freshness depends on any of it.
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
//! `unlinked` when the declaration can be used, `status`, `get`, `trace`, `stale`, `stats`, `diff`) carries
//! `health` (files that are no document, cycles, records that contradict one another
//! or cannot be read). The answers of the writes, of `map` and of `schema` carry
//! none.
//!
//! | status | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `check` found something wrong, `map` or `unlinked` was given a declaration that cannot be used, `stale --exit-code` found an approval that does not hold or one that stands on a name that cannot be told now, `fmt --check` found a file not in the form (one it would write, or one it would leave alone), or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book (in a reference or `unlinked --book`) or a key its grammar refuses, a declaration that cannot be used, `get --approved` / `diff` of a document no approval stands about, a `--successor` that does not take the place of the document deprecated, for `new` a key that is taken, a book of slugs with no `--key`, entries the book refuses, or a file already at the path, for `judge same` a name no document links or one linked in two spaces, a `--from` hash nothing pinned, several earlier hashes and none named, or nothing to declare, and for `judge withdraw` an id that is no decision or declaration, or one already withdrawn |
//! | 3 | the run failed: a file could not be read or written |
//!
//! `check` answers `clean` (whether it exits 0) beside what it found; its
//! `uncovered` lists each document a covering link does not reach as `{"doc",
//! "book", "link", "severity"}`, the book and link expected to reach it and their
//! strength (`warn` or `error`), and the `not_covered` notice carries the same
//! `severity`.
//!
//! `map` answers `{"schema_problems": [...], "books": [...], "links": [...],
//! "diagram": ...}`: each book with its `name`, `dir`, `key` (`{"kind": "prefix",
//! "prefix"}` or `{"kind": "slug"}`), `description` and `fields` (`name`, `type`,
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
