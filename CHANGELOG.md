# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Fixed

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
