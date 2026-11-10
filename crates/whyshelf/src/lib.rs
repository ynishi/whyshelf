//! Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals
//!
//! The umbrella crate: the one name users depend on. The code lives in
//! `whyshelf-core`, re-exported here; the adapters of `whyshelf-infra` are
//! [`infra`], behind the `infra` feature (on by default).
//!
//! This documentation is the whole manual: concepts, the design, the formats of the
//! files, the settings, the command line and the tool server, compatibility, and the
//! known limits. There is no other document; the pages below link to where each topic
//! lives next to its code.
//!
//! - [What whyshelf is](#what-whyshelf-is), [the gates](#the-gates), [public and
//!   private](#public-and-private)
//! - [Where to read what](#where-to-read-what) and the [glossary](#glossary)
//! - [Opening a workspace](#opening-a-workspace) and [examples](#examples)
//! - [Security](#security), [compatibility](#compatibility),
//!   [concurrency and merge](#concurrency-and-merge) and
//!   [limitations](#limitations)
//! - [Books: kinds of document you declare](#books-kinds-of-document-you-declare),
//!   the model that is replacing the four fixed stages below
//!
//! # Books: kinds of document you declare
//!
//! `whyshelf books ...` is the newer model, and the one the fixed stages below are
//! being replaced by. It knows no kind of document of its own. A project declares
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
//!   judge approve <ref>... [--until <time>] [--reason <text>] [--actor <name>]
//!   judge deny|deprecate|retire <ref>... [--reason <text>]   (deprecate: [--successor <ref>])
//!   judge revoke <ref>... --reason <text>
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
//! - Only `judge` writes, one new file per decision; every other command only reads
//!   and creates nothing. Who judges is `--actor`, else `WHYSHELF_ACTOR`, else Git's
//!   `user.name`; never a committed file.
//! - None of these commands needs `.whyshelf.toml`, a Git revision or the cache.
//!
//! ## Output and exit status
//!
//! Every `books` command prints one JSON document on stdout; `books schema` prints
//! the JSON Schema of each, made from the same types. A document is
//! `{"id": ..., "key": ...}`; its `standing` is `pending`, `denied`, `deprecated`,
//! `retired`, or for an approval `fresh`, `expired`, `stale` or `broken`; a cause has
//! a `kind`, a `severity`, the `upstream` it names and a `detail` sentence; every
//! answer carries `health` (files that are no document, cycles, records that
//! contradict one another or cannot be read).
//!
//! | status | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `books check` found something wrong, or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong: bad arguments, a reference that names nothing, more than one document, an unknown book or a key its grammar refuses, or a declaration that cannot be used |
//! | 3 | the run failed: a file could not be read or written |
//!
//! On 1 (a refusal), 2 and 3 the JSON is `{"error": "refused" | "request" | "failed",
//! "messages": [...], "candidates": [...]}` and the same chain goes to stderr.
//!
//! ## Not yet in books
//!
//! The fixed stages below have gates, pins in the tests, the baseline of an item,
//! public and private stores, `find` and tags; books has none of these yet. Every
//! read reads every document.
//!
//! # What whyshelf is
//!
//! whyshelf keeps the reasons behind a piece of software next to the software,
//! so that a change can be traced back to why it was wanted and a reason that
//! no longer holds shows up as work to do. Four kinds of document form a chain,
//! each one standing on the one before it:
//!
//! | stage | answers | stands on |
//! |---|---|---|
//! | **Finding** | what was seen or measured, and how it is known | the world |
//! | **Spec** | what must hold (a Contract), with the pins that bind it and the hypotheses to measure | Findings |
//! | **Design** | the approach (with the options rejected) and its split into items | one Spec |
//! | **Impl** | the code and tests that carry each item's pins | a Design's item |
//!
//! The documents are Markdown files that people (or AI agents) write and edit;
//! the tool never rewrites them. Everything it adds is recorded next to them:
//!
//! - **Hash-pinned.** A document is identified by a hash of its meaning (white
//!   space and line endings are not part of it). An approval records the hash of
//!   what was approved *and* the hash of every upstream document it stood on, so
//!   a change anywhere upstream makes the downstream approval Stale instead of
//!   silently wrong.
//! - **Event-sourced approvals.** Registration, revision, classification and
//!   approval are events in an append-only log, one file per record of an event. Nothing is
//!   updated in place, so two branches that each add something merge without
//!   conflict, and whether something is approved is a question asked of the
//!   whole set of events. An approval is withdrawn by another event, never
//!   erased.
//! - **Derived state.** What is Draft, Approved, Modified, Stale or Blocked is
//!   computed from the files and the events each time; it is never stored.
//!
//! ```text
//!   Finding ----> Spec ----> Design ----> item ----> code + tests
//!   (CR)          (CS)       (CI)         (CA)        `@pin: PIN-...`
//!                   \                       |
//!                    +---- hypotheses ------+---> measurement (CM)
//!
//!   each arrow: the lower stage names the upper by alias or identifier;
//!   each gate: a machine check, then a person's approval that records the
//!   hashes of what it stood on
//! ```
//!
//! # The gates
//!
//! Each stage has a gate with two layers. The machine layer looks at form
//! (references resolve, the upstream is approved and current, every pin is
//! carried by a test and no test carries an undeclared one, the tests pass) and
//! reports every violation, not the first (an Invalid target among them). The human
//! layer records a judgement of meaning as an approval, and can be recorded only
//! from the proof that the machine layer found nothing ([`domain::gate`]). The gates are CR (a Finding), CS (a Spec), CI (a Design), CA (an
//! item, at one revision of the code, with a clean working tree) and CM (what a
//! measurement showed against the hypotheses fixed before it, a record rather
//! than a state). The violations a gate can report, with how to clear each, are
//! tabulated in [`domain::lint`].
//!
//! # Public and private
//!
//! Where a document's history lives is decided by where it is put: the repo
//! store is committed with the project, the private store stays on the user's
//! machine. A public document can name a private one only by an opaque
//! reference, and publishing moves a document from one store to the other.
//!
//! # Where to read what
//!
//! | topic | where |
//! |---|---|
//! | the model: stages, gates, two-axis state, what a hash covers | [`domain`] |
//! | a Finding file (all five types), a Spec file, a Design file: meaning and complete examples | [`domain::finding`], [`domain::spec`], [`domain::design`] |
//! | how those files are read: keys, errors, the one normalization rule | [`infra::parse`], [`infra::markdown`] |
//! | settings (`.whyshelf.toml`), the environment, the trust boundary | [`infra::config`] |
//! | the write path: gate, append, subscribers, warnings | [`app::commit`], [`app`] |
//! | the read path: ingest, the model, evaluation, views | [`app::ingest`], [`app::model`], [`app::view`] |
//! | events, the stream graph, how an identifier is made | [`domain::event`] |
//! | tags, kind, owner, path, alias: the fold and its conflicts | [`domain::classification`], [`domain::tag`] |
//! | approvals and what each gate records | [`domain::approval`], [`app::check`] |
//! | the machine check of each gate and the proof that it passed | [`domain::gate`] |
//! | derived state, causes of Stale and Blocked, what cannot be read here | [`domain::state`], [`domain::evaluate`] |
//! | the next step of an entity, and when a measurement is due | [`domain::next`] |
//! | check violations and how to clear them | [`domain::lint`] |
//! | reasons for Invalid, Stale, Blocked, partial and unparsed files | [`domain::state`] |
//! | the `find` query language | [`domain::query`], [`app::query`] |
//! | references: `private:`, `ext:`, items (`design#1`) | [`domain::reference`] |
//! | the two stores, publish, purge and complete deletion | [`infra::store_fs`], [`app::register`] |
//! | the event file format, versions and compatibility | [`infra::wire`], [`infra::map_dto`] |
//! | generated files (`map.json`, status, changelog) and their marker | [`infra::export`], [`app::projection`] |
//! | the cache | [`infra::cache_sqlite`] |
//! | subscribers, and why there are no user hooks | [`app::subscriber`] |
//! | the secret scan | [`infra::secret`]; the leak lint is in [`domain::lint`] |
//! | Git, revisions, the baseline, pins in code | [`infra::git`] |
//! | running the tests at a revision, and the time limit | [`infra::test_run`] |
//! | which keys of a document's front matter are content, classification, another kind's or a mistake | [`domain::front`], [`infra::parse`] |
//! | the versions in a project's data, and `migrate` | [`infra::format`] |
//! | the ports a program can implement | [`app::ports`] |
//! | **the command line, the walkthrough, the exit status, the tool server** | the crate documentation of `whyshelf-cli` (the `whyshelf` binary; `just rust-doc` builds it as a second step and prints where) |
//!
//! # Glossary
//!
//! | word | meaning |
//! |---|---|
//! | **entity** | a Finding, Spec, Design or item: anything with an identifier (a UUID v7) that approvals and events are about |
//! | **Finding** | a recorded cause, observation or premise; its `finding_type` (`measured_data`, `usage_observation`, `external_source`, `code_observation`, `domain_constraint`) decides what metadata it must carry |
//! | **Spec** | what must hold: a **Contract** (boundaries, invariants, promises) and an acceptance declaration (pins and hypotheses); stands on one or more Findings (`grounds`) |
//! | **Contract** | the `## Contract` section of a Spec, the promise itself |
//! | **pin** | an invariant the code must keep, named `PIN-...` (or `UT-...` for an ordinary unit test) and carried by a test with a comment tag `@pin: PIN-...`; checked in both directions at gate CA |
//! | **HYP** | a hypothesis named `HYP-...` registered in the Spec: which metric should move, and to what; fixed by hash at CS and judged at CM |
//! | **Design** | the approach, in its own file: a Crux and a split into items |
//! | **Crux** | a decision record in structure: the question, the options, the chosen one with its reason, and each rejected one with its reason |
//! | **item** | the smallest unit of implementation inside a Design, numbered (`design/x#1`), carrying the pins it will satisfy |
//! | **gate** | CR, CS, CI, CA or CM: a machine check of form, then a person's approval of meaning |
//! | **approval** | an `Approved` event of a gate: what was approved at which hash, on which upstream hashes, by whom, and (CA) at which revision |
//! | **Draft, Approved, Modified, Invalid** | the approval axis of an entity: never approved, approved at the current hash, approved at an earlier hash, unusable |
//! | **Ok, Stale, Blocked** | the upstream axis: nothing wrong upstream, an approval made against an older upstream, something upstream is not approved or valid |
//! | **revision** | a version of the code, opaque to the model (a Git commit in the Git adapter) |
//! | **baseline** | the branch that counts as "done" (`main` by default, `[git] baseline`) |
//! | **integrated** | a CA-approved revision that has reached the baseline (is an ancestor of it) |
//! | **clean tree** | no uncommitted change outside `.whyshelf/`; CA approval requires it |
//! | **event** | one record of the append-only log: `Registered`, `Revised`, `Reclassified`, `Moved`, `Approved`, `Revoked`, `Purged`, `Published`; one file for each time it was written |
//! | **head, tip** | the tips of an entity's stream are the events no other event lists in its `heads`; an event's heads are the tips its writer saw; more than one tip is a fork, joined by the next write |
//! | **ingest** | turning edits of the document files into events (`Revised`, `Moved`, `Registered`) |
//! | **alias** | a human name for an entity (`spec/cache-timeout`), unique within a store |
//! | **repo store** | `.whyshelf/` in the project: committed, public as the repository is |
//! | **private store** | `<data dir>/<project>-<project_id>`: outside the repository, never committed |
//! | **project_id** | the UUID that `whyshelf init` writes into `.whyshelf.toml`; with the name, it places the private store, so another project of the same name has another store |
//! | **`private:<id>`** | the opaque reference by which a public document names a private entity |
//! | **content hash, raw hash** | the hash of a document's meaning (what approvals use), and the hash of its normalized text (the key of its stored object) |
//! | **map** | the generated `map.json`: entities, their relations and tags, no state |
//! | **cache** | a per-worktree SQLite file of file stamps and searchable text; rebuildable, never a source of truth |
//! | **subscriber** | built-in work run after a write: cache, map, optional status and changelog files |
//! | **port** | a trait the use cases need from the outside world, implemented by `whyshelf-infra` |
//!
//! # Opening a workspace
//!
//! A [`app::Workspace`] is the one entry point: each use case is one of its
//! methods. [`infra::open`] builds one over the real adapters (Git, the file
//! system, SQLite, the clock) for the project in a directory, and returns with it
//! a [`infra::Shutdown`] that must be closed on purpose, because closing the
//! cache does I/O and is not done in `Drop`.
//!
//! ```no_run
//! # #[cfg(feature = "infra")]
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::path::Path;
//!
//! let (mut workspace, shutdown) = whyshelf::infra::open(Path::new(".")).await?;
//!
//! // The files people edit become history at the next ingest.
//! workspace.ingest().await?;
//!
//! // Every entity with its state on both axes, and the command to run next.
//! let status = workspace.status().await?;
//! for row in status.rows() {
//!     println!("{} -> {}", row.kind().name(), row.next());
//! }
//!
//! shutdown.close().await?;
//! # Ok(())
//! # }
//! # fn main() {}
//! ```
//!
//! The settings are read from `.whyshelf.toml` at the project root (see
//! [`infra::config`]); the private store is `<data dir>/<project>-<project_id>`, where the
//! data dir comes from the user's environment and never from the committed file.
//!
//! # Examples
//!
//! Calling a use case: a gate is two calls. `check_cs` is the machine layer and
//! changes nothing; `approve_cs` runs the same check and records the approval only if
//! it finds nothing. Reads take `&self`, writes take `&mut self`.
//!
//! ```no_run
//! # #[cfg(feature = "infra")]
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::path::Path;
//! use whyshelf::domain::reference::Ref;
//!
//! let (mut workspace, shutdown) = whyshelf::infra::open(Path::new(".")).await?;
//! workspace.ingest().await?;
//!
//! let spec: Ref = "spec/session-reuse".parse()?;
//! let report = workspace.check_cs(&spec).await?;
//! if report.passed() {
//!     // `None`: the approver is `user.name` of the settings.
//!     let approved = workspace.approve_cs(&spec, None).await?;
//!     println!("recorded {}", approved.event());
//! } else {
//!     for violation in report.violations() {
//!         println!("{violation:?}");
//!     }
//! }
//!
//! shutdown.close().await?;
//! # Ok(())
//! # }
//! # fn main() {}
//! ```
//!
//! The domain is plain values and runs anywhere. The leak lint, for example, finds
//! the alias of a private entity in a text that is about to be public, as a whole
//! token only, and the identifier of one that is not written `private:<id>`:
//!
//! ```
//! use whyshelf::domain::id::{Alias, EntityId};
//! use whyshelf::domain::lint::{PrivateName, leak_lint};
//!
//! let alias: Alias = "spec/payment-limits".parse().unwrap();
//! let names = [PrivateName::Alias(alias)];
//!
//! // `spec/payment-limits-v2` is another name, so it is not a leak ...
//! assert!(leak_lint("builds on spec/payment-limits-v2", &names, &[]).is_empty());
//! // ... and the name itself, on line 1, is.
//! let found = leak_lint("builds on spec/payment-limits.\nsee below", &names, &[]);
//! assert_eq!(found.len(), 1);
//! assert_eq!(found[0].line(), 1);
//!
//! // An identifier is how a public text may name a private entity, with its mark.
//! let id: EntityId = "01912345-6789-7abc-8def-0123456789ab".parse().unwrap();
//! assert!(leak_lint(&format!("stands on private:{id}"), &[], &[id]).is_empty());
//! assert_eq!(leak_lint(&format!("stands on {id}"), &[], &[id]).len(), 1);
//! ```
//!
//! So is the `find` query language, which is parsed into terms without touching any
//! index:
//!
//! ```
//! use whyshelf::domain::query::Query;
//!
//! let query = Query::parse("tag:area:auth kind:spec state:stale \"request timeout\"").unwrap();
//! assert_eq!(query.terms().len(), 4);
//! assert!(Query::parse("orphan:maybe").is_err());
//! ```
//!
//! **Testing with something other than the real adapters.** [`app::Workspace`] is
//! generic over the [`app::Ports`] bundle, so a program can implement the port traits
//! of [`app::ports`] itself. The in-memory implementation that this crate's own tests
//! use is not part of the public API; the adapters of [`infra`] are tested against a
//! real temporary Git repository, which is also the simplest thing to do from a
//! program of your own.
//!
//! # Security
//!
//! What whyshelf trusts, and what it stops. It sends nothing over a network and runs
//! only `git` and the project's test command.
//!
//! ```text
//!   committed with the repository          the user's machine
//!   (anyone who can change it can)         (only the user)
//!   -----------------------------          ------------------------------
//!   .whyshelf.toml  -> settings,           WHYSHELF_DATA_DIR / XDG_DATA_HOME /
//!                      test command         HOME  -> where private stores are
//!   document files  -> content             the private store itself
//!   .whyshelf/      -> repo store
//! ```
//!
//! - **The private store's place is not in the committed file.** `stores.private` in
//!   `.whyshelf.toml` is refused. Only the environment places it, so a repository
//!   cannot point a user's tool at a directory of the repository author's choice.
//!   Details and the variables: [`infra::config`].
//! - **The test command is code from the repository.** `test.command` is run through
//!   the shell when an item is checked, with the privileges of whoever runs it, in a
//!   temporary checkout of the revision if it is not the current one. Treat the
//!   settings file as you treat a `Makefile` or a CI script: read it before you run a
//!   check in a repository you do not trust. The pin pattern is a regular expression
//!   and the baseline a Git ref; neither runs anything.
//! - **The secret scan** looks at every text before it is stored as an object, for
//!   the shapes of well-known secrets (private key headers, cloud and code-hosting
//!   tokens, and a few more; the list is in [`infra::secret`]). A hit stops that
//!   write and names the rule and the line, never the text. It does **not** find
//!   passwords in prose or any secret of a shape it does not know, does not scan the
//!   fields of an event, and does not remove a secret from a file that already holds
//!   one. A text cannot be taken back from Git history once pushed: scanning before
//!   writing is the protection, and [`infra::store_fs`] says what deletion after the
//!   fact takes.
//! - **The leak lint** stops a text bound for the repo store that carries the alias
//!   or the title of a private entity (ASCII case-insensitively; an alias as a whole
//!   token), or the identifier of one without its mark: a reference in the front
//!   matter and one in the body are seen alike, and every path that writes a text to
//!   the repo store (`add`, `publish`, ingest, a scaffold) goes through it. It lets
//!   `private:<id>` through, which is the point. It checks the texts being stored,
//!   not the files of the repository in general, and it cannot know a private fact
//!   that is not an alias, a title or an identifier.
//! - **Approvers are names, not identities.** `by` is the name given or `user.name`;
//!   nothing authenticates it, and nothing checks that the implementer and the
//!   approver are different people.
//! - **Generated files carry a marker** and are compared with it: a hand edit and an
//!   out-of-date file are told apart and shown by `status`. They hold no state and,
//!   in the repo store, no private titles or aliases.
//!
//! # Compatibility
//!
//! Five things are versioned. A change to any of them is made so that data written
//! before can still be read, or is refused by name; nothing is guessed at.
//!
//! | what | bumped when | how older data is read |
//! |---|---|---|
//! | settings `version` (`.whyshelf.toml`) | a key is renamed, removed or changes meaning | a build reads its own version; any other is an error naming both |
//! | event `v` (each event file) | the JSON of an event changes in a way an old reader would misread | the reader is chosen by `v`; old versions keep their reader; an unknown `v` is refused |
//! | `map.json` `version` and its schema | the shape of the map changes (the schema of the old version is kept) | the map is generated, never input: it is written again |
//! | cache `schema_version` | the tables change | a cache of another version is dropped and built again |
//! | hash domain tags `whyshelf/<kind>/v1` | the canonical form of a value or the scope of a hash changes | there is no reader for two generations of hashes: every identifier and approval written under `v1` was computed with it, so a new tag would need a migration of the stored data |
//!
//! `whyshelf migrate` is the place for such a migration. Today it reads the versions
//! that are in the data (the settings file and the event files of both stores are the
//! sources of truth that decide; the map and the cache are derived and only shown),
//! counts them, and says which this build reads ([`infra::format`]); it writes
//! nothing. The wire formats, and what a version change does to an event's identifier
//! (nothing, for `v`; everything, for a hash tag), are in [`infra::wire`].
//!
//! The JSON a command prints may gain fields in a later version (`status` has a
//! `context` object since it was added); a reader of it ignores the keys it does not
//! know. The words and columns that are already there do not change meaning.
//!
//! # Concurrency and merge
//!
//! One workspace runs one write at a time (`&mut self`); the tool server keeps that
//! across requests with a single task; a command line and a tool server, or two
//! worktrees, can write at once because a record file is named by the hash of the
//! event's content and heads and by the digest of the time it was written, so the
//! same change is one event and the same record is the same file. Merging branches
//! adds record files and never conflicts on them; `map.json` may conflict and
//! `whyshelf rebuild` writes it again; a conflict marker in a document makes that
//! entity Invalid until a person resolves it; the cache is per worktree and not
//! merged. The full account, with a picture, is in [`app`].
//!
//! # Limitations
//!
//! What the code does not do, in plain words, so that nobody finds out by running
//! into it.
//!
//! **Questions the model leaves open**
//!
//! - *Squash and rebase.* "Integrated" asks whether the approved revision is an
//!   ancestor of the baseline. A squash or a rebase makes a different revision, and
//!   the approved one is then never an ancestor: the item stays "approved, pending".
//!   Nothing follows a rewritten history.
//! - *Publish keeps the past private.* Past versions and events stay in the private
//!   store, and so do the approvals; a clone that has only the repo store sees a
//!   published entity as a Draft.
//! - *Private store across machines.* The private store is a directory; moving it
//!   between machines is not something whyshelf does.
//! - *The project id is added once.* `project_id` is written by one person and
//!   committed. If two people each add one and the branches merge, only one survives,
//!   and the private store of the other stays in a directory that is no longer looked
//!   at: what lived in it reads as unresolved until the directory is renamed to the
//!   surviving id.
//! - *No packing of events.* One record of an event is one file, and every command
//!   reads all of them.
//! - *Generated Markdown.* `status.md` and `changelog.md`, when enabled, are written
//!   at the root of the repo store (`.whyshelf/`), not in the documents' directories.
//! - *Approver independence* is not checked (see Security).
//!
//! **Behaviour a user can meet**
//!
//! - *Every command loads everything.* Loading the model reads every registered file
//!   and asks Git for the pins (one `git grep` over the test files) and for the
//!   baseline (a few Git processes for each approved revision); a command line
//!   command does that at least twice (ingest, then the command), a write once more
//!   after it writes. The cache is used to find what changed and for full-text
//!   search, not to avoid the load.
//! - *A clone without the baseline cannot say whether an item is integrated.* Git is
//!   asked about every approved revision against the baseline ref. If the ref (`main`
//!   by default) or an approved commit is not in the clone (a shallow or
//!   single-branch CI checkout, say), the item is shown as `Unknown` and no command
//!   stops; fetch the baseline to learn more.
//! - *Tests at another revision.* The command is the one in the settings now, run in a fresh
//!   checkout of tracked files; its output is not kept (only the last 64 KiB is held
//!   while it runs, so that it cannot fill the memory); it is stopped after
//!   `test.timeout` (thirty minutes unless set), and a stopped run records nothing.
//! - *Test files are told by their path, not by what is in them.* `test.files` names
//!   them by glob under the scan directories, the whole of which is the default. A
//!   language that keeps its tests in the file they test (Rust's `#[cfg(test)] mod
//!   tests`) cannot be narrowed by file, so every `@pin:` in such a file counts,
//!   a comment in the code included.
//! - *Some things have no command.* Setting the owner or changing the alias of a
//!   registered document, and purging a stored text, are methods of the library
//!   ([`app::Workspace::reclassify`], [`app::Workspace::purge`]) and have no
//!   command or tool.
//! - *Classification keys in a file* (`tags:`, `kind:`, `owner:`, `alias:`) are taken
//!   in once, when the file is registered. After that the store is the truth: a file
//!   that says something else is shown in `status` as a difference, and nothing is
//!   written back to the file or to the store. Tags are changed with `tag`.
//! - *User hooks do not exist*; only the built-in subscribers run
//!   ([`app::subscriber`]).
//! - *One revision kind.* The model has room for a revision that is a patch on a
//!   base, but no adapter makes one: an unclean tree is refused at CA.
//! - *The shell.* The test command is started with `sh -c` (`cmd /C` on Windows),
//!   so it is written for the shell of the machine that runs the check.
//!
//! # Crates
//!
//! `whyshelf-core` holds the rules ([`domain`], pure) and the use cases with the
//! ports they need ([`app`]); it depends on no I/O crate. `whyshelf-infra`
//! implements the ports. The `whyshelf` command (and the tool server it starts
//! with `whyshelf mcp`) is the `whyshelf-cli` crate, a thin adapter over the same
//! use cases. The picture of the layers and of which adapter implements which port
//! is in the documentation of the core crate.
#![warn(missing_docs)]

pub use whyshelf_core::*;

#[cfg(feature = "infra")]
pub use whyshelf_infra as infra;
