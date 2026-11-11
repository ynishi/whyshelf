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
- whyshelf's own requirements, decisions and verifications are kept as books
  in the repository (#7; extended in #12).

### Fixed

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

### Removed

- The fixed-stage model that came before books (#9).
