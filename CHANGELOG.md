# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  `ShelfApp::approve_chosen` is the one use case of both (#73).
- whyshelf's own requirements, decisions and verifications are kept as books
  in the repository (#7; extended in #12).

### Fixed

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
