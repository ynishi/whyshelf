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
- `compare = "lines"` and `ignore` on a link to files: white space, blank lines
  and declared comment lines do not count (#11).
- `cover = true` on a link into a book: `check` reports a document no such link
  reaches (#18).
- A value a front matter does not hold is told with the kind it was read as
  (#22).
- `whyshelf stale --exit-code`, and `just check` (so CI) fails when the
  repository's own books are not clean or an approval does not hold (#24).
- whyshelf's own requirements, decisions and verifications are kept as books
  in the repository (#7; extended in #12).

### Changed

- The commands are flat verbs (`whyshelf check`, not `whyshelf books check`),
  and every JSON answer carries `"version": 1` (#9).

### Removed

- The fixed-stage model that came before books (#9).
