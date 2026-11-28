# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Each origin of `stale` carries `withheld`: the approvals by name that would
  settle it and the frame refuses now, each with the document it would be of
  (`doc`), which approval it is (`approve`: `pending`, `as_another` or `until`)
  and why the frame refuses it (`detail`). No command is written for one, and
  each is also a line on stderr; before, such an approval was dropped from
  `next` without a word (#184).
- `check` and `health` list, for each invalid file, `sides`: the identities the
  sides of a merge conflict in it show, when each shows one and they differ
  (empty for any other file). `reach` and `units` count such a file in `untold`
  as `sided`, with a warning on stderr, and the gate of `impact --within` lists
  it in `unreadable` as `sided_before` and `sided_after` and does not pass while
  one is there (#217).
- In the library (#217): `ReadId::OneOf` of a `Sides`, made by
  `ReadId::of_sides`; `MayHold`, the files that may hold a document no file
  carries; `Resolution::Shown`; `DocProblem::MaybeDuplicateId`;
  `InvalidDoc::sides`; and `sided` of `Unwalked`, `Unjoined` and
  `within::Unreadable`.
- Each entry of `unknown` of `check` and `stale` carries `kind`, why the name
  cannot be told: `told`, `no_answer`, `not_asked`, `not_a_name` or
  `not_of_sources`; the last has `read` (what was read), `reason` (`differs` or
  `gone`, each with `file`, `no_trace`, or `unread` with `what`) and `hint`
  (what to do) (#225).
- The contract of an adapter has a fourth promise, of its sources, and each
  adapter's page a heading `What it stands on` (#225).
- In the library (#225): `Untold::NotOfSources` of a `NotOfSources`, with its
  `NotOfSourcesWhy`, and `ArtifactReading::not_of_sources`.
- In the library (#184): `settle::Approval`, an approval of one document by
  naming it, `settle::Withheld`, one the frame refuses with its
  `ledger::FrameRefusal`, and `settle::Settled`, what can be said of an origin
  (`next` and `withheld`).

### Changed

- In the library (#184): `settle::settle_all` returns a `settle::Settled` for
  each origin, and `app::shelf::SettledOrigin` has a new field, `withheld`. The
  variants `Settlement::ApprovePending`, `ApproveAsAnother` and `ApproveUntil`
  are now `Settlement::Approve` of `Approval::Pending`, `AsAnother` and `Until`:
  code that names them no longer builds.
- `RawProblem` has a new variant, `ConflictMarkers`: a match over it that names
  every variant no longer builds (#166).
- `check` and `health` write no `id` for a file left with both sides of a merge
  conflict whose sides show different identities; they wrote the identity of the
  side the file opens with (#166).
- The presence of a document that cannot be told has `shown` beside
  `unreadable`: a file left in a merge conflict a side of which shows the
  document is listed under `shown`, and no longer under `unreadable`, which
  keeps the files whose identity cannot be read. Each end of `untold.shelf` of a
  verdict of `impact --within` is in that form, `{shown, unreadable}`, and no
  longer a list of files (#217).
- In the library (#217): `ReadId` is no longer `Copy`; `DocState::Unknown`,
  `DocFailure::Unknown`, `DocPresence::Unknown`, `lookup::Absence::Unknown`,
  `RekeyProblem::Unknown` and the ends of `within::UnknownAt` carry a `MayHold`
  in place of a list of files; `Resolution` and `DocProblem` have a new variant,
  and `ReadId` one more: a match that names every variant no longer builds.
- In the library (#225): `Untold` has a new variant, `NotOfSources`, and
  `Untold::as_str` returns a `Cow<str>`: a match that names every variant, or
  code that takes the text as a `&str`, no longer builds.
- The space `rust-item` tells the names of the docs of a crate that is none of
  the manifests it reads (a dependency's, or one of a package those manifests do
  not reach) as cannot tell, `not_of_sources`, and `moves` no longer finds
  candidates in them; it answered them and searched them (#225). The manifests
  read are the `Cargo.toml` at the root with its workspace, and, when the root
  has none, the `Cargo.toml` of each directory at the top of the tree with its
  workspace, so a project whose crate is in a directory of its own is answered
  as before (#228).
- Docs built for some crates of the project only, where a crate documented
  depends by a path on a crate of the project whose pages of the source they do
  not hold, cannot be told (`not_of_sources`, `no_trace`): what it re-exports of
  that crate has no copy of its files in them (#228).

### Fixed

- The space `rust-item` holds the docs `cargo doc` built against the sources of
  the tree before it reads them: every page of the source the docs hold of a
  crate of the project is compared with its file, and when one is not the same
  (an item added or removed since the docs were built), every name of those docs
  cannot be told (`not_of_sources`, with the file and what to do), and `moves`
  searches nothing. It told an item added since as gone, with moves to declare,
  and would have told an item removed since as there (#225).
- The holding of the docs of `rust-item` tells a directory of the docs that
  cannot be listed whole (an error, or a listing cut at its bound) as `unread`,
  with why; it was told as docs not built, or as `no_trace`, and the names of a
  crate of the project were given the hint of a dependency. A page under the
  root by its whole path is compared wherever the build directory is
  (`build.target-dir = "."` included); only the output of a build script under
  it is not, where every page under a build directory at or above the root was
  passed over (#228).

- A file left with both sides of a merge conflict whose text starts with the
  marker after a byte order mark is told as holding merge conflict markers too;
  it was told as having no front matter, which a book that skips such files
  passes over (#221).
- A file that a merge left both sides of a conflict in, where the conflict
  touches a front matter, is told by `check` as holding merge conflict markers,
  not as YAML that cannot be read, and is never skipped. When its sides show
  different identities, or one shows none, the documents of both sides are
  `unknown` in `status` until the conflict is resolved, not `gone` or `invalid`:
  the side the file opens with was told as invalid in that file, and the other
  side as gone. When every side shows the one identity, the file is that
  document's, and invalid. While such a file is in the tree, a document no file
  carries is `unknown` too, as with any file whose identity cannot be read
  (#166).
- A file left in a merge conflict whose sides each show one identity, and not
  the same one, is held to those identities: a document no file carries and no
  side shows is `gone`, not `unknown`; a document of a side is found by its
  identifier, also with no record (`get` tells it `unknown`); a record about one
  that names no book is read with the book of the file, not with the rule of the
  whole shelf; `status --book` lists one that has a record under the book of
  the file; and another file that carries the identifier of a side is invalid,
  as the file of the conflict is, each naming the other (#217).
- `judge retire` of a document a side of such a file shows, with no record,
  retires it as any document that cannot be told: the record keeps that it
  cannot be told and names the book of the file. Its identifier named nothing
  before (#217).

- This repository's books: the nine verifications that name the reader tests of
  whyshelf-infra whole under `tests` name again the helper file those tests use.
  A file named whole pins only its own bytes and follows no helper, so its
  helpers are named with it; 0.2.0 had dropped the name, and an edit of that
  helper made none of them stale (#205).

## [0.2.0] - 2026-11-27

### Added

- `ShelfApp::book_links` (#120): the names each document of a book writes in its
  links to artifacts, read once, with the files of the book that are no valid
  document; `Shelf::invalid_files` and `Shelf::invalid_files_in`, the one place the
  files that are no valid document are joined.
- `rust-test` lists the tests of the files a selection takes (#120), so a link to
  tests can declare a cover: a selection is globs over the paths of files, as for
  `file`, and the files are those the state of the project lists at the side, chosen
  by the same code as for `file`. A test is named as the source spells it. The
  listing is not whole where something may make a test the scan does not see (an
  item macro; an attribute that is not inert on any item, an `impl` or a `struct`
  among them; a derive other than those of the standard library; an attribute
  beside the test attribute of a test), and the reason names the file and what in
  it; past the files or the bytes one listing reads, it stops and says so.
  `#[proc_macro]`, `#[proc_macro_derive]`, `#[proc_macro_attribute]`,
  `#[panic_handler]`, every other built-in attribute of the language that is no
  macro (the Reference's index of built-in attributes: `repr`, `non_exhaustive`,
  `used`, `link`, `naked` and the rest), an `#[unsafe(…)]` of an inert attribute and
  a `derive` of the standard library's derives only (`INERT_DERIVES`, by a bare name
  only when no `use` of the file names an item so) are now inert, so they make no
  file's listing partial, nor a test not found one that cannot be told. A test
  attribute that is a path (`#[tokio::test]`) and a module read by `#[path]` from a
  file that is not `.rs` keep a listing from being whole. The page of the space
  `file` now says its listing is of the files the state lists (through Git, those of
  the commit or the index).
- `impact --within <ref>... [--exit-code]` (#174): whether a change stays inside the
  documents it was meant for. For every document `impact` answers, `within` gives a
  `verdict` (tagged `verdict`: `not_claimed`, `inside`, `outside`, `unproven`,
  `undetermined`, each with what it carries) against the reach of the documents named
  as the project stood before the change, and whether the gate `passes`. The change
  reaches a document when it adds a cause that makes its approval not hold
  (`adds_cause`), takes one away (`removes_cause`), changes what was decided about it
  (`decided`), puts it on the shelf or takes it off (`on_shelf`), changes its content
  or what its links name (`edited`), or adds, withdraws or rewrites a record about it
  (`recorded`); the two holdings differing is not read. A file of the books whose
  identity cannot be read, or a file of the ledger that is no record, at either end,
  keeps the gate from passing (`unreadable`). A reference is read at the end before,
  or at the end after when it names nothing before; one that names no document is a
  request error (exit 2). `--exit-code` exits 1 unless the gate passes, and requires
  `--within` (exit 2 without); `--within` alone exits 0. The verdicts, the rule of the
  gate and the known limits are on the page of `domain::shelf::within`, which also
  holds `Within`, `Verdict`, `VerdictKind`, `Witness`, `ClaimUntold` and `Unreadable`;
  beside them `ShelfApp::impact_within`, the field `ImpactReport::within`,
  `ledger::Record::about` and `doc::Doc::links_named`.
- The command `impact` (#112): whether a change makes approvals not hold. It reads
  the project before and after the change the same one way, named by the flags:
  through Git (`--since <rev>`, and `--until <rev>` or `--index`; the default after is
  `HEAD`), reading the tracked content as a checkout of the repository writes it, each
  commit converted by its own `.gitattributes` and no filter driver run (the attributes
  of a person's settings and of the system are not read); or through two directories
  (`--before <dir>`, and `--after <dir>`, by default the project root), which whyshelf
  does not make. Both ends are judged at one time. For every document either end
  lists (`docs`) it answers whether its approval held (`before`) and holds (`after`),
  by the rule `stale` uses (`holding`: `not_there`, `pending`, `against`, `holds`,
  `cannot_tell`, `does_not_hold`), whether the two differ (`changed`), and whether the
  change adds a cause that makes it not hold (`made`): `yes` with the causes added
  (`certain`, and `uncertain`: on a name the side before could not tell), `no`, or
  `cannot_tell` with the side and the names that could not be told. A cause is the
  document that holds it and the cause itself, so a new way to the same cause is no
  new one, and a new edit of what already did not hold is a cause added: `yes` does
  not say the approval held before. Its `way` says how it read. Both ways at once or
  neither, a revision that names no commit, Git outside a repository, a Git that does
  not know a form the reading needs (`--attr-source` among them), an index whose
  `.gitattributes` are not `HEAD`'s, and a directory that is not there are request
  errors (exit 2); no Git to run, a Git that fails, and a declaration, a document or
  the ledger reached through a link out of the project, which the two ends cannot
  read alike, fail the run (exit 3). Nothing is written. Known limits: through Git,
  nothing is built, so an approval that stands on a `rust-item` cannot be told; a
  submodule and a path of a conflict in the index cannot be told; a path added with
  `git add -N` reads at the index as an empty file; a file a filter driver names is
  read as a checkout that runs no driver writes it, and the attributes of a person's
  settings do not convert a file; each converted file costs one process of Git.
- `stale`'s gate is now the fold of each document's holding (`Gate::fold` of
  `Gate::from`), made as each document is asked whether its approval holds; the
  entries and the names that cannot be told are made from that one answer per
  document. In the library, `StaleReport` carries the fold (`held`), which
  `StaleReport::gate` answers. Its answer does not change.
- The command `units [--pending]` (#113): the approvals that no longer hold (with
  `--pending`, the pending documents too) divided into units of work that share
  nothing, at the side after, so that each unit can be mended and approved again
  apart from the others. Two documents are in one unit when one stands on the other
  (its `Depends` links, or what its approval pinned) or its `Depends` link names the
  other, which it cannot stand on (a document that is not valid, or a key two files
  write); when they stand on artifacts that live in one file, whole or in part (two
  tests of one file are one unit); or when the same upstreams outside the documents
  divided hold both back, directly or through what holds such an upstream back in
  turn (that chain joins as the documents divided do, and stops at what holds nothing
  back: an approval that holds, an upstream a new approval may not stand on, and a
  document divided). Documents that wait on one upstream decided against, or on one
  document their links cannot be followed to, are one unit, and so are those whose
  links write one key no document has; such an upstream is followed no further.
  Each unit (`units`) lists its `documents` in the order
  `judge approve` takes them, each with its `standing`; the `files` its artifacts
  live in; its `artifacts`, each with the `documents` that stand on it and its
  `place` as `reach` writes it; and `waits_on`, the upstreams outside it to mend or
  approve first, each once, with `waits`: `held` with its `hold` (`pending` or
  `awaiting`), `refused` with its `status` (`denied`, `deprecated`, `retired`), or
  `unfollowed` with `why` (`invalid`, `ambiguous`). An artifact whose place
  cannot be told joins only what stands on the same name, and is listed in
  `unplaced`. The files of the documents and of the declaration are no key, so two
  units may write the declaration. What could join units without being seen (a
  document whose links cannot be read that is divided, or that a document divided or
  of the chain stands on or names; a file whose identity cannot be read) is counted
  in `untold` (`invalid`,
  `unreadable`, as `reach` names them), with a warning on stderr. Its schema is
  `outputs.units` of `schema`; the JSON stays version 1. It writes nothing, and the
  exit is 0.
- In the library (#113): `units`, the division of what does not hold (`UnitsOf`,
  `plan` and `UnitsPlan` with `asks`, `divide`, and the answer `Units` with `Unit`,
  `UnitDoc`, `UnitArtifact`, `UnplacedArtifact` and `Unjoined` through
  `Units::untold`), and `app::shelf::ShelfApp::units` with its answer `UnitsReport`.
- The command `reach <ref>...` (#112): what depends on the documents named and what
  all of them stand on, at the side after. Its answer lists every document whose
  `Depends` links reach one named, transitively (`docs`, each with `named`, its
  `standing` and the `files` it may be in); the artifacts each stands on, what its
  links to artifacts name now and what the approval that stands about it pinned
  (`artifacts`, each with `by` and its `place`: `only` these files, `partly` these
  files, or `unplaced` with why); the files of the project all of it is in
  (`files`); the artifacts with no place (`unplaced`) and the documents that are
  gone (`gone`) apart; and, when the walk could not read what may depend on them,
  how many (`untold`: `invalid`, documents not reached whose links cannot be read,
  and `unreadable`, files whose identity cannot be read), with a warning on stderr.
  Its schema is `outputs.reach` of `schema`; the JSON stays version 1. A reference
  that names no document, or several, is a request error (exit 2).
- Where a name of a space lives in the files of the project (#112), the fifth ask an
  adapter may answer: `app::shelf::NamePlaces` (`places`, at a side), the
  not-answering `app::shelf::CannotPlace` and `app::shelf::PlacesWay`, the answer
  `placement::Placement` (`Only`, `Partly`, `Unplaced`, with `files`), and
  `app::shelf::ArtifactSource::places`, which the dispatch answers (a space with no
  adapter is unplaced, each name, with why). A placement is the files the content of
  a name is read from, by the rule its reading follows links by: through a link inside
  the root, the link entry and the file it leads to; through one that leaves the root
  or is broken, none, with the reason the reading gives. `file` places a name in the
  file it writes; `rust-test` and `js-test` in the file a test name writes before
  `::`; `rust-item` in part in the file its page in the docs links to as its source,
  under the directory the manifests of the project (`Cargo.toml` at the root and of
  the members of its workspace) give its crate, which must be a file of the project.
  In the library,
  `app::shelf::ShelfApp::reach` and its answer `ReachReport` (with `ReachedDoc` and
  `ReachedArtifact`).
- In the library (#112): the state of a tree of names at one side of a change,
  `app::shelf::state` (`Side`, `StateRead` with `read`, `look`, `read_outside`,
  `read_beyond` and `addresses`, `StateList` with `list`, `list_outside`,
  `list_beyond` and `files`, `AtSide`, `NameAnswers`, `Leads`, `DirListing`,
  `NotADir`, `Entries`, `EntryKind`, `Unnamed`, `Beyond`, `RawPart`,
  `TreeFiles`, `ReadWay`, `ByteBound`), and its adapter for a directory of the
  file system, `infra::shelf::state` (`files::FileStates`, at one side with
  `new` or at two with `between`, with `root`, `dir`, `path_of` and
  `with_timeout`; `files::LEAVES`; `StateError`); every reader of a project
  reads its tree through it. The constructors `BooksToml::on`, `MarkdownDocs::on`, `JsonLedger::on`,
  `FileSpace::on`, `RustTestSpace::on`, `JsTestSpace::on` and
  `RustItemSpace::on`, which read through a given state.
- In the library (#112): `reach::Unwalked`, what a walk down from named
  documents could not follow, counted (`invalid`, `unreadable`).
- In the library (#182): `settle::origins_asked`, `order::OriginsAsked`,
  `order::Foreseen` and `settle::settle_all`, the way origins are settled with
  what is foreseen of them.
- In the library (#176): `ledger::Ledger::may_stand_on`, whether a new approval
  may stand on an upstream, and `ledger::ApproveFrame::foresee`, whether the
  frame of an approval is refused for what the shelf and the declared statuses
  tell.
- In the library (#175): `ledger::Framed`, the edit `judge same` frames, which
  tells a second declaration by the same name (`Framed::not_declared_by`), and
  `ledger::NotComparedWhy`, why a pin is not compared now.
- In the library, the view of an approval tied to its ledger and what a pinned
  content comes to (#153): `ledger::Ledger::declared` and `ledger::DeclaredNow`,
  `ledger::StandingApproval` (`view`, `reading`, `quorum`), `ledger::ApprovalView`
  (`subject`, `judgement`, `at`, `actor`, `attribution`, `seen`, `seen_address`,
  `frame`, `artifacts`, `expires`, `revision`, `read_pins`), `ledger::Approval`
  (`judgement`, `frame`, `expires`), `ledger::RecordOrder`; `drift::Edit`
  (`then`, `now`), `drift::DocDrift`, `drift::ArtifactChange`;
  `snapshot::SnapshotItem::compared` and `kind`, `snapshot::ItemKind`,
  `snapshot::DocRole` (`id`, `seen`, `address`), `snapshot::ItemCompared`
  (`view`, `changed`, `now_hash`, `artifact_now`, `item`), `snapshot::ItemView`
  and `snapshot::HashNow` (which is `Display`).
- `moves` suggests where a gone name of the space `rust-item` moved (#142), from
  the names the built docs hold now: a candidate ends as the gone name does, in
  the most segments of its crate, and is `same_content` when its promise hashes
  to what was pinned, `same_end` otherwise; a member needs a type whose page has
  it. A name keeps at most 16 candidates (`candidates_cut` says how many were
  found), the search is held to bounds that are notes, and docs whose place is
  not known are `search_unavailable`. A move of a prefix that two gone names or
  more carry among their candidates is tried as `judge moved --prefix` would
  plan and frame it, with no admission tried: `prefixes` lists those that stand
  (`{"from", "to", "moved", "not_asked"}`), `refused_prefixes` those refused
  (`{"from", "to", "refused"}`) and `untried_prefixes` how many were past the
  bound of 32. The JSON stays version 1, with these fields added. In the
  library, `FoundBy::SameEnd`, `MoveCandidate::under`, `prefixes` and
  `cmp_rank`, `MoveSuggestions::prefix_trials` and `offer`, `PrefixSuggestion`,
  `RefusedPrefix` and the notes `SearchUnavailable` and `CandidatesCut` are new,
  and `PinnedNames::gone` takes `&self`. The notes of a search that stopped at
  a bound or passed over an entry say entries (files, directories or pages)
  rather than files.
- Who read the content an approval stands on is answered for every approved
  document, in every project, and the approvals read by one name can be asked
  for (#105). A row of `status`, `get` and `trace` of an approved document carries
  `readers`: `{"count", "counted", "lapsed"}`, the names that approved the
  content it stands on now, each once. A name counts while its own latest
  approval of that content has not lapsed and is `lapsed` otherwise, so
  `count` is `nobody`, `one` or `several`, and an approval every reading of which
  lapsed is `nobody`. `status --readers <word>` (repeatable, any of the words)
  lists the approved documents read by that many names: `--readers one` is the
  approvals no second name reads now (a lapsed reading is not counted), after an
  edit approved again by one name too. A project needs no rule of who may decide for it: a second name's
  approval of the same content is written whether or not it declares one. In the
  library, `Ledger::reading` gives the readers (`ReadBy`) and the `Quorum` from
  one walk, `Standing::read_by` reads them, and `StatusFilterAsk::readers` asks
  for a `ReaderCount`; `StatusFilter::admits` takes the `Standing` of the
  document. The documentation of `Readers` no longer says the key of an upstream
  is compared: the content an approval stands on is the pin's link, identity and
  hash.
- `rekey <ref> [--key <key>] [--follow <ref>]... [--stay <ref>]... [--both
  <ref>]... [--dry-run]` gives one document a new key (#117), for two documents
  of one key after a merge. The new key is the book's next (or `--key`, which
  must be free), the file is where `new` would put it, the identity stays, and
  the document's `key:` and the links that mean it are rewritten; its approvals
  hold, as the ledger pins identities, and it is not written. A link writes the
  old key as text, so which document it meant is read from the approval that
  stands about the document that wrote it (what it pinned), or said with
  `--follow` (the renumbered document), `--stay` (the other) or `--both` (the link
  keeps the old key and gains the new one); a document with neither, or a word
  against the approval, is refused with every such document listed, and
  nothing is written. Files that could not be read and may write the key or link
  to it (a file that still holds conflict markers) are listed and stop it too.
  The files are written in the form `fmt` writes, held against the plan before
  any is written, and all or none: a stop puts back what was written, and when
  that cannot be done the files not put back are listed (exit 3). `--dry-run`
  writes nothing. The answer lists what moved, rewrote, stayed and keeps both,
  the documents whose body names the old key (not changed), whether each
  approval holds after the write, and the files written; `schema` describes it.
  In the library, `ShelfApp::rekey` takes a `RekeyRequest` and gives a
  `RekeyReport`; the plan, its reading of each link and its check of the files
  are `domain::shelf::rekey`.
- A cover over artifacts (#111). A link to artifacts may declare `cover = {
  strength = "error", include = ["src/**/*.rs"], exclude = ["src/gen/**"] }`
  (`strength` is `error` when left out): every name its selection takes in a
  space that lists its names, and that is there now, is expected to be reached
  by a pending or approved document that writes it in that link. Only the space
  `file` lists its names (the regular files Git lists, tracked or untracked and
  not ignored); a `cover` on a link to a space that does not, a strength alone
  (`cover = true`) on a link to artifacts, a table on a link into a book, and a
  selection the space does not read are problems of the declaration. `check`
  lists a name no document reaches in `artifact_uncovered`, and each covering
  link, with its selection and whether every name it takes is listed
  (`completeness`), in `artifact_cover`; it fails (exit 1) for a gap of an
  `error` link, and also for an `error` link whose listing is not the whole (too
  many files, a file a reading cannot tell or whose path is no name, or no Git
  work tree) or whose selection takes no name, since a gap
  cannot be ruled out then; a `warn` link is listed, written on stderr and leaves
  `check` clean. `map` and the diagram show a covering link's `selection` (an
  added field; the answers' version stays 1), and `cover` is true of a covering
  link to artifacts as of one into a book. `whyshelf cover [--link
  <book>.<link>]` tells, for each covering link, its selection, whether the
  listing is the whole and which names are reached, exempt or gaps; it fails
  nothing (exit 1 only for a declaration that cannot be used, as `map`).
- `judge exempt --artifact <name>... --from <book>.<link> --reason <text>`
  records that an artifact is not expected to be reached by a covering link to
  artifacts, by the rule of an exemption of a document: needed the moment it is
  written, so a name the selection does not take now, one a document reaches, one
  whose names cannot all be listed, and one already exempt are refused. It does
  not move with the file: after `judge moved` it is no longer needed (`why` is
  `not_listed`). `check` lists them in `artifact_exempt`,
  `artifact_exemption_unneeded` and `artifact_exemption_repeated`, and warns on
  stderr as it does for documents. The record is the exemption's, with the
  subject a map of the `space` and the `name` instead of the document's identity;
  a record of a document keeps its form and hash. A build from before this
  cannot read an exemption of an artifact, and its `check` fails on it (its
  other commands act as if the record were not there).
- The manual has a section "Working from several branches" (#132): what to push
  together, when to fetch and run `check`, which document to renumber when two
  branches took one key and what renumbering rewrites, to link only keys the
  main branch or the branch itself holds, and what to do after two branches approved one document.
  The README's "In CI" says to run the gate on pull requests too, on the result
  of their merge, and how to keep that result current once the main branch
  moves.
- A record keeps who ran a judgement apart from who answers for it (#156).
  `--ran-by <name>` on every `judge` verb, else the variable `WHYSHELF_RAN_BY`
  (a blank value is none), names who ran the command when it is not the actor,
  such as a coding agent running it for a person; there is no other source, so
  neither Git's `user.name` nor a file of the project names one. A record of any
  kind keeps it as an optional key `ran_by` only when it is another name than
  the actor's, so a record written before, and one whose command the actor ran,
  keep the form and the hash they had. A `--ran-by` that is the actor is one
  name: no key, and the answers say `null`. `ran_by` is told and read by no
  judgement: who may decide, who has read a document, what an approval repeats
  and the runs of `stats` read the actor alone. The answers that show a
  record's actor (`decision` of every row, `get --approved`, `exempt` and
  `exemption_unneeded` of `check`) show `ran_by` beside it, `null` when the
  actor ran the command. Like the actor, it is as declared: a variable a
  committed file sets (a direnv `.envrc`, say) is not told from one a person
  set.
- The stale report says whether a gate can say the approvals hold (#151). In
  the library, `StaleReport::gate` answers `Gate::Holds`, `Gate::DoesNotHold`
  (an approval that does not hold, or only expired) or `Gate::CannotTell` (one
  pins a name that cannot be told now), as DEC-26 decides, so a program that
  uses the library no longer writes that rule again; `stale --exit-code` maps
  it to its exit status and behaves as before.
- Who may decide (#114). `.whyshelf/books.toml` may name groups of actors
  (`[[group]]`, each member an exact name, with an optional `from` and `until`)
  and declare, for a book (`[book.decide]`) and for the records of no book
  (`[decide]`, required once a book declares a rule), which groups may write
  each kind of judgement (`approve`, `deny`, `deprecate`, `retire`, `revoke`,
  `same`, `exempt`, `moved`), a `default`, and a `since`. `judge` refuses a
  record whose name the rule does not admit, exit 1, and writes nothing;
  `judge approve --stale` and `--pending` leave such a document out as
  `not_admitted`. A record in the ledger the rule does not admit is not used and
  is a `ledger_problems` entry of `check` (exit 1) until `judge withdraw` by a
  name admitted to `revoke` takes it back. There is no authentication: the rule
  is about the name written, as declared. A project that declares no rule takes
  any name and writes records of the same form as before. In a project that
  declares a rule, decisions and declarations of the same meaning of a document
  name their document's book in an optional record key `book`; a record without
  it keeps its form and hash, and is read with the book its document is in now,
  so a rule declared over an earlier ledger is best given a `since`. A build
  from before this cannot read a record that has the key. In
  the library, `Ledger::build` takes the admission (`Admit::of(&shelf)`, or
  `Admit::unruled()`), and `LedgerAppend::append` takes an `AdmittedRecord`,
  which only `Ledger::accept` makes.
- A second approval by another name (#114). A rule may ask for `approvals = N`
  (1 to 10, 1 when not given): an approval under it holds once N distinct
  names approved the same content, each counted while its own latest reading
  has not lapsed. Until then the document is `awaiting` in `status` (and
  `status --standing awaiting`), listed by `check` (exit unchanged), and not
  failed by `stale --exit-code` itself; what stands on it is stale
  (`upstream_declared`, `unseconded`), `judge approve --stale` holds it back
  (`stands_on`, `upstream_is` `awaiting`), and `stale` offers approving the
  upstream with another `--actor`. A same meaning joins the content it
  declares once N names declared that one edge (the top-level rule's
  `approvals` for an artifact); `check` notes a short edge that a standing
  approval meets (`same_meaning_short`), and a name declaring an edge again
  writes nothing. The declaration is a problem when no N names may approve or
  declare the same meaning, when the top-level `approvals` is below a book's,
  or when its `since` is later than that of a book asking for more than one
  name. A project with no rule counts nothing and answers as before. In the
  library, `Standing::Approved` holds `freshness` and `quorum`, and
  `FreshnessLevel`, `UpstreamStatus`, `OriginEvent`, `Settlement` and `Hold`
  have a new variant each.
- The preset `rustdoc-promise` (`space = "rust-item"`, `compare = "lines"`,
  `means = "depends"`) pins what a Rust item promises: its declaration and its
  docs, read from the page `cargo doc` wrote for it. An edit of a function's
  body, of an inherent method or of an impl leaves an approval that names the
  type fresh; an edit of the declaration or of the docs, docs written for an
  item that had none, or docs taken away, makes it stale (`artifact_changed`).
  The declaration of a type as rustdoc writes it lists its public fields, its
  variants and, for a trait, its items' signatures, and that of a constant holds
  its value, so a change of those is a change of the item's promise; a member
  named on its own is its own promise. For a variant its fields count too, and
  for a module, a crate, a primitive type or a keyword only its docs. The text
  compared is each line of the declaration after `decl ` and each line of the
  docs after `doc `, with the line breaks rustdoc wrote; whether white space and
  blank lines count is the link's `compare` (`lines` collapses white space and
  leaves out blank lines, `bytes` does neither). An item whose page cannot be
  read as that (no declaration where its kind has one, a layout not read, an
  item whose own page is too large) is there for `presence` and cannot be told
  for any other comparison, with why; a member is found by reading its type's
  page, so a too-large page of the type makes its members cannot-tell for
  presence too. `rustdoc` is unchanged (`presence`); a link that changes from
  one preset to the other is a change of comparison
  (`artifact_comparison_changed`). In the library, `RustItemSpace::resolve`
  gives the reading of the text of the promise (`ArtifactReading::of_bytes`)
  instead of presence only, and `RustItemSpace::current` returns that text (none
  when the item is absent or cannot be told), so the library's `diff` report
  item carries it for a link of any comparison, `rustdoc` included; the `diff`
  command shows none, since no text is kept at approval. rustdoc's HTML is no
  contract: a toolchain that writes a declaration another way can make an
  approval stale; the end-to-end tests run
  `cargo doc` and watch the layout of the pinned toolchain (#104).

- Retiring a field, a link or a link to artifacts from a book. `status --with
  <field>`, repeatable, lists the valid documents of a book that declares the
  field and holds a value for it, whichever (one more condition beside
  `--where` and `--without`; the answer's `filters` has `with`); `--with` and
  `--without` of one field is a request error (exit 2), as no document could
  pass. `fmt --drop <name>`, repeatable, takes the entry of that name out of
  each file whose book declares no field, link or link to artifacts of that
  name, whatever the entry holds; an entry of the name in a book that declares
  it, and every entry not named, stay, and a file `fmt` leaves alone (a
  comment, read-only, a hard link) keeps its entry. With `--check` it writes
  nothing, and the answer's new `dropped` lists, for each file written (or
  that would be), the names it loses. `id`, `key` and a name every book
  declares are request errors (exit 2), each told, and so is an empty name;
  nothing is written. `fmt` without `--drop` does what it did. To retire a
  field: `status --with f`, take it out of the declaration (the documents that
  still write it are not valid, and their approvals broken, until the entry is
  out; then those that held a value have another hash, so their approvals are
  stale), `fmt --check --drop f`, `fmt --drop f`, `check`. In the library,
  `FieldWant::AnyValue`,
  `DropAsk`, `DropNames` and `DropProblem`, `ShelfError::Drop`,
  `FmtReport::dropped` (`Dropped`), and `FormEntries::dropped`;
  `ShelfApp::fmt` and `fmt_check` take a `&DropAsk`, and `Shelf::form_entries`
  a `&DropNames` (`DropNames::none()` for what `new` writes) (#143).

- `judge moved --prefix <old> <new> --reason <text>` declares, in one run, that
  every name under a prefix moved to the same name under another, as when a
  directory or a Rust module is renamed. Every name an approval that stands
  pins, in any space, that is under `<old>` is declared moved, one declaration
  each, so that `judge withdraw` takes one back. Where a prefix ends in a name,
  and what the name is under `<new>`, is each space's own grammar, said by its
  adapter: a part of a path is never cut, a Rust path breaks at `::`, the
  titles of a test are read as in a name and keep the spelling they have, and
  one `/` at the end of a directory is as none. A space whose names the prefix
  is not a prefix of has none under it. Every old and new name is held to the
  rule of `judge moved` in one reading, and the run is all or none: each name
  that cannot be moved is listed with why in `refused` (exit 1 when one is
  still at its old name or nothing is at its new one, exit 2 when each is a
  wrong request, such as a new name its space cannot write), and a prefix
  nothing pinned is under is a request error (exit 2).
  In the library, `ShelfApp::moved_prefix`, the port method
  `ArtifactSource::renamed`, and the domain's `plan_prefix_moves` and
  `frame_prefix_moves` (#103).

- Each origin of `stale` carries `next`: the commands that can settle it, as
  they are typed and in the order a person would try them (read the diff,
  declare the edit the same, look for moves and declare one, approve a pending
  upstream, approve again with an expiry, or approve again what an edit made
  stale), with the run's `--root`. A command is offered only where it can
  settle that origin, and whyshelf runs none of them. In the library, an
  origin keeps what happened there and the approvals that hold its causes, and
  `settle::settlements` lists what settles it (#102).
- `status --book <name>`, `--standing <word>` and `--where <field>=<value>`,
  each repeatable: the answer has only the documents that pass, with the
  filters beside the rows. A book or field the declaration does not have, or a
  value no book that declares the field can read, is a request error (exit 2)
  with every problem. In the library, `ShelfApp::status_filtered` beside
  `status`, which is unchanged (#101).
- `new --from <file>` (`-` for standard input) writes several documents in
  one run, from JSON lines, one document per line, whose shape `schema` prints
  under `inputs`. The shelf is read once; each line takes a key as `new` does
  (`key` claims one, none takes the next free number), may be named with `as`
  so that other lines link to it by `{"as": "<name>"}`, and every line is
  judged before anything is written: one refused line makes the run write
  nothing, and every bad line is named. The answer lists the documents
  written and the heads of the run. A file that cannot be read is a failed
  run (exit 3). In the library, `Shelf::plan_new` plans a run and
  `ShelfApp::new_docs` writes it; `new` of one document and `init` go through
  the same plan (#125).
- The field type `line`: a text of one line. A line break in it, the one a YAML
  block scalar (`|`, `>`) leaves at the end included, is a problem; `|-` and
  `>-` write none. A `line` hashes as a `text`, so declaring a `text` field
  `line` changes no approval. The declarations `init` writes declare `title` a
  `line` (#128).
- `status --without <field>`, repeatable: only the valid documents of a book
  that declares the field and holds no value for it (left out, or written with
  nothing in it). The answer's `filters` has `without` beside `where`. A field
  asked both with `--where` and `--without`, and `--without` of a field that
  every book declaring it requires (of the books asked with `--book`, when any
  are and one of them declares it), are request errors (exit 2), as no document could pass. In the library,
  a `FieldAsk` holds a `FieldWant` (`Value` or `NoValue`), and `FilterProblem`
  has `Contradicts` and `AlwaysWritten` (#136).

### Changed

- A listing says which parts it cannot list whole, and a cover can take each part
  whole (#120). `rust-test` answers a file it cannot read, or in which something may
  make a test, as a part of its listing: the file whole in the space `file`, with
  why and the tests found in it, or that it could not be read. A cover may declare
  `parts = { whole = "<link>" }`, a link of the same book to artifacts that depends
  and compares bytes or lines: each part, one read with no test found in it too, is
  a gap until a document names the file under that link (or `judge exempt
  --artifact` exempts it, by its name, from the covering link, with a reason, which
  stands only while no test is found in it: one found later makes the part a gap
  again and the exemption is told as `names_found`), and a
  part in another space than that link's keeps `check` from being clean and leaves
  the link assessed in nothing. A cover with no `parts` reads such a listing as not
  whole, as before. In the library, `NameListing` has `Parted` (`ListingPart`,
  `PartRead`), `CoverDecl::Table` has `parts`, `CoverTarget` has `Part`, `LinkCover`
  tells what it is assessed as (`Assessed`: `Whole` with its names and parts,
  `NotWhole`, or `Unsound` with its `CoverProblem`s), and `UncoveredArtifact` says
  whether it is a name or a part (`UncoveredOf`), and `Unneeded` has `NamesFound`;
  the answers of `check`, `cover` and `map` add `parts_whole`, `assessed_whole`,
  `problems`, each part, and `of` and `whole` of each artifact nobody reaches.
- In the library (#112): a `SpaceAdapter` names how it answers where its names
  live (`type Places`, `Answers` with `NamePlaces` or `CannotPlace`), and an
  `ArtifactSource` answers `places`.
- In the library (#112): the ports that read the project take the `Side` of a
  change they read: `BookSchemaSource::declarations`, `DocSource::read_docs`,
  `forms`, `current` and `addresses`, `LedgerRead::read`, and
  `ArtifactSource::resolve`, `current` and `list`; so do the asks of a space that
  read, `SpaceAdapter::resolve` and `current` and `NameList::list`. Every command
  reads `Side::After`, at a time read once from the clock (`load_at` in the
  application). `ShelfRead::way` names how a run reads. `JsonLedger::new` takes
  the root of the project, from which records are both read and added, where it
  took the directory of the ledger.
- In the library (#112): `BooksError`, `DocsError` and `LedgerError` have an
  `Untold` variant and a `State` one; `ArtifactError`, `RustTestError`,
  `JsTestError` and `RustItemError` a `State` one.
- A kind of file that is none of a file, a directory or a link (a pipe, a socket,
  a device) is not opened by any reader (#112): in the place of the declaration,
  a record, the directory of the ledger or of a range, or a document's file, it
  fails the reading as a file that cannot be read does; among the entries of a
  range it is passed over, as a link is; and a name of a space it is told by
  cannot be told, where the space `file` answered absent.
- The architecture rules `readers-read-through-the-state` and
  `readers-ask-not-git` hold the modules that read the project to its state
  (#112).
- In the library (#182): `settle::settlements` is no longer public;
  `settle::settle_all` settles the origins `settle::origins_asked` was given,
  with the `order::Foreseen` of them.
- `check` lists as short only the declarations a pin that `stale` reads now
  meets: no longer one met only by a pin of an approval whose document is not
  valid (its own hash included), by an artifact's pin under a link that no
  longer writes the name, or by one whose comparison changed; the order is as
  before (#175).
- `judge same` tells nothing behind a name, or a name that cannot be told, by
  the pins that hold a hash of it: a name no pin holds a hash of is nothing to
  declare (exit 2), where it was refused as not there or not told (exit 1); a
  pin `stale` does not compare now is refused, saying why: a link that no
  longer writes the name or a document that is not valid (exit 1), or a
  comparison that changed (exit 2, as before) (#175). In the library,
  `frame_same_meaning` returns a `Framed`, `Ledger::same_meaning_short` takes
  the shelf, and `SameMeaningRefusal::PinnedUnderOtherComparison` is
  `SameMeaningRefusal::NotComparedNow`.
- What a pinned content comes to now is decided once for each kind of content,
  from the approval that stands, and `stale` and `diff` read it (#153). No
  freshness, cause, notice, answer or record changes. In the library,
  `DeclaredStatus::Approved` holds an `Approval` (`judgement`, `frame`,
  `expires`) in place of its fields, and no longer carries `seen` or `artifacts`:
  a library user goes through `Ledger::declared`, `StandingApproval::view()` and
  `ApprovalView::seen()`, `artifacts()` and `read_pins()`; `Before::Record`
  holds a `RecordOrder`, in place of the fields `at` and `id`, which has no
  public constructor, so `Before::decision` is the way to make one;
  `Readers::approvals`, `Readers::standing`
  and `Readers::latest_of` give `ApprovalView`s, not `Decision`s
  (`Readers::actors` and `Readers::at` keep their form), and `Readers` is no
  longer `PartialEq` or `Eq`; `DiffItem` holds `compared: ItemCompared`,
  `approved` and `current`, and no `item` (it is `ItemCompared::item`), `now`,
  `file` or `changed`; a `SnapshotItem`
  says what it is and what was seen of it by `kind()`, an `ItemKind`:
  `ItemKind::Doc(DocRole)`, where `DocRole::Subject { id, seen, address }` is the
  approved document and `DocRole::Upstream(FramePin)` an upstream, or
  `ItemKind::Artifact(ReadPin)`. The doc of
  what an item of `diff` compares no longer says it compares as freshness does:
  it compares the content as it was approved the way it was pinned, whether or
  not `stale` compares it now.
- What was decided against a document is read once from its record, and what an
  upstream is to what stands on it is one reading (#150). No decision, count,
  strength, event or run changes. The words of a cause of an approval on an
  upstream no decision stands about change: `pending (its approval was
  withdrawn)` says `pending (no decision stands about it)`, since the value says
  only that. In the library, `DeclaredStatus` has `Pending`, `Approved` and
  `Against { judgement, status: NotStanding }`; `Standing` has `Pending`,
  `Against(Against)` (`Denied`, `Deprecated { successors }` and `Retired`, with
  `Against::status`) and `Approved`; `UpstreamStatus` is `Unapproved(Unapproved)`
  (`Pending`, `Superseded { successors }` and `Refused(Refused)`, which is
  `Denied`, `Retired` or `Unreplaced`) or `Unseconded`; `Cause::Via` holds
  `carried: Carried` (`Stale` or `Broken`) in place of `severity`, so that no
  `Via` holds an expiry; `Unneeded::DecidedAgainst` and
  `ExemptRefusal::DecidedAgainst` say which decision it was. `NotStanding` moved
  to the ledger's module and keeps its path `ledger::NotStanding`. The module
  `cover` (`cover::Decided`, `cover::counts`, `cover::covering_reach` and
  `cover::Reached`) is no longer public: nothing outside the crate read it.

- What an approval can pin of an artifact is told apart from a name that cannot
  be told in one place, and a snapshot keeps a document that cannot be told apart
  from one that is gone (#152). No answer, JSON or stored record changes. In the
  library, `ArtifactSeen` (there or absent) is new: `PinnedArtifact::state` returns
  it and `SeenHash::Artifact` holds it, so a pin cannot hold "cannot tell";
  `ItemNow::Doc` holds `Result<DocHash, FailureTag>` in place of an `Option`, so a
  document that cannot be told is no longer read as gone.

- Who writes a key is read in one way (#117). A file whose `key:` reads in the
  book's grammar is a writer of that key whether or not its `id:` can be read, for
  the next key, for what `check` says of a key written twice, and for a link
  or a reference to it. A link to a key that such a file writes beside a
  document with an identity is not followed to that document: the key is written
  twice (`Ambiguous`). A link to a key that only such a file writes says why it
  cannot be followed (`NoIdentity`: the file carries no identity that can be
  read), and a reference to it is `ShelfError::NoIdentity`, not "names no
  document". `check` lists each key two or more files write once, in
  `collisions`, with every file and the identity of each that could be read, and the
  sentence of each of those files names the other files, not only how many.
- The repeat rule of `judge approve` no longer compares the key of an upstream
  (#117): a key is a name, and a renumbered upstream is the same document, so
  approving again after a renumber is a repeat (left out unless `--again`). An
  answer names the upstream a record pinned by the key it has now, with the key
  it was pinned under beside it when that is another, and says so when the
  upstream is invalid, gone or cannot be told.
- In the library, `rekey` adds: `DocRemove` (`ShelfWrite::DocRemover`,
  `doc_remover`), `DocReplace::restore`, `FileSync::persist_gone`, `ShelfPort::DocRemove`;
  `DocSource::render` must give the text `DocWrite::create` writes (a type that
  implements these ports does not build until it does: a breaking change before
  1.0). `Shelf::collisions`, `KeyWriter`, `KeyCollision` (`UnresolvedWhy::Ambiguous` holds
  one), `UnresolvedWhy::NoIdentity`, `Resolution::NoIdentity`, `Shelf::now_named`
  (`NowNamed`), `FramePin::named` (`PinName`) and `Names::pin_name` are the reading
  of who writes a key and of what a pin names.
- The contract of a space's adapter has a new optional ask for a library user
  (#111): which names a selection takes. An adapter names `type List: ListWay<Self>`,
  `Answers` (and implements `NameList`: `select`, whether it reads a selection and
  does no I/O, and `list`, the names there now: all, some with why, or that none
  can be listed) or `CannotList`; the dispatch answers `ArtifactSource::select` and
  `ArtifactSource::list`, `CannotList` for a space no adapter names. The
  adapter of `file` answers; the others name `CannotList`. In the library,
  `Schema::declared` takes the answers to `Schema::selection_asks`
  (`SelectionChecks`), and `Schema::validate`, which asks nothing, refuses a cover
  of artifacts; the application builds a schema with one function for `load` and
  `init`; `freshness::assess` and `Loaded::assess` take `&Listings`
  (`Listings::NotAsked` for a command that reads no cover of artifacts), and
  `Assessment::cover()` is the assessment of coverage
  (`coverage::assess`), which decides a gap, a repeated exemption, how complete a
  listing is and a selection that takes nothing for documents and artifacts
  alike; `BooksCheck::Checked` has `cover` and `artifacts`, `BooksCheck::is_clean`
  reads the assessment only, `ExemptRequest` holds an `ExemptAsk` (a document or
  an artifact), `Exempted::doc` is an `Option`, `ExemptOf::doc` an `Option` and
  `ExemptSubject` tells which, and `SchemaProblem::CoverOnArtifactLink` is gone
  (`CoverNeedsSelection`, `SelectionOnBookLink`, `BadSelection`, `NotASelection`
  and `SpaceCannotList` take its place). The glob grammar of a book's `include`
  and `exclude` is `PathGlob`, which the adapter of `file` matches with too.
- A build from before `ran_by` (#156) does not read a record that has the key,
  and its `check` fails on it; its other commands act as if the record were not
  there, so its `judge` can write over that decision with no warning. A CI that
  checks the books, and everyone who judges in the project, needs this version
  before such a record is committed. Every other record is written and read as
  before, with the same hash; a stored record whose `ran_by` is its `actor` is
  refused as unreadable, not read as one name. In the library, the requests of
  the `judge` use cases (`JudgeRequest`, `ApproveChosenRequest`,
  `ApproveStaleRequest`, `SameMeaningRequest`, `MovedRequest`,
  `PrefixMovedRequest`, `ExemptRequest`, `WithdrawRequest`) take
  `by: AttributionAsk` instead of `actor`, the record constructors (`Decision`,
  `Revocation`, `SameMeaning`, `Moved`, `Exempt`) take an `Attribution` instead
  of an `Actor` (`Attribution::new` holds the one place two names are
  compared), `ActorSource` gains `ran_by`, `ProjectActor::new` and
  `Project::new` take the declared `ran_by`, `RecordProblem` has
  `RanByIsActor`, and `ActorError` has `RanByRefused` (a `WHYSHELF_RAN_BY` that is
  no name, said as that variable's).
- The text a name of the space `rust-test` comes to holds the items beside the
  test in its test part (#155): the outermost inline module on the way whose
  `cfg` holds only under test, or the file when none does. Every item there
  but the functions read as tests is taken, inner modules read through, so an
  edit of a helper, a `use`, a constant, a type or an impl beside a test makes
  its approvals stale, while an edit of another test, of the code outside the
  part, or a module renamed inside it does not. A module declared out of line
  and a file `include!` reads are not followed. An approval of a test made
  before reads as stale once, unless nothing is beside it.
- The text of a `rust-test` name holds the helpers the test reaches (#205): the
  modules and items of its crate that compile only under test, found by the paths
  its text writes, in any file, and the files its includes read (as a hash).
  Editing a shared helper (`tests/common/mod.rs`, a `fixtures_tests` module) makes
  stale the approvals of exactly the tests that use it, so no verification names
  one by hand any more, and a guard fails when one does. An approval of a test that
  holds something is stale once on upgrade, since its text changed; a test that
  holds nothing keeps its approval. `rust-test` places a test in its own file and
  the files it holds.
- The origins of `stats` match an artifact's pin by the name it is read as now
  through the declared moves, as freshness reads it (#154): a pin of the old
  name before a move and one of the new name after it are one pin, counted
  once under the new name when its content changed; before, each was held by
  one approval only and no origin was counted. When moves bring two pins of the
  earlier approval to one name, each is compared. In the library,
  `Ledger::pins_read_now` reads the pins of any approval as `Ledger::read_pins`
  reads those of the approval that stands.
- Another actor's approval of the content that stands is a second reading, and
  `judge approve <ref>` writes it in every project; before, it repeated the
  approval that stands and was left out as `repeats` unless `--again` was given.
  An approval repeats only when its actor already approved that content and asks
  for the expiry that reading and the approval that stands both have, which has
  not passed; once the approval that stands expired, when a new one would
  make the document expire at another time, or while a declared move reads one
  of its pins (where a repeat was left out before), anyone's approval of it is
  an approval made again, `--stale` too. `judge approve --stale` and `--pending` write no
  second reading: such a document is left out with the reason `second_reading`
  and the approval that stands, warned of on stderr, and holds back nothing on it.
  `stats` counts a second reading as an approval and in its run, not as one made
  again, so a ledger that holds approvals made with `--again` by another actor
  answers fewer approvals made again. The actor is now asked once per run, after
  every reference resolves and every expiry is after now and before any document
  is decided: with no `--actor`, `WHYSHELF_ACTOR` or Git `user.name`, a run of
  `judge approve` that would only leave out repeats fails (exit 3) where it
  exited 0, and so does an empty `--stale` or `--pending` run. Every verb of
  `judge` asks it there, so with no name `judge deny`, `deprecate`, `retire` or
  `revoke` fails (exit 3) where the document's state would refuse it (a revoke
  of a pending document exits 2 with a name). In the library,
  `ledger::repeated` is gone: `Ledger::readers` lists who approved the content
  that stands (`Readers`), `Ledger::kind_of` tells a new approval apart
  (`ApprovalKind`: `New`, `SecondReading`, `Repeat`, `Renewal`, asked with
  `Asked` and `Before`), and `ApprovalKind::left_out` says what a run leaves out
  by how it took the document (`Taking`, `LeaveOut`); `LeftOutWhy` has
  `SecondReading`; `JudgedAt::lapsed_by` and `DecisionProblem::expires_after`
  say once when an expiry has passed (#114).
- Two adapters that name one space, two built-in presets of one name, a preset
  an adapter offers for another space, or a name of a space that is no space,
  are an error when the project is opened: the command reads nothing and exits
  with 3 (DEC-86). Before, the later one silently won. In the library, the
  contract of a space's adapter is one trait in the core, `SpaceAdapter` (its
  space and built-in presets, reading names, the current bytes of a name), with
  the optional asks declared by type (`NamePrefixes`, `SpaceSuggest`,
  `SpaceReadBack`, answered with `Answers` or not with `NoPrefixes`,
  `CannotSuggest`, `NoReadBack`); the dispatch `Spaces` in the core answers
  `ArtifactSource` and `MoveSource` and reads back for `ContentStore`, and holds
  the answers for a space with no adapter. `ArtifactSpaces` is now
  `Spaces<Registry>`, `Project::new` returns a `Result` with `DispatchError`,
  and the adapters' `presets` take no space (#146).
- One name of the space `file` that cannot be read is told as cannot tell, with
  why, and every other name asked with it is answered; before, it failed the
  whole reading (exit 3). An approval that pinned such a file stays fresh, with
  a notice, as for every name that cannot be told (DEC-45, DEC-85). Each thing
  an adapter reads for a name is held to a bound of bytes: a file of the space
  `file` (and a blob it reads back) to 16 MiB, past which it cannot be told
  (and reads back none), and the cargo settings rust-item reads to 1 MiB, past
  which none of its names can be told; Git's answer of the renames is held to
  the listing's bound, past which the suggestions give a note and no rename.
  rust-item tells a page or a directory of modules outside the project root and
  the build directory as cannot tell, settings linked out of the root included.
  Inputs past 1 MiB are hashed, scanned and parsed on a blocking thread. In the
  library, `ArtifactError::Io` is gone (the adapter of `file` no longer fails a
  reading for one name), and `GitError` gains `Stopped` and `Cut` (#146).
- The records a `judge` run writes (`judge`, `judge approve --stale` and
  `--pending`, `judge same`, `judge moved`, `judge exempt`, `judge withdraw`)
  are put on disk once, at the end of the run, as the files of `new` and
  `init` are: each record is written whole or removed, not synced one at a
  time. A run that stops part way puts on disk the records it wrote before it
  lists them. In the library, `LedgerAppend::append` returns the path of the
  record it wrote and no longer syncs it (#103).
- A run of `new` (one document or `--from`) or of `init` puts its files on
  disk once, at its end, not one file at a time: each file is written whole,
  then every file and directory the run wrote is synced and, on macOS, the
  drive is asked once to flush its cache (one `F_FULLFSYNC` a device, which a
  file system that cannot do it leaves at `fsync`). With release builds on
  macOS, 100 rows of `new --from` in a project with no document take 0.08 s
  where they took 0.6 s; one row takes the same. A run that stops part way
  still puts on disk the files it wrote before it lists them. When the files
  cannot be put on disk, the answer says so: after the cause when the run
  stopped part way (and the run is a failure, exit 3, whatever stopped it), or
  as "written whole and left in place, but could not be put on disk" when every
  file was written. In the library, the new port `FileSync` (which `Project`
  implements), `ShelfError::NotDurable`, `PersistProblem`, and
  `ShelfError::PartWay::not_durable` (#133).
- A word of `key` the declaration does not have (`key = "uuid"`) is a problem
  of the declaration, told with the others (`check` exits 1, and a command that
  needs a sound declaration exits 2): "book `note`: `key` is "uuid", and it
  takes "slug" or a table { prefix = "..." }"; it used to fail the read of the
  declaration (exit 3). A `key` that is neither a word nor a table still does.
  A word of `without_front_matter` it does not have is told as before, word
  for word. `map` draws the word of a key from the domain. In the library,
  `KeyPatternDecl` keeps a key as written (`Word`, made by
  `KeyPatternDecl::slug()`), the words of an entry of a book are read by
  `read_book_word` (`BookWord`, `BookEntry`, `KeyWord`,
  `WithoutFrontMatter::ALL`), `SchemaProblem::BadWithoutFrontMatter` is
  `UnknownBookWord { book, entry, word }`, and `DeclWord`'s `all` and `word`
  are on its supertrait `Words` (#130).
- A failed document is said one way wherever it is told: "no file in the
  books' ranges carries the document" (for `stale`, `self_gone`; was "no file
  carries the document"), "whether a file carries the document cannot be told;
  unreadable: ..." (`self_unknown`), "no file in the books' ranges carries
  `<key>`" (`upstream_gone`; was "`<key>` is gone"), and "whether a file
  carries `<key>` cannot be told; unreadable: ..." (`upstream_unknown`). A
  refused exemption or declaration of a document that is not valid says that
  clause of it ("`<id>` is invalid"), not "is not a valid document now". An
  unneeded exemption of one says "the document is invalid". `in_cycle` says
  which cycle: "the document is in a cycle of `Depends` links (see
  health.cycles)", or "of approvals' frames". A link that names two documents
  says "it names 2 documents with the key `K`: `<id>` and `<id>`". In the
  library, `FailureTag` and `DocFailure` have `said_of` (with `Detail`) and the
  Display of `FailureTag` is its clause of "it" ("it is invalid", was "its
  files are invalid"), `DocLabel` names a document by its key,
  `UnresolvedLink` and `LabelledCycle` have a Display, `FrameRefusal::InCycle`
  holds a `LabelledCycle`, `FrameRefusal::UpstreamNotStanding` has the `key` of
  its target, and `words::listed` joins names for the messages (#130).

- `fmt` writes the form `new` writes: an entry with nothing in it that the
  build reads as not written (an optional field, or a link whose count admits
  none) is left out, and the file reads as the same document, so no approval
  changes; `fmt --check` lists a file that holds one. A required field, a link
  whose count does not admit none, an `id`, a `key` or an undeclared entry
  written with nothing is kept as it is written (`key:` is written back as
  `key: null`). In the library,
  `Shelf::form_entries` gives the entries of the form as `FormEntries`, which
  `DocWrite::create` takes; `DocSource::forms` returns each file's reading,
  bytes and first comment (`FileRead`), and `DocSource` has `render` and
  `replaceable`; `Form` and `FileForm` are gone (#136).
- `init` resolves a link of its documents to a document it writes later in
  the same run: such a link is no longer listed as unresolved (#125).
- A required field written with nothing in it makes the document invalid: a
  `text` that is empty or white space only, or a `list` with no item, is the
  problem "the required field `<name>` is empty or blank". `new` refuses such a
  document and writes nothing, and `check` lists a file that has one as
  invalid. An optional field written with nothing in it reads as left out
  (#128). A project whose documents write a required field empty sees them
  invalid after upgrading; `check` names each field to fill (#123).
- A value written with nothing in it (`key:` or `null`, a text of white space
  only, a list with no item, for a field of any type) is no value: an optional
  field so written is as if it were left out, in `status`, in filters and in
  the hash; a required one is empty, and an `id` or a `key` so written is
  empty. A document that writes an optional field `""` or `[]` hashes as one
  that leaves it out, so its approval is stale once after upgrading (`judge
  approve --stale`). A null value no longer makes a document invalid, and an
  optional integer, truth value or enum written `""` is now valid. Every item of
  a list (a field, a link to documents or to artifacts) that is empty or white
  space only is a problem, so a document that writes one is invalid. `new`
  writes no entry that says nothing, and `new --from` takes `null`. `status
  --where <field>=` with an empty value is a request error (exit 2). In the
  library, `FieldValue`
  holds `NonBlank` texts and `RawValue` has `Null` (#128).
- Problems say a count, a type and a book as the declaration writes them: "the
  link `grounds` has 2 keys, which its count `optional` (none or one) does not
  admit", "the field `n` is a text, not `integer`", and "the book `note`
  declares no field or link `source`: declare it in the book first". A
  `count`, `type` or `means` that is no word of whyshelf no longer fails the
  read of the declaration (exit 3, with the TOML line and column): it is a
  problem in `schema_problems`, naming the book and the entry and the words it
  takes, told with the others (exit 1). In the library, `FieldType` is
  `Scalar(ScalarType) | Enum(..)`, a declaration as written keeps these words
  as text (`FieldTypeDecl::Word`, `WrittenLinkDecl::count` and `means`,
  `WrittenPresetValues`), and `read_word` reads them, with
  `SchemaProblem::UnknownWord` for one it does not take (#126).
- `check` and `stale` write one warning line on stderr per cause of names that
  cannot be told now, not one per name: names of one space with an equal reason
  are one line, which says the reason once, how many names, and the documents
  that write them by key. A cause of one name keeps the line it had. The JSON
  answers still list every name (#99).

### Removed

- In the library (#175): `Ledger::artifact_same_meaning` and
  `Ledger::not_declared_by` (the frame has it: `Framed::not_declared_by`).
- In the library (#153): `Ledger::read_pins`, `Ledger::reading` and
  `Ledger::quorum` (the approval that stands has them:
  `ApprovalView::read_pins`, `StandingApproval::reading` and
  `StandingApproval::quorum`), `Ledger::pins_read_now` and `Asked::decision`,
  the public `Ledger::pin_same_meaning`; `snapshot::ItemNow`,
  `snapshot::ItemRole`, `snapshot::SeenHash`, `SnapshotItem::changed_from`,
  `SnapshotItem::role`, `SnapshotItem::seen` and `SnapshotItem::moved`, and the
  public `snapshot::doc_now` and `snapshot::DocNow`. No answer changes.

### Fixed

- `judge exempt --artifact` (#120): when parts of the listing of a cover are in
  another space than its link of parts, the name is no longer taken as a part, and
  the exemption is refused as one that cannot be told.
- `stale` offers `judge approve <ref>` (with `--until`, `--actor`, or for a
  pending upstream) only when the frame would take the approval: no longer for
  an approval on an upstream decided against, of a document that is not valid,
  on a link that cannot be followed, on an artifact that cannot be told now, or
  one that would close a cycle among the frames (#182).
- `stale` offers `judge approve --stale` for an origin only when the run would
  approve one of its approvals again: an approval the run refuses (on an
  upstream decided against, a deprecated one with successors included, or on an
  artifact that cannot be told now), and what stands on it, are no longer
  counted (#176).
- `judge same` offers only an edit that `stale` tells as a change: of an
  approval whose document is valid, of a document or of a name under a Depends
  link that still writes it, with the comparison that link declares, and not
  one a declaration made before a move already keeps. It joins pinned hashes
  along each pin's own moves, tells a second declaration by the same name by
  each pin's own way, and its refusals follow the same pins, in one order
  (#175).
- No problem or refusal prints a value as Rust debug output: books clashing on
  a key prefix ("the books `a` and `b` use the key prefix `X`"), a refused
  approval of a document that is not valid, of a link that cannot be followed
  and of a cycle (which names its members by key), and a name read or linked in
  more than one space ("`file` and `rust-item`"). A refused approval on a
  document that does not stand names it by its key (#130).

- The page of `whyshelf_core::domain::shelf` and the doc of `Slug` no longer
  describe the earlier model, removed in #9, as present; the doc of `Slug`
  states its own rules (#98).
- `new` with no `--key`, finding a file at the key it chose (another run
  writing at the same time, or a file whose front matter cannot be read),
  passes over it to the next number, so runs started together in one working
  tree each write a document at a key of their own. After 64 keys each with a
  file it writes nothing and says to run again once the other runs are done or
  the files `check` lists are put right. `new --key` of a key that is there is
  refused as before. In the library, `Shelf::replan_after_taken` decides, with
  `PlanAfterTaken` and `NoFreeKey` (#124, #125).

## [0.1.0] - 2026-11-12

### Added

- Books: a project declares its own kinds of document (books, with key
  grammars, fields and links that depend, inform or replace), and approvals
  are records whose freshness is derived on every read (#7).
- `whyshelf new`, which makes a document valid for its book and never writes
  over a file (#8); `--title` is optional, a shorthand for `title=` (#19), and
  `<entry>+=<item>` gives a list item that holds commas (#20).
- Links to files that are no document (artifacts), pinned by an approval, and
  `get --approved` and `diff`, which read back what an approval saw (#7).
- `judge same`, a declaration that an edit keeps the meaning, and
  `judge withdraw`, which takes back a record by its id (#10).
- `compare = "lines"` and `ignore` on a link to files: indentation and runs of
  white space, blank lines and declared comment lines do not count (#11).
- `cover = true` on a link into a book: `check` reports a document no such link
  reaches (#18).
- A value a front matter does not hold is told with the kind it was read as
  (#22).
- `whyshelf stale --exit-code`, and `just check` (so CI) fails when the
  repository's own books are not clean or an approval does not hold (#24).
- Every row of an answer carries `decision`: the decision that stands about
  the document, with its record's `judgement` id, `actor`, `at` and `reason`
  (#32).
- `stale` answers `origins` too: each document, file, cycle or the
  declaration where the stale approvals start, with the approvals it
  degrades, so an edit of a file several approvals pin is read once (#34).
- A path of a link to artifacts that names no file is told: `check` lists it
  under `absent` (it stays clean), `judge approve` under each judgement's
  `absent`, and its row has an `artifact_absent` notice (#36).
- `whyshelf map` answers what the declaration says, every book and every link
  in its order, with a drawing of them; a book may say what it is for with
  `description`, one line (#39). Answers that list books, fields or links
  (`check`'s `books`, the missing entries of an invalid document, `not_covered`
  notices) now follow the declaration's order, not the order of the names.
- `whyshelf unlinked` answers the documents no link reaches and none leaves
  (`isolated`), then those with no link in or out where the declaration lets
  one be; documents decided against are listed apart. It is a question, not a
  rule, and never fails the check (#47).
- `whyshelf fmt` writes every document's file in the one form `new` writes
  (`id` and `key` first, every text in double quotes, the whole text with
  `\n` line endings and no trailing white space, as it is read), only
  where the text read back is the same reading, so no hash or approval
  changes; a file it cannot write so (unparsed, a comment in its front matter,
  a form that would read otherwise, read-only, more than one link, changed
  since it was read) is left as it is and listed. A rewrite replaces the file,
  so its owner and extended attributes are not kept. `fmt --check` writes
  nothing and exits 1 while a file is not in the form or would be left alone
  (#44).
- `cover` takes a strength: `true` or `"error"` fails `check` as before, and
  `"warn"` lists the documents the link does not reach and leaves `check`
  clean, with a warning line on stderr for each. `check`'s `uncovered` and the
  `not_covered` notice gain `severity`, and `map`'s links `cover_severity`. A
  quoted `cover = "true"` is now a problem of the declaration, not a failure
  to read it (#49).
- `judge approve --stale --reason <text>` approves again, in one run and
  upstream first by the links, every approval that is expired or stale and
  would hold once approved. The rest are left out, listed under `left_out` with
  why (`broken`, `refused`, `stands_on`) and warned of on stderr; pending
  documents are not touched (#41).
- Presets of links to artifacts, and a default: a link may name
  `preset = "<name>"` and write only what differs; a project declares its own
  with `[[preset]]` (optionally `from` another), and `[defaults] artifact =
  "<preset>"` is the preset of a link that names neither a preset nor a space.
  The file adapter offers `file` (space `file`, `bytes`, `depends`). Presets
  are expanded before the declaration is checked, so pins, causes and notices
  see only the values; `map` gives each link's `preset` and
  `preset_by_default`, and draws `preset=<name>`. `means` may be left to a
  preset. `compare` and its `ignore` are one value, and a `from` of a preset's
  own name refines the built-in of that name. A problem of the declaration now
  says its reason (#54).
- The space `rust-item`: a Rust path (`crate::module::Item`, `Type::member`)
  is looked up in the docs rustdoc wrote in the project's build directory
  (`CARGO_TARGET_DIR`, `build.target-dir`, or beside `Cargo.toml`; no program
  is run), and is there, absent, or cannot be told when the docs of its crate
  are not built or not rustdoc's. Its adapter offers the preset `rustdoc`
  (space `rust-item`, `presence`, `depends`): an edit of an item changes
  nothing, and an item that goes away breaks the approvals that stand on it.
  `check` writes each name that is absent or cannot be told on stderr, and
  `stale --exit-code` exits 1 while an approval stands on a name that cannot
  be told (#56).
- `whyshelf stats [--since <time>]`, a reading command that derives from the
  ledger how the work went: approvals per document and per book with those
  made again and those revoked, the time from a document's identity to its
  first approval, the runs of `judge` (decisions of one time and actor) and
  their sizes, and the documents and files whose pinned hash changed under
  approvals made again. No measure reads a clock, so the same ledger gives the
  same answer (#58).
- The answer of `whyshelf stats` carries a `drawing`, a plain text summary in
  five sections that a person reads with `jq -r .drawing`, made from the
  answer alone (#64).
- `judge approve` leaves out a document whose standing approval a new one
  would only repeat (the same frame and the same expiry), lists it under
  `left_out` with the reason `repeats` and warns of it on stderr, so a command
  run twice writes its records once; `--again` writes it anyway. In the library,
  `ShelfApp::judge` and `judge_all` return `JudgedAll` (`judged` and
  `left_out`) (#66).
- `whyshelf judge moved <old> <new> --reason` declares that an artifact moved.
  An approval's pin of the old name, made at or before the declaration, is
  read as the new name: an unchanged move leaves the approval as it was, a move
  with an edit is `artifact_changed` at the new name instead of broken, and a
  link that still writes a name left behind is the cause `artifact_moved`,
  listed by `check` and warned of on stderr. `get --approved`, `diff` and the
  causes show the name pinned and, beside it, `read_as` (#42).
- `whyshelf moves` lists the names approvals pinned that are gone now, and
  the moves each space can suggest for them: for files, one whose content is
  what was pinned, and the renames Git sees between the approval's revision
  and the index. It declares nothing and writes nothing (#69).
- `whyshelf init` writes a minimal declaration, and `whyshelf init --sample` a
  declaration of four books (finding, requirement, decision, verification)
  with one pending document each, in one chain. Neither writes over anything,
  a parent directory included, nor approves; a write that fails part way names
  what was written and leaves no file half written. The manual starts with
  Getting started (#71).
- `judge approve --pending [<ref>...] --reason` approves the documents never
  approved, or those the named ones stand on through `depends` links, upstream
  first in one run, as `--stale` does for stale ones; the two may be given
  together, and each entry names the rule that took it. In the library,
  `ShelfApp::approve_chosen` is the one use case of both (#91).
- A book may declare which files under its `dir` are its documents: `include`
  and `exclude` patterns of `*`, `?` and `**`, and `without_front_matter =
  "skip"` to leave aside a file with no front matter at all, which `check`
  lists. A front matter that opens wrongly is never skipped, `new` refuses a
  key outside the range, and a book that declares none of them reads as
  before (#92).
- A space `rust-test` names test functions (`<file>.rs::<module>::<fn>`), and
  the built-in preset `test` pins each one's own tokens, so an edit of another
  test in the file, or of whitespace, comments or docs, makes nothing stale.
  It reads the file with a shallow scan of its tokens and runs no program
  (#79).
- `whyshelf judge exempt <ref>... --from <book>.<link> --reason` records that
  one document is not expected to be reached by one covering link. It is then
  no gap of that cover, `check` lists it, and an exemption that is no longer
  needed, or is held twice, is told until it is withdrawn. An exemption that
  would be unneeded at once is refused (#78).
- `whyshelf init --template minimal | sample | qa-first` chooses a built-in
  template; `--sample` stays as a shorthand for `--template sample`.
  `qa-first` is the sample's four books with two rules loosened and marked, so
  verifications can be written before the decisions they check, and the manual
  says how to tighten them at release (#82).
- A template `experiment` keeps a pre-registered measurement as two
  verifications: a plan that pins its table (`sheet`), approved before the run,
  and a result that pins its record (`log`) and names its plan (`plans`),
  approved after it. `init` writes the table beside the documents. A built-in
  preset `code` pins only that a file is there, for a decision's link to its
  code (#93).
- A space `js-test` names TypeScript and JavaScript tests by their titles
  (`<file>::<describe>::…::<test>`), Playwright's `test.describe` and the usual
  aliases included, and the built-in preset `js-test` pins each one's own call,
  so an edit of another test, or of whitespace or comments, makes nothing
  stale. It reads the file with a lexer of its own and runs no program (#90).
- whyshelf's own requirements, decisions and verifications are kept as books
  in the repository (#7; extended in #12).
- `release-plz.toml`: a release is a release PR that bumps the workspace
  version; after its merge, `release-plz release` publishes the four crates
  and tags `v<version>`.

### Fixed

- A front matter that holds several values of kinds it does not hold tells
  every one, each with its key and kind, not only the first; and a whole
  number beyond 64 bits is told as too large or too small, not as a front
  matter that is not valid YAML (#21).
- `init` and `new` follow a link on a path only while it stays inside the
  root: a book's directory, or another parent, that links out of the root is
  in the way, where a document could be written outside the project (#93).
- Every call of Git goes through one runner that lets nothing of the
  repository choose a program or write: a clean or process filter the
  repository configures no longer runs when an address is made, a partial
  clone fetches nothing, and every call stops after a bound of time. A name of
  a file whose path leaves the root through a symbolic link cannot be told,
  where it was read from outside the root (#69).
- `--since` and `--until` take an RFC 3339 time with an offset or `Z`, or a
  date written `YYYY-MM-DD` (the start of that day in UTC). A time with no
  offset was read as its date, its time dropped in silence; it is now refused
  with exit 2, as is a date in another form (#64).
- whyshelf's own verifications say `e2e` for the tests that run the binary: a
  verification that rested on both kinds of test is split in two (#45).
- A path names a file only as written, ASCII case included: on a file system that
  does not tell case apart, `justfile` read the file `Justfile`, which Git and
  CI on Linux do not find. An approval made there that pinned such a path is
  now broken (`artifact_gone`) until the path is written as the file is named
  (#36).
- An upstream edited inside a cycle of frames, where no `Via` passes, is a
  root of the approval that pinned it; it was left out of the roots. An edit
  or a cycle reached two ways up is one root (#34).
- A wrong argument, or none, is answered with the error JSON of a request and
  exit 2, as every wrong request is; only `--help` and `--version` answer in
  text, and there is no `help` command (#28).
- Statements of the books and of the documentation that the code does not
  hold, found by a blind evaluation, and a test of the ledger that checked
  nothing (#26).

### Changed

- The README is the way in: what whyshelf does, getting started with
  `init --template`, the model in brief, the commands at a glance and the gate
  for CI. Every command it shows runs as written; the manual keeps the rules
  (#84).
- A link to artifacts names its space (`space = "file"` for files of the
  project); one with no space, or an unknown one, is refused. Everything
  outside the documents is an artifact of a space, resolved by that space's
  adapter, and files are one space among others. A name resolves to present,
  absent, cannot tell or not a name: an approval on a name that cannot be told
  is refused, and one that later cannot be told is a notice, listed by `check`
  under `unknown`. `presence` compares only whether an artifact is there. Every
  answer about an artifact carries its `space`; pins stored before spaces read
  as files and stay as stored (#38).

- The commands are flat verbs (`whyshelf check`, not `whyshelf books check`),
  and every JSON answer carries `"version": 1` (#9).
- The normalization of a document's text is a function of the domain,
  `whyshelf_core::domain::shelf::doc::normalize`; `whyshelf_infra` no longer
  has one of its own. whyshelf's own decisions state one rule each (#30).
- The end-to-end tests of `whyshelf-cli` are one file per feature, sharing
  their helpers in one module, and whyshelf's own verifications each name the
  files of their feature; a test fails when one file is named by more than two
  verifications. Temporary directories of these tests hold a counter, so tests
  run in parallel never share one (#60).
- The end-to-end tests show on the binary the whole order of who judges: the
  actor given, else `WHYSHELF_ACTOR`, else Git's `user.name`, and with none the
  judgement is refused, whatever a file of the project says; they run with the
  person's Git settings shut out (#62).

### Removed

- The fixed-stage model that came before books (#9).
- The sample declaration `books.sample.toml` and `whyshelf_infra::shelf::books::SAMPLE`,
  in favour of `whyshelf init --sample` (#71).
