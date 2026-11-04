//! Adapters of whyshelf: the ports of `whyshelf_core::app` implemented with
//! real I/O (Git, the file system, SQLite, the clock).
//!
//! The outside world's formats and errors are translated here, at the edge,
//! so that nothing past this crate sees them. Markdown / YAML parsing into
//! domain types lives here too: the domain receives values, never text.
//!
//! This is a crate of its own, not a module of `whyshelf-core`, so that the
//! core cannot reach I/O: the boundary is a `[dependencies]` decision that the
//! compiler enforces.
#![warn(missing_docs)]
