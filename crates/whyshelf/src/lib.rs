//! Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with event-sourced approvals
//!
//! The umbrella crate: the one name users depend on. The code lives in
//! `whyshelf-core`, re-exported here; the adapters of `whyshelf-infra` are
//! [`infra`], behind the `infra` feature (on by default).
//!
//! # What whyshelf is
//!
//! whyshelf keeps the reasons behind a piece of software next to the software,
//! so that a change can be traced back to why it was wanted and a reason that
//! no longer holds shows up as work to do. Four kinds of document form a chain,
//! each one standing on the one before it:
//!
//! | stage | answers | stands on |
//! |---|---|---|
//! | **Finding** | what was seen or measured, and how it is known | the world |
//! | **Spec** | what must hold (a Contract), with the pins that bind it and the hypotheses to measure | Findings |
//! | **Design** | the approach (with the options rejected) and its split into items | one Spec |
//! | **Impl** | the code and tests that carry each item's pins | a Design's item |
//!
//! The documents are Markdown files that people (or AI agents) write and edit;
//! the tool never rewrites them. Everything it adds is recorded next to them:
//!
//! - **Hash-pinned.** A document is identified by a hash of its meaning (white
//!   space and line endings are not part of it). An approval records the hash of
//!   what was approved *and* the hash of every upstream document it stood on, so
//!   a change anywhere upstream makes the downstream approval Stale instead of
//!   silently wrong.
//! - **Event-sourced approvals.** Registration, revision, classification and
//!   approval are events in an append-only log, one file per event. Nothing is
//!   updated in place, so two branches that each add something merge without
//!   conflict, and whether something is approved is a question asked of the
//!   whole set of events. An approval is withdrawn by another event, never
//!   erased.
//! - **Derived state.** What is Draft, Approved, Modified, Stale or Blocked is
//!   computed from the files and the events each time; it is never stored.
//!
//! # The gates
//!
//! Each stage has a gate with two layers. The machine layer looks at form
//! (references resolve, the upstream is approved and current, every pin is
//! carried by a test and no test carries an undeclared one, the tests pass) and
//! reports every violation, not the first. The human layer records a judgement
//! of meaning as an approval, and can be recorded only when the machine layer
//! finds nothing. The gates are CR (a Finding), CS (a Spec), CI (a Design), CA (an
//! item, at one revision of the code, with a clean working tree) and CM (what a
//! measurement showed against the hypotheses fixed before it, a record rather
//! than a state).
//!
//! # Public and private
//!
//! Where a document's history lives is decided by where it is put: the repo
//! store is committed with the project, the private store stays on the user's
//! machine. A public document can name a private one only by an opaque
//! reference, and publishing moves a document from one store to the other.
//!
//! # Opening a workspace
//!
//! A [`app::Workspace`] is the one entry point: each use case is one of its
//! methods. [`infra::open`] builds one over the real adapters (Git, the file
//! system, SQLite, the clock) for the project in a directory, and returns with it
//! a [`infra::Shutdown`] that must be closed on purpose, because closing the
//! cache does I/O and is not done in `Drop`.
//!
//! ```no_run
//! # #[cfg(feature = "infra")]
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::path::Path;
//!
//! let (mut workspace, shutdown) = whyshelf::infra::open(Path::new(".")).await?;
//!
//! // The files people edit become history at the next ingest.
//! workspace.ingest().await?;
//!
//! // Every entity with its state on both axes, and the command to run next.
//! let status = workspace.status().await?;
//! for row in status.rows() {
//!     println!("{} -> {}", row.kind().name(), row.next());
//! }
//!
//! shutdown.close().await?;
//! # Ok(())
//! # }
//! # fn main() {}
//! ```
//!
//! The settings are read from `.whyshelf.toml` at the project root (see
//! [`infra::config`]); the private store is `<data dir>/<project>`, where the
//! data dir comes from the user's environment and never from the committed file.
//!
//! # Crates
//!
//! `whyshelf-core` holds the rules ([`domain`], pure) and the use cases with the
//! ports they need ([`app`]); it depends on no I/O crate. `whyshelf-infra`
//! implements the ports. The `whyshelf` command (and the tool server it starts
//! with `whyshelf mcp`) is the `whyshelf-cli` crate, a thin adapter over the same
//! use cases.
#![warn(missing_docs)]

pub use whyshelf_core::*;

#[cfg(feature = "infra")]
pub use whyshelf_infra as infra;
