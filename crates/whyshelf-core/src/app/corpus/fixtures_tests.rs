//! A small project in the fake ports, which the tests of the use cases of the corpus
//! share.
//!
//! ```text
//!   finding 101 --- spec 111 (pin PIN-A) --- design 121 (item 131: PIN-A)
//! ```
//!
//! The numbers start at 100 so that the identifiers the fake mints, which count from 1,
//! never meet a document's.

use std::collections::BTreeSet;

use crate::app::config::Config;
use crate::app::corpus::{Approve, Approved, GateCommand, approve};
use crate::app::fake::{FakeCorpus, Reply, block_on, test_config};
use crate::app::outcome::Outcome;
use crate::app::ports::{Confirmed, CorpusRead};
use crate::app::raw::{NormalizedText, RawDocument, ReadFile};
use crate::domain::corpus::{Document, Entity, FileFacts, GitPlace, Header, Location, Visibility};
use crate::domain::design::test_support::{design, identified, num};
use crate::domain::event::DocKind;
use crate::domain::finding::test_support::finding;
use crate::domain::hash::{BytesHash, TextHash};
use crate::domain::id::test_support::docid;
use crate::domain::knowable::Knowable;
use crate::domain::location::RelPath;
use crate::domain::naming::Ref;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::parsed::ParsedDocument;
use crate::domain::revision::RevisionId;
use crate::domain::spec::test_support::spec;
use crate::domain::state::UnparsedReason;

pub(crate) const FINDING: u128 = 101;
pub(crate) const SPEC: u128 = 111;
pub(crate) const DESIGN: u128 = 121;
pub(crate) const ITEM: u128 = 131;

pub(crate) fn rel(path: &str) -> RelPath {
    path.parse().expect("a path")
}

/// The reference to a document by its id.
pub(crate) fn by_id(n: u128) -> Ref {
    Ref::Id(docid(n))
}

/// The reference to item 1 of the Design.
pub(crate) fn item_one() -> Ref {
    Ref::Item(Box::new(by_id(DESIGN)), num(1))
}

pub(crate) fn revision() -> RevisionId {
    RevisionId::from_validated("rev-1".to_owned()).expect("a revision")
}

/// The text a document has when nothing else is said: what it says, so a document that
/// says something else has another text.
pub(crate) fn text_of(entity: &Entity) -> String {
    format!("{entity:?}\n")
}

/// The reading of a document and the text it was read from, as the adapter gives them.
pub(crate) fn raw_document(entity: Entity, text: &str) -> RawDocument {
    let header = Header::new(entity.id(), entity.kind(), None, BTreeSet::new(), None);
    let document = Document::new(header.clone(), entity).expect("a document");
    let parsed = ParsedDocument::from_document(document, TextHash::of(text));
    RawDocument::new(
        ReadFile::Document {
            header,
            entity: Box::new(parsed.entity().clone()),
        },
        NormalizedText::new(text.to_owned()),
    )
}

/// Put a file in the fake: listed with its reading, and on the disk with its bytes.
pub(crate) fn place(fake: &FakeCorpus, root: Visibility, path: &str, raw: RawDocument) {
    let at = rel(path);
    let bytes = raw.text().as_str().as_bytes().to_vec();
    let git = match root {
        Visibility::Public => GitPlace::Tracked,
        Visibility::Private => GitPlace::Ignored,
    };
    fake.set_place(root, &at, git);
    fake.add_document(
        FileFacts::new(
            Location::new(root, at.clone(), Knowable::Unknown),
            BytesHash::of(&bytes),
        ),
        raw,
    );
    fake.put_file(root, &at, &bytes);
}

/// What a file edited to say `entity`, with `text`, does to the fake: the old file is
/// gone and the new one is in its place. For a hook, which runs when the world changes.
pub(crate) fn edited(
    root: Visibility,
    path: &'static str,
    entity: Entity,
    text: &'static str,
) -> impl FnOnce(&FakeCorpus) + Send + 'static {
    move |fake| {
        fake.remove_document(root, &rel(path));
        place(fake, root, path, raw_document(entity, text));
    }
}

/// The fake ports, with a project in them and the person who is acting.
pub(crate) struct World {
    pub(crate) fake: FakeCorpus,
    pub(crate) config: Config,
}

impl World {
    /// Empty ports. The person is `ada`, a human, who answers yes when asked; the tests
    /// carry the pin `PIN-A`.
    pub(crate) fn new() -> Self {
        let fake = FakeCorpus::new();
        fake.set_reply(Reply::Always(Confirmed::Yes));
        fake.scanner().set_pins(&["PIN-A"]);
        Self {
            fake,
            config: test_config(),
        }
    }

    /// The project, all in one root.
    pub(crate) fn project(root: Visibility) -> Self {
        let world = Self::new();
        world.add(
            root,
            "finding.md",
            Entity::Finding(finding(FINDING, "body")),
        );
        world.add(
            root,
            "spec.md",
            Entity::Spec(spec(SPEC, &[FINDING], &["PIN-A"])),
        );
        world.add(
            root,
            "design.md",
            Entity::Design(design(
                DESIGN,
                SPEC,
                vec![(1, identified(ITEM, "the item", &["PIN-A"]))],
            )),
        );
        world
    }

    /// Put a document in a root, with the text it has when nothing else is said.
    pub(crate) fn add(&self, root: Visibility, path: &str, entity: Entity) {
        let text = text_of(&entity);
        self.add_text(root, path, entity, &text);
    }

    /// The same, with the text given: one entity and two texts are two versions.
    pub(crate) fn add_text(&self, root: Visibility, path: &str, entity: Entity, text: &str) {
        place(&self.fake, root, path, raw_document(entity, text));
    }

    /// A file with no `id:`, of the kind it shows.
    pub(crate) fn unidentified(&self, root: Visibility, path: &str, text: &str) {
        let raw = RawDocument::new(
            ReadFile::Unidentified {
                kind: Some(DocKind::Finding),
            },
            NormalizedText::new(text.to_owned()),
        );
        place(&self.fake, root, path, raw);
    }

    /// A file that cannot be read as a document, though it shows the `id:` of document `n`.
    pub(crate) fn broken(&self, root: Visibility, path: &str, n: u128) {
        let reason = UnparsedReason::InvalidField {
            field: "kind".to_owned(),
            detail: "a merge-conflict marker".to_owned(),
        };
        let raw = RawDocument::new(
            ReadFile::Unparsed {
                id: Some(docid(n)),
                reasons: NonEmptyVec::new(reason, Vec::new()),
            },
            NormalizedText::new("<<<<<<<\n".to_owned()),
        );
        place(&self.fake, root, path, raw);
    }

    /// The document at this place is gone.
    pub(crate) fn remove(&self, root: Visibility, path: &str) {
        self.fake.remove_document(root, &rel(path));
    }

    /// Run a gate's approval, which must come to `Done`.
    pub(crate) fn approved(&self, gate: GateCommand, target: Ref) -> Approved {
        match block_on(approve(&self.fake, &self.config, Approve { target, gate })) {
            Ok(Outcome::Done(approved)) => approved,
            other => panic!("the approval did not come to done: {other:?}"),
        }
    }

    /// CR on the Finding, CS on the Spec, CI on the Design: the state CA starts from.
    pub(crate) fn approved_to_ci(&self) {
        self.approved(GateCommand::Cr, by_id(FINDING));
        self.approved(GateCommand::Cs, by_id(SPEC));
        self.approved(GateCommand::Ci, by_id(DESIGN));
    }

    pub(crate) fn ca() -> GateCommand {
        GateCommand::Ca {
            revision: revision(),
        }
    }

    /// How many records the ledger of a side holds.
    pub(crate) fn records(&self, side: Visibility) -> usize {
        self.fake.records(side).len()
    }
}
