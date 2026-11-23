//! What depends on some named documents: the one walk down from them (`walk`),
//! and what it could not follow ([`Unwalked`]).
//!
//! Which documents the `Depends` links of a document name is `Doc::depends_names`. The
//! walk takes every valid document of the shelf whose links name one it reached,
//! until none is left, and counts what it could not read the links of: a document
//! whose links cannot be read, and a file whose identity cannot be read, may depend
//! on the named without the walk seeing it.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptySet;

use super::build::Shelf;

/// What a walk down from the named reached, and what it could not follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Walked {
    /// The named, and every document that depends on one of them.
    reached: BTreeSet<DocId>,
    /// What may depend on them and could not be read, when anything may.
    untold: Option<Unwalked>,
}

impl Walked {
    /// The named, and every document that depends on one of them, transitively.
    pub(crate) fn reached(&self) -> &BTreeSet<DocId> {
        &self.reached
    }

    /// What may depend on the named and could not be read, when anything may.
    pub(crate) fn untold(&self) -> Option<&Unwalked> {
        self.untold.as_ref()
    }
}

/// What a walk down from named documents could not follow, counted; at least one of
/// the two counts is more than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unwalked {
    invalid: usize,
    unreadable: usize,
}

impl Unwalked {
    /// The documents not reached whose links cannot be read: an identity whose files
    /// are not valid.
    pub fn invalid(&self) -> usize {
        self.invalid
    }

    /// The files whose identity cannot be read: each may be any document.
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }
}

/// What depends on `named` in `shelf`:
///
/// - The named, and every valid document of the shelf whose `Depends` links name a
///   document reached (`Doc::depends_names`: one not valid, and each document a key
///   two files write may mean, included), until no other
///   does: down from the named, through the links as the documents hold them now.
/// - What it could not follow, counted ([`Unwalked`]): an identity whose files are
///   not valid and that the walk did not reach (its links cannot be read), and a file
///   whose identity cannot be read (it may be any document). None when there is
///   neither.
/// - Not counted: a file with no `id:` (it is no document and has no approval), and
///   a document that is gone (it has no file, and so no link now).
pub(crate) fn walk(named: &NonEmptySet<DocId>, shelf: &Shelf) -> Walked {
    let mut reached: BTreeSet<DocId> = named.as_set().clone();
    // What each document a link names is named by: the valid documents whose links
    // name it, read once.
    let mut named_by: BTreeMap<DocId, Vec<DocId>> = BTreeMap::new();
    for (id, doc) in shelf.docs() {
        for on in doc.depends_names() {
            named_by.entry(on).or_default().push(*id);
        }
    }
    let mut next: Vec<DocId> = reached.iter().copied().collect();
    while let Some(on) = next.pop() {
        for id in named_by.get(&on).into_iter().flatten() {
            if reached.insert(*id) {
                next.push(*id);
            }
        }
    }
    let invalid = shelf
        .invalid()
        .keys()
        .filter(|id| !reached.contains(id))
        .count();
    let unreadable = shelf.unreadable().len();
    let untold = (invalid > 0 || unreadable > 0).then_some(Unwalked {
        invalid,
        unreadable,
    });
    Walked { reached, untold }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
    use crate::domain::shelf::fixtures_tests::{
        docid, entries, fr, list, path, reading, shelf, text,
    };

    fn named(ns: &[u128]) -> NonEmptySet<DocId> {
        NonEmptySet::from_set(ns.iter().map(|n| docid(*n)).collect()).unwrap()
    }

    fn ids(ns: &[u128]) -> BTreeSet<DocId> {
        ns.iter().map(|n| docid(*n)).collect()
    }

    /// The counts of what the walk could not follow, `(invalid, unreadable)`.
    fn untold(walked: &Walked) -> Option<(usize, usize)> {
        walked.untold().map(|u| (u.invalid(), u.unreadable()))
    }

    /// The finding `FR-<n>` with identity `n`, not valid: it has no title.
    fn invalid(n: u128) -> DocReading {
        reading(&format!("findings/fr-{n}.md"), n, &format!("FR-{n}"), &[])
    }

    /// A file of the book of findings whose identity cannot be read.
    fn unreadable() -> DocReading {
        DocReading::Unparsed {
            origin: path("findings/unreadable.md"),
            id: ReadId::Unreadable,
            why: RawProblem::NotAMapping,
        }
    }

    /// A file of the book of findings with no `id:`.
    fn anonymous() -> DocReading {
        DocReading::Parsed {
            origin: path("findings/anonymous.md"),
            entries: entries(&[("key", text("FR-50")), ("title", text("t"))]),
            body: "body\n".to_owned(),
        }
    }

    #[test]
    fn the_walk_reaches_what_depends_on_the_named_and_nothing_upstream() {
        let s = shelf(vec![
            fr(1, &[]),
            fr(2, &["FR-1"]),
            fr(3, &["FR-2"]),
            fr(4, &[]),
            fr(5, &["FR-4"]),
        ]);
        let walked = walk(&named(&[2]), &s);
        assert_eq!(walked.reached(), &ids(&[2, 3]));
        assert_eq!(untold(&walked), None);
        let walked = walk(&named(&[1, 4]), &s);
        assert_eq!(walked.reached(), &ids(&[1, 2, 3, 4, 5]));
    }

    #[test]
    fn only_a_depends_link_is_walked() {
        let informed = reading(
            "findings/fr-2.md",
            2,
            "FR-2",
            &[("title", text("t")), ("related", list(&["FR-1"]))],
        );
        let s = shelf(vec![fr(1, &[]), informed, fr(3, &["FR-1"])]);
        assert_eq!(walk(&named(&[1]), &s).reached(), &ids(&[1, 3]));
    }

    #[test]
    fn a_named_document_that_is_not_valid_reaches_what_links_to_it() {
        let s = shelf(vec![invalid(9), fr(2, &["FR-9"]), fr(3, &["FR-2"])]);
        assert_eq!(s.invalid().len(), 1);
        let walked = walk(&named(&[9]), &s);
        assert_eq!(walked.reached(), &ids(&[9, 2, 3]));
        assert_eq!(untold(&walked), None);
    }

    #[test]
    fn a_link_to_a_key_two_named_files_write_reaches_its_document() {
        let one = reading("findings/a.md", 5, "FR-5", &[("title", text("t"))]);
        let two = reading("findings/b.md", 6, "FR-5", &[("title", text("t"))]);
        let s = shelf(vec![one, two, fr(2, &["FR-5"]), fr(3, &["FR-2"])]);
        assert_eq!(s.collisions().len(), 1);
        let walked = walk(&named(&[5, 6]), &s);
        assert_eq!(walked.reached(), &ids(&[5, 6, 2, 3]));
        assert_eq!(untold(&walked), None);
        // One of the two named: the link may mean it, so it is reached too; the other
        // writer, not reached, cannot be read.
        let walked = walk(&named(&[5]), &s);
        assert_eq!(walked.reached(), &ids(&[5, 2, 3]));
        assert_eq!(untold(&walked), Some((1, 0)));
    }

    #[test]
    fn a_cycle_a_link_to_itself_and_a_link_written_twice_end_the_walk() {
        // 1 and 2 depend on each other, and 3 on 2.
        let s = shelf(vec![fr(1, &["FR-2"]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
        assert_eq!(walk(&named(&[1]), &s).reached(), &ids(&[1, 2, 3]));
        assert_eq!(walk(&named(&[3]), &s).reached(), &ids(&[3]));
        // A document that names itself.
        let s = shelf(vec![fr(1, &["FR-1"]), fr(2, &["FR-1"])]);
        assert_eq!(walk(&named(&[1]), &s).reached(), &ids(&[1, 2]));
        // One target named twice by one link.
        let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1", "FR-1"]), fr(3, &["FR-2"])]);
        assert_eq!(walk(&named(&[1]), &s).reached(), &ids(&[1, 2, 3]));
    }

    #[test]
    fn a_named_document_that_depends_on_another_named_one_is_reached_once() {
        let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
        let walked = walk(&named(&[1, 2]), &s);
        assert_eq!(walked.reached(), &ids(&[1, 2, 3]));
        assert_eq!(untold(&walked), None);
    }

    #[test]
    fn a_document_whose_links_cannot_be_read_and_is_not_reached_is_counted() {
        let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), invalid(9)]);
        let walked = walk(&named(&[1]), &s);
        assert_eq!(walked.reached(), &ids(&[1, 2]));
        assert_eq!(untold(&walked), Some((1, 0)));
    }

    #[test]
    fn a_file_whose_identity_cannot_be_read_is_counted() {
        let s = shelf(vec![fr(1, &[]), unreadable()]);
        assert_eq!(untold(&walk(&named(&[1]), &s)), Some((0, 1)));
        let s = shelf(vec![fr(1, &[]), invalid(9), unreadable()]);
        assert_eq!(untold(&walk(&named(&[1]), &s)), Some((1, 1)));
    }

    #[test]
    fn a_named_invalid_document_a_file_with_no_identity_and_a_gone_one_count_nothing() {
        // The named itself is not valid: it is reached, and not counted.
        let s = shelf(vec![invalid(9), fr(2, &[])]);
        let walked = walk(&named(&[9]), &s);
        assert_eq!(walked.reached(), &ids(&[9]));
        assert_eq!(untold(&walked), None);
        // A file with no `id:` is no document.
        let s = shelf(vec![fr(1, &[]), anonymous()]);
        assert_eq!(s.anonymous().len(), 1);
        assert_eq!(untold(&walk(&named(&[1]), &s)), None);
        // A document that is gone has no file, and no link now.
        let s = shelf(vec![fr(1, &[])]);
        let walked = walk(&named(&[77]), &s);
        assert_eq!(walked.reached(), &ids(&[77]));
        assert_eq!(untold(&walked), None);
    }
}
