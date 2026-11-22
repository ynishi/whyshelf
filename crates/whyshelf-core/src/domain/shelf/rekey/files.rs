//! What a [`RekeyPlan`] is as files: which files it writes, the entries each is to
//! have, and the check that the files, read back, build the shelf the plan said (R7).
//!
//! ```text
//!   RekeyPlan ──► files()          the target's file and where it is made; the files whose
//!        │                         links are rewritten
//!        ├──────► entries_of(..)   the entries a file is to have, from the entries it has:
//!        │                         the target's `key:`, and the items of the links that
//!        │                         follow or keep both (R5); nothing else
//!        └──────► confirm(..)      the files as the adapter says they read, built by the
//!                                  same `Shelf::build`, held against the plan (R7)
//! ```
//!
//! The adapter writes the entries as text and says what that text reads as; this
//! module never sees the text. [`RekeyPlan::confirm`] takes those readings, puts them in
//! the place of the files they replace, and builds the shelf again, so that what is
//! checked is what the files will be when they are read next, not what the plan meant:
//! a link written under a count that admits one key, a key that reads otherwise than it
//! was written, a problem a rewritten document did not have. Any difference refuses the
//! plan, and nothing is written ([`PlanDiffers`]).

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{
    DocProblem, DocReading, LinkWriters, NowNamed, RawValue, Shelf, Writers,
};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::schema::{EntryName, KEY_ENTRY};
use crate::domain::words::{Join, Mark, listed};

use super::{Meaning, RekeyPlan, Side};

/// The files a plan writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    /// The file the target is in now, which is removed once its document is written in
    /// [`Files::to`].
    pub from: RelPath,
    /// The file the target is made in, with the new key.
    pub to: RelPath,
    /// The files of other documents whose links are rewritten, in the order of the
    /// files: those that follow the target or keep both (a document that stays is not
    /// written).
    pub rewritten: Vec<RelPath>,
}

/// Proof that the files a plan writes, read as the adapter says they read, build the
/// shelf the plan said: only [`RekeyPlan::confirm`] makes one, and writing the files
/// asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed(());

/// What would not be as planned.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Difference {
    /// The files were read again and are not the ones the plan was made from: something
    /// changed them since.
    #[error("the files are not what the plan was made from: they changed since they were read")]
    Read,
    /// The target would not be one document in the new file, with the new key.
    #[error("the document would not be one file, {file}, with the key `{key}`")]
    Target {
        /// The file it was to be in.
        file: RelPath,
        /// The key it was to have.
        key: DocKey,
    },
    /// The old key would not be written by the files that write it now but the target.
    #[error(
        "the old key `{key}` would be written by {}, not by {}",
        files_said(found),
        files_said(expected)
    )]
    OldKey {
        /// The old key.
        key: DocKey,
        /// The files that were to write it.
        expected: Vec<RelPath>,
        /// The files that would.
        found: Vec<RelPath>,
    },
    /// The item of a link would not be what its meaning says.
    #[error("the item `{written}` of `{link}` in {file} would not {}", item_said(*side))]
    Item {
        /// The file.
        file: RelPath,
        /// The link.
        link: EntryName,
        /// The key as the file wrote it.
        written: String,
        /// What it was to come to.
        side: Side,
    },
    /// A rewritten document would have other problems than it has.
    #[error(
        "the problems of {file} would change: {} now, {} then",
        problems_said(before),
        problems_said(after)
    )]
    Problems {
        /// The file, as it is written.
        file: RelPath,
        /// Its problems now (the target's less the old key's `DuplicateKey`).
        before: Vec<DocProblem>,
        /// Its problems as written.
        after: Vec<DocProblem>,
    },
}

/// The files as a sentence, `none` for no file.
fn files_said(files: &[RelPath]) -> String {
    if files.is_empty() {
        "none".to_owned()
    } else {
        listed(files, Mark::Plain, Join::And)
    }
}

/// What the item of a link is to do, as the end of a sentence.
fn item_said(side: Side) -> &'static str {
    match side {
        Side::Follow => "name the new key and no longer the old one",
        Side::Stay => "still name the old key",
        Side::Both => "name both the old key and the new one",
    }
}

/// The problems as a sentence, `none` for no problem.
fn problems_said(problems: &[DocProblem]) -> String {
    if problems.is_empty() {
        "none".to_owned()
    } else {
        listed(problems, Mark::Plain, Join::Semicolon)
    }
}

/// Why the files of a plan are not written: what would not be as planned, every
/// difference listed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "the files the plan would write do not read as planned: {}",
    listed(differences.iter(), Mark::Plain, Join::Semicolon)
)]
pub struct PlanDiffers {
    /// Each difference.
    pub differences: NonEmptyVec<Difference>,
}

/// The problems of the file `file` in `shelf`, in the order they are told: none for a
/// valid document and for a file the shelf has nothing of.
fn problems_of(shelf: &Shelf, file: &RelPath) -> Vec<DocProblem> {
    shelf
        .invalid()
        .values()
        .flat_map(|docs| docs.iter())
        .chain(shelf.unreadable())
        .chain(shelf.anonymous())
        .filter(|doc| doc.origin() == file)
        .flat_map(|doc| doc.problems().iter().cloned())
        .collect()
}

/// Whether `links` has the item `written` of `link` in `file`.
fn has(links: &LinkWriters, file: &RelPath, link: &EntryName, written: &str) -> bool {
    links
        .found()
        .iter()
        .any(|w| w.file() == file && w.link() == link && w.written() == written)
}

/// The files that write a key, as a set.
fn files_of(writers: &Writers) -> BTreeSet<RelPath> {
    writers.files().into_iter().collect()
}

impl RekeyPlan {
    /// The files this plan writes.
    pub fn files(&self) -> Files {
        Files {
            from: self.target.file.clone(),
            to: self.new_file.clone(),
            rewritten: self
                .writers
                .iter()
                .filter(|w| w.file != self.target.file && !matches!(w.meaning, Meaning::Stays(_)))
                .map(|w| w.file.clone())
                .collect(),
        }
    }

    /// The entries the file `file` is to have, from the entries it has `now`: the
    /// target's `key:` is the new key; and in each link of a document that follows, the
    /// first item that wrote the old key is the new key and the others are taken out
    /// (none when the list has the new key already), and in each of a document that
    /// means both, the new key is added after the old one (unless the list has it).
    /// Nothing else changes (R5), so no hash does.
    ///
    /// A file the plan changes nothing in comes out as it is.
    pub fn entries_of(
        &self,
        file: &RelPath,
        now: &BTreeMap<String, RawValue>,
    ) -> BTreeMap<String, RawValue> {
        let mut entries = now.clone();
        let new = self.new_key.key().form.to_string();
        if *file == self.target.file {
            entries.insert(KEY_ENTRY.to_owned(), RawValue::Text(new.clone()));
        }
        let Some(writer) = self.writers.iter().find(|w| w.file == *file) else {
            return entries;
        };
        if matches!(writer.meaning, Meaning::Stays(_)) {
            return entries;
        }
        let links: BTreeSet<&EntryName> = writer.items.iter().map(|item| &item.link).collect();
        for link in links {
            let Some(RawValue::List(texts)) = entries.get_mut(link.as_str()) else {
                continue;
            };
            let names_old = |text: &String| {
                writer
                    .items
                    .iter()
                    .any(|item| item.link == *link && item.written == *text)
            };
            let had_new = texts.contains(&new);
            let mut out = Vec::with_capacity(texts.len() + 1);
            let mut added = had_new;
            for text in texts.iter() {
                if !names_old(text) {
                    out.push(text.clone());
                    continue;
                }
                match writer.meaning {
                    // The new key once: a list that names it already, or that wrote the
                    // old key twice, does not come to name it twice.
                    Meaning::Follows(_) => {
                        if !added {
                            out.push(new.clone());
                            added = true;
                        }
                    }
                    Meaning::Both => {
                        out.push(text.clone());
                        if !added {
                            out.push(new.clone());
                            added = true;
                        }
                    }
                    Meaning::Stays(_) => out.push(text.clone()),
                }
            }
            *texts = out;
        }
        entries
    }

    /// Hold the files this plan writes, as the adapter says they read, against the
    /// plan, before any is written (R7).
    ///
    /// `shelf` is the shelf the plan was made from, built from `readings` (every file
    /// of the books as it was read); `planned` are the readings of the files the plan
    /// writes, [`Files::to`] and each of [`Files::rewritten`], as the adapter says the
    /// text it would write reads. The planned files are put in the place of the ones
    /// they replace (the target's file is gone), and the shelf is built again by
    /// [`Shelf::build`], the same build that reads the files next. Then:
    ///
    /// 1. `readings` build `shelf`: nothing changed since the plan was made.
    /// 2. The target is one document, in the new file, with the new key.
    /// 3. The new key is written by the target alone; the old key by the files that
    ///    wrote it, less the target's.
    /// 4. Each item of each document that writes the old key in a link is where its
    ///    meaning puts it: for a document that follows, in the new key and no longer in
    ///    the old; that stays, in the old; that means both, in both. They are read
    ///    from what the files write (`Shelf::link_writers`), so that the links of an
    ///    invalid document are checked as those of a valid one.
    /// 5. Each rewritten document has the problems it has, and the target the problems
    ///    it has less the old key's `DuplicateKey`: a link of a count that admits one
    ///    key that gained another by `--both` is told here.
    ///
    /// Every difference is listed.
    // The differences are the answer, read once per request; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn confirm(
        &self,
        shelf: &Shelf,
        readings: Vec<DocReading>,
        planned: Vec<DocReading>,
        artifacts: &ArtifactReadings,
    ) -> Result<Confirmed, PlanDiffers> {
        let schema = shelf.schema();
        if Shelf::build(schema, readings.clone(), artifacts) != *shelf {
            return Err(PlanDiffers {
                differences: NonEmptyVec::new(Difference::Read, Vec::new()),
            });
        }
        let replaced: BTreeSet<RelPath> = planned
            .iter()
            .map(|reading| reading.origin().clone())
            .chain([self.target.file.clone()])
            .collect();
        let after = Shelf::build(
            schema,
            readings
                .into_iter()
                .filter(|reading| !replaced.contains(reading.origin()))
                .chain(planned)
                .collect(),
            artifacts,
        );
        let mut differences = Vec::new();

        let old = &self.target.key;
        let new = self.new_key.key();
        // One document: the identity is carried by the new file alone, and it is
        // named by the new key, whether it is valid or invalid there (an invalid
        // file has no key of its own: the one its files write is what `now_named`
        // says).
        let one_document = after.state_of(self.target.id).files() == [&self.new_file]
            && matches!(
                after.now_named(self.target.id),
                NowNamed::Valid(key) | NowNamed::Invalid(Some(key)) if key == *new
            );
        let new_by_target = matches!(
            after.key_writers(new).writers(),
            Writers::One(writer)
                if writer.file() == &self.new_file && writer.id() == Some(self.target.id)
        );
        if !(one_document && new_by_target) {
            differences.push(Difference::Target {
                file: self.new_file.clone(),
                key: new.clone(),
            });
        }
        let mut expected = files_of(shelf.key_writers(old).writers());
        expected.remove(&self.target.file);
        let found = files_of(after.key_writers(old).writers());
        if found != expected {
            differences.push(Difference::OldKey {
                key: old.clone(),
                expected: expected.into_iter().collect(),
                found: found.into_iter().collect(),
            });
        }

        let at_old = after.link_writers(old);
        let at_new = after.link_writers(new);
        let new_text = new.form.to_string();
        for writer in &self.writers {
            let file = if writer.file == self.target.file {
                &self.new_file
            } else {
                &writer.file
            };
            for item in &writer.items {
                let keeps = has(&at_old, file, &item.link, &item.written);
                let gains = has(&at_new, file, &item.link, &new_text);
                let (side, ok) = match writer.meaning {
                    Meaning::Follows(_) => (Side::Follow, !keeps && gains),
                    Meaning::Stays(_) => (Side::Stay, keeps),
                    Meaning::Both => (Side::Both, keeps && gains),
                };
                if !ok {
                    differences.push(Difference::Item {
                        file: file.clone(),
                        link: item.link.clone(),
                        written: item.written.clone(),
                        side,
                    });
                }
            }
        }

        for rewritten in self.files().rewritten {
            let (before, now) = (
                problems_of(shelf, &rewritten),
                problems_of(&after, &rewritten),
            );
            if before != now {
                differences.push(Difference::Problems {
                    file: rewritten,
                    before,
                    after: now,
                });
            }
        }
        let before: Vec<DocProblem> = problems_of(shelf, &self.target.file)
            .into_iter()
            .filter(
                |problem| !matches!(problem, DocProblem::DuplicateKey { key, .. } if key == old),
            )
            .collect();
        let now = problems_of(&after, &self.new_file);
        if before != now {
            differences.push(Difference::Problems {
                file: self.new_file.clone(),
                before,
                after: now,
            });
        }

        match NonEmptyVec::from_vec(differences) {
            Ok(differences) => Err(PlanDiffers { differences }),
            Err(_) => Ok(Confirmed(())),
        }
    }
}
