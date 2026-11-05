//! The rules and types of the problem, in its own words.
//!
//! Pure: nothing here touches the file system, a process, the environment, the
//! network, the clock or a random source. A value that comes from outside (a
//! file's contents, the current time, a new identifier) is passed in by the
//! caller. A value with rules is a type, not a `String`: an enum for a closed set,
//! a newtype with a private field and a checking constructor for anything else.
//! This module documents the whole model; the submodules document the details.
//!
//! # What whyshelf is
//!
//! whyshelf ties a change to its reasons. It follows four stages, pins each to the
//! one above by name and hash, and records every approval as an event. State that
//! can be derived (is it approved, is it stale, has it reached the baseline, are
//! the pins in the tests) is never written down; it is computed from the files and
//! the event log whenever it is asked for.
//!
//! # The four stages
//!
//! ```text
//!   Finding   a cause, observation or premise                     gate CR
//!      |      (finding_type decides the required metadata)
//!      v
//!   Spec      a Contract + the acceptance declaration              gate CS
//!      |      (PIN / UT tags to bind it, HYP hypotheses to measure)
//!      v
//!   Design    a Crux (structured decision record) + items          gate CI
//!      |      (every declared pin is assigned to some item)
//!      v
//!   Impl      code + tests tagged `@pin: PIN-*`                    gate CA (per item)
//!             a measurement of the hypotheses                      gate CM
//! ```
//!
//! | stage | module | in short |
//! |---|---|---|
//! | Finding | [`finding`] | what is true underneath; five types, each with required metadata |
//! | Spec | [`spec`] | what must hold, and how it will be bound |
//! | Design | [`design`] | how it will be built, and why not another way |
//! | Impl | [`pin`], [`evaluate`] | code and tests live outside the model; their pins are scanned and matched |
//!
//! Every change starts from a Finding: a Spec that names none is refused by type.
//! Each stage refers to the one above by identifier only. The hash of the upstream
//! is not in the file; it is recorded by the approval event, so an upstream change
//! never forces an edit of a downstream file.
//!
//! # Gates: a machine layer and a human layer
//!
//! Each stage has a CHECK gate. Layer 1 is a machine check of form ([`gate`]; the
//! violations are in [`lint`]):
//! links resolve, upstream is approved and current, pins match in both directions,
//! tests pass. Layer 2 is a judgement of meaning by a person, recorded as an
//! `Approved` event ([`approval`]); it can be recorded only from the proof that layer 1
//! passed ([`gate::Passed`], which only a gate's check makes). The machine never judges
//! whether a claim is well founded, and the person is not asked to count links.
//!
//! | gate | about | approved by |
//! |---|---|---|
//! | CR | a Finding is trustworthy | a Reviewer |
//! | CS | the Contract follows from the Findings and is bound by its pins (by default; see [`policy`]) | the User |
//! | CI | the approach and the split into items are sound | a Reviewer |
//! | CA | an item's tests really hold the Contract (checked at one revision) | an independent Reviewer |
//! | CM | a measurement met the hypotheses; a miss is either a wiring gap or a wrong hypothesis | whoever reads the measurement |
//!
//! # State has two axes
//!
//! Every entity has an approval state (Invalid, Draft, Approved, Modified) and an
//! upstream state (Ok, Stale, Blocked), so "edited after approval" and "upstream
//! changed" are told apart (an entity is Stale only while its own approval stands;
//! see [`state`] for which pairs occur). A third fact, completeness, says whether
//! everything it stands on can be read in this environment (Partial when a `private:`
//! upstream cannot, as in a public clone). An item adds an integration state (whether the
//! approved revision reached the baseline) and a progress display (how many of its
//! pins the tests carry). See [`state`] for the meanings and [`evaluate`] for how
//! they are derived, upstream to downstream, with the causes passed down.
//!
//! "Is this approved?" is asked of the whole set of events: an approval exists and
//! no revocation names it. It is never "does it match the latest approval", so two
//! worktrees that approve different versions of the same entity do not undo each
//! other.
//!
//! # What a hash covers
//!
//! Hashes are computed from a canonical form of the parsed value ([`canon`]) in one
//! place ([`hash`]), under a domain separation tag so that no two kinds of hash can
//! collide. The scope is deliberate: it covers what a reviewer approved and leaves
//! out what may change freely.
//!
//! | hash | covers | does not cover |
//! |---|---|---|
//! | Finding | type, metadata, title, evidence label, body | tags, owner, path |
//! | Spec `contract_hash` | grounds, pins, hypotheses, Contract | supplementary sections, tags |
//! | Spec `hyp_hash` | hypotheses only | everything else |
//! | Design | Spec reference, Crux, the items as a set (summary, assigned pins) | Design Notes, tags, item numbers and ids |
//! | item | Design, summary, assigned pins | item number and id, other items |
//!
//! Correcting a typo in a note, retagging, or renumbering the items cannot remove an
//! approval: an item number is a name for people, as a tag is. Text is
//! normalized once, by the adapter that reads a file (line breaks to LF, trailing
//! spaces and tabs removed, the file ends in exactly one LF), before the domain
//! sees it.
//!
//! # Identity and classification
//!
//! Each entity has a UUID v7 that never changes ([`id`]). A path is not an identity:
//! renaming a file or moving it with `mv` keeps its approvals. A human-readable alias
//! can be added. Classification (tags, kind, owner, alias, path) belongs to the tool
//! and lives in events, not in files; the tool never rewrites a user's file. A file's
//! own classification keys are taken in once, at registration, and compared after
//! that ([`front`]). Tags are
//! official (`key:value`, from the project's vocabulary) or extended ([`tag`]).
//! [`classification::fold`] turns the events into the current classification and
//! reports conflicts instead of guessing.
//!
//! # Events and the stream graph
//!
//! The event log is append-only ([`event`]): registration, revision,
//! reclassification, move, approval, revocation, purge, publication. Content lives in
//! files that people edit; history lives in the log. The events of one entity form a
//! graph in which each event lists the tips it saw. An event identifier is the hash of
//! its content and heads, excluding the time (and including the person only for the
//! judgements `Approved` and `Revoked`), so two writers who see the same state and
//! find the same change write records of one event, each with its own time. A fork
//! made by two branches heals at the next write, which lists every tip.
//!
//! # Two stores and what crosses
//!
//! The repo store is committed with the project; the private store lives outside it
//! and is never published. Which one an entity lives in is decided by where it is put,
//! not by a flag in the document, because a forgotten flag would default to public.
//! Publishing writes an event in each store; the one in the repo store is a root that
//! carries a snapshot of the classification, so that store can be folded alone. Past
//! versions and events stay in the private store.
//!
//! References say where they point ([`mod@reference`]): a bare alias or identifier is in
//! the same store, `private:<id>` in the private store, `ext:<project>/<id>` in another
//! project, a URL outside. A `private:` or `ext:` reference into a store that cannot
//! be read here (a CI machine, a public clone) is shown as unresolved, never as an
//! error; one into a store that can be read, and names nothing, is dangling. What a
//! reference comes to for the entity that holds it is decided in one place
//! ([`reference::bind`]). A public document may point at a private entity only by its
//! opaque identifier, written `private:<id>` ([`event::StoreKind::mark_for`]); copying
//! its alias or title, or writing its identifier without the mark, is stopped by the
//! leak lint ([`lint::leak_lint`]).
//!
//! # Merging judgements
//!
//! Git merges the documents. The judgements that people made live in two ledgers
//! (public and private), each an append-only set that merges by union, whose
//! object files are a set that can also delete. The declaration, and what follows
//! from it for revocation, retirement and publication, is in [`ledger`].
//!
//! # Versions of code
//!
//! The model speaks of a revision, of being integrated into a baseline, and of the
//! approved revision ([`revision`]). It knows no branch, no merge strategy and no hash
//! length. A revision identifier is opaque: an adapter validates its text and only then
//! constructs it. A CA approval records the current revision by itself and refuses a
//! dirty working tree.
//!
//! # Known limitations
//!
//! - **Published entities look Draft in a public clone.** Approvals stay in the
//!   private store when an entity is published, so a clone that has only the repo
//!   store sees the entity and its classification but no approval. An unresolved
//!   reference is not an error, but the missing approval is visible as Draft.
//! - **Past versions are not copied on publish.** Older events and stored objects of a
//!   published entity stay in the private store.
//! - **Approvers are declared, not authenticated.** The name in `by` is taken as given;
//!   nothing checks that the implementer and the approver differ.
//! - **A rebase or squash can change a revision identifier.** Whether an approved
//!   revision "reached the baseline" is asked of the revision adapter; how it follows a
//!   rewritten history is its decision.
//! - **Conflicts are reported, not merged.** When two branches set a value differently,
//!   the fold lists the conflict, nobody chooses a side, and a person settles it by
//!   setting the value again (`mv` for the path). A kind or a path in conflict makes
//!   the document Invalid and leaves it out of the typed entities (see
//!   [`classification`]); `publish` and `purge` are refused while one of the values
//!   they rely on is in conflict.
//! - **Time is for display.** Event order is the graph, never the timestamp.
//!
//! # Where to look
//!
//! | module | holds |
//! |---|---|
//! | [`canon`], [`hash`] | canonical encoding and the only place a hash is computed |
//! | [`id`], [`revision`], [`pin`], [`nonempty`], [`tag`], [`actor`], [`location`], [`mod@reference`] | value objects with checking constructors |
//! | [`knowable`] | a fact that may be impossible to tell in this environment |
//! | [`corpus`] | the document files of a project taken together: duplicate ids and aliases, a private document Git would publish, a publication cut short, files with no `id:` |
//! | [`finding`], [`spec`], [`design`] | the three stages with files |
//! | [`front`] | which key of a front matter is content, classification, another kind's or a mistake |
//! | [`event`], [`classification`], [`approval`] | the log, its fold, and the set of approvals |
//! | [`judgement`], [`ledger`], [`judge`], [`adoption`] | the judgements people made as an append-only set that merges by union (concurrent approvals survive a revocation that did not see them; the objects are a set that can delete), who made them, and taking an earlier decision in as an approval |
//! | [`state`], [`evaluate`] | derived state and how it is derived |
//! | [`policy`] | what a project decides for itself: what CS demands of a Spec ([`policy::GatePolicy`], recorded in the approval it lets through) and the pin prefixes and spellings ([`policy::PinVocabulary`]) |
//! | [`gate`], [`next`] | the machine check of each gate and the proof it passed; the next step of an entity |
//! | [`lint`], [`query`] | the violations and the order of upstream problems, the leak lint, drift of generated files, the `find` language |
//! | [`error`] | one enum per kind of failure |

mod vocabulary;

pub mod actor;
pub mod adoption;
pub mod approval;
pub mod canon;
pub mod classification;
pub mod corpus;
pub mod design;
pub mod error;
pub mod evaluate;
pub mod event;
pub mod finding;
pub mod front;
pub mod gate;
pub mod hash;
pub mod id;
pub mod judge;
pub mod judgement;
pub mod knowable;
pub mod ledger;
pub mod lint;
pub mod location;
pub mod next;
pub mod nonempty;
pub mod pin;
pub mod policy;
pub mod query;
pub mod reference;
pub mod revision;
pub mod spec;
pub mod state;
pub mod tag;
