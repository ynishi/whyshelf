//! Core (SDK) of whyshelf: documents whose kinds a project declares, linked to one
//! another and to the files they stand on, judged, and checked for whether each
//! judgement still holds.
//!
//! A project declares its books (a directory, a grammar of keys, fields and kinds
//! of links); whyshelf reads the documents, resolves their links, records the
//! decisions people make about them in an append-only ledger, and computes, every
//! time it is asked, whether each approval still holds and where it starts to fail.
//! The model is in [`domain::shelf`].
//!
//! Two layers, inner to outer. `arch-lint.toml` at the workspace root states them,
//! and `cargo test` (`tests/architecture.rs`) fails when code breaks them.
//!
//! - [`domain`]: the rules and types of the problem. Pure: no file system,
//!   process, environment, network, clock or randomness. Those come in as values.
//!   It has no serialization either; the wire formats belong to the adapters.
//! - [`app`]: the use cases, and the ports (traits) they need from the outside
//!   world. Also free of direct I/O.
//!
//! The adapters that implement the ports with real I/O are the `whyshelf-infra`
//! crate. This crate depends on no I/O crate.
//!
//! ```text
//!   whyshelf-cli (bin `whyshelf`)       whyshelf (umbrella: re-exports core)
//!          |                                    |
//!          v                                    v
//!   whyshelf-infra  adapters: books.toml, Markdown, JSON ledger, Git, clock, ids
//!          |  implements the ports of
//!          v
//!   whyshelf-core   app::shelf (use cases, ports) -> domain::shelf (the model)
//! ```

#![warn(missing_docs)]

pub mod app;
pub mod domain;
