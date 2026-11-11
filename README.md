# whyshelf

Books of documents whose kinds a project declares, linked to the files they stand on, and whether each decision about them still holds.

A project keeps its reasons as Markdown documents with front matter: what was found, what is required, what was decided, and how it is checked. Each kind is a *book* the project declares, and documents link to each other and to the files, Rust items and tests they stand on. A person *approves* a document; the approval pins what it stood on. On every read, whyshelf tells whether each approval still holds:

- *fresh*;
- *stale*, when something it stood on changed;
- *expired*, past its `--until`;
- *broken*, when something it stood on is gone or no longer stands.

Nothing about freshness is stored; it is derived again each time.

## Getting started

```sh
cargo install --path crates/whyshelf-cli   # from a checkout: installs the `whyshelf` command
```

In the root of a project:

```sh
whyshelf init --template sample   # a declaration of four books, and one pending document of each
whyshelf check                    # nothing wrong: exit 0
whyshelf map | jq -r .diagram     # the books and their links, drawn
whyshelf trace V-1                # the chain, from the verification up to the finding
whyshelf judge approve --pending F-1 V-1 --reason "read, and it holds"
                                  # F-1, REQ-1, DEC-1 and V-1, upstream first
whyshelf status                   # each document, approved and fresh
```

Every command answers in JSON; `map` also carries a text drawing in `diagram`.

`--pending F-1 V-1` takes V-1 and the pending documents it depends on, DEC-1 and REQ-1. F-1 is named as well, because REQ-1 only *grounds* on it, an informs link, which an approval does not stand on.

Edit `books/requirement/REQ-1.md` and run `whyshelf stale`. REQ-1 no longer holds, nor do the decision and the verification that depend on it, and `stale` says it starts at REQ-1. F-1 still holds: a change makes stale what depends on it, never what it stands on.

`init --template <name>` chooses one of three templates, listed by `init --help`:

| template | what it writes |
|---|---|
| `minimal` | the default: one book of notes, with comments on how to add a field, a link and a link to files; no document |
| `sample` | four books, `finding` → `requirement` → `decision` → `verification`, and one document of each in one chain |
| `qa-first` | the same four books, with two rules loosened so checks can be written before the decisions they check; a finding, a requirement and a verification, no decision yet. The manual says how to tighten the rules at release |

`init` writes over nothing and approves nothing.

## The model in brief

- **Books and links.** `.whyshelf/books.toml` declares each book: its directory, its key grammar, its fields and its links.
  - A link that *depends* makes a document stale when its target changes; one that *informs* does not.
  - A link may declare a *cover*: every pending or approved document of its target book is expected to be reached by it. `true` or `"error"` fails `check`; `"warn"` lists the gap and warns on stderr.
  - A book may declare which files of its directory are documents: `include`, `exclude` and `without_front_matter`.
- **Approvals.** An approval is a record in `.whyshelf/ledger/`, with who, when and why, pinning the hashes of what it stood on.
  - `judge approve --pending` approves pending documents: never approved, or with every decision withdrawn. Bare, it takes all of them; with references, it takes those and the pending documents they depend on.
  - `judge approve --stale` approves again what an edit made stale or what expired.
  - Both run upstream first, in one run. An approval that would only repeat the one that stands is left out, unless `--again`.
- **Artifacts and spaces.** What a document stands on outside the books is an artifact named in a *space*:
  - `file`, a path of the project;
  - `rust-item`, a public Rust path, read from the docs `cargo doc` builds. Build them afresh before you check or approve: before they are built a name cannot be told, and docs left from an earlier build are read as they are;
  - `rust-test`, a test function, `<file>.rs::[<module>::…]<fn>`, read by a shallow scan of the file that runs no program.

  Each link says how to compare (`bytes`, `lines` or `presence`). *Presets* (`file`, `rustdoc`, `test`) are short names for the usual settings.
- **Declarations.** A person may declare:
  - that an edit keeps the meaning (`judge same`);
  - that an artifact moved (`judge moved`; `whyshelf moves` suggests where);
  - that a document is not expected to be reached by a cover (`judge exempt`).

  Each is a ledger record with a reason, and `judge withdraw` takes it back.

## Commands

| reading | |
|---|---|
| `check` | the declaration, the documents and the ledger; exit 1 if anything is wrong |
| `status`, `get <ref>` | where each document stands, or one with why |
| `stale [--exit-code]` | the approvals that no longer hold, with where that starts |
| `trace <ref>`, `map` | the chain through a document, and the books and their links |
| `unlinked` | the documents no link reaches, or that hold none |
| `stats`, `moves` | how the work went, from the ledger; and where pinned files may have moved |
| `get <ref> --approved`, `diff <ref>` | what an approval saw; and that against now |
| `schema` | the JSON Schema of every answer |

| writing | |
|---|---|
| `init`, `new <book>` | start a project; make one document |
| `judge approve`, `deny`, `deprecate`, `retire`, `revoke` | decide about documents |
| `judge same`, `moved`, `exempt`, `withdraw` | declare, and take back |
| `fmt [--check]` | put every document in the one form |

Every command prints one JSON document on stdout (`--help` and `--version` aside); warnings and errors also go to stderr.

## In CI

```sh
whyshelf check && whyshelf fmt --check && whyshelf stale --exit-code
```

This fails in any of these cases:

- the declaration cannot be used, or a document or the ledger is not sound;
- a covering link leaves an error gap;
- a file is not in the one form;
- an approval no longer holds;
- an approval stands on a name that cannot be told now. For `rust-item`, build the docs first (`cargo doc --no-deps`).

## whyshelf's own books

The findings, requirements, decisions and verifications of whyshelf itself are books under `books/`, approved in `.whyshelf/ledger/`. `just check` builds the docs and runs the gate above on them. `whyshelf status` lists them.

## Documentation

Everything else is the RustDoc: the model, the file formats, the command line, its output and exit status. Build and read it with:

```sh
just rust-doc
```

Cargo prints the path of each page it generates. Start at the `whyshelf` crate page, which is the manual. The page of the `whyshelf` binary (the `whyshelf-cli` crate) is built as the second step.

## Crates

| crate | role | depends on |
|---|---|---|
| `whyshelf` | Umbrella: the name users depend on. Re-exports the core; the adapters behind the `infra` feature. | `-core`, `-infra` (optional) |
| `whyshelf-core` | Core (SDK): `domain` (pure rules and types), `app` (use cases and their ports). No I/O crate. | — |
| `whyshelf-infra` | Adapters of the ports: file system, Git, clock, TOML / YAML front matter / JSON, and the spaces. | `-core` |
| `whyshelf-cli` | The `whyshelf` command. | `-core`, `-infra` |

The layer rules inside the core are in `arch-lint.toml` and run with `cargo test`.
The rule between crates is the `[dependencies]` table: the core never depends on an I/O crate.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
