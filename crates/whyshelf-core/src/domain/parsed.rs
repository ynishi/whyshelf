//! A document as an adapter read it, before the references it holds are looked up.
//!
//! An adapter can parse the shape of a file; it cannot say what an alias names, because
//! that depends on the other files. So what it hands over for a document is a
//! [`ParsedDocument`]: the header, the entity with its references still written as
//! [`Ref`]s ([`ParsedSpec::grounds`], [`ParsedDesign::spec`]), and the [`TextHash`] of
//! the text it was read from. The [`Corpus`](super::corpus::Corpus) looks the
//! references up once it has every file ([`Index`], then
//! [`Resolver`]) and builds the [`Entity`] from the ids they
//! come to.
//!
//! ```text
//!   adapter        ParsedDocument { header, ParsedEntity (Refs), TextHash }
//!                         |
//!   Corpus::build         |  Index of every header  -->  Resolver
//!                         v
//!                  Document { header, Entity (ids) }  +  Written  +  TextHash
//! ```
//!
//! # What a reference comes to
//!
//! A key that holds a reference (`grounds`, `spec`) keeps what the lookup comes to,
//! and the corpus's build is the one place that says what. The line between
//! the rows is whether an identifier is available: with one the holder is built and
//! the evaluator judges it; without one there is nothing to hold and the file is
//! [`Reading::Unparsed`](super::corpus::Reading::Unparsed).
//!
//! | resolution | written as | the holder |
//! |---|---|---|
//! | one document, the kind the key wants | any | built |
//! | one document, another kind | any | not built: [`UnparsedReason::WrongKind`] |
//! | one id held by files of several kinds | any | built; Blocked by the upstream's state |
//! | `Unresolved` (`private:<id>` that names nothing here) | `private:<id>` | built; Partial ([`Written::unresolved`]) |
//! | `Dangling` | an id, with or without `private:` | built; Invalid ([`Written::dangling`]) |
//! | `Dangling` | an alias | not built: [`UnparsedReason::DanglingRef`] |
//! | `Ambiguous` | an alias | not built: [`UnparsedReason::AmbiguousRef`] |
//!
//! The mark does not reach identity: an entity holds the identifiers of its upstreams,
//! not the marks they were written with, because a mark is how a file was written and
//! changes with publication, so it stays out of every hash.

use std::collections::BTreeMap;

use super::corpus::{Document, Entity, Header};
use super::design::{Crux, Design, WorkItem};
use super::error::{DocumentError, EntityError};
use super::event::DocKind;
use super::finding::Finding;
use super::hash::TextHash;
use super::id::{DesignId, DocId, EntityId, FindingId, ItemNumber, SpecId};
use super::naming::{Index, Named, Ref, Resolution, Resolver, Written};
use super::nonempty::{NonBlank, NonEmptyVec};
use super::pin::{HypId, Hypothesis, PinContract, PinId};
use super::reference;
use super::spec::Spec;
use super::state::UnparsedReason;

/// A Spec as the file wrote it: its grounds are references, not yet identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpec {
    id: SpecId,
    grounds: NonEmptyVec<Ref>,
    contract: NonBlank,
    pins: BTreeMap<PinId, PinContract>,
    hyps: BTreeMap<HypId, Hypothesis>,
}

impl ParsedSpec {
    /// A Spec from the parts of its file.
    pub fn new(
        id: SpecId,
        grounds: NonEmptyVec<Ref>,
        contract: NonBlank,
        pins: BTreeMap<PinId, PinContract>,
        hyps: BTreeMap<HypId, Hypothesis>,
    ) -> Self {
        Self {
            id,
            grounds,
            contract,
            pins,
            hyps,
        }
    }

    /// The identifier.
    pub fn id(&self) -> SpecId {
        self.id
    }

    /// The grounds as written.
    pub fn grounds(&self) -> &NonEmptyVec<Ref> {
        &self.grounds
    }
}

/// A Design as the file wrote it: its Spec is a reference, not yet an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDesign {
    id: DesignId,
    spec: Ref,
    crux: Crux,
    items: BTreeMap<ItemNumber, WorkItem>,
}

impl ParsedDesign {
    /// A Design from the parts of its file.
    pub fn new(id: DesignId, spec: Ref, crux: Crux, items: BTreeMap<ItemNumber, WorkItem>) -> Self {
        Self {
            id,
            spec,
            crux,
            items,
        }
    }

    /// The identifier.
    pub fn id(&self) -> DesignId {
        self.id
    }

    /// The Spec as written.
    pub fn spec(&self) -> &Ref {
        &self.spec
    }

    /// The items by number.
    pub fn items(&self) -> &BTreeMap<ItemNumber, WorkItem> {
        &self.items
    }
}

/// The entity a document file holds, with its references still to be looked up.
///
/// A Finding holds none, so it is already the entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedEntity {
    /// A Finding.
    Finding(Finding),
    /// A Spec, with its grounds as written.
    Spec(ParsedSpec),
    /// A Design, with its Spec as written.
    Design(ParsedDesign),
}

impl ParsedEntity {
    /// The kind of document this is.
    pub fn kind(&self) -> DocKind {
        match self {
            Self::Finding(_) => DocKind::Finding,
            Self::Spec(_) => DocKind::Spec,
            Self::Design(_) => DocKind::Design,
        }
    }

    /// The identity the entity carries, as a document identity.
    pub fn id(&self) -> DocId {
        let entity: EntityId = match self {
            Self::Finding(finding) => finding.id().into(),
            Self::Spec(spec) => spec.id().into(),
            Self::Design(design) => design.id().into(),
        };
        DocId::new(entity)
    }
}

/// A file read as a document, before its references are looked up: its header, the
/// entity it holds with the references as written, and the hash of its text.
///
/// The header and the entity agree on the id and on the kind; that is checked once,
/// here. The [`TextHash`] is the hash of the normalized text the file was read from:
/// it is made once, where the text and the reading are put together
/// ([`RawDocument::new`](crate::app::raw::RawDocument::new)), so what a gate is told
/// it read and the key of the stored version are the same value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    header: Header,
    // Boxed so that a reading that is not a document stays small.
    entity: Box<ParsedEntity>,
    text: TextHash,
}

impl ParsedDocument {
    /// A parsed document; refused when the header and the entity disagree on the id
    /// or the kind.
    pub fn new(
        header: Header,
        entity: ParsedEntity,
        text: TextHash,
    ) -> Result<Self, DocumentError> {
        if header.kind() != entity.kind() {
            return Err(DocumentError::KindMismatch {
                header: header.kind(),
                entity: entity.kind(),
            });
        }
        if header.id() != entity.id() {
            return Err(DocumentError::IdMismatch {
                header: header.id(),
                entity: entity.id(),
            });
        }
        Ok(Self {
            header,
            entity: Box::new(entity),
            text,
        })
    }

    /// A document whose references are identifiers already, written as such: each
    /// ground and the Spec become a [`Ref::Id`].
    #[cfg(test)]
    pub(crate) fn from_document(document: Document, text: TextHash) -> Self {
        let entity = match document.entity() {
            Entity::Finding(finding) => ParsedEntity::Finding(finding.clone()),
            Entity::Spec(spec) => {
                let mut grounds = spec
                    .grounds()
                    .iter()
                    .map(|id| Ref::Id(DocId::new(id.entity())));
                let head = grounds.next();
                ParsedEntity::Spec(ParsedSpec::new(
                    spec.id(),
                    NonEmptyVec::new(
                        head.unwrap_or_else(|| Ref::Id(DocId::new(spec.id().entity()))),
                        grounds.collect(),
                    ),
                    spec.contract().clone(),
                    spec.pins().clone(),
                    spec.hyps().clone(),
                ))
            }
            Entity::Design(design) => ParsedEntity::Design(ParsedDesign::new(
                design.id(),
                Ref::Id(DocId::new(design.spec().entity())),
                design.crux().clone(),
                design.items().clone(),
            )),
        };
        Self {
            header: document.header().clone(),
            entity: Box::new(entity),
            text,
        }
    }

    /// The same document with each reference behind a `private:` mark.
    #[cfg(test)]
    pub(crate) fn with_private_marks(self) -> Self {
        self.marking(|_| true)
    }

    /// The same document with the references to these documents behind a `private:`
    /// mark, as a file writes the ones that are private.
    #[cfg(test)]
    pub(crate) fn with_private_marks_of(self, private: &std::collections::BTreeSet<DocId>) -> Self {
        self.marking(|reference| matches!(reference, Ref::Id(id) if private.contains(id)))
    }

    #[cfg(test)]
    fn marking(mut self, which: impl Fn(&Ref) -> bool) -> Self {
        let marked = |reference: &Ref| {
            if which(reference) {
                Ref::Private(Box::new(reference.clone()))
            } else {
                reference.clone()
            }
        };
        match &mut *self.entity {
            ParsedEntity::Finding(_) => {}
            ParsedEntity::Spec(spec) => {
                let grounds: Vec<Ref> = spec.grounds.iter().map(marked).collect();
                if let Ok(grounds) = NonEmptyVec::from_vec(grounds) {
                    spec.grounds = grounds;
                }
            }
            ParsedEntity::Design(design) => design.spec = marked(&design.spec),
        }
        self
    }

    /// The classification keys of the file.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The entity, with its references as written.
    pub fn entity(&self) -> &ParsedEntity {
        &self.entity
    }

    /// The identity.
    pub fn id(&self) -> DocId {
        self.header.id()
    }

    /// The kind.
    pub fn kind(&self) -> DocKind {
        self.header.kind()
    }

    /// The hash of the normalized text the file was read from.
    pub fn text(&self) -> TextHash {
        self.text
    }
}

/// A document whose references were looked up.
pub(super) struct Settled {
    pub(super) document: Document,
    pub(super) text: TextHash,
    pub(super) written: Written,
}

/// How a reference in a key came to hold an identifier.
enum Held {
    /// The document the reference names.
    Named(EntityId),
    /// The identifier is spelled and nothing visible holds it.
    Dangling(EntityId),
    /// The identifier is spelled behind a `private:` mark and nothing here holds it.
    Unresolved(EntityId),
}

fn invalid(field: &str, detail: impl Into<String>) -> UnparsedReason {
    UnparsedReason::InvalidField {
        field: field.to_owned(),
        detail: detail.into(),
    }
}

/// The identifier a reference spells: a plain id, or the id behind a `private:` mark.
fn spelled(reference: &Ref) -> Option<EntityId> {
    match reference {
        Ref::Id(id) => Some(id.entity()),
        Ref::Private(inner) => match inner.as_ref() {
            Ref::Id(id) => Some(id.entity()),
            _ => None,
        },
        _ => None,
    }
}

/// A reason that quotes the reference; the reasons keep the reference in the form
/// the earlier model wrote, and a form that has none is told in words.
fn quoting(
    reference: &Ref,
    field: &str,
    make: impl FnOnce(reference::Ref) -> UnparsedReason,
) -> UnparsedReason {
    match reference.to_string().parse::<reference::Ref>() {
        Ok(quoted) => make(quoted),
        Err(_) => invalid(field, format!("`{reference}` cannot be held here")),
    }
}

/// What the lookup of a reference in the key `field` comes to, for a holder that
/// wants a document of `expected`.
fn hold(
    resolver: &Resolver<'_>,
    index: &Index,
    reference: &Ref,
    field: &str,
    expected: DocKind,
) -> Result<Held, UnparsedReason> {
    let dangling = || {
        quoting(reference, field, |reference| UnparsedReason::DanglingRef {
            reference,
        })
    };
    match resolver.resolve(reference) {
        Resolution::Resolved(Named::Doc(doc)) => {
            let mut kinds = index.kinds(&doc).into_iter();
            match (kinds.next(), kinds.next()) {
                (Some(found), None) if found != expected => {
                    Err(quoting(reference, field, |reference| {
                        UnparsedReason::WrongKind {
                            reference,
                            expected: expected.entity_kind(),
                            found: found.entity_kind(),
                        }
                    }))
                }
                // The kind wanted, or files of one id that disagree on theirs: the
                // holder is built, and the document in conflict is Invalid.
                _ => Ok(Held::Named(doc.entity())),
            }
        }
        // A key refuses a reference to an item, so this does not arise from a file.
        Resolution::Resolved(Named::Item(_)) => Err(dangling()),
        Resolution::Ambiguous(_) => Err(quoting(reference, field, |reference| {
            UnparsedReason::AmbiguousRef { reference }
        })),
        Resolution::Dangling { .. } => spelled(reference).map(Held::Dangling).ok_or_else(dangling),
        Resolution::Unresolved(_) => spelled(reference)
            .map(Held::Unresolved)
            .ok_or_else(dangling),
    }
}

impl ParsedDocument {
    /// Look the references up and build the document, or say why it cannot be built.
    ///
    /// `resolver` is the one that sees what a file of this document's root sees.
    pub(super) fn settle(
        self,
        resolver: &Resolver<'_>,
        index: &Index,
    ) -> Result<Settled, NonEmptyVec<UnparsedReason>> {
        let one = |reason| NonEmptyVec::new(reason, Vec::new());
        let Self {
            header,
            entity,
            text,
        } = self;
        let mut written = Written::default();
        let entity = match *entity {
            ParsedEntity::Finding(finding) => Entity::Finding(finding),
            ParsedEntity::Spec(spec) => {
                let ParsedSpec {
                    id,
                    grounds,
                    contract,
                    pins,
                    hyps,
                } = spec;
                let mut held = Vec::new();
                let mut reasons = Vec::new();
                for reference in grounds.iter() {
                    // An identifier is available in each case but the last: the Spec
                    // is built with it, and the evaluation judges what stands behind
                    // it. No ground is dropped, which would change the hash in silence.
                    match hold(resolver, index, reference, "grounds", DocKind::Finding) {
                        Ok(Held::Named(found)) => held.push(FindingId::new(found)),
                        Ok(Held::Dangling(found)) => {
                            written.dangling.insert(found);
                            held.push(FindingId::new(found));
                        }
                        Ok(Held::Unresolved(found)) => {
                            written.unresolved.insert(found);
                            held.push(FindingId::new(found));
                        }
                        Err(reason) => reasons.push(reason),
                    }
                }
                if let Ok(reasons) = NonEmptyVec::from_vec(reasons) {
                    return Err(reasons);
                }
                let grounds =
                    NonEmptyVec::from_vec(held).map_err(|_| one(UnparsedReason::EmptyGrounds))?;
                let spec: Spec =
                    Spec::new(id, grounds, contract, pins, hyps).map_err(|error| match error {
                        EntityError::DuplicateGround { finding } => {
                            one(UnparsedReason::DuplicateGround { finding })
                        }
                        other => one(invalid("grounds", other.to_string())),
                    })?;
                Entity::Spec(spec)
            }
            ParsedEntity::Design(design) => {
                let ParsedDesign {
                    id,
                    spec,
                    crux,
                    items,
                } = design;
                let upstream =
                    match hold(resolver, index, &spec, "spec", DocKind::Spec).map_err(one)? {
                        Held::Named(found) => found,
                        Held::Dangling(found) => {
                            written.dangling.insert(found);
                            found
                        }
                        Held::Unresolved(found) => {
                            written.unresolved.insert(found);
                            found
                        }
                    };
                let design: Design = Design::new(id, SpecId::new(upstream), crux, items);
                Entity::Design(design)
            }
        };
        let document = Document::new(header, entity)
            .map_err(|error| one(invalid(error.field(), error.to_string())))?;
        Ok(Settled {
            document,
            text,
            written,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::corpus::{Corpus, FileFacts, GitPlace, Location, Reading, Visibility};
    use crate::domain::design::test_support::{design, identified};
    use crate::domain::finding::test_support::finding;
    use crate::domain::hash::BytesHash;
    use crate::domain::id::test_support::{docid, sid};
    use crate::domain::knowable::Knowable;
    use crate::domain::naming::Place;
    use crate::domain::spec::test_support::spec;

    fn id_text(n: u128) -> String {
        docid(n).to_string()
    }

    fn facts(visibility: Visibility, path: &str) -> FileFacts {
        let git = match visibility {
            Visibility::Public => GitPlace::Tracked,
            Visibility::Private => GitPlace::Ignored,
        };
        FileFacts::new(
            Location::new(visibility, path.parse().unwrap(), Knowable::Known(git)),
            BytesHash::of(path.as_bytes()),
        )
    }

    fn header(n: u128, kind: DocKind, alias: Option<&str>) -> Header {
        Header::new(
            docid(n),
            kind,
            alias.map(|alias| alias.parse().unwrap()),
            BTreeSet::new(),
            None,
        )
    }

    fn parsed(header: Header, entity: ParsedEntity, path: &str) -> Reading {
        Reading::Document(ParsedDocument::new(header, entity, TextHash::of(path)).unwrap())
    }

    fn a_finding(
        n: u128,
        alias: Option<&str>,
        visibility: Visibility,
        path: &str,
    ) -> (FileFacts, Reading) {
        let entity = ParsedEntity::Finding(finding(n, "body"));
        (
            facts(visibility, path),
            parsed(header(n, DocKind::Finding, alias), entity, path),
        )
    }

    /// A Spec whose grounds are written as `grounds`, read for the place of a key.
    fn a_spec(
        n: u128,
        grounds: &[&str],
        visibility: Visibility,
        path: &str,
    ) -> (FileFacts, Reading) {
        let base = spec(n, &[900], &["PIN-A"]);
        let grounds: Vec<Ref> = grounds
            .iter()
            .map(|text| Ref::parse(text, Place::Key).unwrap())
            .collect();
        let entity = ParsedEntity::Spec(ParsedSpec::new(
            sid(n),
            NonEmptyVec::from_vec(grounds).unwrap(),
            base.contract().clone(),
            base.pins().clone(),
            base.hyps().clone(),
        ));
        (
            facts(visibility, path),
            parsed(header(n, DocKind::Spec, None), entity, path),
        )
    }

    fn a_design(n: u128, spec_text: &str, path: &str) -> (FileFacts, Reading) {
        let base = design(n, 900, vec![(1, identified(31, "item", &[]))]);
        let entity = ParsedEntity::Design(ParsedDesign::new(
            base.id(),
            Ref::parse(spec_text, Place::Key).unwrap(),
            base.crux().clone(),
            base.items().clone(),
        ));
        (
            facts(Visibility::Public, path),
            parsed(header(n, DocKind::Design, None), entity, path),
        )
    }

    fn build(files: Vec<(FileFacts, Reading)>) -> Corpus {
        Corpus::build(files, &BTreeSet::new())
    }

    fn grounds_of(corpus: &Corpus, n: u128) -> Vec<DocId> {
        let file = corpus.by_id(&docid(n)).first().expect("the Spec is here");
        match file.document().entity() {
            Entity::Spec(spec) => spec
                .grounds()
                .iter()
                .map(|ground| DocId::new(ground.entity()))
                .collect(),
            other => panic!("not a Spec: {other:?}"),
        }
    }

    fn reasons_of(corpus: &Corpus, n: u128) -> Vec<UnparsedReason> {
        let file = corpus
            .unparsed()
            .iter()
            .find(|file| file.id() == Some(docid(n)))
            .expect("the file is listed as unreadable");
        file.reasons().iter().cloned().collect()
    }

    #[test]
    fn a_ground_is_held_as_the_id_it_resolves_to_by_alias_or_by_id() {
        let corpus = build(vec![
            a_finding(1, Some("finding/slow"), Visibility::Public, "f.md"),
            a_spec(2, &["finding/slow"], Visibility::Public, "s2.md"),
            a_spec(3, &[&id_text(1)], Visibility::Public, "s3.md"),
        ]);
        assert_eq!(grounds_of(&corpus, 2), [docid(1)]);
        assert_eq!(grounds_of(&corpus, 3), [docid(1)]);
        assert!(corpus.written().is_empty());
    }

    #[test]
    fn the_hash_of_the_text_reaches_the_document_unchanged() {
        let corpus = build(vec![a_finding(1, None, Visibility::Public, "f.md")]);
        let file = corpus.by_id(&docid(1)).first().unwrap();
        assert_eq!(file.text(), TextHash::of("f.md"));
        // It is not the hash of the bytes, which is another thing of another type.
        assert_eq!(file.facts().bytes(), BytesHash::of(b"f.md"));
    }

    #[test]
    fn an_id_that_nothing_holds_is_built_and_written_as_dangling() {
        let corpus = build(vec![a_spec(2, &[&id_text(9)], Visibility::Public, "s.md")]);
        assert_eq!(grounds_of(&corpus, 2), [docid(9)]);
        let written = &corpus.written()[&docid(2)];
        assert_eq!(written.dangling, BTreeSet::from([docid(9).entity()]));
        assert!(written.unresolved.is_empty());
    }

    #[test]
    fn an_alias_that_names_nothing_cannot_be_built() {
        let corpus = build(vec![a_spec(2, &["no-such"], Visibility::Public, "s.md")]);
        assert!(corpus.by_id(&docid(2)).is_empty());
        assert!(matches!(
            reasons_of(&corpus, 2).as_slice(),
            [UnparsedReason::DanglingRef { .. }]
        ));
    }

    #[test]
    fn an_alias_of_two_documents_cannot_be_built() {
        let corpus = build(vec![
            a_finding(1, Some("same"), Visibility::Public, "a.md"),
            a_finding(2, Some("same"), Visibility::Public, "b.md"),
            a_spec(3, &["same"], Visibility::Public, "s.md"),
        ]);
        assert!(matches!(
            reasons_of(&corpus, 3).as_slice(),
            [UnparsedReason::AmbiguousRef { .. }]
        ));
    }

    #[test]
    fn a_ground_of_another_kind_cannot_be_built() {
        let corpus = build(vec![
            a_spec(2, &[&id_text(900)], Visibility::Public, "s2.md"),
            a_spec(3, &[&id_text(2)], Visibility::Public, "s3.md"),
        ]);
        // Spec 2 is grounded on a Finding that is not here, and is built. Spec 3 names
        // Spec 2 where a Finding is wanted.
        assert!(matches!(
            reasons_of(&corpus, 3).as_slice(),
            [UnparsedReason::WrongKind { .. }]
        ));
    }

    #[test]
    fn a_private_mark_that_names_nothing_is_unresolved_and_one_that_names_something_is_found() {
        let marked = format!("private:{}", id_text(7));
        let elsewhere = build(vec![a_spec(2, &[&marked], Visibility::Public, "s.md")]);
        assert_eq!(grounds_of(&elsewhere, 2), [docid(7)]);
        let written = &elsewhere.written()[&docid(2)];
        assert_eq!(written.unresolved, BTreeSet::from([docid(7).entity()]));
        assert!(
            written.dangling.is_empty(),
            "never dangling: it may be someone else's"
        );

        let here = build(vec![
            a_finding(7, None, Visibility::Private, "p.md"),
            a_spec(2, &[&marked], Visibility::Public, "s.md"),
        ]);
        assert_eq!(grounds_of(&here, 2), [docid(7)]);
        assert!(here.written().is_empty());
    }

    #[test]
    fn a_public_file_does_not_see_a_private_document_by_alias_or_id_and_a_private_file_sees_both() {
        let corpus = build(vec![
            a_finding(1, Some("hidden"), Visibility::Private, "p.md"),
            a_finding(2, Some("shown"), Visibility::Public, "f.md"),
            a_spec(3, &["hidden"], Visibility::Public, "leak.md"),
            a_spec(5, &[&id_text(1)], Visibility::Public, "by-id.md"),
            a_spec(4, &["hidden", "shown"], Visibility::Private, "s.md"),
        ]);
        assert!(matches!(
            reasons_of(&corpus, 3).as_slice(),
            [UnparsedReason::DanglingRef { .. }]
        ));
        // By id it is built and dangling, as a document that stands on what it cannot see.
        assert_eq!(
            corpus.written()[&docid(5)].dangling,
            BTreeSet::from([docid(1).entity()])
        );
        assert_eq!(grounds_of(&corpus, 4), [docid(1), docid(2)]);
    }

    #[test]
    fn a_design_holds_the_id_of_its_spec_by_the_same_rules() {
        let corpus = build(vec![
            a_finding(1, None, Visibility::Public, "f.md"),
            a_spec(2, &[&id_text(1)], Visibility::Public, "s.md"),
            a_design(4, &id_text(2), "d.md"),
            a_design(5, &id_text(9), "gone.md"),
            a_design(6, &id_text(1), "kind.md"),
        ]);
        let spec_of = |n: u128| match corpus.by_id(&docid(n)).first().unwrap().document().entity() {
            Entity::Design(design) => DocId::new(design.spec().entity()),
            other => panic!("not a Design: {other:?}"),
        };
        assert_eq!(spec_of(4), docid(2));
        assert_eq!(spec_of(5), docid(9));
        assert_eq!(
            corpus.written()[&docid(5)].dangling,
            BTreeSet::from([docid(9).entity()])
        );
        assert!(matches!(
            reasons_of(&corpus, 6).as_slice(),
            [UnparsedReason::WrongKind { .. }]
        ));
    }

    #[test]
    fn files_of_one_id_that_disagree_on_the_kind_still_hold_the_id() {
        let corpus = build(vec![
            a_finding(1, None, Visibility::Public, "f.md"),
            a_spec(1, &[&id_text(9)], Visibility::Public, "s.md"),
            a_spec(2, &[&id_text(1)], Visibility::Public, "mine.md"),
        ]);
        // The id is in conflict; the Spec is built holding it and is judged by the
        // state of the upstream, which is Invalid.
        assert_eq!(grounds_of(&corpus, 2), [docid(1)]);
    }

    #[test]
    fn a_file_that_could_not_be_read_but_shows_its_id_is_a_name_that_something_holds() {
        let unreadable = (
            facts(Visibility::Public, "broken.md"),
            Reading::Unparsed {
                id: Some(docid(1)),
                reasons: NonEmptyVec::new(UnparsedReason::MergeConflictMarker, Vec::new()),
            },
        );
        let corpus = build(vec![
            unreadable,
            a_spec(2, &[&id_text(1)], Visibility::Public, "s.md"),
        ]);
        assert_eq!(grounds_of(&corpus, 2), [docid(1)]);
        assert!(
            corpus.written().is_empty(),
            "not dangling: the file is there"
        );
    }

    #[test]
    fn two_grounds_that_name_one_finding_are_a_duplicate() {
        let corpus = build(vec![
            a_finding(1, Some("finding/slow"), Visibility::Public, "f.md"),
            a_spec(
                2,
                &["finding/slow", &id_text(1)],
                Visibility::Public,
                "s.md",
            ),
        ]);
        assert!(matches!(
            reasons_of(&corpus, 2).as_slice(),
            [UnparsedReason::DuplicateGround { .. }]
        ));
    }

    #[test]
    fn a_header_and_an_entity_that_disagree_are_not_a_parsed_document() {
        let entity = ParsedEntity::Finding(finding(1, "body"));
        assert!(matches!(
            ParsedDocument::new(
                header(2, DocKind::Finding, None),
                entity.clone(),
                TextHash::of("x")
            ),
            Err(DocumentError::IdMismatch { .. })
        ));
        assert!(matches!(
            ParsedDocument::new(header(1, DocKind::Spec, None), entity, TextHash::of("x")),
            Err(DocumentError::KindMismatch { .. })
        ));
    }

    #[test]
    fn the_index_of_a_corpus_finds_what_the_build_held() {
        let corpus = build(vec![
            a_finding(1, Some("finding/slow"), Visibility::Public, "f.md"),
            a_spec(2, &["no-such"], Visibility::Public, "s.md"),
        ]);
        let resolver = Resolver::new(corpus.index(), Knowable::Unknown, Knowable::Unknown);
        let by_alias = Ref::parse("finding/slow", Place::Argument).unwrap();
        assert_eq!(
            resolver.resolve(&by_alias),
            Resolution::Resolved(Named::Doc(docid(1)))
        );
    }
}
