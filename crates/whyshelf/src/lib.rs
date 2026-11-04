//! Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals
//!
//! The umbrella crate: the one name users depend on. The code lives in
//! `whyshelf-core`, re-exported here; the adapters of `whyshelf-infra` are
//! [`infra`], behind the `infra` feature (on by default).
#![warn(missing_docs)]

pub use whyshelf_core::*;

#[cfg(feature = "infra")]
pub use whyshelf_infra as infra;
