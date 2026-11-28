//! What depends on some named documents: the one walk down from them (`walk`),
//! and what it could not follow ([`Unwalked`]); and what a reach of them comes to
//! (`Reach`): the artifacts those documents stand on, and the files of the project
//! all of it is in.
//!
//! Which documents the `Depends` links of a document name is `Doc::depends_names`. The
//! walk takes every valid document of the shelf whose links name one it reached,
//! until none is left, and counts what it could not read the links of: a document
//! whose links cannot be read, a file left in a merge conflict a side of which shows a
//! document not reached, and a file whose identity cannot be read, may depend on the
//! named without the walk seeing it.
//!
//! | rule | what | the one place |
//! |---|---|---|
//! | R1 | the documents reached: the named and what depends on them, and what the walk could not follow | `walk` |
//! | R2 | the artifacts they stand on, by space: each reached document's `Footing::artifacts` (its links to artifacts now and the pins of its approval), outside the walk | `Reach::of` |
//! | R3 | the files: every file a reached document may be in (`DocState::may_be_in`) and every file of the placement of an artifact ([`Placement`], its adapter's answer), with the artifacts whose place cannot be told and the documents that are gone apart | `Reach::files` |

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptySet;

use super::artifact::ArtifactKey;
use super::build::{DocState, Shelf};
use super::footing::Footing;
use super::ledger::Ledger;
use super::placement::Placement;

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
/// the three counts is more than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unwalked {
    invalid: usize,
    sided: usize,
    unreadable: usize,
}

impl Unwalked {
    /// The documents not reached whose links cannot be read: an identity whose files
    /// are not valid.
    pub fn invalid(&self) -> usize {
        self.invalid
    }

    /// The files left in a merge conflict whose sides show different identities, one
    /// of which the walk did not reach: each may be a document of a side not
    /// reached.
    pub fn sided(&self) -> usize {
        self.sided
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
///   not valid and that the walk did not reach (its links cannot be read), a file
///   left in a merge conflict a side of which shows an identity the walk did not
///   reach (it may be that document), and a file whose identity cannot be read (it
///   may be any document). None when there is none of them. A file left in a merge
///   conflict whose sides all show identities reached is not counted: whichever
///   document it holds is reached.
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
    let sided = shelf
        .sided()
        .iter()
        .filter(|file| file.shown().any(|id| !reached.contains(&id)))
        .count();
    let unreadable = shelf.unreadable().len();
    let untold = (invalid > 0 || sided > 0 || unreadable > 0).then_some(Unwalked {
        invalid,
        sided,
        unreadable,
    });
    Walked { reached, untold }
}

/// A reach of named documents: what the walk down from them reached and could not
/// follow (R1), and the artifacts the documents reached stand on (R2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reach {
    walked: Walked,
    /// Each artifact a document reached stands on, with those of them that do.
    artifacts: BTreeMap<ArtifactKey, BTreeSet<DocId>>,
}

/// What a reach comes to in the files of the project (R3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReachFiles {
    /// Each document reached, with the files it may be in (none when it is gone).
    pub(crate) docs: BTreeMap<DocId, BTreeSet<RelPath>>,
    /// Each artifact the documents reached stand on, with those documents and where
    /// it lives.
    pub(crate) artifacts: BTreeMap<ArtifactKey, Placed>,
    /// Every file a document reached may be in, and every file the placement of an
    /// artifact it stands on names, whole or in part.
    pub(crate) files: BTreeSet<RelPath>,
    /// The documents reached that are gone: no file carries them, so none is in
    /// `files` for them.
    pub(crate) gone: BTreeSet<DocId>,
}

/// One artifact of a reach: the documents reached that stand on it, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    /// The documents reached that stand on it.
    pub(crate) by: BTreeSet<DocId>,
    /// Its adapter's answer, or unplaced with why when its adapter gave none.
    pub(crate) placement: Placement,
}

impl Reach {
    /// The reach of `named` in `shelf`: the walk down from them (R1, [`walk`]), and for
    /// each document it reached, the artifacts it stands on by its footing in `shelf`
    /// and `ledger` ([`Footing::artifacts`]), outside the walk (R2).
    pub(crate) fn of(named: &NonEmptySet<DocId>, shelf: &Shelf, ledger: &Ledger) -> Self {
        let walked = walk(named, shelf);
        let mut artifacts: BTreeMap<ArtifactKey, BTreeSet<DocId>> = BTreeMap::new();
        for id in walked.reached() {
            for artifact in Footing::of(shelf, *id, ledger).artifacts(shelf, ledger) {
                artifacts.entry(artifact).or_default().insert(*id);
            }
        }
        Self { walked, artifacts }
    }

    /// The documents reached: the named, and every document that depends on one of
    /// them, transitively.
    pub(crate) fn docs(&self) -> &BTreeSet<DocId> {
        self.walked.reached()
    }

    /// Each artifact a document reached stands on, by space and name, with the
    /// documents reached that stand on it.
    pub(crate) fn artifacts(&self) -> &BTreeMap<ArtifactKey, BTreeSet<DocId>> {
        &self.artifacts
    }

    /// What may depend on the named and could not be read, when anything may.
    pub(crate) fn untold(&self) -> Option<&Unwalked> {
        self.walked.untold()
    }

    /// The files of the reach (R3), given where each artifact lives (`placements`,
    /// the adapters' answers): every file a document reached may be in
    /// (`DocState::may_be_in`: its file, the files of an invalid one, or every file
    /// whose identity cannot be read when it may be in any), and every file of the
    /// placement of each artifact, whole or in part. An artifact whose adapter cannot
    /// say where it lives, or gave no answer for it, is unplaced with why, and a
    /// document that is gone is told apart.
    pub(crate) fn files(
        &self,
        shelf: &Shelf,
        placements: &BTreeMap<ArtifactKey, Placement>,
    ) -> ReachFiles {
        let mut found = ReachFiles::default();
        for id in self.docs() {
            let state = shelf.state_of(*id);
            if matches!(state, DocState::Gone) {
                found.gone.insert(*id);
            }
            let files: BTreeSet<RelPath> = state.may_be_in().into_iter().cloned().collect();
            found.files.extend(files.iter().cloned());
            found.docs.insert(*id, files);
        }
        for (artifact, by) in &self.artifacts {
            let placement = placements
                .get(artifact)
                .cloned()
                .unwrap_or_else(Placement::unanswered);
            if let Some(files) = placement.files() {
                found.files.extend(files.iter().cloned());
            }
            let placed = Placed {
                by: by.clone(),
                placement,
            };
            found.artifacts.insert(artifact.clone(), placed);
        }
        found
    }
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
    fn a_file_left_in_a_conflict_is_counted_only_while_a_side_it_shows_is_not_reached() {
        use crate::domain::shelf::fixtures_tests::sided;
        let counts = |walked: &Walked| {
            walked
                .untold()
                .map(|u| (u.invalid(), u.sided(), u.unreadable()))
        };
        // The sides show 5 and 6, which no file carries.
        let s = shelf(vec![fr(1, &[]), sided("findings/x.md", &[5, 6])]);
        // Both reached: whichever the file holds is reached.
        assert_eq!(counts(&walk(&named(&[5, 6]), &s)), None);
        // One reached, or none.
        assert_eq!(counts(&walk(&named(&[5]), &s)), Some((0, 1, 0)));
        assert_eq!(counts(&walk(&named(&[1]), &s)), Some((0, 1, 0)));
        // Beside a file that may be any document, each is counted apart.
        let s = shelf(vec![
            fr(1, &[]),
            sided("findings/x.md", &[5, 6]),
            unreadable(),
        ]);
        assert_eq!(counts(&walk(&named(&[1]), &s)), Some((0, 1, 1)));
        assert_eq!(counts(&walk(&named(&[5, 6]), &s)), Some((0, 0, 1)));
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

    mod reach_of {
        use super::*;
        use crate::domain::shelf::fixtures_tests::{approved, file_artifact, ledger, sp_testing};

        fn only(paths: &[&str]) -> Placement {
            let mut files = paths.iter().map(|p| path(p));
            let first = files.next().unwrap();
            Placement::Only(NonEmptySet::new(first, files))
        }

        fn files(paths: &[&str]) -> BTreeSet<RelPath> {
            paths.iter().map(|p| path(p)).collect()
        }

        #[test]
        fn a_reach_stands_on_the_artifacts_of_every_document_reached_and_no_other() {
            let s = shelf(vec![
                fr(1, &[]),
                sp_testing(2, &["FR-1"], &["t/a.rs", "t/b.rs"]),
                sp_testing(3, &["FR-4"], &["t/c.rs"]),
                fr(4, &[]),
            ]);
            let reach = Reach::of(&named(&[1]), &s, &ledger(Vec::new()));
            assert_eq!(reach.docs(), &ids(&[1, 2]));
            assert_eq!(
                reach.artifacts(),
                &BTreeMap::from([
                    (file_artifact("t/a.rs"), ids(&[2])),
                    (file_artifact("t/b.rs"), ids(&[2])),
                ])
            );
            assert_eq!(reach.untold(), None);
        }

        #[test]
        fn a_reach_stands_on_what_the_approval_of_a_document_pinned_too() {
            // Approved while it named `t/old.rs`; it names only `t/a.rs` now.
            let before = shelf(vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["t/old.rs"])]);
            let approvals = approved(&before, &[1, 2]);
            let now = shelf(vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["t/a.rs"])]);
            let reach = Reach::of(&named(&[1]), &now, &approvals);
            assert_eq!(
                reach.artifacts().keys().cloned().collect::<BTreeSet<_>>(),
                BTreeSet::from([file_artifact("t/a.rs"), file_artifact("t/old.rs")])
            );
        }

        #[test]
        fn the_files_are_the_documents_and_the_places_of_their_artifacts_with_the_rest_apart() {
            let s = shelf(vec![
                fr(1, &[]),
                sp_testing(2, &["FR-1"], &["t/a.rs", "t/b.rs", "t/c.rs"]),
                invalid(9),
                sp_testing(3, &["FR-9"], &[]),
            ]);
            let reach = Reach::of(&named(&[1, 9, 77]), &s, &ledger(Vec::new()));
            let why = crate::domain::nonempty::NonBlank::new("no place").unwrap();
            let placements = BTreeMap::from([
                (file_artifact("t/a.rs"), only(&["t/a.rs"])),
                (
                    file_artifact("t/b.rs"),
                    Placement::Partly(NonEmptySet::new(path("src/b.rs"), [])),
                ),
                (file_artifact("t/c.rs"), Placement::Unplaced(why.clone())),
            ]);
            let found = reach.files(&s, &placements);
            assert_eq!(
                found.files,
                files(&[
                    "findings/fr-1.md",
                    "findings/fr-9.md",
                    "specs/sp-2.md",
                    "specs/sp-3.md",
                    "t/a.rs",
                    "src/b.rs",
                ])
            );
            let placed: BTreeMap<ArtifactKey, Placement> = found
                .artifacts
                .iter()
                .map(|(artifact, placed)| (artifact.clone(), placed.placement.clone()))
                .collect();
            assert_eq!(placed, placements);
            assert!(
                found
                    .artifacts
                    .values()
                    .all(|placed| placed.by == ids(&[2]))
            );
            assert_eq!(
                found.artifacts[&file_artifact("t/c.rs")].placement,
                Placement::Unplaced(why)
            );
            // Each document reached with its files, one gone with none.
            assert_eq!(
                found.docs.keys().copied().collect::<BTreeSet<_>>(),
                ids(&[1, 2, 3, 9, 77])
            );
            assert_eq!(found.docs[&docid(2)], files(&["specs/sp-2.md"]));
            assert!(found.docs[&docid(77)].is_empty());
            assert_eq!(found.gone, ids(&[77]));
            // An artifact its adapter gave no answer for has no place, and says so.
            let found = reach.files(&s, &BTreeMap::new());
            let none = Placement::unanswered();
            assert_eq!(found.artifacts.len(), 3);
            assert!(
                found
                    .artifacts
                    .values()
                    .all(|placed| placed.placement == none)
            );
            assert_eq!(found.files.len(), 4);
        }

        #[test]
        fn a_document_that_may_be_in_any_unreadable_file_reaches_every_such_file() {
            let s = shelf(vec![fr(1, &[]), unreadable()]);
            let reach = Reach::of(&named(&[5]), &s, &ledger(Vec::new()));
            let found = reach.files(&s, &BTreeMap::new());
            assert_eq!(found.files, files(&["findings/unreadable.md"]));
            assert!(found.gone.is_empty());
            assert_eq!(reach.untold().map(|u| u.unreadable()), Some(1));
        }
    }
}
