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
//! `whyshelf books ...` knows no kind of document of its own. A project declares
//! its *books* in `.whyshelf/books.toml`: for each, a directory, how its keys are
//! written, its fields, and the kinds of link it may hold to other books. People
//! decide about documents (approve, deny, deprecate, retire, or revoke an earlier
//! decision), each decision is a record of its own, and whether an approval still
//! holds is computed on every read. The model is [`domain::shelf`], the use cases
//! [`app::shelf`], the files and their formats `whyshelf_infra::shelf`.
//!
//! | kind of link | what it means for an approval |
//! |---|---|
//! | `depends` | the approval pins the target; a change of it makes the approval stale, and a target that is gone or decided against makes it broken |
//! | `informs` | the target is worth reading: one that does not hold is a notice, never a staleness |
//! | `replaces` | the document takes the place of the target, and is its successor when the target is deprecated |
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
//! ## Links to files that are no document
//!
//! A link may point at *artifacts*, files of the project that are no document (code,
//! tests, data), with `target = "artifact"`; it `depends` or `informs`, never
//! `replaces` (and no book can be called `artifact`). The sample's `design` book has
//! one, `tests`, so a design names the tests that show it holds:
//!
//! ```text
//!   tests: [crates/cache/tests/expiry.rs]      paths under the root, with `/`
//! ```
//!
//! An approval pins, for each path of a `depends` link, the hash of the file's bytes,
//! or that no file was there: a path that has no file yet is the evidence not
//! written yet, not a broken link. Afterwards:
//!
//! | the pinned file | now | the approval is |
//! |---|---|---|
//! | there, with hash A | there, with hash A | unchanged by it |
//! | there, with hash A | there, with hash B | `stale` (`artifact_changed`) |
//! | there | gone | `broken` (`artifact_gone`) |
//! | not there | not there | unchanged by it |
//! | not there | there | `stale` (`artifact_arrived`): the evidence came, look again |
//!
//! A path added to or taken from the link since is `links_changed` (`stale`). An
//! `informs` link to a file changes no freshness.
//!
//! In a Git repository an approval also records, for the approved document, each
//! upstream it pinned and each file, the Git blob its content is in, and the commit
//! `HEAD` names (a hint). Freshness never reads either: it is decided by hashes, so a
//! directory outside Git works the same, with nothing recorded to read back. The
//! records of the ledger written before these were kept read back unchanged.
//!
//! ## What an approval saw
//!
//! `books show <ref> --approved` lists what the approval that stands saw (the
//! document, its upstreams and its files), each with its content as it was approved,
//! read back from Git, or why it cannot be: `no_address` (made outside Git, by an
//! older version, or no file was there) or `address_not_readable` (the content was
//! never committed, or is not in this clone). `books diff <ref>` sets each beside
//! the file now: the approved and current hash, whether it changed, and a unified
//! line diff when both are text. Both only read; putting a file back as it was is
//! Git's work (`git checkout`) and yours. A release is a Git tag: check it out and
//! `books status` says what held then, because freshness is derived, never stored.
//!
//! ## Commands
//!
//! ```text
//!   whyshelf [--root <dir>] books <command>
//!
//!   check                     the declaration, the documents and the ledger; exit 1 if anything is wrong
//!   status                    every document: what was decided, and where it stands now
//!   get <ref>                 one document, with where its approval starts to fail
//!   trace <ref>               what it links to and what links to it, transitively
//!   stale                     the approvals that no longer hold, strongest first, with their roots
//!   show <ref> --approved     what the approval that stands saw, with its content read back
//!   diff <ref>                each item of that approval, as approved and now, with a line diff
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>]
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
//!   new <book> --title <text> [--key <key>] [<entry>=<value>]... [--body <text>]
//!                             one new document, valid for its book, at <book dir>/<key>.md
//!   schema                    the JSON Schema of the answer of every books command
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
//! - An approval is refused when the document is not valid, a `depends` link cannot
//!   be followed, it would close a cycle, or a `depends` link names a document people
//!   decided does not stand (denied, deprecated or retired: pin the successor). An
//!   upstream that is only pending is accepted, and the approval is stale from the
//!   start.
//! - `new` takes the book's next key (`V-4` after `V-3`, counting every file of the
//!   book, valid or not, so a key is never reused) unless `--key` gives one; a book
//!   of slugs needs `--key`, and a key that is taken is refused. Each `<entry>` is
//!   read by the book's declaration: a list field or a link takes `a,b` (links are
//!   keys), a truth value `true` or `false`, a whole number its digits. The document
//!   is checked by the same rules as every file before anything is written (fields,
//!   required fields, enum words, link names and counts, types), with every problem
//!   listed; a link to a key no document has yet is allowed, and listed in
//!   `unresolved_links`. The identity is a new UUID v7.
//! - Only `judge` and `new` write: `judge` one new file per decision, `new` one new
//!   document's file, never in place of a file that is there; every other command
//!   only reads and creates nothing. Who judges is `--actor`, else `WHYSHELF_ACTOR`,
//!   else Git's `user.name`; never a committed file.
//! - None of these commands needs a Git repository. Git is used, when it is there,
//!   only to name and read back the content an approval saw.
//!
//! ## Output and exit status
//!
//! Every `books` command prints one JSON document on stdout; `books schema` prints
//! the JSON Schema of each, made from the same types. A document is
//! `{"id": ..., "key": ...}`; its `standing` is `pending`, `denied`, `deprecated`,
//! `retired`, or for an approval `fresh`, `expired`, `stale` or `broken`; a cause has
//! a `kind`, a `severity`, the `upstream` it names (or the `artifact`, a path) and a
//! `detail` sentence; every
//! answer carries `health` (files that are no document, cycles, records that
//! contradict one another or cannot be read).
//!
//! | status | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `books check` found something wrong, or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book or a key its grammar refuses, a declaration that cannot be used, `show --approved` / `diff` of a document no approval stands about, or for `new` a key that is taken, a book of slugs with no `--key`, entries the book refuses, or a file already at the path |
//! | 3 | the run failed: a file could not be read or written |
//!
//! `new` answers `{"doc": {"id", "key"}, "file": ..., "unresolved_links": [{"link",
//! "written", "why"}]}`. On 1 (a refusal), 2 and 3 the JSON is `{"error": "refused" |
//! "request" | "failed", "messages": [...], "candidates": [...], "problems": [...]}`
//! (`problems`: each problem of a declaration that cannot be used, or of a new
//! document that would be invalid) and the same chain goes to stderr.
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
