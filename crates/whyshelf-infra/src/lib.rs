//! Adapters of whyshelf: the ports of `whyshelf_core::app` implemented with real
//! I/O (the file system, Git, the clock).
//!
//! Every wire format (TOML, YAML front matter, JSON) is read and written here and
//! translated into the values of the domain at this boundary. The adapters and the
//! files they use are in [`shelf`]; [`shelf::Project`] bundles them for a project
//! root.
//!
//! This is a crate of its own, not a module of `whyshelf-core`, so that the core
//! cannot reach I/O: the boundary is a `[dependencies]` decision that the compiler
//! enforces.

#![warn(missing_docs)]

pub mod shelf;
