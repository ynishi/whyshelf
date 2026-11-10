//! A shelf of documents whose kinds a project declares: the second model of whyshelf.
//!
//! The model of the rest of [`domain`](crate::domain) has three fixed kinds of
//! document (Finding, Spec, Design), and every rule knows them. A shelf has none
//! built in. A project declares *books* (a directory, a grammar of keys, fields,
//! links to other books), and the same rules serve every book: what a document is,
//! what it links to, what people decided about it, and whether that decision still
//! holds.
//!
//! # The pieces
//!
//! ```text
//!   BookSchemaDecl ─► schema    Schema::validate ──► Schema           (declared once)
//!                         │
//!   DocReading ──────► doc, build   Shelf::build ──► Shelf            (what the files are)
//!                         │                           valid Docs, InvalidDocs, link cycles
//!   Record ──────────► ledger   Ledger::build ──► Ledger              (what people decided)
//!                         │                           DeclaredStatus, FrameGraph
//!                         │          frame_approve / frame_present / frame_retire
//!                         │            └─► Decision::approve / deny / deprecate / retire
//!                         ├─► lookup     resolve ──► Resolution       (what a person typed)
//!                         └─► freshness  assess ──► Assessment        (whether it still holds)
//! ```
//!
//! Each arrow points from a module to what it uses, and nothing points back: a
//! schema knows no document, a document no ledger, a ledger no freshness. The
//! problem and refusal types are in the module that makes them (there is no shared
//! error module, which would turn that around).
//!
//! The documents are read, and the ledger's records are turned into JSON and back,
//! by adapters; this module sees only values. It has no file, process,
//! environment, clock or randomness (the caller passes in each identifier and
//! each time), and no wire format.
//!
//! # Who decides what
//!
//! Every judgement the model makes has one place, and the rest asks it. This table
//! is where they are; when a judgement seems to be needed elsewhere, the entry here
//! is missing something.
//!
//! | judgement | the one place | what it returns |
//! |---|---|---|
//! | is the declaration sound | [`Schema::validate`] | the [`Schema`], or all its problems |
//! | which book a file is in | [`Schema::membership`] | [`Membership`] |
//! | the grammar of a key | [`KeyPattern::parse`] | a [`KeyForm`], or a [`KeyProblem`] |
//! | a key in a book | [`Shelf::find_key`] | a [`KeyHit`] |
//! | a file that cannot be read | the adapter's parser makes a [`DocReading::Unparsed`] and [`Shelf::build`] turns it into a [`DocProblem::Unreadable`] | a [`RawProblem`] |
//! | the entries of a front matter | [`Shelf::build`] | [`FieldValue`]s and [`LinkState`]s, or [`DocProblem`]s |
//! | the form and validity of a document | [`Shelf::build`] | a valid [`Doc`] or an [`InvalidDoc`] |
//! | a link | [`Shelf::build`] (through `find_key`) | a [`LinkState`] |
//! | what an identity is now | [`Shelf::state_of`], and [`DocState::valid_or_kind`] into an owned form | a [`DocState`] |
//! | which links have which meaning | [`Shelf::links_of`] | the links of one [`Propagation`] |
//! | what a hash covers | `hash_doc` in `build.rs`, private | a [`DocHash`] (made by `DocHash::computed`) |
//! | a hash or a pin read from a record | `DocHash::restore`, `FramePin::restore` | called by [`Record::from_canon`] only |
//! | cycles among links | [`Shelf::build`] | [`Cycle`]s of [`CycleSource::Links`] |
//! | the graph of frames and its cycles | [`Ledger::frame_graph`] | a [`FrameGraph`], [`Cycle`]s of [`CycleSource::Frames`] |
//! | who replaces whom (a fact of links) | [`Shelf::replacers_of`] | the candidates |
//! | who succeeds whom (a verdict) | [`Successors::of`] | [`Successors`] |
//! | the facts a decision stands on | [`frame_approve`], [`frame_present`], [`frame_retire`] | an [`ApproveFrame`], a [`PresentFrame`], a [`RetireFrame`], or a [`FrameRefusal`] |
//! | the invariants of a decision | `Decision::assemble`, private | a [`Decision`] or a [`DecisionProblem`] |
//! | reading a record back | [`Record::from_canon`] | a [`Record`] or a [`RecordProblem`] |
//! | a contradiction among records | [`Ledger::build`] | [`LedgerProblem`]s |
//! | what was decided | [`Ledger::status_of`] | a [`DeclaredStatus`] |
//! | what a person typed | [`resolve`] | a [`Resolution`] |
//! | how strong a cause is | [`Cause::severity`] | a [`Severity`] |
//! | the level of a freshness | [`Freshness::from_causes`] | a [`Freshness`] |
//! | freshness, propagation, notices | [`assess`] | an [`Assessment`] |
//! | where a degradation starts | [`Assessment::roots_of`] | [`RootCause`]s |
//!
//! Two of these cannot be enforced by visibility alone: `DocHash::computed` and
//! `DocHash::restore` (and the two of [`FramePin`]) are visible to the whole module,
//! so the rule is a name. `computed` and `taken` are called when a document is read
//! and when an approval is framed; `restore` only where a record is read back.
//!
//! # What a hash covers
//!
//! A [`DocHash`] covers the name of the book, the fields and the body; it leaves out
//! the key, the file's place and the links. A document that is renamed or moved
//! within its book stays approved; one whose upstream's key changed does not change
//! its own hash; a change in what a `Depends` link names is told by
//! [`Cause::LinksChanged`], by comparison with what the approval pinned. `Informs`
//! and `Replaces` links never enter a freshness.
//!
//! # Where freshness is read
//!
//! [`assess`] reads the strength of a cause from [`Cause::severity`] and the level of
//! a freshness from [`Freshness::from_causes`]; the rules that find causes are in the
//! module page of [`freshness`].
//!
//! # Known limitations
//!
//! - **Files of the older model.** This module uses of the older model only value
//!   types that do not depend on a kind of document: [`ContentHash`] and
//!   [`HashKind`], [`Canon`], [`EntityId`], [`DocId`] and [`ShortId`], [`RelPath`],
//!   [`Actor`], the non-empty collections and [`ValueError`]. Those files still sit
//!   beside the older model's (`error.rs` and `location.rs` use its types for other
//!   errors and values), so the whole dependency on it is cut only when the older
//!   model is removed and these types are split out. Nothing here calls a function
//!   of the older model.
//! - **A key is unique within its book.** Across books the same text can be a key,
//!   which is why a bare key can be [`Resolution::Ambiguous`].
//! - **An identity in an unreadable file.** When some file's `id:` cannot be read,
//!   an identity no readable file carries is [`DocState::Unknown`], not
//!   [`DocState::Gone`], and what stands on it is [`Cause::SelfUnknown`] or
//!   [`Cause::UpstreamUnknown`]: the model does not claim a document is gone when
//!   it may be in a file it could not read.
//! - **Incremental reading is out of scope.** [`Shelf::build`] takes every file.
//!
//! [`ContentHash`]: crate::domain::hash::ContentHash
//! [`HashKind`]: crate::domain::hash::HashKind
//! [`Canon`]: crate::domain::canon::Canon
//! [`EntityId`]: crate::domain::id::EntityId
//! [`DocId`]: crate::domain::id::DocId
//! [`ShortId`]: crate::domain::id::ShortId
//! [`RelPath`]: crate::domain::location::RelPath
//! [`Actor`]: crate::domain::actor::Actor
//! [`ValueError`]: crate::domain::error::ValueError
//! [`Schema::validate`]: schema::Schema::validate
//! [`Schema`]: schema::Schema
//! [`Schema::membership`]: schema::Schema::membership
//! [`Membership`]: schema::Membership
//! [`KeyPattern::parse`]: schema::KeyPattern::parse
//! [`KeyForm`]: schema::KeyForm
//! [`KeyProblem`]: schema::KeyProblem
//! [`Propagation`]: schema::Propagation
//! [`Shelf::find_key`]: build::Shelf::find_key
//! [`Shelf::build`]: build::Shelf::build
//! [`Shelf::state_of`]: build::Shelf::state_of
//! [`Shelf::links_of`]: build::Shelf::links_of
//! [`Shelf::replacers_of`]: build::Shelf::replacers_of
//! [`KeyHit`]: build::KeyHit
//! [`DocReading::Unparsed`]: build::DocReading::Unparsed
//! [`DocProblem::Unreadable`]: build::DocProblem::Unreadable
//! [`DocProblem`]: build::DocProblem
//! [`RawProblem`]: build::RawProblem
//! [`InvalidDoc`]: build::InvalidDoc
//! [`DocState`]: build::DocState
//! [`DocState::valid_or_kind`]: build::DocState::valid_or_kind
//! [`DocState::Unknown`]: build::DocState::Unknown
//! [`DocState::Gone`]: build::DocState::Gone
//! [`Cycle`]: build::Cycle
//! [`CycleSource::Links`]: build::CycleSource::Links
//! [`CycleSource::Frames`]: build::CycleSource::Frames
//! [`Doc`]: doc::Doc
//! [`DocHash`]: doc::DocHash
//! [`FramePin`]: doc::FramePin
//! [`FieldValue`]: doc::FieldValue
//! [`LinkState`]: doc::LinkState
//! [`Record::from_canon`]: ledger::Record::from_canon
//! [`Record`]: ledger::Record
//! [`RecordProblem`]: ledger::RecordProblem
//! [`Ledger::frame_graph`]: ledger::Ledger::frame_graph
//! [`Ledger::build`]: ledger::Ledger::build
//! [`Ledger::status_of`]: ledger::Ledger::status_of
//! [`FrameGraph`]: ledger::FrameGraph
//! [`frame_approve`]: ledger::frame_approve
//! [`frame_present`]: ledger::frame_present
//! [`frame_retire`]: ledger::frame_retire
//! [`ApproveFrame`]: ledger::ApproveFrame
//! [`PresentFrame`]: ledger::PresentFrame
//! [`RetireFrame`]: ledger::RetireFrame
//! [`FrameRefusal`]: ledger::FrameRefusal
//! [`Decision`]: ledger::Decision
//! [`DecisionProblem`]: ledger::DecisionProblem
//! [`LedgerProblem`]: ledger::LedgerProblem
//! [`DeclaredStatus`]: ledger::DeclaredStatus
//! [`resolve`]: lookup::resolve
//! [`Resolution`]: lookup::Resolution
//! [`Resolution::Ambiguous`]: lookup::Resolution::Ambiguous
//! [`Successors::of`]: freshness::Successors::of
//! [`Successors`]: freshness::Successors
//! [`Cause::severity`]: freshness::Cause::severity
//! [`Cause::LinksChanged`]: freshness::Cause::LinksChanged
//! [`Cause::SelfUnknown`]: freshness::Cause::SelfUnknown
//! [`Cause::UpstreamUnknown`]: freshness::Cause::UpstreamUnknown
//! [`Severity`]: freshness::Severity
//! [`Freshness::from_causes`]: freshness::Freshness::from_causes
//! [`Freshness`]: freshness::Freshness
//! [`assess`]: freshness::assess
//! [`Assessment`]: freshness::Assessment
//! [`Assessment::roots_of`]: freshness::Assessment::roots_of
//! [`RootCause`]: freshness::RootCause

pub mod build;
pub mod doc;
pub mod freshness;
pub mod ledger;
pub mod lookup;
pub mod schema;

#[cfg(test)]
mod fixtures_tests;
