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
//!   module       uses                                  entry ──► what it makes
//!
//!   schema                                             Schema::validate ──► Schema
//!                                                        (BookSchemaDecl in; declared once)
//!   artifact                                           ArtifactHash::of_content, ContentAddress,
//!                                                        Revision (opaque), Artifact
//!                                                        (a file that is no document)
//!   doc       ──► schema, artifact                     DocDraft, Doc::new ──► Doc, with its Links
//!                                                        and its ArtifactLinks
//!   graph                                              cycles_of ──► Cycle; components
//!   build     ──► schema, artifact, doc, graph         Shelf::artifact_paths ──► the paths to read
//!                                                      Shelf::build ──► Shelf
//!                                                        (DocReading and ArtifactReadings in: valid
//!                                                         Docs, InvalidDocs, link cycles)
//!   ledger    ──► schema, artifact, doc, graph, build  Ledger::build ──► Ledger, FrameGraph
//!                                                        (Record in; what people decided)
//!                                                      frame_approve / frame_present / frame_retire
//!                                                        └─► ApproveFrame::addressed
//!                                                        └─► Decision::approve / deny / deprecate / retire
//!   lookup    ──► schema, build, ledger                resolve ──► Resolution  (what a person typed)
//!   trace     ──► schema, artifact, doc, build         trace ──► Trace         (the links, up and down)
//!   freshness ──► schema, artifact, doc, graph,        assess ──► Assessment   (whether it still holds)
//!                 build, ledger
//!   snapshot  ──► artifact, doc, build, ledger         Snapshot::of ──► Snapshot (what an approval saw),
//!                                                        SnapshotItem::retrieval ──► Retrieval
//! ```
//!
//! Each arrow points from a module to what it uses, and nothing points back: a
//! schema knows no document, a document no shelf, a graph no document at all (only
//! identities), a ledger no freshness. The problem and refusal types are in the
//! module that makes them (there is no shared error module, which would turn that
//! around).
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
//! | which book a file is in | `Schema::membership` (visible to the shelf's modules only) | a `Membership` |
//! | the grammar of a key | [`KeyPattern::parse`] | a [`KeyForm`], or a [`KeyProblem`] |
//! | a key in a book | `Shelf::find_key` (the same) | a `KeyHit` |
//! | a file that cannot be read | the adapter's parser makes a [`DocReading::Unparsed`] and [`Shelf::build`] turns it into a [`DocProblem::Unreadable`] | a [`RawProblem`] |
//! | the entries of a front matter | [`Shelf::build`] | [`FieldValue`]s and the keys written in each [`Link`], or [`DocProblem`]s |
//! | the form and validity of a document | [`Shelf::build`], which makes each valid [`Doc`] once, whole, with every link its book declares | a valid [`Doc`] or an [`InvalidDoc`] |
//! | a link | [`Shelf::build`], once: the written key read in the target book's grammar and looked up in the index `Shelf::find_key` reads | a [`LinkState`]: a [`ResolvedLink`] with the target's id, key and hash, or an [`UnresolvedLink`] with the target book and the key it read as |
//! | what an identity is now | [`Shelf::state_of`], and [`DocState::valid_or_failure`] / [`DocState::presence`] into an owned form | a [`DocState`], a [`DocFailure`] (the one "not valid", carried by refusals and causes; its [`FailureTag`] kept by a retirement), a [`DocPresence`] |
//! | how many files each book holds | [`Shelf::tally_by_book`] (through `Schema::membership`) | a [`BookTally`] per book |
//! | the links around a document, up and down | [`trace`](trace::trace) | a [`Trace`] of [`LinkEdge`]s |
//! | which links have which meaning | [`Doc::links_of`], from the [`Link`]s of the document (each carries its [`LinkKind`]) | the links of one [`Propagation`] |
//! | what a hash covers | `hash_doc` in `doc.rs`, private, called by `DocDraft::new` only | a [`DocHash`] (made by `DocHash::computed`, private to `doc.rs`) |
//! | a hash read from a record | `DocHash::restore` | called by [`Record::from_canon`] only |
//! | a pin | the private constructor of [`FramePin`] in `ledger.rs`: [`frame_approve`] takes it from a [`ResolvedLink`] as it is, [`Record::from_canon`] reads it back | a [`FramePin`] |
//! | cycles, in either graph | `graph::cycles_of` (and `graph::components` for the order upstream first), called by [`Shelf::build`] and by the `FrameGraph` that `Ledger::frame_graph` makes | [`Cycle`]s of [`CycleSource::Links`] or [`CycleSource::Frames`] |
//! | the graph of frames | `Ledger::frame_graph` (visible to the shelf's modules only) | a `FrameGraph` |
//! | who replaces whom (a fact of links) | [`Shelf::replacers_of`] | the candidates |
//! | who succeeds whom (a verdict) | [`Successors::of`] | [`Successors`] |
//! | the facts a decision stands on | [`frame_approve`], [`frame_present`], [`frame_retire`] | an [`ApproveFrame`], a [`PresentFrame`], a [`RetireFrame`], or a [`FrameRefusal`] |
//! | the invariants of a decision | `Decision::assemble`, private | a [`Decision`] or a [`DecisionProblem`] |
//! | reading a record back | [`Record::from_canon`] | a [`Record`] or a [`RecordProblem`] |
//! | a contradiction among records | [`Ledger::build`] | [`LedgerProblem`]s |
//! | what was decided | [`Ledger::status_of`] | a [`DeclaredStatus`] |
//! | what a person typed | [`resolve`] | a [`Resolution`] (an identity only the ledger knows has an [`Absence`], which cannot be invalid) |
//! | which paths a document's links to artifacts name (A1) | [`Shelf::artifact_paths`], before the build, and [`Shelf::build`], which makes each path an [`Artifact`] that is there or absent (never unresolved); a text that is no path is a [`DocProblem::ArtifactPathInvalid`] | the paths; an [`Artifact`] |
//! | what an artifact's hash covers | the adapter, which hands the bytes it chose to [`ArtifactHash::of_content`] | an [`ArtifactHash`] |
//! | the form of a content address and of a revision (A7) | the adapter, then `from_validated` ([`ContentAddress::from_validated`], [`Revision::from_validated`]) checks only that it is not empty and has no space | an opaque value |
//! | a link to artifacts never replaces | [`Schema::validate`] ([`SchemaProblem::ArtifactReplaces`]); the type [`ArtifactPropagation`] has no `Replaces` | an [`ArtifactLinkKind`] |
//! | a pin of an artifact (A2) | the private constructor of [`ArtifactPin`]: [`frame_approve`] pins every artifact of a `Depends` link as it is, a file or none, and refuses nothing for it | an [`ArtifactPin`] with a [`PinnedArtifact`] |
//! | which address goes on which item of an approval (A5) | [`ApproveFrame::addressed`], from the files the frame knows ([`ApproveFrame::files`]) | the frame, with the addresses and the revision |
//! | which records keep addresses and a revision, and how a record without them reads (A5) | [`Record::to_canon`] / [`Record::from_canon`]: optional keys, left out when empty | a [`Record`] |
//! | how strong a cause is | [`Cause::severity`] | a [`Severity`] |
//! | what a pinned artifact makes of an approval (A3), with hashes only (A4) | `artifact_causes` in `freshness.rs`, called by [`assess`] | [`Cause::ArtifactChanged`], [`Cause::ArtifactGone`], [`Cause::ArtifactArrived`], [`Cause::ArtifactLinksChanged`] |
//! | what an approval that stands saw | [`Snapshot::of`], from [`Ledger::standing_decision`] (which [`Ledger::status_of`] reads too) | a [`Snapshot`] of [`SnapshotItem`]s |
//! | whether the content of an item came back (A6) | [`SnapshotItem::retrieval`] | a [`Retrieval`]: read, an address that cannot be read, or no address |
//! | whether an item of a snapshot changed | [`SnapshotItem::changed_from`], by hash | a truth value |
//! | the level of a freshness | [`Degradation::level`], computed from the causes; [`Freshness::from_causes`] is the only maker of a [`Degradation`] | a [`Severity`] |
//! | freshness, propagation, notices | [`assess`] | an [`Assessment`] |
//! | where one identity stands, listed or not | [`Assessment::standing_of`] | a [`Standing`] |
//! | where a degradation starts | [`Assessment::roots_of`] | [`RootCause`]s |
//!
//! Two of these cannot be enforced by visibility alone: `DocHash::restore` and
//! `ArtifactHash::restore` are visible to the whole module (the hash is made in
//! `doc.rs` or by an adapter, and read back in `ledger.rs`), so the rule is a name:
//! `restore` only where a record is read back.
//!
//! # Files that are no document
//!
//! A book may link to *artifacts* (`target = "artifact"`): code, tests, data, named
//! by their path. An approval pins what is at each path of a `Depends` link, a hash
//! or "no file", the way it pins an upstream document, and freshness compares the
//! same way, by hash only. Beside each hash, a record may keep a
//! [`ContentAddress`] where the content can be read back later, and a
//! [`Revision`], both opaque: the domain knows no version control, and a project
//! outside one has the same freshness. What an approval saw is a [`Snapshot`], and
//! whether its content came back is a [`Retrieval`].
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
//! a freshness from [`Degradation::level`], which [`Freshness::from_causes`] makes
//! from the causes; the rules that find causes are in the module page of
//! [`freshness`].
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
//!   [`DocState::Gone`], and what stands on it is [`Cause::SelfNotValid`] or
//!   [`Cause::UpstreamNotValid`] of [`DocFailure::Unknown`]: the model does not
//!   claim a document is gone when it may be in a file it could not read.
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
//! [`Shelf::artifact_paths`]: build::Shelf::artifact_paths
//! [`Artifact`]: artifact::Artifact
//! [`ArtifactHash`]: artifact::ArtifactHash
//! [`ArtifactHash::of_content`]: artifact::ArtifactHash::of_content
//! [`ContentAddress`]: artifact::ContentAddress
//! [`ContentAddress::from_validated`]: artifact::ContentAddress::from_validated
//! [`Revision`]: artifact::Revision
//! [`Revision::from_validated`]: artifact::Revision::from_validated
//! [`DocProblem::ArtifactPathInvalid`]: build::DocProblem::ArtifactPathInvalid
//! [`SchemaProblem::ArtifactReplaces`]: schema::SchemaProblem::ArtifactReplaces
//! [`ArtifactPropagation`]: schema::ArtifactPropagation
//! [`ArtifactLinkKind`]: schema::ArtifactLinkKind
//! [`ArtifactPin`]: ledger::ArtifactPin
//! [`PinnedArtifact`]: ledger::PinnedArtifact
//! [`ApproveFrame::addressed`]: ledger::ApproveFrame::addressed
//! [`ApproveFrame::files`]: ledger::ApproveFrame::files
//! [`Record::to_canon`]: ledger::Record::to_canon
//! [`Ledger::standing_decision`]: ledger::Ledger::standing_decision
//! [`Cause::ArtifactChanged`]: freshness::Cause::ArtifactChanged
//! [`Cause::ArtifactGone`]: freshness::Cause::ArtifactGone
//! [`Cause::ArtifactArrived`]: freshness::Cause::ArtifactArrived
//! [`Cause::ArtifactLinksChanged`]: freshness::Cause::ArtifactLinksChanged
//! [`Snapshot`]: snapshot::Snapshot
//! [`Snapshot::of`]: snapshot::Snapshot::of
//! [`SnapshotItem`]: snapshot::SnapshotItem
//! [`SnapshotItem::retrieval`]: snapshot::SnapshotItem::retrieval
//! [`SnapshotItem::changed_from`]: snapshot::SnapshotItem::changed_from
//! [`Retrieval`]: snapshot::Retrieval
//! [`Schema::validate`]: schema::Schema::validate
//! [`Schema`]: schema::Schema
//! [`KeyPattern::parse`]: schema::KeyPattern::parse
//! [`KeyForm`]: schema::KeyForm
//! [`KeyProblem`]: schema::KeyProblem
//! [`Propagation`]: schema::Propagation
//! [`Shelf::build`]: build::Shelf::build
//! [`Shelf::state_of`]: build::Shelf::state_of
//! [`Shelf::replacers_of`]: build::Shelf::replacers_of
//! [`DocReading::Unparsed`]: build::DocReading::Unparsed
//! [`DocProblem::Unreadable`]: build::DocProblem::Unreadable
//! [`DocProblem`]: build::DocProblem
//! [`RawProblem`]: build::RawProblem
//! [`InvalidDoc`]: build::InvalidDoc
//! [`DocState`]: build::DocState
//! [`DocState::valid_or_failure`]: build::DocState::valid_or_failure
//! [`DocState::presence`]: build::DocState::presence
//! [`DocFailure`]: build::DocFailure
//! [`DocFailure::Unknown`]: build::DocFailure::Unknown
//! [`FailureTag`]: build::FailureTag
//! [`DocPresence`]: build::DocPresence
//! [`Shelf::tally_by_book`]: build::Shelf::tally_by_book
//! [`BookTally`]: build::BookTally
//! [`Trace`]: trace::Trace
//! [`LinkEdge`]: trace::LinkEdge
//! [`DocState::Unknown`]: build::DocState::Unknown
//! [`DocState::Gone`]: build::DocState::Gone
//! [`Cycle`]: graph::Cycle
//! [`CycleSource::Links`]: graph::CycleSource::Links
//! [`CycleSource::Frames`]: graph::CycleSource::Frames
//! [`Doc`]: doc::Doc
//! [`DocHash`]: doc::DocHash
//! [`FramePin`]: ledger::FramePin
//! [`FieldValue`]: doc::FieldValue
//! [`LinkState`]: doc::LinkState
//! [`ResolvedLink`]: doc::ResolvedLink
//! [`UnresolvedLink`]: doc::UnresolvedLink
//! [`Link`]: doc::Link
//! [`Doc::links_of`]: doc::Doc::links_of
//! [`LinkKind`]: schema::LinkKind
//! [`Record::from_canon`]: ledger::Record::from_canon
//! [`Record`]: ledger::Record
//! [`RecordProblem`]: ledger::RecordProblem
//! [`Ledger::build`]: ledger::Ledger::build
//! [`Ledger::status_of`]: ledger::Ledger::status_of
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
//! [`Absence`]: lookup::Absence
//! [`Successors::of`]: freshness::Successors::of
//! [`Successors`]: freshness::Successors
//! [`Cause::severity`]: freshness::Cause::severity
//! [`Cause::LinksChanged`]: freshness::Cause::LinksChanged
//! [`Cause::SelfNotValid`]: freshness::Cause::SelfNotValid
//! [`Cause::UpstreamNotValid`]: freshness::Cause::UpstreamNotValid
//! [`Severity`]: freshness::Severity
//! [`Freshness::from_causes`]: freshness::Freshness::from_causes
//! [`Degradation`]: freshness::Degradation
//! [`Degradation::level`]: freshness::Degradation::level
//! [`Freshness`]: freshness::Freshness
//! [`assess`]: freshness::assess
//! [`Assessment`]: freshness::Assessment
//! [`Assessment::roots_of`]: freshness::Assessment::roots_of
//! [`Assessment::standing_of`]: freshness::Assessment::standing_of
//! [`Standing`]: freshness::Standing
//! [`RootCause`]: freshness::RootCause

pub mod artifact;
pub mod build;
pub mod doc;
pub mod freshness;
pub mod graph;
pub mod ledger;
pub mod lookup;
pub mod schema;
pub mod snapshot;
pub mod trace;

#[cfg(test)]
mod fixtures_tests;
