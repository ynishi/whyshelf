# whyshelf

Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals

```sh
cargo install whyshelf-cli   # installs the `whyshelf` command
```

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
