//! Core (SDK) of whyshelf: Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals
//!
//! Two layers, inner to outer. `arch-lint.toml` at the workspace root states
//! them, and `cargo test` (`tests/architecture.rs`) fails when code breaks them.
//!
//! - [`domain`]: the rules and types of the problem. Pure: no file system,
//!   process, environment, network, clock or randomness. Those come in as values.
//! - [`app`]: the use cases, and the ports (traits) they need from the outside
//!   world. Also free of direct I/O.
//!
//! The adapters that implement the ports with real I/O are the
//! `whyshelf-infra` crate. This crate depends on no I/O crate.
#![warn(missing_docs)]

pub mod app;
pub mod domain;
