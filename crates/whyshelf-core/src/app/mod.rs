//! The use cases: read what they need, decide with [`crate::domain`], record
//! the outcome.
//!
//! What a use case needs from the outside world is a port: a trait declared
//! here, by its consumer, and implemented in the `whyshelf-infra` crate. Code here calls
//! ports, never the file system, a process or the environment directly.
