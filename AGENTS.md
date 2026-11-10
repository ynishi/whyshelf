# whyshelf — pointers for coding agents

- What the project is and promises, and the crate layout: [README.md](README.md)
- Disclosure policy — read before anything ships:
  [PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md)
- Issue, branch, commit and PR conventions:
  [CONTRIBUTING.md](CONTRIBUTING.md)
- Security reports: [SECURITY.md](SECURITY.md)
- Where documents live:
  - requirements, decisions and verifications: whyshelf under `books/`
    (declared in `.whyshelf/books.toml`, decisions in `.whyshelf/ledger/`);
    check them with `whyshelf check` and `whyshelf stale`.
  - everything else (concepts, file formats, the command line, the code):
    RustDoc (crate and module docs, diagrams as ASCII in code blocks tagged
    `text`). `cargo doc` must be warning-free (`just rust-doc`). Code
    documentation outranks stale issue text.
- Never work on `main`; one branch per issue, cut from `origin/main` after a
  fetch. `just worktree-new <type> <slug>` from the main checkout does that and
  runs `just branch-check`.
- `just check` is green. [CONTRIBUTING.md](CONTRIBUTING.md#verification) says
  what it covers.
- Review a change locally before it goes up — by a person or an AI agent — and
  say in the PR body who or what reviewed it.
  [CONTRIBUTING.md](CONTRIBUTING.md#review) lists the questions a review
  answers.
- Layers: `whyshelf-core` (domain, app) never depends on an I/O crate; adapters
  go in `whyshelf-infra`. See [README.md](README.md#crates).
