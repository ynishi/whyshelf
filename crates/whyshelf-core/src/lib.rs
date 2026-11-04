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
#![warn(missing_docs)]

pub mod app;
pub mod domain;
