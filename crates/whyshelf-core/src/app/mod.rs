//! The use cases: read what they need, decide with [`crate::domain`], record
//! the outcome.
//!
//! What a use case needs from the outside world is a port ([`ports`]): a trait
//! declared here, by its consumer, and implemented in the `whyshelf-infra` crate.
//! Code here calls ports, never the file system, a process or the environment
//! directly, and names no async runtime. A [`Workspace`] is the one entry point:
//! it owns the ports, the [`Config`] and the built-in subscribers, and each use
//! case is one of its methods.
//!
//! # Reading and writing
//!
//! A use case that **writes** takes `&mut self`; one that **reads** takes
//! `&self`. Because Rust lets one `&mut` exist at a time, the writes of one
//! workspace are serialized by the type: each loads the tips of the stream it
//! writes to, decides, records and notifies, with no other write in between, so a
//! process cannot fork a stream with itself. Two processes (a command line and a
//! tool server, or two worktrees) can still write at once; that is safe because the
//! identifier of an event is the hash of its content and heads, so the same change
//! is one event whichever of them found it, a record file is named by that
//! identifier and the digest of its time, and the next write lists every tip it sees
//! and joins a fork.
//!
//! | writes (`&mut self`) | reads (`&self`) |
//! |---|---|
//! | [`ingest`](Workspace::ingest), [`rebuild`](Workspace::rebuild) | [`status`](Workspace::status) |
//! | [`add`](Workspace::add), [`tag`](Workspace::tag), [`reclassify`](Workspace::reclassify), [`mv`](Workspace::mv), [`publish`](Workspace::publish), [`purge`](Workspace::purge) | [`get`](Workspace::get), [`find`](Workspace::find) |
//! | [`approve_cr`](Workspace::approve_cr), [`approve_cs`](Workspace::approve_cs), [`approve_ci`](Workspace::approve_ci), [`approve_ca`](Workspace::approve_ca), [`record_cm`](Workspace::record_cm), [`revoke`](Workspace::revoke) | [`check_cr`](Workspace::check_cr), [`check_cs`](Workspace::check_cs), [`check_ci`](Workspace::check_ci), [`check_ca`](Workspace::check_ca) |
//! | [`finding_new`](Workspace::finding_new), [`spec_new`](Workspace::spec_new), [`design_new`](Workspace::design_new) | [`trace`](Workspace::trace), [`changelog`](Workspace::changelog), [`audit`](Workspace::audit), [`context`](Workspace::context) |
//!
//! # Concurrency and merge
//!
//! Who can write at the same time, and what keeps them from hurting each other:
//!
//! | writers | what makes it safe |
//! |---|---|
//! | two calls on one [`Workspace`] | none needed: `&mut self` lets one write run at a time |
//! | the tool server's requests | the server hands its one workspace to a single task with a bounded queue, so requests run one after another; the workspace is not behind a lock |
//! | a command line and a tool server, in one worktree or in two | a record file is named by the event's identifier (the hash of its content and heads) and the digest of its time, so the same change is one event, the same record is the same file, and a record at another time is a file beside it; a file is written under a name of its own and renamed, and an existing target is a success; the cache is SQLite in WAL mode with a busy timeout; generated files are rebuilt whole, so the last writer wins and the state converges |
//! | two branches (a merge) | one record is one file, so the files of both sides simply add up; see below |
//!
//! ```text
//!   worktree A (branch a)                     worktree B (branch b)
//!   .whyshelf/events/  e1 e2 e3               .whyshelf/events/  e1 e2 e4
//!   .whyshelf/cache/   (its own, ignored)     .whyshelf/cache/   (its own)
//!                \                                 /
//!                 +------- git merge  -----------+
//!                                  |
//!                                  v
//!   .whyshelf/events/  e1 e2 e3 e4      the union; a record both wrote is one file, the
//!                                       same change at two times is two files, read as
//!                                       one event with two times
//!   docs/*.md          ordinary text merge; a conflict marker makes that
//!                      entity Invalid until a person resolves it
//!   .whyshelf/map.json may conflict (one file rewritten by both sides):
//!                      `whyshelf rebuild` writes it again from the events
//!   .whyshelf/cache/   not merged; each worktree's next command catches up
//!                      with what its own checkout now holds
//! ```
//!
//! After a merge a stream can have more than one tip. A value that the two sides set
//! differently (a path, an owner, an alias) is listed as a conflict by the status; a
//! person settles it with `tag` or `mv`, and that write lists every tip, which joins
//! the stream. Tags need no settling: concurrent additions both stay, and a removal
//! only removes the additions it saw.
//!
//! One thing is shared that looks separate. The private store is
//! `<data dir>/<project>-<project_id>`, so every clone and worktree of a project on
//! one machine reads and writes the same private store, while each has its own
//! repo-store files and its own cache. The id is in the committed settings, so two
//! unrelated projects that use one name do not share one.
//!
//! # The write path
//!
//! One picture of a write, from the command to the warnings; [`commit`] has the same
//! picture with each step spelled out.
//!
//! ```text
//!   Workspace::tag / add / approve_cs / ingest / ...        (&mut self)
//!        |
//!        |  Model::load -> decide -> Draft { events, texts, check results }
//!        v
//!   gate     machine check -> secret scan -> leak lint   refused: nothing written
//!        |
//!        v
//!   append   texts, then events, into the store(s)       already there = success
//!        |
//!        v
//!   notify   Model::load again; cache -> map -> [status md] -> [changelog md]
//!        |
//!        v
//!   Result { events, warnings }    a failed subscriber is a warning, no undo
//! ```
//!
//! # The two layers of a gate (CHECK)
//!
//! Each stage has a gate with a machine layer and a human layer. The machine layer
//! (`check_*`) looks at form: references resolve, the upstream is approved and
//! current, pins match in both directions, tests pass. It returns every violation,
//! not the first. The human layer (`approve_*`) records a judgement of meaning as an
//! `Approved` event, and can be recorded only when the machine layer finds nothing.
//!
//! | gate | target | approved by |
//! |---|---|---|
//! | CR | a Finding | a Reviewer |
//! | CS | a Spec: the Contract follows from its Findings and is bound by its pins | the User |
//! | CI | a Design: the approach and the split into items | a Reviewer |
//! | CA | an item, at one revision of the code | an independent Reviewer |
//! | CM | a measurement against the hypotheses fixed before the run | whoever reads it |
//!
//! `by` is a declared name, as a Git author is: the argument, else
//! [`Config::user`]. Nothing authenticates it.
//!
//! **CA records the revision itself.** [`approve_ca`](Workspace::approve_ca) takes
//! the current revision; the person types no hash. An older one can be named. A dirty
//! working tree stops the approval ("commit first") and there is no override. The pin
//! check and the test run use the very revision that is recorded, and the pin check
//! reads the test files (`test.files`: globs under the scan directories
//! `paths.scan_dirs`, the whole of them by default) in both directions: a pin the
//! item declares that no file carries, and a tag in those files that **no Spec of the
//! project** declared. A tag that is shaped like a pin and is none is a violation of
//! its own, and a test run that outlasts `test.timeout` is stopped and records
//! nothing.
//!
//! **CM is a record, not a state.** [`record_cm`](Workspace::record_cm) takes a
//! [`CmInput`](check::CmInput): a pass, a *wiring gap* with a hint at the missing item
//! (add an item to the Design), or a *wrong hypothesis* with the Finding that records
//! the measurement (revise the Spec). The hypotheses compared are the ones the CS
//! approval fixed, found by the tool.
//!
//! [`revoke`](Workspace::revoke) withdraws an approval with a reason. Whether
//! something is approved is asked of the whole set of events (an approval exists and
//! no revocation names it), never "does it match the latest approval".
//!
//! # One place decides what is written
//!
//! Every write goes through one commit ([`commit`]): the **gate** (the secret scan of
//! every text about to be stored, the leak lint for the repo store, the machine check
//! of an approval), then the **append** (objects first, so an event never points at a
//! missing text), then every **subscriber** ([`subscriber`]). A gate failure writes
//! nothing. A subscriber failure never undoes or stops the write: it comes back as a
//! [`SubscriberWarning`] in the result. The built-in subscribers rebuild the map
//! ([`MapExporter`]), update the cache ([`CacheUpdater`]) and, if the configuration
//! asks, the status and changelog Markdown.
//!
//! # Ingest: files to events
//!
//! The document files are the source of content and are edited by people; the
//! event log records history. [`ingest`](Workspace::ingest) keeps them in step:
//!
//! 1. The stamps (path, modification time, size) are compared with the cache; only
//!    changed files are read. More changed files than `rebuild_threshold` (a branch
//!    switch) means the cache is rebuilt instead of diffed.
//! 2. A new, unregistered file whose text equals the last recorded text of a file
//!    that disappeared is a rename: a `Moved` event.
//! 3. A file that disappeared writes **no event**; the entity stays, shown as "file
//!    missing".
//! 4. A file that does not parse (a broken front matter, a conflict marker) is not
//!    dropped: it is recorded as a version of its own (its text is stored, the secret
//!    scan applies) and the entity is Invalid.
//! 5. A file that parses is built into an entity (references resolved, revisions
//!    checked); a changed hash, or only a changed supplementary section, writes a
//!    `Revised`, and each new item number in a Design writes a `Registered`.
//!
//! A file that nobody registered is never taken in silently: it is listed as
//! unregistered, with the command that registers it.
//!
//! # Public and private
//!
//! Where an entity lives is decided by where it is put, never by a flag in the
//! document: [`Config::default_store`] chooses, [`add`](Workspace::add) can override,
//! and [`publish`](Workspace::publish) moves a document to the repo store, writing an
//! event in each store. A public document refers to a private entity only by its
//! opaque `private:<id>`; copying its alias or title is stopped by the leak lint at
//! the gate. The reverse direction is free. A reference that cannot be looked up here
//! (a CI machine, a public clone) is shown as unresolved, not as an error.
//!
//! # Query and projection
//!
//! [`get`](Workspace::get) returns one entity of any kind, content and classification
//! together; [`find`](Workspace::find) answers one query over classification, derived
//! state, full text and relations, with `limit`, `cursor` and `fields` so a tool
//! server cannot flood a context. The views are [`status`](Workspace::status) (both
//! axes, integration, pins found and declared, the next command, unregistered files),
//! [`trace`](Workspace::trace) (Finding to Spec to Design to item to approved
//! revision), [`changelog`](Workspace::changelog) (from events) and
//! [`audit`](Workspace::audit) (who approved which version, and when). Generated files
//! are values handed to the storage adapter ([`projection`]); they carry no derived
//! state except the optional status file, so a commit does not change them for
//! unrelated reasons.

pub mod check;
pub mod commit;
pub mod config;
pub mod error;
pub mod ingest;
pub mod model;
pub mod ports;
pub mod projection;
pub mod query;
pub mod raw;
pub mod register;
pub mod scaffold;
pub mod store_contract;
pub mod subscriber;
pub mod view;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

pub use config::Config;
pub use error::AppError;
pub use ports::Ports;
pub use subscriber::{CacheUpdater, MapExporter, SubscriberError, SubscriberWarning};

use subscriber::Builtin;

/// The one entry point: ports, settings and subscribers, with a method per use
/// case.
///
/// Generic over the ports, so a test runs the same code on an in-memory fake and the
/// command line runs it on real adapters.
#[derive(Debug)]
pub struct Workspace<P: Ports> {
    ports: P,
    config: Config,
    subscribers: Vec<Builtin>,
}

impl<P: Ports> Workspace<P> {
    /// A workspace over these ports, with the subscribers the configuration
    /// asks for.
    pub fn new(ports: P, config: Config) -> Self {
        let subscribers = Builtin::from_config(&config);
        Self {
            ports,
            config,
            subscribers,
        }
    }

    /// The settings.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The ports, for an adapter that needs to close them.
    pub fn ports(&self) -> &P {
        &self.ports
    }
}
