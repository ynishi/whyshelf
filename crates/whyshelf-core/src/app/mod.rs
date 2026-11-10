//! The use cases: read what they need through ports, decide with
//! [`crate::domain`], record the outcome.
//!
//! Free of direct I/O. Every port is a trait declared here, by its consumer, and
//! implemented in the `whyshelf-infra` crate. The use cases are in [`shelf`].

pub mod shelf;
