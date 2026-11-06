//! Values the tests of the evidence chain, the confirmation and the fakes share.

use std::collections::BTreeSet;

use crate::app::raw::NormalizedText;
use crate::domain::adoption::Adoptable;
use crate::domain::approval::{Attestation, Gate, Pinned};
use crate::domain::boundary::{ObjectText, PrivateRoot};
use crate::domain::corpus::{
    Corpus, Document, Entity, FileFacts, GitPlace, Header, Location, Reading, Visibility,
};
use crate::domain::design::test_support::{design, identified};
use crate::domain::finding::test_support::finding;
use crate::domain::gate::Passed;
use crate::domain::hash::BytesHash;
use crate::domain::id::test_support::{did, docid, eid, sid};
use crate::domain::judge::Judge;
use crate::domain::judge::test_support::judge;
use crate::domain::judgement::test_support::{
    Filed, approve, ca_passed, corpus_of, cr_passed, hash_of,
};
use crate::domain::judgement::{Judgement, JudgementId, ObjectJudgement, PlainJudgement, Split};
use crate::domain::knowable::Knowable;
use crate::domain::ledger::{Ledger, Ledgers};
use crate::domain::location::SourceUri;
use crate::domain::nonempty::NonBlank;
use crate::domain::publication::{
    AbortEvidence, AbortPlan, ObjectTexts, PublicationPlan, PublishEvidence, plan_abort,
    plan_publication,
};
use crate::domain::stored::StoredJudgement;

pub(crate) fn ada() -> Judge {
    judge("ada")
}

/// The text a test approval keeps an object for: the version named `tag`.
pub(crate) fn text_of(tag: &str) -> NormalizedText {
    NormalizedText::new(format!("text of {}", hash_of(tag)))
}

/// A corpus of one public Finding, id 1.
pub(crate) fn public_corpus() -> Corpus {
    corpus_of(vec![Filed::public(Entity::Finding(finding(1, "body")))])
}

/// A corpus of one private Finding, id 1.
pub(crate) fn private_corpus() -> Corpus {
    corpus_of(vec![Filed::private(Entity::Finding(finding(1, "body")))])
}

pub(crate) fn no_ledgers() -> Ledgers {
    Ledgers::new(Ledger::new(), Ledger::new())
}

/// An approval of the Finding at the version `tag`, first written in `origin`.
pub(crate) fn approval_of(tag: &str, origin: Visibility) -> Judgement {
    approve(1, &ada(), origin, cr_passed(1, tag))
}

/// The same approval, as the view of a judgement that keeps an object.
pub(crate) fn approval_with_object(tag: &str, origin: Visibility) -> ObjectJudgement {
    object_view(approval_of(tag, origin))
}

/// `judgement` as the view of a judgement that keeps an object.
pub(crate) fn object_view(judgement: Judgement) -> ObjectJudgement {
    match judgement.split() {
        Split::WithObject(view) => view,
        Split::Plain(_) => panic!("the test judgement keeps no object"),
    }
}

/// `judgement` as the view of a judgement that keeps no object.
pub(crate) fn plain_view(judgement: Judgement) -> PlainJudgement {
    match judgement.split() {
        Split::Plain(view) => view,
        Split::WithObject(_) => panic!("the test judgement keeps an object"),
    }
}

/// The document a plan publishes: a private Design, id 21, with an item, id 31.
const DOC: u128 = 21;

fn ci_passed(tag: &str) -> Passed {
    Passed::for_test(Attestation::Ci {
        design: Pinned::new(did(DOC), hash_of(tag)),
        spec: Pinned::new(sid(11), hash_of("spec").into()),
    })
}

/// A publication plan, with what it was made from.
pub(crate) struct Publishing {
    pub(crate) plan: PublicationPlan,
    /// The two records the plan carries, as read from the private ledger.
    pub(crate) carried: Vec<StoredJudgement>,
    /// A record of the private ledger about another document: not in the plan.
    pub(crate) other: StoredJudgement,
    /// The private ledger the plan was made from: the carried records and `other`.
    pub(crate) private: Ledger,
}

impl Publishing {
    /// The plan to publish the Design whose file holds `bytes`.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        let entity = Entity::Design(design(DOC, 11, vec![(1, identified(31, "the item", &[]))]));
        let header = Header::new(entity.id(), entity.kind(), None, BTreeSet::new(), None);
        let facts = FileFacts::new(
            Location::new(
                Visibility::Private,
                "d21.md".parse().expect("a path"),
                Knowable::Known(GitPlace::Ignored),
            ),
            BytesHash::of(bytes),
        );
        let reading = Reading::resolved(Document::new(header, entity).expect("a document"));
        let corpus = Corpus::build([(facts, reading)], &BTreeSet::new());

        let carried: Vec<StoredJudgement> = vec![
            approve(1, &ada(), Visibility::Private, ci_passed("d1")).into(),
            approve(2, &ada(), Visibility::Private, ca_passed(31, "i1")).into(),
        ];
        let other: StoredJudgement =
            approve(6, &ada(), Visibility::Private, cr_passed(1, "other")).into();
        let private = carried.iter().cloned().chain([other.clone()]);
        let ledgers = Ledgers::new(Ledger::new(), Ledger::from_stored(private));
        let objects = ObjectTexts::of(
            ledgers
                .judgements()
                .filter_map(StoredJudgement::object)
                .map(|key| ObjectText::new(format!("text of {}", key.content()))),
        );
        let plan = plan_publication(
            docid(DOC),
            &corpus,
            &ledgers,
            "docs/d21.md".parse().expect("a path"),
            &ada(),
            &PublishEvidence {
                objects: &objects,
                document: bytes,
                private_root: PrivateRoot::Present,
            },
        )
        .expect("a plan");
        Self {
            plan,
            carried,
            other,
            private: ledgers.private().clone(),
        }
    }

    pub(crate) fn carried_ids(&self) -> Vec<JudgementId> {
        self.carried.iter().map(StoredJudgement::id).collect()
    }

    /// The plan of a publication recorded as published, whose private copy holds
    /// `private`: only the copy is left to remove.
    pub(crate) fn recorded(private: &[u8]) -> Self {
        let (corpus, ledgers) = cut_short(true, b"bytes", private);
        let plan = plan_publication(
            docid(DOC),
            &corpus,
            &ledgers,
            "docs/d21.md".parse().expect("a path"),
            &ada(),
            &PublishEvidence {
                objects: &ObjectTexts::new(),
                document: b"bytes",
                private_root: PrivateRoot::Present,
            },
        )
        .expect("a plan");
        Self {
            plan,
            carried: Vec::new(),
            other: approve(6, &ada(), Visibility::Private, cr_passed(1, "other")).into(),
            private: ledgers.private().clone(),
        }
    }
}

/// The design 21 as a public file of `public` and as a private file of `private`, in a
/// publication that stopped part way (a copy that differs is a publication only when
/// it is recorded as published).
fn cut_short(recorded: bool, public: &[u8], private: &[u8]) -> (Corpus, Ledgers) {
    let entity = Entity::Design(design(DOC, 11, vec![(1, identified(31, "the item", &[]))]));
    let file = |visibility, git, path: &str, bytes: &[u8]| {
        let header = Header::new(entity.id(), entity.kind(), None, BTreeSet::new(), None);
        let facts = FileFacts::new(
            Location::new(
                visibility,
                path.parse().expect("a path"),
                Knowable::Known(git),
            ),
            BytesHash::of(bytes),
        );
        let reading = Reading::resolved(Document::new(header, entity.clone()).expect("a document"));
        (facts, reading)
    };
    let published = if recorded {
        BTreeSet::from([docid(DOC)])
    } else {
        BTreeSet::new()
    };
    let corpus = Corpus::build(
        [
            file(Visibility::Public, GitPlace::Tracked, "docs/d21.md", public),
            file(Visibility::Private, GitPlace::Ignored, "d21.md", private),
        ],
        &published,
    );
    (corpus, Ledgers::new(Ledger::new(), Ledger::new()))
}

/// An abort plan, with what it was made from.
pub(crate) struct Aborting {
    pub(crate) plan: AbortPlan,
}

impl Aborting {
    /// The plan to abort the publication of Design 21 whose public file and private
    /// copy both hold `bytes`: a copy not yet recorded, the only publication that can
    /// be aborted.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        let (corpus, ledgers) = cut_short(false, bytes, bytes);
        let plan = plan_abort(
            docid(DOC),
            &corpus,
            &ledgers,
            &AbortEvidence { public_file: bytes },
        )
        .expect("a plan");
        Self { plan }
    }
}

/// A proof that taking the decision of Finding 1 in was checked.
pub(crate) fn adoptable_finding() -> Adoptable {
    Adoptable::for_test(
        (Gate::Cr, eid(1), hash_of("v1")),
        Vec::new(),
        SourceUri::Citation(NonBlank::new("the old tracker").expect("a citation")),
    )
}
