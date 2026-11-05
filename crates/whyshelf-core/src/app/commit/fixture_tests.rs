//! Values the tests of the evidence chain, the confirmation and the fakes share.

use std::collections::BTreeSet;

use crate::app::raw::NormalizedText;
use crate::domain::approval::{Attestation, Pinned};
use crate::domain::boundary::{ObjectText, PrivateRoot};
use crate::domain::corpus::{
    Corpus, Document, Entity, FileFacts, GitPlace, Header, Location, Reading, Visibility,
};
use crate::domain::design::test_support::{design, identified};
use crate::domain::finding::test_support::finding;
use crate::domain::gate::Passed;
use crate::domain::hash::BytesHash;
use crate::domain::id::test_support::{did, docid, sid};
use crate::domain::judge::Judge;
use crate::domain::judge::test_support::judge;
use crate::domain::judgement::test_support::{
    Filed, approve, ca_passed, corpus_of, cr_passed, hash_of,
};
use crate::domain::judgement::{Judgement, JudgementId, ObjectJudgement, PlainJudgement, Split};
use crate::domain::knowable::Knowable;
use crate::domain::ledger::{Ledger, Ledgers};
use crate::domain::publication::{ObjectTexts, PublicationPlan, PublishEvidence, plan_publication};

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
    /// The two records the plan carries, in the private ledger.
    pub(crate) carried: Vec<Judgement>,
    /// A record of the private ledger about another document: not in the plan.
    pub(crate) other: Judgement,
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
        let reading = Reading::Document(Document::new(header, entity).expect("a document"));
        let corpus = Corpus::build([(facts, reading)], &BTreeSet::new());

        let carried = vec![
            approve(1, &ada(), Visibility::Private, ci_passed("d1")),
            approve(2, &ada(), Visibility::Private, ca_passed(31, "i1")),
        ];
        let other = approve(6, &ada(), Visibility::Private, cr_passed(1, "other"));
        let private = carried.iter().cloned().chain([other.clone()]);
        let ledgers = Ledgers::new(Ledger::new(), Ledger::from_judgements(private));
        let objects = ObjectTexts::of(
            ledgers
                .judgements()
                .filter_map(Judgement::object)
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
        self.carried.iter().map(Judgement::id).collect()
    }
}
