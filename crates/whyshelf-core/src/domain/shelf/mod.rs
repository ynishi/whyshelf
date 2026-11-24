//! A shelf of documents whose kinds a project declares: the model of whyshelf.
//!
//! A shelf has no kind of document built in. A project declares *books* (a
//! directory, a grammar of keys, fields, links to other books), and the same rules
//! serve every book: what a document is, what it links to, what people decided
//! about it, and whether that decision still holds.
//!
//! # The pieces
//!
//! ```text
//!   module       uses                                  entry ──► what it makes
//!
//!   schema    ──► artifact                             Schema::validate ──► Schema
//!                                                        (BookSchemaDecl and the spaces
//!                                                        offered in; declared once)
//!   map       ──► schema, artifact                     ShelfMap::of ──► ShelfMap (the books and
//!                                                        links as declared, in order)
//!   artifact                                           Space, ArtifactName, Comparison::
//!                                                        fingerprint, ArtifactReading (the four
//!                                                        answers), ContentAddress, Revision
//!                                                        (opaque), Artifact (a name in a space)
//!   doc       ──► schema, artifact                     DocDraft, Doc::new ──► Doc, with its Links
//!                                                        and its ArtifactLinks; what its Depends
//!                                                        links resolve to, and name
//!   drift     ──► artifact, doc                        Edit, DocDrift, ArtifactChange (what a pinned
//!                                                        content comes to now)
//!   graph                                              cycles_of ──► Cycle; components
//!   build     ──► schema, artifact, doc, graph         Shelf::artifact_asks ──► the names to resolve
//!                                                      Shelf::build ──► Shelf
//!                                                        (DocReading and ArtifactReadings in: valid
//!                                                         Docs, InvalidDocs, link cycles)
//!                                                      Shelf::next_key / claim_key ──► NewKey
//!                                                      Shelf::check_new ──► NewDoc
//!                                                        (a document not written yet)
//!                                                      Shelf::plan_new ──► NewPlan, or every
//!                                                        PlanProblem (a run of new documents)
//!                                                      Shelf::replan_after_taken ──► PlanAfterTaken
//!   ledger    ──► schema, artifact, doc, graph, build, Ledger::build ──► Ledger, FrameGraph
//!                 drift                                  (Record in; what people decided)
//!                                                      Ledger::declared ──► DeclaredNow (the
//!                                                        approval that stands, as a view of its
//!                                                        record; what its pins come to now)
//!                                                      frame_approve / frame_present / frame_retire
//!                                                        └─► ApproveFrame::addressed
//!                                                        └─► Decision::approve / deny / deprecate / retire
//!   footing   ──► artifact, doc, build, ledger,        Footing::of ──► Footing (what a document stands
//!                 schema                                 on: its links now, its approval's pins,
//!                                                        the artifacts of both)
//!   reach     ──► artifact, doc, build, footing,       walk ──► Walked (what depends on some
//!                 ledger, placement                      documents, and Unwalked: what the walk
//!                                                        could not follow); Reach::of ──► Reach
//!                                                        (and the artifacts they stand on),
//!                                                        Reach::files ──► ReachFiles (the files
//!                                                        of the project all of it is in)
//!   lookup    ──► schema, build, ledger                resolve ──► Resolution  (what a person typed)
//!   cover     ──► schema, artifact, doc, build,        counts ──► whether a document stands;
//!                 ledger                               covering_reach ──► what a covering link
//!                                                        reaches (documents and artifacts) from
//!                                                        those that stand
//!   exemption ──► schema, artifact, doc, build,        exemption_reading ──► Needed, Unneeded or
//!                 ledger, cover                          CannotTell;
//!                                                      frame_exempt ──► ExemptOf (or a refusal)
//!   coverage  ──► schema, artifact, doc, build,        assess ──► CoverAssessment (what covering
//!                 ledger, cover, exemption               links expect, what is missing, what is
//!                                                        exempt, how complete each listing is)
//!   freshness ──► schema, artifact, doc, graph,        assess ──► Assessment   (whether it still holds;
//!                 build, ledger, cover, exemption,       its cover is coverage's answer)
//!                 coverage
//!   trace     ──► schema, artifact, doc, build,        trace ──► Trace         (the links, up and down)
//!                 freshness                            linkage ──► Linkage     (linked at all, each way,
//!                                                        and what the declaration lets be)
//!   snapshot  ──► artifact, doc, build, ledger, drift  Snapshot::of ──► Snapshot (what an approval saw),
//!                                                        SnapshotItem::retrieval ──► Retrieval,
//!                                                        SnapshotItem::compared ──► ItemCompared
//!   order     ──► schema, doc, graph, build, ledger,   stale, pending ──► the documents a run takes;
//!                 freshness, footing                   upstream_first ──► the order approvals are
//!                                                        judged in again;
//!                                                      Foreseen::of ──► Foreseen (what the
//!                                                        approving commands of stale would approve)
//!   settle    ──► freshness, artifact, order           origins_asked ──► OriginsAsked;
//!                                                      settle_all ──► Settlement (what can settle
//!                                                        an origin: read, declare, approve)
//!   units     ──► doc, build, ledger, graph,           plan ──► UnitsPlan (what does not hold,
//!                 freshness, order, footing,             what each stands on and what holds it
//!                 placement                              back, and the places to ask);
//!                                                      divide ──► Units (units of work that
//!                                                        share nothing)
//!   stats     ──► artifact, doc, build, ledger,        stats::of ──► Stats         (how the work went,
//!                 schema                                 from the records alone)
//!   untold    ──► artifact                             untold_causes ──► UntoldCause (names that
//!                                                        cannot be told, by cause)
//!   placement ──► (none)                               Placement (where a name of a space lives
//!                                                        in the files of the project)
//!   filter    ──► schema, doc, build, freshness        StatusFilter::new ──► StatusFilter;
//!                                                      StatusFilter::admits ──► whether a document
//!                                                        passes
//!   rekey     ──► schema, doc, build, ledger           rekey::plan ──► RekeyPlan, or every
//!                                                        RekeyProblem (a document takes a new key;
//!                                                        what its old key's links mean)
//! ```
//!
//! Each arrow points from a module to what it uses, and nothing points back: a
//! schema knows no document, a document no shelf, a graph no document at all (only
//! identities), a ledger no freshness. The problem and refusal types are in the
//! module that makes them (there is no error module shared among them, which would
//! turn that around).
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
//! | what the declaration says, book by book and link by link | [`ShelfMap::of`], from the [`Schema`] alone, in the order of the declaration | a [`ShelfMap`] |
//! | which book a file is in | `Schema::membership` (visible to the shelf's modules only) | a `Membership` |
//! | the grammar of a key | [`KeyPattern::parse`] | a [`KeyForm`], or a [`KeyProblem`] |
//! | a key in a book | `Shelf::find_key` (the same) | a `KeyHit` |
//! | a file that cannot be read | the adapter's parser makes a [`DocReading::Unparsed`] and [`Shelf::build`] turns it into a [`DocProblem::Unreadable`] | a [`RawProblem`] |
//! | a file read as a mapping that holds values of kinds a front matter does not hold | the adapter's parser makes a [`DocReading::Unheld`] and [`Shelf::build`] turns each value into a [`DocProblem::Unheld`] | an [`UnheldValue`] per value |
//! | the entries of a front matter | [`Shelf::build`] | [`FieldValue`]s and the keys written in each [`Link`], or [`DocProblem`]s |
//! | whether a written value says something (no value, a text empty or blank, a list with no item: nothing, for a field of any type, a link, the `id` and the `key`) | `Written::of` in `build.rs` (private), once per entry at the head of the read of the entries | `Written`: not written, written with nothing, a text that says something (as the [`NonBlank`](crate::domain::nonempty::NonBlank) that proves it), or another value |
//! | what nothing means | the field: `declared_field` (an optional field has no value, as if left out; a required one is a [`DocProblem::FieldMissing`] or a [`DocProblem::FieldEmpty`]); a link: no key, counted as it was; the `id` and the `key`: a [`DocProblem::IdEmpty`] or [`DocProblem::KeyEmpty`], the file's identity left unreadable | the [`FieldValue`] or none; a problem |
//! | whether an item of a list says something | `read_items` in `build.rs` (private), for a list field, a link into a book and a link to artifacts (before a text is read as a name) | the items that say something as [`NonBlank`](crate::domain::nonempty::NonBlank)s, and a [`DocProblem::ItemBlank`] with its place for every one that does not |
//! | whether a text is one line | [`OneLine::new`](crate::domain::nonempty::OneLine::new), for a field declared `line` | a [`OneLine`](crate::domain::nonempty::OneLine), or a [`DocProblem::NotOneLine`] |
//! | whether the names `fmt --drop` is asked to drop fit the declaration | [`DropNames::new`](build::DropNames::new) against the [`Schema`], once, through the one test of whether a book declares a name (`Declared::of` in `build.rs`, private) | the [`DropNames`](build::DropNames), or every [`DropProblem`](build::DropProblem) (`id` and `key`, and a name every book declares) |
//! | the entries a file of a book is written with, for a new document and for `fmt` | [`Shelf::form_entries`] leaves out those the build reads as not written and tells no problem for (an entry that says nothing, by the build's own `Written::of`, of an optional field or of a link whose count admits none), and those `fmt` is asked to drop that the book does not declare (the unknown entries of the [`DropNames`](build::DropNames) it is given; `new` gives none); [`Shelf::check_new`] asks it | a [`FormEntries`](build::FormEntries), which [`NewDoc::entries`] holds, with the names it left out for being dropped |
//! | whether the value of a condition of `status` says something | [`StatusFilter::new`](filter::StatusFilter::new), which makes it a [`NonBlank`](crate::domain::nonempty::NonBlank) once | a [`FilterProblem::EmptyValue`](filter::FilterProblem::EmptyValue), or the text |
//! | one text as the value of a field of a type | [`FieldValue::read`](doc::FieldValue::read), of a text that says something already (it never judges that), a `line` through [`OneLine::new`](crate::domain::nonempty::OneLine::new) | the [`FieldValue`](doc::FieldValue), or none when it is no value of the type |
//! | the form and validity of a document | [`Shelf::build`], which makes each valid [`Doc`] once, whole, with every link its book declares | a valid [`Doc`] or an [`InvalidDoc`] |
//! | a link | [`Shelf::build`], once: the written key read in the target book's grammar and looked up in the index `Shelf::find_key` reads | a [`LinkState`]: a [`ResolvedLink`] with the target's id, key and hash, or an [`UnresolvedLink`] with the target book and the key it read as |
//! | what an identity is now | [`Shelf::state_of`], and [`DocState::valid_or_failure`] / [`DocState::presence`] into an owned form | a [`DocState`], a [`DocFailure`] (the one "not valid", carried by refusals and causes; its [`FailureTag`] kept by a retirement), a [`DocPresence`] |
//! | how many files each book holds | [`Shelf::tally_by_book`] (through `Schema::membership`) | a [`BookTally`] per book |
//! | whether a filter of `status` fits the declaration, and which documents it admits | [`StatusFilter::new`](filter::StatusFilter::new) against the [`Schema`], then [`StatusFilter::admits`](filter::StatusFilter::admits) per document, read from the [`Shelf`](build::Shelf) | a [`StatusFilter`](filter::StatusFilter), or every [`FilterProblem`](filter::FilterProblem) (a field asked with a value, or any, and with no value, and no value asked of a field every declaring book requires, among them); whether one document passes, a condition of no value ([`FieldWant::NoValue`](filter::FieldWant::NoValue)) held where a value is not, and a condition of any value ([`FieldWant::AnyValue`](filter::FieldWant::AnyValue)) held where one is |
//! | which files write a key, and whether that is every file that may | `KeyWriters` in `build/keys.rs` (private), made once by [`Shelf::build`] from every file that reads a key in its book's grammar, valid or not, identity read or not; asked through `Shelf::key_writers` (the files of the key's own book that could not be read at all may write it too, and are named when there are any) | the writers of the key (none, one, or a [`KeyCollision`](doc::KeyCollision)), and whether there may be more |
//! | which keys two files write | [`Shelf::collisions`](build::Shelf::collisions), from the same reading; each file of one is invalid, with a [`DocProblem::DuplicateKey`](build::DocProblem::DuplicateKey) that names the others | the [`KeyCollision`](doc::KeyCollision)s, with every file and the identity of each that could be read |
//! | which documents write a key in a link, and which places a link could not be read in | `Shelf::link_writers`, from every file read before any is judged (an invalid file's links are kept); the key of a link is read by the one reader `read_link_key` in `build.rs` (private), the same as the state of a link | the documents that write it, and the places that were not read ([`Unread`](build::Unread)) |
//! | what an identity is called now, for an answer that names an upstream a record pinned | [`Shelf::now_named`](build::Shelf::now_named), from [`Shelf::state_of`], then [`FramePin::named`](ledger::FramePin::named) | a [`NowNamed`](build::NowNamed), a [`PinName`](ledger::PinName) |
//! | which keys of a book are taken | `Shelf::key_writers`, through the two entries below | read by the two entries below only |
//! | what the links of a key mean when a document takes a new key, and what to do | [`rekey::plan`], which asks for every writer and every link to be read (`gather`, which alone makes a [`Gathered`](rekey::Gathered)) and reads each document that writes the key in a link by [`rekey::meaning`] (the evidence, then the caller's words) | a [`RekeyPlan`](rekey::RekeyPlan), or every [`RekeyProblem`](rekey::RekeyProblem) |
//! | the next key of a book | [`Shelf::next_key`]: the highest number taken plus one (`PREFIX-1` when none; a name tail is no number), the text read by [`KeyPattern::parse`] | a [`NewKey`], or a [`NextKeyProblem`] (a book of slugs has none) |
//! | a key given for a new document | [`Shelf::claim_key`]: read by [`KeyPattern::parse`], and not taken | a [`NewKey`], or a [`ClaimKeyProblem`] |
//! | a set of new documents: their keys, their names, whether each would be valid, and what each link comes to | [`Shelf::plan_new`], once, in the steps of its module page: the keys asked for by [`Shelf::claim_key`], the keys chosen as the numbers after the highest any file writes (passing over the keys asked for and the keys written, as [`Shelf::next_key`] counts), the name of a link replaced by the key of the row (in the book the link points into), each row made a [`NewDoc`] by the one function [`Shelf::check_new`] calls, and every link resolved in the shelf's index with the run in it | a [`NewPlan`] (the documents in the order to write them, and its [`NewPlan::heads`], by the rule `order::heads` reads), or every [`PlanProblem`], each with its row |
//! | what to do when the file of the key a new document was to take is there at the write, and when to give up | [`Shelf::replan_after_taken`]: a key that was asked for is refused as it is; a row written that links to the key by name stops the run; otherwise only the row taken is given the number after the highest of the run (the keys of the other rows stand), with the names and the links made again, until a fixed number of keys were tried over the run | a [`PlanAfterTaken`]: the [`NewPlan`] again, a refusal, a [`NoFreeKey`], or a [`NextKeyProblem`] |
//! | whether a new document would be valid | [`Shelf::check_new`], which hands the would-be file `<book dir>/<key>.md` to the same reader and judgement [`Shelf::build`] uses and resolves its links in the same index (no second validator); [`Shelf::plan_new`] makes each row by the same function | a [`NewDoc`] with its unresolved links, or a [`NewDocProblem`] with every [`DocProblem`] |
//! | the links around a document, up and down | [`trace`](trace::trace) | a [`Trace`] of [`LinkEdge`]s |
//! | whether a document is linked at all, each way (a link reaches it, it holds one) | [`linkage`](trace::linkage); the rule is on [`Linkage`] | a [`Linkage`] per valid document |
//! | which links have which meaning | [`Doc::links_of`], from the [`Link`]s of the document (each carries its [`LinkKind`]) | the links of one [`Propagation`] |
//! | what a hash covers | `hash_doc` in `doc.rs`, private, called by `DocDraft::new` only | a [`DocHash`] (made by `DocHash::computed`, private to `doc.rs`) |
//! | a hash read from a record | `DocHash::restore` | called by [`Record::from_canon`] only |
//! | a pin | the constructor of [`FramePin`] in `ledger/pin.rs`, visible to the ledger's modules only: [`frame_approve`] takes it from a [`ResolvedLink`] as it is, [`Record::from_canon`] reads it back | a [`FramePin`] |
//! | cycles, in either graph | `graph::cycles_of` (and `graph::components` for the order upstream first), called by [`Shelf::build`] and by the `FrameGraph` that `Ledger::frame_graph` makes | [`Cycle`]s of [`CycleSource::Links`] or [`CycleSource::Frames`] |
//! | the graph of frames | `Ledger::frame_graph` (visible to the shelf's modules only) | a `FrameGraph` |
//! | who replaces whom (a fact of links) | [`Shelf::replacers_of`] | the candidates |
//! | who succeeds whom (a verdict) | [`Successors::of`] | [`Successors`] |
//! | the facts a decision stands on | [`frame_approve`], [`frame_present`], [`frame_retire`] | an [`ApproveFrame`], a [`PresentFrame`], a [`RetireFrame`], or a [`FrameRefusal`] |
//! | whether a new approval may stand on an upstream | [`Ledger::may_stand_on`](ledger::Ledger::may_stand_on), from what was decided ([`Ledger::declared`]), which the frame reads | nothing, or the [`NotStanding`](ledger::NotStanding) decided against it |
//! | which refusals of an approval's frame can be told before it is made | [`ApproveFrame::foresee`](ledger::ApproveFrame::foresee): every refusal of [`frame_approve`] but a cycle among the frames of approvals, by the same code | nothing, or the [`FrameRefusal`] |
//! | the invariants of a decision | `Decision::assemble`, visible to the ledger's modules only | a [`Decision`] or a [`DecisionProblem`] |
//! | reading a record back | [`Record::from_canon`] | a [`Record`] or a [`RecordProblem`] |
//! | a contradiction among records | [`Ledger::build`] | [`LedgerProblem`]s |
//! | what a rule of who may decide says of a name | [`Authority::permission`](schema::Authority::permission) | a [`Permission`](schema::Permission) |
//! | whether a record is admitted by who may decide, and the book it is read with | `Admit::admission` in `ledger/admit.rs`, asked by [`Ledger::build`] of every record and by [`Ledger::accept`](ledger::Ledger::accept) of every record to be written; the book is `Admit::book_of`, from the key `book` or [`Shelf::book_of`](build::Shelf::book_of) | used or a [`LedgerProblem::NotAdmitted`](ledger::LedgerProblem::NotAdmitted); an [`AdmittedRecord`](ledger::AdmittedRecord), the one thing a write takes, or a [`NotAdmitted`](ledger::NotAdmitted) |
//! | the order of the records, and the decision that stands | [`RecordOrder`](ledger::RecordOrder), by time and then by identity, for a decision and a move alike; [`Ledger::standing_decision`] is the latest in it | a [`Decision`] |
//! | what an approval is | the view of its record ([`ApprovalView`](ledger::ApprovalView)), the one reading of the verb of a decision for every reader of the ledger | an [`ApprovalView`](ledger::ApprovalView), or the [`NotStanding`](ledger::NotStanding) it is |
//! | what was decided | [`Ledger::declared`], and its owned copy [`Ledger::status_of`] | a [`DeclaredNow`](ledger::DeclaredNow) (the [`StandingApproval`](ledger::StandingApproval)), a [`DeclaredStatus`] |
//! | what a pinned content comes to now, by its kind (a document, an artifact) | [`drift`]: an edit is one unless a declaration joins it | an [`Edit`](drift::Edit), a [`DocDrift`](drift::DocDrift), an [`ArtifactChange`](drift::ArtifactChange) |
//! | what the pins of the approval that stands come to now | `StandingApproval::compared` (visible to the shelf's modules only), each pin once by the kind of its content | an `ApprovalPins`, told in causes by [`assess`] |
//! | what a person typed | [`resolve`] | a [`Resolution`] (an identity only the ledger knows has an [`Absence`], which cannot be invalid) |
//! | what a link that names a preset, or the default, declares | [`expand`](schema::expand), before [`Schema::validate`]; the presets of the adapters come as a value, as the spaces do | the declaration with the values put in, and the preset of each link for `map` alone |
//! | which spaces a declaration may name | [`Schema::validate`], given the spaces the adapters offer as a value; a link to artifacts that names none, or one not offered, is a [`SchemaProblem::NoSpace`] or a [`SchemaProblem::UnknownSpace`] | an [`ArtifactLinkKind`] with its [`Space`] |
//! | what a link's `cover` is, and which selections of a space to ask its adapter about | the one reader of a `cover` (a strength on a link into a book, a table of a strength and a selection on a link to artifacts), which [`Schema::validate`] and [`Schema::selection_asks`](schema::Schema::selection_asks) both read with; [`Schema::declared`](schema::Schema::declared) is given what the adapters answered, and refuses a selection a space does not read, a space that lists nothing, and one nobody asked about | an [`ArtifactCover`](schema::ArtifactCover), or a [`SchemaProblem`](schema::SchemaProblem) |
//! | which names a document's links to artifacts write (A1) | [`Shelf::artifact_asks`], before the build, per space, and [`Shelf::build`], which makes each name an [`Artifact`] that is there, absent or unknown (never unresolved); a text that is no [`ArtifactName`] is a [`DocProblem::ArtifactNameInvalid`], one its adapter says is no name of the space a [`DocProblem::NotAnArtifactName`] | the names, with their comparisons; an [`Artifact`] |
//! | whether a text is a name of its space, and what a name comes to now | the adapter of the space, through the port of the application, as an [`ArtifactReading`]: there, absent, cannot tell (why), or not a name (why) | an [`ArtifactReading`] |
//! | what of an artifact counts, and which hashes are compared | [`Comparison`](artifact::Comparison), declared per link and kept in each pin; [`Comparison::fingerprint`](artifact::Comparison::fingerprint) is the one maker of an artifact's hash; pins of another comparison than the link's are [`Cause::ArtifactComparisonChanged`](freshness::Cause::ArtifactComparisonChanged), never compared | an [`ArtifactHash`](artifact::ArtifactHash) |
//! | the form of a content address and of a revision (A7) | the adapter, then `from_validated` ([`ContentAddress::from_validated`], [`Revision::from_validated`]) checks only that it is not empty and has no space | an opaque value |
//! | a link to artifacts never replaces | [`Schema::validate`] ([`SchemaProblem::ArtifactReplaces`]); the type [`ArtifactPropagation`] has no `Replaces` | an [`ArtifactLinkKind`] |
//! | a pin of an artifact (A2) | an [`ArtifactPin`], built only in `ledger/frame.rs` and `ledger/canon.rs` (its fields are visible to the ledger's modules only): [`frame_approve`] pins every artifact of a `Depends` link as it is, there or not, by its space and name, and refuses only one its adapter cannot tell about now ([`FrameRefusal::ArtifactUnknown`]) | an [`ArtifactPin`] with a [`PinnedArtifact`] |
//! | which address goes on which item of an approval (A5) | [`ApproveFrame::addressed`], from the files of documents the frame knows ([`ApproveFrame::files`]) and what the adapters answered for its artifacts | the frame, with the addresses and the revision |
//! | which records keep addresses and a revision, and how a record without them reads (A5) | [`Record::to_canon`] / [`Record::from_canon`]: optional keys, left out when empty | a [`Record`] |
//! | how a record names an artifact, and how one written before spaces reads | [`Record::to_canon`] / [`Record::from_canon`]: `space` and `name`; a `path` with no `space` is a name in the space `file`, and the record is written again as it was stored | a [`Record`] |
//! | how strong a cause is | [`Cause::severity`] | a [`Severity`] |
//! | what a pinned artifact makes of an approval (A3), with hashes only (A4) | `artifact_causes` in `freshness.rs`, called by [`assess`], from what the pin comes to now (`StandingApproval::compared`) | [`Cause::ArtifactChanged`], [`Cause::ArtifactGone`], [`Cause::ArtifactArrived`], [`Cause::ArtifactLinksChanged`]; none for one that cannot be told now, which is a [`Notice::ArtifactUnknown`] |
//! | what an approval that stands saw | [`Snapshot::of`], from the [`StandingApproval`](ledger::StandingApproval) that [`Ledger::declared`] gives, its pins read by [`ApprovalView::read_pins`](ledger::ApprovalView::read_pins) | a [`Snapshot`] of [`SnapshotItem`]s |
//! | whether the content of an item came back (A6) | [`SnapshotItem::retrieval`] | a [`Retrieval`]: read, an address that cannot be read, or no address |
//! | whether an item of a snapshot changed, and its hash now | [`SnapshotItem::compared`], by hash, the way it was pinned, and no declaration asked | an [`ItemCompared`](snapshot::ItemCompared) |
//! | the level of a freshness | [`Degradation::level`], computed from the causes; [`Freshness::from_causes`] is the only maker of a [`Degradation`] | a [`Severity`] |
//! | which covering links expect which documents and names, which of them nobody reaches, which exemptions stand and read as needed, unneeded or cannot-tell, which are repeated, and how complete each listing of names is | [`coverage::assess`], once for an [`assess`], from the same shelf and ledger and the [`Listings`](artifact::Listings) the command asked for | a [`CoverAssessment`](coverage::CoverAssessment), which [`Assessment::cover`] returns |
//! | whether an exemption is needed | [`exemption::exemption_reading`], and [`exemption::frame_exempt`] for one about to be written | an [`ExemptionReading`](exemption::ExemptionReading): needed, unneeded (why) or cannot tell (a listing that is not the whole) |
//! | freshness, propagation, notices | [`assess`] | an [`Assessment`] |
//! | where one identity stands, listed or not | [`Assessment::standing_of`] | a [`Standing`] |
//! | where a degradation starts | [`Assessment::roots_of`] | [`RootCause`]s |
//! | where a cause starts and what happened there | [`Cause::located`](freshness::Cause::located), gathered by [`Assessment::origins`](freshness::Assessment::origins) | an [`Origin`](freshness::Origin) and an [`OriginEvent`](freshness::OriginEvent); per origin, the events and the approvals that hold a cause there |
//! | what can settle each origin | [`origins_asked`](settle::origins_asked), for the documents the settlements approve by name, then [`Foreseen::of`](order::Foreseen::of), for what the approving commands would approve (what `--stale` would approve again, [`order::stale_taken`], and whether the frame takes each approval by name), then [`settle_all`](settle::settle_all) | the [`Settlement`](settle::Settlement)s of each origin, read first, then declare, then approve |
//! | which documents a run of approvals takes: the expired or stale ones, or the pending ones | [`order::stale`], [`order::pending`] and [`order::pending_upstream`] (one document: [`order::pending_of`]), from an [`Assessment`] | the identities, with the broken ones told apart for `stale` |
//! | the order a set of approvals is judged in again | [`upstream_first`](order::upstream_first), from the links and the pins | the identities, upstream first |
//! | which documents that do not hold (and, asked, which pending ones) share nothing, so that each part can be mended and approved again apart | [`units::plan`], then [`units::divide`] with where the artifacts live, by the rules of [`units`] | [`Units`](units::Units): the units, each with its documents upstream first, its files, its artifacts and the upstreams it waits on, and the artifacts whose place cannot be told |
//! | how the work went: approvals made again, pending time, runs, the pins that changed | [`stats::of`], from every decision and the keys now; the measures are defined on its page | a [`Stats`](stats::Stats) |
//! | what a document stands on: the documents its links resolve to and its approval pinned, and the artifacts its links to artifacts name and its approval pinned (each pin as it is read now) | `Footing::of` in `footing.rs` (visible to the shelf's modules only) | a `Footing` |
//! | which documents depend on some named documents, and what of the shelf a walk down from them cannot follow | `walk` in [`reach`] (visible to the crate only), by `Doc::depends_names` | a `Walked`, with an [`Unwalked`](reach::Unwalked) of what it could not follow |
//! | what a reach of named documents stands on, and the files of the project it is in | `Reach::of` (the artifacts of each document reached, by its `Footing`) and `Reach::files` (every file a document may be in, `DocState::may_be_in`, and every file of the placement of its artifacts) in [`reach`] | a `Reach`, a `ReachFiles` with the artifacts that have no place and the documents that are gone apart |
//! | whether a change reaches each document, and whether those it reaches are inside the reach of named documents as the shelf stood before the change | `within` in [`within`] (visible to the crate only), from [`impact`](impact::impact) of the change and of the change read backwards, the ledger and the shelf of each end, and `walk` on the shelf before | a [`Within`](within::Within): a [`Verdict`](within::Verdict) per document, and whether all pass |
//! | where a name of a space lives in the files of the project | the adapter of the space, through the port of the application | a [`Placement`](placement::Placement): only these files, partly these files, or cannot say (why) |
//! | which names that cannot be told now share a cause, whether a cause holds back one (document, name) or many, and whether one document or many writes the names of such a cause | [`untold_causes`](untold::untold_causes), by the space and the [`Untold`](artifact::Untold) (`Eq`), once | [`UntoldCause`](untold::UntoldCause) with its [`Held`](untold::Held) and [`Writers`](untold::Writers) |
//!
//! Two of these cannot be enforced by visibility alone: `DocHash::restore` and
//! `ArtifactHash::restore` are visible to the whole module (the hash is made in
//! `doc.rs` or by an adapter, and read back in `ledger/canon.rs`), so the rule is a name:
//! `restore` only where a record is read back.
//!
//! # Artifacts
//!
//! A book may link to *artifacts* (`target = "artifact"`): things outside the
//! documents, such as code, tests, data, named in a *space* the link declares. The
//! model knows a space by its name only; the adapter of the space says whether a
//! text is one of its names and what each name comes to. An approval pins what each
//! name of a `Depends` link comes to, a hash or "not there", the way it pins an
//! upstream document, and freshness compares the same way, by hash only. Beside
//! each hash, a record may keep a [`ContentAddress`] where the content can be read
//! back later, and a [`Revision`], both opaque: the domain knows no version
//! control, and a project outside one has the same freshness. What an approval saw
//! is a [`Snapshot`], and whether its content came back is a [`Retrieval`]. The
//! values and their rules are on the page of [`artifact`].
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
//! - **A key is unique within its book.** Across books the same text can be a key,
//!   which is why a bare key can be [`Resolution::Ambiguous`].
//! - **An identity in an unreadable file.** When some file's `id:` cannot be read,
//!   an identity no readable file carries is [`DocState::Unknown`], not
//!   [`DocState::Gone`], and what stands on it is [`Cause::SelfNotValid`] or
//!   [`Cause::UpstreamNotValid`] of [`DocFailure::Unknown`]: the model does not
//!   claim a document is gone when it may be in a file it could not read. A
//!   snapshot keeps it apart too: the [`FailureTag`] of a document it cannot tell is
//!   `Unknown`, not `Gone`. That it is broken, and not a notice as a name of an
//!   artifact that cannot be told is ([`ArtifactState::Unknown`]), is because the
//!   files it may be in are the shelf's own and invalid.
//! - **Incremental reading is out of scope.** [`Shelf::build`] takes every file.
//! - **A key in a file that cannot be parsed is not seen.** A file whose front
//!   matter is no mapping shows no key, so [`Shelf::next_key`] and
//!   [`Shelf::claim_key`] cannot count it; the file `<key>.md` itself is never
//!   replaced, and a duplicate key that results is told by the build when the file
//!   is mended.
//!
//! [`Shelf::artifact_asks`]: build::Shelf::artifact_asks
//! [`Artifact`]: artifact::Artifact
//! [`ArtifactHash`]: artifact::ArtifactHash
//! [`ContentAddress`]: artifact::ContentAddress
//! [`ContentAddress::from_validated`]: artifact::ContentAddress::from_validated
//! [`Revision`]: artifact::Revision
//! [`Revision::from_validated`]: artifact::Revision::from_validated
//! [`DocProblem::ArtifactNameInvalid`]: build::DocProblem::ArtifactNameInvalid
//! [`DocProblem::NotAnArtifactName`]: build::DocProblem::NotAnArtifactName
//! [`SchemaProblem::NoSpace`]: schema::SchemaProblem::NoSpace
//! [`SchemaProblem::UnknownSpace`]: schema::SchemaProblem::UnknownSpace
//! [`Space`]: artifact::Space
//! [`ArtifactName`]: artifact::ArtifactName
//! [`ArtifactReading`]: artifact::ArtifactReading
//! [`FrameRefusal::ArtifactUnknown`]: ledger::FrameRefusal::ArtifactUnknown
//! [`Notice::ArtifactUnknown`]: freshness::Notice::ArtifactUnknown
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
//! [`SnapshotItem::compared`]: snapshot::SnapshotItem::compared
//! [`Ledger::declared`]: ledger::Ledger::declared
//! [`Retrieval`]: snapshot::Retrieval
//! [`ShelfMap::of`]: map::ShelfMap::of
//! [`ShelfMap`]: map::ShelfMap
//! [`Schema::validate`]: schema::Schema::validate
//! [`Schema`]: schema::Schema
//! [`KeyPattern::parse`]: schema::KeyPattern::parse
//! [`KeyForm`]: schema::KeyForm
//! [`KeyProblem`]: schema::KeyProblem
//! [`Propagation`]: schema::Propagation
//! [`Shelf::build`]: build::Shelf::build
//! [`Shelf::next_key`]: build::Shelf::next_key
//! [`Shelf::claim_key`]: build::Shelf::claim_key
//! [`Shelf::check_new`]: build::Shelf::check_new
//! [`Shelf::form_entries`]: build::Shelf::form_entries
//! [`NewKey`]: build::NewKey
//! [`Shelf::plan_new`]: build::Shelf::plan_new
//! [`Shelf::replan_after_taken`]: build::Shelf::replan_after_taken
//! [`NewPlan`]: build::NewPlan
//! [`NewPlan::heads`]: build::NewPlan::heads
//! [`PlanProblem`]: build::PlanProblem
//! [`PlanAfterTaken`]: build::PlanAfterTaken
//! [`NoFreeKey`]: build::NoFreeKey
//! [`NewDoc`]: build::NewDoc
//! [`NextKeyProblem`]: build::NextKeyProblem
//! [`ClaimKeyProblem`]: build::ClaimKeyProblem
//! [`NewDocProblem`]: build::NewDocProblem
//! [`Shelf::state_of`]: build::Shelf::state_of
//! [`Shelf::replacers_of`]: build::Shelf::replacers_of
//! [`DocReading::Unparsed`]: build::DocReading::Unparsed
//! [`DocReading::Unheld`]: build::DocReading::Unheld
//! [`DocProblem::Unreadable`]: build::DocProblem::Unreadable
//! [`DocProblem::Unheld`]: build::DocProblem::Unheld
//! [`DocProblem::FieldMissing`]: build::DocProblem::FieldMissing
//! [`DocProblem::FieldEmpty`]: build::DocProblem::FieldEmpty
//! [`DocProblem::IdEmpty`]: build::DocProblem::IdEmpty
//! [`DocProblem::KeyEmpty`]: build::DocProblem::KeyEmpty
//! [`DocProblem::ItemBlank`]: build::DocProblem::ItemBlank
//! [`DocProblem::NotOneLine`]: build::DocProblem::NotOneLine
//! [`NewDoc::entries`]: build::NewDoc::entries
//! [`UnheldValue`]: build::UnheldValue
//! [`DocProblem`]: build::DocProblem
//! [`RawProblem`]: build::RawProblem
//! [`InvalidDoc`]: build::InvalidDoc
//! [`DocState`]: build::DocState
//! [`DocState::valid_or_failure`]: build::DocState::valid_or_failure
//! [`DocState::presence`]: build::DocState::presence
//! [`DocFailure`]: build::DocFailure
//! [`DocFailure::Unknown`]: build::DocFailure::Unknown
//! [`FailureTag`]: build::FailureTag
//! [`ArtifactState::Unknown`]: artifact::ArtifactState::Unknown
//! [`DocPresence`]: build::DocPresence
//! [`Shelf::tally_by_book`]: build::Shelf::tally_by_book
//! [`BookTally`]: build::BookTally
//! [`Trace`]: trace::Trace
//! [`LinkEdge`]: trace::LinkEdge
//! [`Linkage`]: trace::Linkage
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
//! [`Assessment::cover`]: freshness::Assessment::cover
//! [`Assessment::roots_of`]: freshness::Assessment::roots_of
//! [`Assessment::standing_of`]: freshness::Assessment::standing_of
//! [`Standing`]: freshness::Standing
//! [`RootCause`]: freshness::RootCause

pub mod artifact;
pub mod build;
pub(crate) mod cover;
pub mod coverage;
pub mod doc;
pub mod drift;
pub mod exemption;
pub mod filter;
mod footing;
pub mod freshness;
pub mod graph;
pub mod impact;
pub mod ledger;
pub mod lookup;
pub mod map;
pub mod order;
pub mod placement;
pub mod reach;
pub mod rekey;
pub mod schema;
pub mod settle;
pub mod snapshot;
pub mod stats;
pub mod trace;
pub mod units;
pub mod untold;
pub mod within;

#[cfg(test)]
mod fixtures_tests;
#[cfg(test)]
mod now_tests;
