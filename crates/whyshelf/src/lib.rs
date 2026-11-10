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
//! `whyshelf ...` knows no kind of document of its own. A project declares
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
//! An approval pins, for each path of a `depends` link, the hash of the file (of its
//! bytes, unless the link compares otherwise, below), or that no file was there: a
//! path that has no file yet is the evidence not written yet, not a broken link.
//! Afterwards:
//!
//! | the pinned file | now | the approval is |
//! |---|---|---|
//! | there, with hash A | there, with hash A | unchanged by it |
//! | there, with hash A | there, with hash B | `stale` (`artifact_changed`) |
//! | there | gone | `broken` (`artifact_gone`) |
//! | not there | not there | unchanged by it |
//! | not there | there | `stale` (`artifact_arrived`): the evidence came, look again |
//! | there, under one comparison | there, and the link now declares another | `stale` (`artifact_comparison_changed`): the hashes are not compared |
//!
//! A path added to or taken from the link since is `links_changed` (`stale`). An
//! `informs` link to a file changes no freshness.
//!
//! What of a file counts is declared per link, with `compare`:
//!
//! ```text
//!   [[book.link]]
//!   name = "tests"
//!   target = "artifact"
//!   means = "depends"
//!   compare = "lines"        # default "bytes": every byte counts
//!   ignore = ["//"]          # lines that start with these (after white space) do not count
//! ```
//!
//! `lines` compares the lines of the file with their white space collapsed and the
//! blank lines left out, so indentation, spacing and blank lines change nothing, and
//! neither do the lines `ignore` names (`//` takes `//`, `///` and `//!` comments; `#`
//! takes those of TOML, shell or Python). It knows no language: a comment at the end
//! of a line of code still counts, and so does every byte that is not UTF-8 (a file
//! that is no text is better compared as `bytes`). Each pin keeps the comparison its
//! hash was made with. A link whose `compare` is changed afterwards makes the
//! approvals that pinned a file through it `stale` (`artifact_comparison_changed`),
//! and the hashes are not compared, since they were made differently: look again and
//! approve. A file gone or come is told as such, whatever the comparison.
//!
//! In a Git repository an approval also records, for the approved document, each
//! upstream it pinned and each file, the Git blob its content is in, and the commit
//! `HEAD` names (a hint). Freshness never reads either: it is decided by hashes, so a
//! directory outside Git works the same, with nothing recorded to read back. The
//! records of the ledger written before these were kept read back unchanged.
//!
//! ## An edit that keeps the meaning
//!
//! Any change of a hash makes the approvals that pinned it stale, a typo fixed as
//! much as a rule rewritten: whyshelf does not guess what an edit means. When a person
//! has read an edit and it keeps the meaning, `judge same <ref>` (or `--artifact
//! <path>` for a file) records that, with a reason. The declaration is of the edit
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
//!   check                     the declaration, the documents and the ledger; exit 1 if anything is wrong
//!   status                    every document: what was decided, and where it stands now
//!   get <ref>                 one document, with where its approval starts to fail
//!   get <ref> --approved      what the approval that stands saw, with its content read back
//!   trace <ref>               what it links to and what links to it, transitively
//!   stale                     the approvals that no longer hold, strongest first, with their roots
//!   diff <ref>                each item of that approval, as approved and now, with a line diff
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>]
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
//!   judge same <ref>... [--artifact <path>]... [--from <hash>] --reason <text>
//!                             the edits named keep the meaning: the approvals that pinned the
//!                             hashes before them hold again, each with a notice
//!   judge withdraw <record id>... --reason <text>
//!                             take back decisions or declarations by the id of their record
//!   new <book> --title <text> [--key <key>] [<entry>=<value>]... [--body <text>]
//!                             one new document, valid for its book, at <book dir>/<key>.md
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
//! - Only `judge` and `new` write: `judge` one new file per decision or declaration, `new` one new
//!   document's file, never in place of a file that is there; every other command
//!   only reads and creates nothing. Who judges is `--actor`, else `WHYSHELF_ACTOR`,
//!   else Git's `user.name`; never a committed file.
//! - None of these commands needs a Git repository. Git is used, when it is there,
//!   only to name and read back the content an approval saw.
//!
//! ## Output and exit status
//!
//! Every command prints one JSON document on stdout, with a `version` field (now
//! `1`; within a version the shape only grows: new fields, new words in a field of
//! words); `schema` prints the JSON Schema of each, made from the same types. A
//! document is
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
//! | 1 | `check` found something wrong, or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book or a key its grammar refuses, a declaration that cannot be used, `get --approved` / `diff` of a document no approval stands about, for `new` a key that is taken, a book of slugs with no `--key`, entries the book refuses, or a file already at the path, for `judge same` a path no document links, a `--from` hash nothing pinned, several earlier hashes and none named, or nothing to declare, and for `judge withdraw` an id that is no decision or declaration, or one already withdrawn |
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
