//! Core (SDK) of whyshelf: Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals
//!
//! whyshelf ties a change to its reasons. A change is traced through four stages
//! (a Finding, a Spec, a Design, an implementation), each pinned to the one above
//! by name and hash, and each approval is recorded as an append-only event. What can
//! be derived (approved, stale, blocked, integrated, pins found) is never written
//! down; it is computed when asked. The full model, with the stages, the gates, the
//! two-axis state, the hash scope, the event graph and the known limitations, is in
//! the documentation of [`domain`].
//!
//! Two layers, inner to outer. `arch-lint.toml` at the workspace root states
//! them, and `cargo test` (`tests/architecture.rs`) fails when code breaks them.
//!
//! - [`domain`]: the rules and types of the problem. Pure: no file system,
//!   process, environment, network, clock or randomness. Those come in as values.
//!   It has no serialization either; the wire formats belong to the adapters.
//! - [`app`]: the use cases, and the ports (traits) they need from the outside
//!   world. Also free of direct I/O.
//!
//! The adapters that implement the ports with real I/O are the
//! `whyshelf-infra` crate. This crate depends on no I/O crate.
//!
//! # Crates and layers
//!
//! An arrow reads "depends on". Dependencies point inward; the domain depends on
//! nothing in the workspace.
//!
//! ```text
//!   whyshelf-cli  (bin `whyshelf`)         whyshelf  (umbrella: re-exports
//!   clap commands, MCP tool server                    core; `infra` feature)
//!          |        \                                 |           |
//!          |         \________________________________|___________|
//!          v                                          v           v
//!   +----------------------------------------------------------------------+
//!   | whyshelf-infra   adapters, real I/O                                  |
//!   |   open() -> Workspace<InfraPorts>                                    |
//!   +----------------------------------+-----------------------------------+
//!                                      | implements the app ports
//!                                      v
//!   +----------------------------------------------------------------------+
//!   | whyshelf-core                                                        |
//!   |   +----------------------------------------------------------------+ |
//!   |   | app     use cases (Workspace<P: Ports>), the port traits       | |
//!   |   |   +----------------------------------------------------------+ | |
//!   |   |   | domain   rules and types; pure, no I/O, no serde         | | |
//!   |   |   +----------------------------------------------------------+ | |
//!   |   +----------------------------------------------------------------+ |
//!   +----------------------------------------------------------------------+
//! ```
//!
//! Which adapter implements which port (the `Ports` bundle is `InfraPorts` in
//! `whyshelf_infra`; each port trait is declared in [`app::ports`]):
//!
//! ```text
//!   port trait (whyshelf-core)      adapter (whyshelf-infra)
//!   -------------------------       ----------------------------------------
//!   RevisionPort                    git::GitRevision       (git processes)
//!   CodeScanPort                    git::GitCodeScan       (git show, no checkout)
//!   TestRunPort                     test_run::GitTestRun   (test command)
//!   StorePort                       store_fs::FsStore      (events/, objects/)
//!   CurrentPort                     fs_current::FsCurrent  (document files, read)
//!   ScaffoldPort                    scaffold::FsScaffold   (new files only)
//!   IndexCachePort                  cache_sqlite::SqliteCache (SQLite, FTS5)
//!   SecretScanPort                  secret::RegexSecretScan
//!   ClockPort                       clock::SystemClock
//!   IdPort                          id::UuidV7Ids
//! ```
//!
//! The crate boundary is the first guard (a `[dependencies]` table the compiler
//! enforces); the second is `arch-lint.toml`, which `cargo test` checks, and which
//! keeps the domain free of the clock, randomness, serialization and I/O and the
//! app free of them as well.
#![warn(missing_docs)]

pub mod app;
pub mod domain;
