//! Names that cannot be told now, gathered by cause.
//!
//! A name a link to artifacts writes can be one its adapter cannot tell about now
//! ([`ArtifactState::Unknown`](super::artifact::ArtifactState::Unknown), with an
//! [`Untold`] that says why). What a person does about it is done to the cause, not
//! to the name: building the docs of a crate mends every name of that crate at
//! once. So a surface that speaks of such names reads each cause once, and
//! [`untold_causes`] is the one place that says which names share one: the same
//! [`Space`] and an equal [`Untold`] (`Eq`), never the text a surface writes. It is
//! the same reason [`Assessment::origins`](super::freshness::Assessment::origins)
//! gathers the approvals one edit degrades.
//!
//! ```text
//!   items (UntoldName: document, space and name, why)
//!     └─► untold_causes ──► UntoldCause: the space, the Untold, and
//!                             Held::One      one (document, name) pair, the first item
//!                             Held::Many     the names of two or more pairs, and their
//!                                            documents:
//!                                              Writers::One   one document, its first item
//!                                              Writers::Many  two or more, the first item of each
//! ```
//!
//! The rule is on [`untold_causes`]. The module reads no file, clock or randomness,
//! and does not know the report that carries the names (the application implements
//! [`UntoldName`] for it).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::btree_map::Entry;

use crate::domain::id::DocId;
use crate::domain::nonempty::{Count, NonEmptyVec, TwoOrMore};

use super::artifact::{ArtifactKey, ArtifactName, Space, Untold};

/// What one name that cannot be told now says about itself, for gathering by cause.
///
/// Why: the domain gathers these names but does not know the report that carries
/// them, since the arrows point at the domain; the report answers these three
/// questions and decides nothing.
pub trait UntoldName {
    /// The document that writes the name.
    fn doc(&self) -> DocId;
    /// The space and the name as written.
    fn artifact(&self) -> &ArtifactKey;
    /// Why it cannot be told.
    fn why(&self) -> &Untold;
}

/// The names that cannot be told now for one reason, in one space.
///
/// Why: what a person mends is the cause (build the docs), not the name, so one
/// cause is read once, as an origin of stale approvals is. Two names share a cause
/// when their space and their [`Untold`] are equal; for [`Untold::Said`] that is
/// the equality of the adapter's sentence. That is enough while an adapter gives
/// the same sentence for the same cause; where it does not (one cause, two
/// sentences; two causes, one sentence), the names are gathered otherwise, and none
/// is lost from the answer.
///
/// Only [`untold_causes`] makes one, so a cause holds back at least one name.
#[derive(Debug)]
pub struct UntoldCause<'a, T> {
    space: &'a Space,
    why: &'a Untold,
    held: Held<'a, T>,
}

impl<'a, T> UntoldCause<'a, T> {
    /// The space of every name of the cause.
    ///
    /// Why: a reason is told per space; the same `Untold` in two spaces is two
    /// causes.
    pub fn space(&self) -> &'a Space {
        self.space
    }

    /// Why the names of the cause cannot be told.
    ///
    /// Why: it is the one reason the cause is read once for.
    pub fn why(&self) -> &'a Untold {
        self.why
    }

    /// What the cause holds back: one (document, name) pair, or many.
    ///
    /// Why: whether a cause is one case or many is decided once, in
    /// [`untold_causes`], so that no surface counts again.
    pub fn held(&self) -> &Held<'a, T> {
        &self.held
    }
}

/// How many distinct (document, name) pairs a cause holds back, with what a reader
/// needs of each case.
///
/// Why: for one pair, the document and the name can be said as they are; for more,
/// a reader needs the names and the documents, not a number that hides them. The
/// unit is the pair: a document that writes one name by two links is one pair.
#[derive(Debug)]
pub enum Held<'a, T> {
    /// One pair: the first item of the cause (a document may write the same name by
    /// two links).
    One(&'a T),
    /// Two or more pairs.
    Many {
        /// The distinct names, in the order they first appear.
        names: NonEmptyVec<&'a ArtifactName>,
        /// The documents that write them: one, or how many.
        docs: Writers<'a, T>,
    },
}

/// The documents that write the names of a cause that holds back many pairs.
///
/// Why: a surface can name the documents, so that a failing `stale --exit-code` is
/// traced on stderr alone, as it was when each name had a line of its own. The first
/// item of a document carries what the surface names it by, which the domain does
/// not know. Whether one document or several write them is decided once, in
/// [`untold_causes`], the same for one document and for many.
#[derive(Debug)]
pub enum Writers<'a, T> {
    /// One document writes every name: its first item.
    One(&'a T),
    /// Two or more documents: the first item of each, in the order each first
    /// appears.
    Many(TwoOrMore<&'a T>),
}

/// Gathers names that cannot be told now by cause.
///
/// Why: a person mends a cause, not a name, so a cause is read once; the rule that
/// says which names share one is here, for every surface that writes them. Two items
/// are of one cause when the space of their [`UntoldName::artifact`] and their
/// [`UntoldName::why`] are equal (`Eq` on the [`Space`] and the [`Untold`], never
/// on a text a surface writes). The causes come in the order each first appears in
/// `items`, and the same input gives the same output.
///
/// A cause holds back one pair when all its items write the same name in the same
/// document (by two links, say), and many otherwise; the first item of each pair
/// stands for it. The decision is made here, once, by the count of the pairs after
/// the first ([`Count::of`]). Where an adapter gives one cause two
/// sentences, or two causes one, the names are gathered otherwise; none is lost.
pub fn untold_causes<T: UntoldName>(items: &[T]) -> Vec<UntoldCause<'_, T>> {
    let mut index: BTreeMap<(&Space, &Untold), usize> = BTreeMap::new();
    let mut gathered: Vec<Gathering<'_, T>> = Vec::new();
    for item in items {
        let artifact = item.artifact();
        let pair = (item.doc(), artifact.name());
        match index.entry((artifact.space(), item.why())) {
            Entry::Occupied(at) => {
                let cause = &mut gathered[*at.get()];
                if cause.pairs.insert(pair) {
                    cause.later.push(item);
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(gathered.len());
                gathered.push(Gathering {
                    space: artifact.space(),
                    why: item.why(),
                    first: item,
                    later: Vec::new(),
                    pairs: BTreeSet::from([pair]),
                });
            }
        }
    }
    gathered.into_iter().map(Gathering::into_cause).collect()
}

/// A cause while its items are read: the first item of its first pair, the first
/// item of each pair after it, and the pairs seen.
struct Gathering<'a, T> {
    space: &'a Space,
    why: &'a Untold,
    first: &'a T,
    later: Vec<&'a T>,
    pairs: BTreeSet<(DocId, &'a ArtifactName)>,
}

impl<'a, T: UntoldName> Gathering<'a, T> {
    /// The cause, with its [`Held`] decided: one when no pair comes after the first.
    fn into_cause(self) -> UntoldCause<'a, T> {
        let held = match Count::of(&self.later) {
            Count::None => Held::One(self.first),
            Count::One(_) | Count::Many(_) => many(self.first, &self.later),
        };
        UntoldCause {
            space: self.space,
            why: self.why,
            held,
        }
    }
}

/// Two or more pairs: their distinct names in the order they first appear, and
/// the documents that write them, told one or many by reading what follows the
/// first document's first item.
fn many<'a, T: UntoldName>(first: &'a T, later: &[&'a T]) -> Held<'a, T> {
    let head = first.artifact().name();
    let mut seen_names = BTreeSet::from([head]);
    let mut names = Vec::new();
    let mut seen_docs = BTreeSet::from([first.doc()]);
    let mut other_docs = Vec::new();
    for &item in later {
        let name = item.artifact().name();
        if seen_names.insert(name) {
            names.push(name);
        }
        if seen_docs.insert(item.doc()) {
            other_docs.push(item);
        }
    }
    let mut rest = other_docs.into_iter();
    let docs = match rest.next() {
        None => Writers::One(first),
        Some(second) => Writers::Many(TwoOrMore::new(first, second, rest.collect())),
    };
    Held::Many {
        names: NonEmptyVec::new(head, names),
        docs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::nonempty::NonBlank;
    use crate::domain::shelf::fixtures_tests::docid;

    /// A name that cannot be told, as a test writes it: the link is only there to
    /// tell two items of one document and one name apart.
    #[derive(Debug)]
    struct Item {
        doc: DocId,
        artifact: ArtifactKey,
        why: Untold,
        link: &'static str,
    }

    impl UntoldName for Item {
        fn doc(&self) -> DocId {
            self.doc
        }
        fn artifact(&self) -> &ArtifactKey {
            &self.artifact
        }
        fn why(&self) -> &Untold {
            &self.why
        }
    }

    fn said(text: &str) -> Untold {
        Untold::Said(NonBlank::new(text).unwrap())
    }

    fn item(doc: u128, space: &str, name: &str, why: &Untold, link: &'static str) -> Item {
        Item {
            doc: docid(doc),
            artifact: ArtifactKey::new(
                Space::new(space).unwrap(),
                ArtifactName::new(name).unwrap(),
            ),
            why: why.clone(),
            link,
        }
    }

    fn names<'a>(names: &NonEmptyVec<&'a ArtifactName>) -> Vec<&'a str> {
        names.iter().map(|n| n.as_str()).collect()
    }

    /// The documents of a cause, each with the link of the item that stands for it,
    /// and whether the cause told them as one document.
    fn writers(docs: &Writers<'_, Item>) -> (bool, Vec<(DocId, &'static str)>) {
        match docs {
            Writers::One(item) => (true, vec![(item.doc, item.link)]),
            Writers::Many(items) => (false, items.iter().map(|i| (i.doc, i.link)).collect()),
        }
    }

    #[test]
    fn no_item_is_no_cause() {
        let none: [Item; 0] = [];
        assert!(untold_causes(&none).is_empty());
    }

    #[test]
    fn names_of_one_space_and_one_why_are_one_cause() {
        let why = said("not built");
        let items = [
            item(1, "rust-item", "a::A", &why, "x"),
            item(1, "rust-item", "a::B", &why, "x"),
        ];
        let causes = untold_causes(&items);
        assert_eq!(causes.len(), 1);
        assert_eq!(causes[0].space().as_str(), "rust-item");
        assert_eq!(causes[0].why(), &why);
    }

    #[test]
    fn a_different_why_is_another_cause() {
        let items = [
            item(1, "rust-item", "a::A", &said("a is not built"), "x"),
            item(1, "rust-item", "b::B", &said("b is not built"), "x"),
        ];
        assert_eq!(untold_causes(&items).len(), 2);
    }

    #[test]
    fn the_same_why_in_another_space_is_another_cause() {
        let why = said("cannot read");
        let items = [
            item(1, "rust-item", "a", &why, "x"),
            item(1, "file", "a", &why, "x"),
        ];
        let causes = untold_causes(&items);
        assert_eq!(causes.len(), 2);
        assert_eq!(causes[0].space().as_str(), "rust-item");
        assert_eq!(causes[1].space().as_str(), "file");
    }

    #[test]
    fn the_causes_come_in_the_order_each_first_appears() {
        let (first, second, third) = (said("one"), said("two"), said("three"));
        let items = [
            item(1, "s", "n1", &second, "x"),
            item(1, "s", "n2", &first, "x"),
            item(1, "s", "n3", &second, "x"),
            item(1, "s", "n4", &third, "x"),
            item(1, "s", "n5", &first, "x"),
        ];
        let causes = untold_causes(&items);
        let order: Vec<&Untold> = causes.iter().map(|c| c.why()).collect();
        assert_eq!(order, [&second, &first, &third]);
    }

    #[test]
    fn the_names_of_a_cause_keep_the_order_they_first_appear_in_when_causes_alternate() {
        let (a, b) = (said("a"), said("b"));
        let items = [
            item(1, "s", "z1", &a, "x"),
            item(1, "s", "y1", &b, "x"),
            item(1, "s", "x1", &a, "x"),
            item(1, "s", "w1", &b, "x"),
            item(1, "s", "v1", &a, "x"),
        ];
        let causes = untold_causes(&items);
        let Held::Many { names: of_a, .. } = causes[0].held() else {
            panic!("the first cause holds three names: {:?}", causes[0]);
        };
        assert_eq!(names(of_a), ["z1", "x1", "v1"]);
        let Held::Many { names: of_b, .. } = causes[1].held() else {
            panic!("the second cause holds two names: {:?}", causes[1]);
        };
        assert_eq!(names(of_b), ["y1", "w1"]);
    }

    #[test]
    fn one_name_written_by_two_documents_is_many_with_one_name_and_two_documents() {
        let why = said("not built");
        let items = [
            item(2, "s", "a::A", &why, "p"),
            item(1, "s", "a::A", &why, "q"),
        ];
        let causes = untold_causes(&items);
        assert_eq!(causes.len(), 1);
        let Held::Many { names: n, docs: d } = causes[0].held() else {
            panic!("two pairs are many: {:?}", causes[0]);
        };
        assert_eq!((names(n), n.len()), (vec!["a::A"], 1));
        // Two documents, the first item of each, in the order each first appears.
        assert_eq!(writers(d), (false, vec![(docid(2), "p"), (docid(1), "q")]));
    }

    #[test]
    fn three_documents_are_listed_by_the_first_item_of_each_in_the_order_they_appear() {
        let why = said("not built");
        let items = [
            item(3, "s", "a::A", &why, "p"),
            item(1, "s", "a::B", &why, "q"),
            item(3, "s", "a::C", &why, "r"),
            item(2, "s", "a::D", &why, "s"),
            item(1, "s", "a::E", &why, "t"),
        ];
        let causes = untold_causes(&items);
        let Held::Many { names: n, docs: d } = causes[0].held() else {
            panic!("five pairs are many: {:?}", causes[0]);
        };
        assert_eq!(names(n), ["a::A", "a::B", "a::C", "a::D", "a::E"]);
        assert_eq!(
            writers(d),
            (
                false,
                vec![(docid(3), "p"), (docid(1), "q"), (docid(2), "s")]
            )
        );
    }

    #[test]
    fn one_document_writing_one_name_by_two_links_is_one_pair_told_by_its_first_item() {
        let why = said("not built");
        let items = [
            item(1, "s", "a::A", &why, "first"),
            item(1, "s", "a::A", &why, "second"),
        ];
        let causes = untold_causes(&items);
        assert_eq!(causes.len(), 1);
        let Held::One(only) = causes[0].held() else {
            panic!("one pair is one: {:?}", causes[0]);
        };
        assert_eq!(only.link, "first");
    }

    #[test]
    fn two_names_of_one_document_are_many() {
        let why = said("not built");
        let items = [
            item(1, "s", "a::A", &why, "p"),
            item(1, "s", "a::B", &why, "q"),
        ];
        let causes = untold_causes(&items);
        let Held::Many { names: n, docs: d } = causes[0].held() else {
            panic!("two pairs are many: {:?}", causes[0]);
        };
        assert_eq!(names(n), ["a::A", "a::B"]);
        // One document: its first item, so a surface can name it.
        assert_eq!(writers(d), (true, vec![(docid(1), "p")]));
    }

    #[test]
    fn no_answer_for_two_names_is_one_cause() {
        let items = [
            item(1, "s", "a", &Untold::NoAnswer, "x"),
            item(1, "s", "b", &Untold::NoAnswer, "x"),
            item(1, "s", "c", &Untold::NotAsked, "x"),
        ];
        let causes = untold_causes(&items);
        assert_eq!(causes.len(), 2);
        assert_eq!(causes[0].why(), &Untold::NoAnswer);
        assert!(matches!(causes[0].held(), Held::Many { .. }));
        assert_eq!(causes[1].why(), &Untold::NotAsked);
        assert!(matches!(causes[1].held(), Held::One(_)));
    }
}
