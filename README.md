# whyshelf

Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals

```sh
cargo install whyshelf-cli   # installs the `whyshelf` command
```

## Documentation

The requirements, decisions and verifications of whyshelf itself are whyshelf
books under `books/` (`whyshelf books status`). Everything else is the RustDoc:
concepts, the file formats, settings, the command line and its walkthrough, the
tool server, compatibility and the known limitations. Build and read it with:

```sh
just rust-doc
```

Cargo prints the path of each page it generates. Start at the `whyshelf` crate
page, which has a map from each topic to the module that documents it. The
command line's guide is the page of the `whyshelf` binary (the `whyshelf-cli`
crate), built as the second step.

## Crates

| crate | role | depends on |
|---|---|---|
| `whyshelf` | Umbrella: the name users depend on. Re-exports the core; the adapters behind the `infra` feature. | `-core`, `-infra` (optional) |
| `whyshelf-core` | Core (SDK): `domain` (pure rules and types), `app` (use cases and their ports). No I/O crate. | — |
| `whyshelf-infra` | Adapters of the ports: Git, file system, SQLite, clock, Markdown / YAML parsing. | `-core` |
| `whyshelf-cli` | The `whyshelf` command: CLI and the MCP server (`whyshelf mcp`). | `-core`, `-infra` |

The layer rules inside the core are in `arch-lint.toml` and run with `cargo test`.
The rule between crates is the `[dependencies]` table: the core never depends on an I/O crate.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
