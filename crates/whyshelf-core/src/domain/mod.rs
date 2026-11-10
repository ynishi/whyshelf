//! The rules and types of the problem, in its own words.
//!
//! Pure: nothing here touches the file system, a process, the environment, the
//! network, the clock or a random source. A value that comes from outside (a
//! file's contents, the current time, a new identifier) is passed in by the
//! caller. A value with rules is a type, not a `String`: an enum for a closed set,
//! a newtype with a private field and a checking constructor for anything else.
//!
//! The model is [`shelf`]: books a project declares, the documents in them and the
//! files they stand on, what people decided, and whether each decision still holds.
//! The other modules are the value types it is built from.
//!
//! | module | in short |
//! |---|---|
//! | [`shelf`] | the model: books, documents, links, artifacts, decisions, freshness |
//! | [`hash`] | content hashes, by kind |
//! | [`canon`] | the canonical encoding a hash is taken over |
//! | [`id`] | identifiers (UUID v7) and their short forms |
//! | [`location`] | a path relative to the project root |
//! | [`actor`] | who decides |
//! | [`nonempty`] | collections and texts that cannot be empty |
//! | [`error`] | why a value is refused by its rule |
//!
//! ```text
//!   shelf ──► hash ──► canon
//!     │  └──► id, location, actor, nonempty
//!     └─────► error (every value type reports through it)
//! ```

pub mod actor;
pub mod canon;
pub mod error;
pub mod hash;
pub mod id;
pub mod location;
pub mod nonempty;
pub mod shelf;
