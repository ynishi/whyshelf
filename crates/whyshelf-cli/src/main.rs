//! The `whyshelf` command (CLI, and the MCP server as `whyshelf mcp`): parses
//! arguments, wires the `whyshelf-infra` adapters into the `whyshelf-core`
//! use cases, prints the result.
//!
//! The argument parser's definition ([`args`], clap's derive types) is the
//! only description of the commands: help, the command list and validation
//! come from it, not from a second hand-kept table. Structured input is read
//! from a file argument, not pasted into the command line.
//!
//! The crate is an adapter and nothing more; what a command means is decided by
//! the use cases of `whyshelf-core`. This page is the user's guide to the command
//! and to the tool server: a walkthrough, the commands, the output formats, the
//! exit status, the environment, and the tools. What a word means (Finding, Spec,
//! pin, gate, stale, ...) is in the glossary of the `whyshelf` crate; the formats of
//! the files you write are in the documentation of `whyshelf_core::domain::finding`,
//! `spec` and `design`, and how they are read in `whyshelf_infra::parse`.
//!
//! | module | job |
//! |---|---|
//! | [`args`] | the commands as clap types, parsed into core types |
//! | [`command`] | a command as a value, and the one function that runs it |
//! | [`actor`] | the task that owns the workspace for the tool server |
//! | [`mcp`] | the tools, one per command |
//! | [`render`] | an outcome as a table, JSON or Markdown |
//! | [`init`] | `whyshelf init` (the only command that writes files itself) |
//! | [`run`] | opening and closing the workspace, printing, the exit status |
//! | [`error`] | the errors and how they are shown |
//!
//! ```text
//!   argv --clap--> Cli { root, format, command }
//!                      |
//!                      v   Action
//!        init / schema map / migrate     -> no workspace needed
//!        mcp                              -> open, serve until the client leaves
//!        every other command (Command)    -> open
//!                                              |
//!                                              v
//!                          execute:  ingest (files -> events), unless `rebuild`
//!                                    one use case of Workspace
//!                                              |
//!                                              v
//!                     Done { outcome, warnings }  -> render (table|json|markdown)
//!                                              |
//!                                              v
//!                          close the cache (always), exit status
//! ```
//!
//! # Walkthrough
//!
//! The story: a team finds that signing in feels slow, and follows it from the
//! observation to an approved item of work. Every name is made up. The project is a
//! Git repository with at least one commit (every command that opens the project
//! asks Git for the current revision), and the branch the work stands on is
//! `feat/session`.
//!
//! ```text
//!   init --> finding new --> add --> check cr --> spec new --> check cs
//!        --> design new --> check ci --> (tests carry the pins) --> check ca
//!        --> status                                   ... check cm after merging
//! ```
//!
//! **1. `init`.** It writes a minimal `.whyshelf.toml`, creates `.whyshelf/`, and
//! keeps the cache out of Git. It never overwrites a settings file. The file gets a
//! `project_id`, a new UUID (version 7) that identifies the project: the private
//! store is placed by it, so commit the file as it is. The test files are written out
//! as the whole of the scan directories, with a comment on how to narrow them.
//!
//! ```text
//! $ whyshelf init --project demo --test-command "cargo test"
//! created .whyshelf.toml for project `demo` (0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506)
//! created .whyshelf/
//! added .whyshelf/cache/ to .gitignore
//! ```
//!
//! A settings file copied from a template repository carries the template's
//! `project_id`, which would make the two projects share a private store. Replace the
//! line with the one `whyshelf init --print-project-id` prints (it only prints; no
//! file is read or written). A settings file with no `project_id` is refused by every
//! command, with the line to add in the message: the tool never edits your file.
//!
//! Then add what the story needs to the settings, and commit. New documents go to
//! the **private** store unless you say `repo`; this project commits its history, so
//! it says `repo`. The user's name is what an approval records.
//!
//! ```toml
//! version = 1
//! project = "demo"
//! project_id = "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506"
//!
//! [test]
//! command = "cargo test"
//! files = ["src", "tests"]
//!
//! [stores]
//! default = "repo"
//!
//! [tags.official]
//! area = ["auth", "build"]
//!
//! [user]
//! name = "alice"
//! ```
//!
//! **2. `finding new`.** A Finding is the ground under a change: what was seen or
//! measured. The command writes a template (it never overwrites a file) and
//! registers it. A template asks for what only you can say, so until it is filled in
//! the file is registered and shown as invalid.
//!
//! ```text
//! $ whyshelf finding new --type usage_observation --title "Login feels slow"
//! id     0192e3a4-7b5c-7d3e-8f01-23456789abcd
//! alias  finding/login-feels-slow
//! path   docs/finding/login-feels-slow.md
//! ```
//!
//! Open `docs/finding/login-feels-slow.md`, fill the keys in and write what was
//! seen under them:
//!
//! ```markdown
//! ---
//! finding_type: usage_observation
//! title: "Login feels slow"
//! evidence: measured
//! context: "cold start of the app, three testers"
//! reproducibility: reproducible
//! ---
//! Signing in took about eight seconds on a cold start for all three testers.
//! ```
//!
//! **3. `add`.** A file you wrote yourself becomes known by registering it. Nothing
//! is taken in silently: `status` lists a file that nobody registered, with this
//! command as its next step. The tool never edits your file.
//!
//! ```text
//! $ whyshelf add docs/finding/notes-on-timeouts.md --alias finding/timeouts
//! ID                                    PATH
//! 0192e3a5-0f10-7a42-9c33-7e5d10ab94c2  docs/finding/notes-on-timeouts.md
//! ```
//!
//! **4. `check cr`.** Every gate has two layers. `check` alone is the machine
//! layer: it looks at form and prints every problem it finds. With `--approve` the
//! same check runs and, if nothing is found, your judgement is recorded as an
//! `Approved` event.
//!
//! ```text
//! $ whyshelf check cr finding/login-feels-slow
//! gate    CR
//! target  0192e3a4-7b5c-7d3e-8f01-23456789abcd
//! result  passed
//!
//! $ whyshelf check cr finding/login-feels-slow --approve
//! recorded  CR
//! event     sha256:4be0...
//! ```
//!
//! `recorded` says what the approval recorded, and nothing about where your working
//! tree is: only a CA records a revision (step 7), and it shows that one.
//!
//! **5. `spec new`, `check cs`.** A Spec stands on at least one Finding and says what
//! must hold. It declares the pins that bind it (a pin is carried later by a test)
//! and the hypotheses to measure. The template has the grounds filled in; the rest is
//! yours. Until `## Contract` is written the file is invalid, and the gate refuses it
//! with a violation, like any other (so the exit status is the one of a check that
//! found something; `status` and `get` say why too):
//!
//! ```text
//! $ whyshelf spec new spec/session-reuse --from finding/login-feels-slow
//! $ whyshelf check cs spec/session-reuse
//! gate    CS
//! target  0192e3a6-1c2d-7e4f-8a90-b1c2d3e4f506
//! result  failed
//!
//! Violations
//! the target is invalid: the file did not parse: the key `## Contract` is missing
//! ```
//!
//! ```markdown
//! ---
//! grounds:
//!   - finding/login-feels-slow
//! accept:
//!   pins:
//!     - id: PIN-LOGIN-CACHE
//!       contract: "The session is reused for five minutes"
//!   hyp:
//!     - id: HYP-FASTER-LOGIN
//!       metric: "median login time"
//!       target: "under 2 s"
//! ---
//!
//! ## Contract
//! A session younger than five minutes is reused without signing in again.
//! ```
//!
//! ```text
//! $ whyshelf check cs spec/session-reuse --approve
//! recorded  CS at 9f2c71b0e4d85a36c1f0b7a2d3e4f5061728394a (feat/session)
//! event     sha256:b17d...
//! ```
//!
//! The approval records the hash of the Spec **and** the hash of the Finding it stood
//! on. If the Finding is corrected later, the Spec becomes stale, and `status` says so.
//!
//! **6. `design new`, `check ci`.** A Design answers how, in its own file: the
//! question, the options (one chosen, the others rejected with a reason) and the
//! split into items, each carrying the pins it will satisfy.
//!
//! ```text
//! $ whyshelf design new design/session-reuse --spec spec/session-reuse
//! $ whyshelf check ci design/session-reuse --approve
//! recorded  CI at 9f2c71b0e4d85a36c1f0b7a2d3e4f5061728394a (feat/session)
//! event     sha256:03c9...
//! ```
//!
//! **7. `check ca`.** An item is approved at one revision of the code. Write the test
//! with a `@pin:` tag in a comment, **commit**, and then check. A tree with
//! uncommitted work is refused ("commit first"; changes under `.whyshelf/` do not
//! count), and there is no way around it. The check runs `test.command` at that
//! revision and compares the pins in both directions.
//!
//! ```text
//! // tests/session.rs
//! // @pin: PIN-LOGIN-CACHE
//! #[test]
//! fn the_session_is_reused() { /* ... */ }
//!
//! $ git add -A && git commit -m "Reuse the session"
//! $ whyshelf check ca design/session-reuse#1 --approve
//! recorded  CA at 5d0e83c2a91f7b64e0c3d8a1f2b4c6d7e8f90a1b (feat/session)
//! event     sha256:e72a...
//! ```
//!
//! **8. `status`.** Every entity, both axes of its state, whether the approved
//! revision reached the baseline branch, how many pins the tests carry, and what to
//! do next. It opens with the context line, which says where the tool is pointed
//! (in JSON it is a `context` object: `project`, `worktree`, `revision`, `branch`).
//! The status that is exported to a file for the repository leaves the line out: a
//! worktree's path does not belong in a committed file.
//!
//! ```text
//! $ whyshelf status
//! [context: project=demo, worktree=/path/to/project, revision=5d0e83c2a91f7b64e0c3d8a1f2b4c6d7e8f90a1b (feat/session)]
//!
//! ID                      KIND     TITLE                            APPROVAL  UPSTREAM  INTEGRATION  PINS  NEXT
//! finding/login-feels-slow  finding  Login feels slow                 Approved  Ok        -            -     -
//! spec/session-reuse        spec     -                                Approved  Ok        -            1/1   -
//! design/session-reuse      design   -                                Approved  Ok        -            -     -
//! design/session-reuse#1    item     Reuse the session inside the...  Approved  Ok        Pending      1/1   awaiting integration
//! finding/timeouts          finding  Notes on timeouts                Draft     Ok        -            -     whyshelf check cr finding/timeouts
//! ```
//!
//! A clone that does not have the baseline ref, or the approved commit (a shallow or
//! single-branch checkout of a CI machine), cannot tell whether it was merged: the
//! item reads `Unknown`, and no command stops for it.
//!
//! After the branch is merged into the baseline (`main`, unless `[git] baseline`
//! says otherwise) the item reads `Integrated`, and the Spec's next step is the
//! measurement: `whyshelf check cm spec/session-reuse pass` records that the
//! hypotheses were met (or `fail-wiring --item-hint "..."`, or `fail-hyp --finding
//! <ref>`, see below). Edit the Finding again and the rows below it turn stale:
//!
//! ```text
//! spec/session-reuse  spec  -  Approved  Stale  -  1/1  changed: whyshelf check cs spec/session-reuse
//! ```
//!
//! # Commands
//!
//! Every command takes the global options `--root <DIR>` (the project root; the
//! current directory by default) and `--format table|json|markdown` (`table` by
//! default). A *ref* is an alias (`spec/session-reuse`), an identifier, or
//! `private:<id>`; an *item* is `<design ref>#<n>`.
//!
//! Every command except `init`, `schema`, `migrate` and `rebuild` first runs an
//! **ingest**: it compares the document files with what is recorded and writes a
//! `Revised` (or `Moved`, or `Registered`) event for what changed, so that the
//! command always looks at what is recorded. A file the secret scan or the leak
//! lint refuses is left out and reported as a warning. Warnings go to standard error
//! as `warning: ...` and never change the exit status.
//!
//! | command | what it does |
//! |---|---|
//! | `init --test-command <CMD> [--project <NAME>]` | writes `.whyshelf.toml` (with a new `project_id` and `[test] files`) and `.whyshelf/`, adds the cache to `.gitignore`; the project name defaults to the directory's name when that is a valid name |
//! | `init --print-project-id` | prints the line `project_id = "<new UUID v7>"` and nothing else, for a settings file that lacks one or was copied from a template; it reads and writes no file |
//! | `context` | the project, the worktree, the current revision, the branch, the default store, the number of events in each store |
//! | `add <PATH>... [--kind finding\|spec\|design] [--tag T]... [--alias A] [--store repo\|private]` | registers files, or every document under a directory; a file already registered is skipped and listed; `--alias` is for one file; official tags must be in the vocabulary. The kind is told by the file's keys when they name one kind; for a file whose keys name several or none, `--kind` is asked for. The classification keys of the file (`tags`, `kind`, `owner`, `alias`) are taken in once, here, and afterwards a difference with the store is shown in `status` |
//! | `tag <REF> +tag -tag ...` | adds and removes tags (`+area:auth`, `-wip`); never changes an approval |
//! | `mv <REF> <NEW_PATH>` | records that the document's file is now at `NEW_PATH` (move the file yourself first); the identity and the approvals stay |
//! | `publish <REF>` | moves a document from the private store to the repo store; refuses text that carries a private alias or title, or the identifier of a private entity not written `private:<id>`; refused while the kind, the path, the version, the owner or the alias of the document was set differently on two branches |
//! | `finding new --type <T> --title <TITLE> [--alias A]` | a Finding file from a template, registered; `T` is `measured_data`, `usage_observation`, `external_source`, `code_observation` or `domain_constraint`; the alias defaults to `finding/` and the title as a slug |
//! | `spec new <ALIAS> --from <FINDING_REF>` | a Spec file from a template, registered |
//! | `design new <ALIAS> --spec <SPEC_REF>` | a Design file from a template, registered |
//! | `check cr\|cs\|ci <REF> [--approve] [--by NAME]` | the machine check of a gate; with `--approve` the approval is recorded when it passes |
//! | `check ca <ITEM> [--approve] [--revision <40 hex>] [--by NAME]` | the same for an item; runs the tests; `--approve` needs a clean tree and records the current revision, or the one named |
//! | `check cm <SPEC> [--by NAME] pass\|fail-wiring --item-hint <TEXT>\|fail-hyp --finding <REF>` | records what a measurement showed, against the hypotheses the CS approval fixed; always records; `fail-hyp` needs a `measured_data` Finding |
//! | `revoke <EVENT> --reason <TEXT> [--by NAME]` | withdraws an approval (the `event` column of `audit`); it stays in the log, marked revoked |
//! | `get <REF\|ITEM>` | one entity: its content and classification, state and approvals |
//! | `find <TERM>... [--limit N] [--cursor C] [--fields a,b]` | entities by one query (below), 50 rows at most by default, 200 at most; columns: `store`, `path`, `alias`, `title`, `tags`, `owner`, `state` |
//! | `status` | the state of every entity, files nobody registered, vanished files, drift of generated files |
//! | `trace <REF>` | the chain Finding, Spec, Design, item and approved revision around one entity |
//! | `changelog <REF>` | the events of one entity |
//! | `audit [<TERM>...]` | who approved which version at which revision, and when; narrowed by a query |
//! | `rebuild` | reads every file, replaces the cache, writes the generated files again (after a merge, say) |
//! | `schema map` | prints the JSON Schema of `map.json` |
//! | `migrate` | reads the versions of the project's data and sets them against the ones this build reads (below); it only looks and writes nothing, and opens no workspace, so it works on data the other commands cannot read |
//! | `mcp` | serves the commands as tools over standard input and output (below) |
//!
//! **Queries** for `find` and `audit`: terms separated by spaces, all of which must
//! match. `tag:area:auth`, `kind:spec` (finding, spec, design, item),
//! `state:stale` (invalid, draft, approved, modified, ok, stale, blocked, unresolved),
//! `owner:alice`, `grounds:<ref>` (what takes this Finding as a ground),
//! `upstream:<ref>`, `orphan:true`, and any other word or `"quoted phrase"` is
//! searched in the text.
//!
//! ```text
//! $ whyshelf find tag:area:auth kind:spec state:stale
//! $ whyshelf find owner:alice "request timeout" --fields alias,state --limit 20
//! $ whyshelf find grounds:finding/login-feels-slow
//! ```
//!
//! # Output formats
//!
//! One result is rendered three ways, which say the same thing.
//!
//! | `--format` | shape |
//! |---|---|
//! | `table` | aligned columns with an upper-case header, for a terminal; a value that is absent is `-` |
//! | `markdown` | headings and pipe tables |
//! | `json` | one object, one key per section (`entities`, `notes`, `found`, `page`, `check`, `violations`, ...); a table is an array of objects whose keys are the lower-case column names (`approved at` is `approved_at`), the fields of an entity are an object, a list of lines is an array, and a page that has more says `next_cursor` |
//!
//! ```text
//! $ whyshelf status --format json
//! {
//!   "entities": [
//!     {
//!       "id": "design/session-reuse#1",
//!       "kind": "item",
//!       "title": "Reuse the session inside the window",
//!       "approval": "Approved",
//!       "upstream": "Ok",
//!       "integration": "Pending",
//!       "pins": "1/1",
//!       "next": "awaiting integration"
//!     }
//!   ]
//! }
//! ```
//!
//! # Exit status
//!
//! Each status says who acts, and on what ([`run`] holds the one table that maps a
//! result to a status, and a new kind of failure does not build until it has a place
//! in it).
//!
//! | status | means | when |
//! |---|---|---|
//! | 0 | success | the command did what it was asked, including a `check` that found nothing and a `migrate` that found every file readable |
//! | 1 | a check or a gate stopped it; the user clears it in a file, or in the build of whyshelf in use | a `check` without `--approve` found violations (they are the result: `result` is `failed` in the `check` section on standard output, and the violations follow; an invalid target is one of them); an approval refused by the machine check (a `check cm` too, which always records), each violation on a `- ` line of standard error; a secret found in text to be stored; a private name in text for the repo store; an unclean tree at `check ca --approve`; a file of a document gone; a classification two branches set differently, which stops `publish` and `purge` until it is set again; a `migrate` that found files of a version this build does not read, or could not look at a store |
//! | 2 | the request is wrong; the user corrects the command line, the query or the settings | an unknown command or option, a missing argument, a value its type refuses (a malformed reference, tag change, finding type, revision or `--format`), `check cm ... fail-wiring` without `--item-hint` (clap prints the reason); a `find` query that does not parse and a `--fields` name that does not exist; a name that is not there, names several, or is of the wrong kind; a settings file that is wrong, missing or without `project_id`; a directory that is not a project (so also `migrate` there); a file whose keys name no single kind, added without `--kind` |
//! | 3 | the run failed; whoever runs it looks at the machine | Git, the disk, the store, the cache or the test run failed (including a test run that could not be started); **a data file of a version this build does not read**, which no command but `migrate` can work on (the message says so); a failure to close the cache; a failure of the tool server |
//!
//! The message of a failed command goes to standard error as `error: ...`, each cause
//! on a `caused by:` line. A script that must tell "the check found something" from
//! "the tool failed" reads the status: 1 is a result of the check, 3 is a failure.
//! Standard output has the `check` section only for the former.
//!
//! # Environment and files
//!
//! | what | meaning |
//! |---|---|
//! | `WHYSHELF_DATA_DIR` | where private stores go (`<it>/<project>-<project_id>`); an absolute path |
//! | `XDG_DATA_HOME`, `HOME` | the fallbacks: `$XDG_DATA_HOME/whyshelf`, then `$HOME/.local/share/whyshelf` |
//! | `.whyshelf.toml` | the project's settings, committed; it cannot say where the private store is |
//! | `.whyshelf/` | the repo store: `events/`, `objects/`, `map.json`, `map.schema.json`, and the cache in `cache/` |
//!
//! The settings and what each key does are in the documentation of
//! `whyshelf_infra::config`; the stores in `whyshelf_infra::store_fs`.
//!
//! # `whyshelf migrate`
//!
//! It counts the versions in the project's data and sets them against the ones this
//! build reads. It opens no workspace and writes nothing, so it can be run on data the
//! other commands refuse. Today it only checks; when a format has a second version,
//! the plan to move the files over will be made from what it finds.
//!
//! ```text
//! $ whyshelf migrate
//! Versions:
//! WHAT              FOUND    BUILD      NOTE
//! settings          v1: 1    reads: v1  -
//! events (repo)     v1: 42   reads: v1  -
//! events (private)  v1: 7    reads: v1  -
//! map               v0: 1    reads: v1  derived: `whyshelf rebuild` writes it again
//! cache             v1: 1    reads: v1  derived: `whyshelf rebuild` writes it again
//!
//! verdict  readable: every file counted is of a version this build reads
//! ```
//!
//! The **sources of truth** decide the verdict: the settings file and the event files
//! of each store. The map and the cache are derived from them; their versions are
//! shown and decide nothing, and `whyshelf rebuild` writes them again. A version is
//! counted as it is found, so a file of a version this build does not read, one with
//! no version and one that is not JSON are listed with how many (`v2: 3`,
//! `unknown: 1`), not left out. The verdict is one of:
//!
//! - **readable**: every counted file is of a version this build reads, and every
//!   store was looked at. Exit status 0. A private store that is not there (a clone
//!   that has none yet) was looked at and holds nothing.
//! - **unreadable**: some file is of a version this build does not read. Exit status 1.
//! - **incomplete**: a store was not looked at, because its place cannot be decided:
//!   the settings file cannot be read as one this build reads, it has no
//!   `project_id`, or there is no usable data directory. The row says
//!   `not looked at: the place cannot be decided (why)`; nothing is called readable
//!   that was not read. Exit status 1.
//!
//! A directory without a settings file is not a project, and `migrate` there exits
//! with 2.
//!
//! # The tool server: `whyshelf mcp`
//!
//! `whyshelf mcp` speaks the Model Context Protocol on standard input and output, for
//! an AI agent or any client. It opens the project once and serves until the client
//! closes the connection or the process is interrupted. Point a client at it with the
//! project root:
//!
//! ```json
//! { "mcpServers": { "whyshelf": { "command": "whyshelf", "args": ["--root", "/path/to/project", "mcp"] } } }
//! ```
//!
//! **Every answer starts with a context line**, an error answer too:
//!
//! ```text
//! [context: project=demo, worktree=/path/to/project, revision=5d0e83c2a91f7b64e0c3d8a1f2b4c6d7e8f90a1b (feat/session)]
//! ```
//!
//! so a client that works on several checkouts can never take an answer for the
//! answer about another project. The rest is the result, in `markdown` unless the
//! call says `format` (`markdown`, `table` or `json`), then any warnings.
//!
//! One tool is one command, with the same name and the same meaning, and the same
//! ingest before it. Requests are run one after another by a task that owns the
//! workspace, so two clients' writes cannot interleave.
//!
//! | tool | the command | arguments besides `format` |
//! |---|---|---|
//! | `whyshelf_context` | `context` | none |
//! | `whyshelf_add` | `add` | `paths`; `kind`, `tags`, `alias`, `store` |
//! | `whyshelf_tag` | `tag` | `target`, `changes` (`["+area:auth", "-wip"]`) |
//! | `whyshelf_mv` | `mv` | `target`, `new_path` |
//! | `whyshelf_publish` | `publish` | `target` |
//! | `whyshelf_finding_new` | `finding new` | `finding_type`, `title`; `alias` |
//! | `whyshelf_spec_new` | `spec new` | `alias`, `from` |
//! | `whyshelf_design_new` | `design new` | `alias`, `spec` |
//! | `whyshelf_check` | `check` | `gate` (`cr`, `cs`, `ci`, `ca`, `cm`), `target`; `approve`, `revision` (ca), `result` (`pass`, `fail-wiring`, `fail-hyp`), `item_hint`, `finding` (cm), `by` |
//! | `whyshelf_revoke` | `revoke` | `approval`, `reason`; `by` |
//! | `whyshelf_get` | `get` | `target` |
//! | `whyshelf_find` | `find` | `query`; `limit`, `cursor`, `fields` |
//! | `whyshelf_status` | `status` | `limit`, `cursor`, `fields` |
//! | `whyshelf_trace` | `trace` | `target`; `limit`, `cursor`, `fields` |
//! | `whyshelf_changelog` | `changelog` | `target`; `limit`, `cursor`, `fields` |
//! | `whyshelf_audit` | `audit` | `query`; `limit`, `cursor`, `fields` |
//! | `whyshelf_rebuild` | `rebuild` | none |
//!
//! `init`, `schema`, `migrate` and `mcp` have no tool. A tool that lists takes
//! `limit`, `cursor` and `fields`, so that an answer cannot flood the caller's
//! context; an answer that has more says `next cursor` (`next_cursor` in JSON), which
//! is the `cursor` of the call that continues it. For `whyshelf_find` the cursor is
//! the last identifier returned and `fields` are the columns above; for the other
//! lists the cursor is a position and `fields` names the columns of the table.
//! The server's own advice to a client is to begin with `whyshelf_status`.
//!
//! # Limits of the command line
//!
//! - A command loads and evaluates the whole project at least twice (the ingest, then
//!   the command), reading every registered file and asking Git each time. It is
//!   made for projects of the size a person keeps in documents, not for thousands of
//!   files.
//! - There is no command to purge a stored text, to set an owner, or to change an
//!   alias after registration; the library has them.
//! - `status` and the other lists on the command line print everything; the paging
//!   options exist on `find` and on the tools.

mod actor;
mod args;
mod command;
mod error;
mod init;
mod mcp;
mod render;
mod run;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    run::main().await
}
