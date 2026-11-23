//! A run of new documents, planned all at once: their keys, the names they link each
//! other by, and whether each would be valid.
//!
//! One entry asks it, [`Shelf::plan_new`], and another says what comes of a key
//! whose file was there when the run wrote it, [`Shelf::replan_after_taken`]. Both
//! are asked of a built [`Shelf`]; nothing else decides any of this.
//!
//! ```text
//!   requests (a row each: book, key?, name?, entries, body) + an identity each
//!      │
//!      ├─ 1  the keys asked for       Shelf::claim_key, each; two rows asking one key
//!      │                                                    is KeyTwice
//!      ├─ 2  the keys chosen          the numbers after the highest any file of the book
//!      │                              writes, in the order of the run, passing over the
//!      │                              keys asked for and the keys written
//!      ├─ 3  the names                a name of a row, twice: NameTwice; a link to a
//!      │                              name no row has, or one of another book than the
//!      │                              link points into: UnknownName, NameOtherBook;
//!      │                              else the key of that row stands in the link
//!      ├─ 4  the judgement            each row made a NewDoc by the one function
//!      │                              Shelf::check_new calls too, against the shelf's index
//!      ├─ 5  the resolution           the run's documents added to the shelf's index, and
//!      │                              the rows made again against it: a link to a row
//!      │                              later in the run, or in a ring, is resolved too (a
//!      │                              hash covers no link)
//!      └─ 6  every row refused is a PlanProblem; one is enough for `Err`, and nothing of
//!         the run is planned
//! ```
//!
//! The order the documents are written in puts what a row links to by name before it;
//! rows whose names link in a ring stay in the order of the run.
//!
//! A key a run wrote at is found taken only when its file is written: what comes of
//! that is [`Shelf::replan_after_taken`]'s, for the row whose key it is. Only that
//! row is given another key; the rows that link to it by name are made again with its
//! new key, and every link of the run is resolved again.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::error::ValueError;
use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptyVec, TwoOrMore};
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::doc::{DocKey, KeyWriter};
use crate::domain::shelf::schema::BookName;

use super::new::{AfterTaken, KeySearch, key_number, numbered};
use super::{
    ClaimKeyProblem, KeyIndex, NewDoc, NewDocProblem, NewKey, NextKeyProblem, NoFreeKey, RawValue,
    Shelf,
};

/// The name of a document in a run of new documents: what another row of the run
/// links to it by. It is never written to a file; the key of the row stands in the
/// link that names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunName(NonBlank);

impl RunName {
    /// A name, refused when it is empty or only white space.
    pub fn new(text: impl Into<String>) -> Result<Self, ValueError> {
        NonBlank::new(text).map(Self)
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for RunName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One item of a link to documents of a book: the key of a document, as written, or
/// the name of a row of the run, which the plan turns into that row's key. Only an
/// item of a link to documents can be a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkItem {
    /// A key, as written.
    Key(String),
    /// The name of a row of the run.
    Named(RunName),
}

/// The value of one entry of a new document, read by the type its book declares for
/// the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedValue {
    /// A value of the front matter as it is written: a text, a whole number, a truth
    /// value, or a list of texts (a text list field, or a link to artifacts).
    Raw(RawValue),
    /// The items of a link to documents, which declares what each is: a key or the
    /// name of a row.
    Link(Vec<LinkItem>),
}

/// A request for one new document of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRequest {
    /// The number the caller gives the row, which every problem of it carries: the
    /// line of the file for a run read from one (blank lines counted), 1 for a single
    /// document.
    pub row: usize,
    /// The book.
    pub book: BookName,
    /// The key, as typed; `None` is the next key of the book.
    pub key: Option<String>,
    /// The name other rows of the run link to this one by; `None` is no name.
    pub name: Option<RunName>,
    /// The entries of the front matter, by name (`id` and `key` are whyshelf's).
    pub entries: BTreeMap<String, PlannedValue>,
    /// The body.
    pub body: String,
}

/// Why a row of a run cannot be planned, or two rows together cannot. Each says the
/// row (or rows) it is about by the numbers the caller gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanProblem {
    /// The document would be invalid, or cannot be made of what was given.
    Doc {
        /// The row.
        row: usize,
        /// Why.
        problem: NewDocProblem,
    },
    /// The key given for the row is malformed or taken, or its book unknown.
    ClaimKey {
        /// The row.
        row: usize,
        /// Why.
        problem: ClaimKeyProblem,
    },
    /// The book of the row has no next key.
    NextKey {
        /// The row.
        row: usize,
        /// Why.
        problem: NextKeyProblem,
    },
    /// Several rows ask for one key.
    KeyTwice {
        /// The rows.
        rows: TwoOrMore<usize>,
        /// The key.
        key: DocKey,
    },
    /// Several rows have one name.
    NameTwice {
        /// The rows.
        rows: TwoOrMore<usize>,
        /// The name.
        name: RunName,
    },
    /// A link names a row no row of the run is named.
    UnknownName {
        /// The row that links.
        row: usize,
        /// The link, by its entry.
        link: String,
        /// The name.
        name: RunName,
    },
    /// A link names a row of another book than the link points into: the key of that
    /// row would be read in the grammar of the link's book and could be the key of
    /// another document.
    NameOtherBook {
        /// The row that links.
        row: usize,
        /// The link, by its entry.
        link: String,
        /// The name.
        name: RunName,
        /// The book of the row named.
        book: BookName,
        /// The book the link points into.
        target: BookName,
    },
}

impl PlanProblem {
    /// The first row the problem is about, which the problems of a plan are put in
    /// the order of.
    fn first_row(&self) -> usize {
        match self {
            Self::Doc { row, .. }
            | Self::ClaimKey { row, .. }
            | Self::NextKey { row, .. }
            | Self::UnknownName { row, .. }
            | Self::NameOtherBook { row, .. } => *row,
            Self::KeyTwice { rows, .. } | Self::NameTwice { rows, .. } => {
                rows.iter().copied().min().unwrap_or(0)
            }
        }
    }
}

/// `2 and 5`, `2, 5 and 7`.
fn rows_text(rows: &TwoOrMore<usize>) -> String {
    let rows: Vec<String> = rows.iter().map(ToString::to_string).collect();
    match rows.as_slice() {
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    }
}

impl fmt::Display for PlanProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Doc { row, problem } => write!(f, "row {row}: {problem}"),
            Self::ClaimKey { row, problem } => write!(f, "row {row}: {problem}"),
            Self::NextKey { row, problem } => write!(f, "row {row}: {problem}"),
            Self::KeyTwice { rows, key } => {
                write!(f, "rows {} ask for the key `{}`", rows_text(rows), key.form)
            }
            Self::NameTwice { rows, name } => write!(
                f,
                "rows {} are {} named `{name}`",
                rows_text(rows),
                if rows.len() == 2 { "both" } else { "all" }
            ),
            Self::UnknownName { row, link, name } => write!(
                f,
                "row {row} links `{link}` to `{name}`, which no row of the run is named"
            ),
            Self::NameOtherBook {
                row,
                link,
                name,
                book,
                target,
            } => write!(
                f,
                "row {row} links `{link}` to `{name}`, a document of the book `{book}`, not of `{target}`"
            ),
        }
    }
}

impl std::error::Error for PlanProblem {}

/// One row of a plan: what was asked, the identity it was given, the key it is at
/// now, the key it was first planned at, how many keys it has tried (the first
/// included), and the document made of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedRow {
    request: NewRequest,
    id: DocId,
    key: NewKey,
    first: DocKey,
    tries: u32,
    new: NewDoc,
}

impl PlannedRow {
    /// Whether the row links to the row named `name`, in any link to documents.
    fn names(&self, name: &RunName) -> bool {
        names_in(&self.request).any(|named| named == name)
    }
}

/// The names of rows a request links to, in every link to documents.
fn names_in(request: &NewRequest) -> impl Iterator<Item = &RunName> {
    request
        .entries
        .values()
        .filter_map(|value| match value {
            PlannedValue::Link(items) => Some(items),
            PlannedValue::Raw(_) => None,
        })
        .flatten()
        .filter_map(|item| match item {
            LinkItem::Named(name) => Some(name),
            LinkItem::Key(_) => None,
        })
}

/// A run of new documents that would all be valid: each made, in the order to write
/// them in, with what is needed to plan again when the file of a key is found there.
///
/// Made by [`Shelf::plan_new`] and moved on by [`Shelf::replan_after_taken`] only; it
/// holds no schema, so both are asked of the shelf it was planned on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlan {
    /// The rows, in the order of the run.
    rows: Vec<PlannedRow>,
    /// The order they are written in: positions in `rows`.
    order: Vec<usize>,
    /// What the adapters answered for the names the rows' links to artifacts write,
    /// so that a row made again is judged on the same answers.
    artifacts: ArtifactReadings,
}

impl NewPlan {
    /// How many documents the run makes.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the run makes none.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The documents, in the order they are to be written: a document a row links to
    /// by name before the row.
    pub fn docs(&self) -> impl Iterator<Item = &NewDoc> {
        (0..self.order.len()).filter_map(|at| self.doc_at(at))
    }

    /// The document written `at`th (from 0), when there is one.
    pub fn doc_at(&self, at: usize) -> Option<&NewDoc> {
        let row = self.order.get(at)?;
        self.rows.get(*row).map(|row| &row.new)
    }

    /// The documents of the run that no other of them stands on through a `Depends`
    /// link, in the order they are written: named to approve, they take every
    /// document of the run and nothing else. The same rule as
    /// [`order::heads`](crate::domain::shelf::order::heads), over the documents of the
    /// run, which are not in a shelf yet.
    pub fn heads(&self) -> Vec<DocKey> {
        let stood_on: BTreeSet<DocId> = self
            .docs()
            .flat_map(|new| new.doc().depends_links().map(|(_, target)| target))
            .collect();
        self.docs()
            .map(NewDoc::doc)
            .filter(|doc| !stood_on.contains(&doc.id()))
            .map(|doc| doc.key().clone())
            .collect()
    }

    /// The highest number a key of the run in `book` holds (0 when none).
    fn highest_in(&self, book: &BookName) -> u64 {
        self.rows
            .iter()
            .filter(|row| row.key.key.book == *book)
            .filter_map(|row| key_number(&row.key.key))
            .max()
            .unwrap_or(0)
    }
}

/// What comes of a key whose file was there when the document of the run was written
/// ([`Shelf::replan_after_taken`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAfterTaken {
    /// The row was given another key and the run was made again with it: write the
    /// same place of the order once more.
    Replanned(NewPlan),
    /// The key was asked for, so it is refused as it is.
    Asked,
    /// A row already written links to the row whose key was taken, by name, so its
    /// file holds a key that cannot be told apart from another document's now. Only a
    /// ring of names can come to this: the row named is written before the row that
    /// names it otherwise.
    Stuck {
        /// The row whose key was taken.
        row: usize,
        /// The row already written that links to it.
        written_row: usize,
    },
    /// The row made again with its new key is refused, which its new key should not
    /// cause: told as it is, not passed over.
    Refused {
        /// The row.
        row: usize,
        /// Why.
        problem: NewDocProblem,
    },
    /// The row has tried [`NEXT_KEY_TRIES`](super::NEXT_KEY_TRIES) keys, and a file was
    /// at each: give up. The answer names the first key the row was planned at and the
    /// last it tried.
    NoFreeKey(NoFreeKey),
    /// The key chosen has no next one.
    NoNextKey(NextKeyProblem),
}

/// What the rows of a run need to be made a [`NewDoc`] each: the request, the
/// identity and the key (none for a row whose key was refused, which is not made).
struct Seed<'a> {
    request: &'a NewRequest,
    id: DocId,
    key: Option<&'a NewKey>,
}

/// The entries of a request as the values of a front matter: a name of a row stands
/// as that row's key (a name no row has, which the plan refuses, stands as itself, so
/// that the entry stays whole until then).
fn entries_of(
    request: &NewRequest,
    keys_by_name: &BTreeMap<&RunName, String>,
) -> BTreeMap<String, RawValue> {
    request
        .entries
        .iter()
        .map(|(name, value)| {
            let raw = match value {
                PlannedValue::Raw(raw) => raw.clone(),
                PlannedValue::Link(items) => RawValue::List(
                    items
                        .iter()
                        .map(|item| match item {
                            LinkItem::Key(text) => text.clone(),
                            LinkItem::Named(named) => keys_by_name
                                .get(named)
                                .cloned()
                                .unwrap_or_else(|| named.to_string()),
                        })
                        .collect(),
                ),
            };
            (name.clone(), raw)
        })
        .collect()
}

/// Two or more rows of a list of them.
fn twice(rows: Vec<usize>) -> Option<TwoOrMore<usize>> {
    let mut rows = rows.into_iter();
    let first = rows.next()?;
    let second = rows.next()?;
    Some(TwoOrMore::new(first, second, rows.collect()))
}

/// The order to write the rows in: the lowest row first among those whose names point
/// at rows written already; when every row left waits for another (a ring of names),
/// the lowest of them goes, so a ring stays in the order of the run.
fn write_order(targets: &[Vec<usize>]) -> Vec<usize> {
    let mut written = vec![false; targets.len()];
    let mut order = Vec::with_capacity(targets.len());
    while order.len() < targets.len() {
        let ready = |at: &usize| {
            targets[*at]
                .iter()
                .all(|target| target == at || written[*target])
        };
        let next = (0..targets.len())
            .filter(|at| !written[*at])
            .find(ready)
            .or_else(|| (0..targets.len()).find(|at| !written[*at]));
        let Some(next) = next else { break };
        written[next] = true;
        order.push(next);
    }
    order
}

impl Shelf {
    /// Plan a run of new documents on this shelf, each with its identity, with the
    /// answers `artifacts` of the adapters for what [`Shelf::new_artifact_asks`]
    /// asked of the requests together.
    ///
    /// The one place a set of new documents is decided: their keys (asked for or
    /// chosen), the documents the names stand for, whether each would be valid and
    /// what each link comes to, resolved in this shelf's index with the documents of
    /// the run added to it, so that a link to a row later in the run, or in a ring, is
    /// resolved as it will be when the files are read back. See the module page for
    /// the steps.
    ///
    /// A key that is chosen is the number after the highest any file of its book
    /// writes, and then the next, row by row in the order given, passing over the keys
    /// the run asks for and the keys files write ([`Shelf::next_key`] is the same for
    /// one). Every row that is refused is in the answer, and one is enough to plan
    /// nothing.
    // The problems are the answer, read once per run; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn plan_new(
        &self,
        requests: Vec<(NewRequest, DocId)>,
        artifacts: &ArtifactReadings,
    ) -> Result<NewPlan, NonEmptyVec<PlanProblem>> {
        let (requests, ids): (Vec<NewRequest>, Vec<DocId>) = requests.into_iter().unzip();
        let mut problems: Vec<PlanProblem> = Vec::new();
        let mut keys: Vec<Option<NewKey>> = vec![None; requests.len()];

        // 1. The keys asked for.
        let mut claimed: BTreeMap<DocKey, Vec<usize>> = BTreeMap::new();
        for (at, request) in requests.iter().enumerate() {
            let Some(text) = &request.key else { continue };
            match self.claim_key(&request.book, text) {
                Ok(key) => {
                    claimed
                        .entry(key.key.clone())
                        .or_default()
                        .push(request.row);
                    keys[at] = Some(key);
                }
                Err(problem) => problems.push(PlanProblem::ClaimKey {
                    row: request.row,
                    problem,
                }),
            }
        }
        for (key, rows) in &claimed {
            if let Some(rows) = twice(rows.clone()) {
                problems.push(PlanProblem::KeyTwice {
                    rows,
                    key: key.clone(),
                });
            }
        }

        // 2. The keys chosen.
        let mut next: BTreeMap<BookName, u64> = BTreeMap::new();
        for (at, request) in requests.iter().enumerate() {
            if request.key.is_some() {
                continue;
            }
            let counter = next
                .entry(request.book.clone())
                .or_insert_with(|| self.highest_written(&request.book).saturating_add(1));
            match self.chosen_key(&request.book, counter, &claimed) {
                Ok(key) => keys[at] = Some(key),
                Err(problem) => problems.push(PlanProblem::NextKey {
                    row: request.row,
                    problem,
                }),
            }
        }

        // 3. The names.
        let mut named: BTreeMap<&RunName, Vec<usize>> = BTreeMap::new();
        for (at, request) in requests.iter().enumerate() {
            if let Some(name) = &request.name {
                named.entry(name).or_default().push(at);
            }
        }
        for (name, ats) in &named {
            let rows: Vec<usize> = ats.iter().map(|at| requests[*at].row).collect();
            if let Some(rows) = twice(rows) {
                problems.push(PlanProblem::NameTwice {
                    rows,
                    name: (*name).clone(),
                });
            }
        }
        for request in &requests {
            let book = self.schema.book(&request.book);
            for (link, value) in &request.entries {
                let PlannedValue::Link(items) = value else {
                    continue;
                };
                for name in items.iter().filter_map(|item| match item {
                    LinkItem::Named(name) => Some(name),
                    LinkItem::Key(_) => None,
                }) {
                    let ats = named.get(name).map_or(&[][..], Vec::as_slice);
                    let [at] = ats else {
                        // None: no row has the name. Several: told as `NameTwice`.
                        if ats.is_empty() {
                            problems.push(PlanProblem::UnknownName {
                                row: request.row,
                                link: link.clone(),
                                name: name.clone(),
                            });
                        }
                        continue;
                    };
                    let target = book
                        .and_then(|book| book.links().get(link.as_str()))
                        .map(|kind| &kind.target);
                    if let Some(target) = target
                        && *target != requests[*at].book
                    {
                        problems.push(PlanProblem::NameOtherBook {
                            row: request.row,
                            link: link.clone(),
                            name: name.clone(),
                            book: requests[*at].book.clone(),
                            target: target.clone(),
                        });
                    }
                }
            }
        }

        // 4 and 5. The judgement, and the resolution.
        let made = {
            let seeds: Vec<Seed<'_>> = requests
                .iter()
                .zip(&ids)
                .zip(&keys)
                .map(|((request, id), key)| Seed {
                    request,
                    id: *id,
                    key: key.as_ref(),
                })
                .collect();
            self.realize(&seeds, artifacts)
        };
        let mut rows = Vec::with_capacity(requests.len());
        for (((request, id), key), made) in requests.into_iter().zip(ids).zip(keys).zip(made) {
            match (key, made) {
                (Some(key), Some(Ok(new))) => rows.push(PlannedRow {
                    first: key.key.clone(),
                    tries: 1,
                    request,
                    id,
                    key,
                    new,
                }),
                (_, Some(Err(problem))) => problems.push(PlanProblem::Doc {
                    row: request.row,
                    problem,
                }),
                // A row with no key was refused above, and is not made.
                (_, _) => {}
            }
        }

        // 6. Every row refused.
        problems.sort_by_key(PlanProblem::first_row);
        if let Ok(problems) = NonEmptyVec::from_vec(problems) {
            return Err(problems);
        }

        let by_name: BTreeMap<&RunName, usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(at, row)| row.request.name.as_ref().map(|name| (name, at)))
            .collect();
        let targets: Vec<Vec<usize>> = rows
            .iter()
            .map(|row| {
                names_in(&row.request)
                    .filter_map(|name| by_name.get(name).copied())
                    .collect()
            })
            .collect();
        let order = write_order(&targets);
        Ok(NewPlan {
            rows,
            order,
            artifacts: artifacts.clone(),
        })
    }

    /// What comes of the key of the document written `at`th (from 0 in the order of
    /// [`NewPlan::docs`]), whose file was there when it was written: a fact learnt
    /// after the shelf was read (a run beside this one wrote the file, or its front
    /// matter cannot be read, so the shelf shows no key for it).
    ///
    /// - A key that was asked for is not chosen again: [`PlanAfterTaken::Asked`].
    /// - A row gives up after it has tried [`NEXT_KEY_TRIES`](super::NEXT_KEY_TRIES)
    ///   keys (its first included), each row counting its own: a run that meets
    ///   another run at many of its rows, each once, is not stopped by their number.
    ///   [`PlanAfterTaken::NoFreeKey`] names the first key that row was planned at and
    ///   the last it tried.
    /// - A row already written links to the row by name: [`PlanAfterTaken::Stuck`].
    ///   Only a ring of names comes to this; a row another links to by name is
    ///   written before it.
    /// - Otherwise only that row is given another key, the number after the highest
    ///   any key of the run holds in its book (rows written, rows not written and keys
    ///   asked for all counted), and the run is made again: the rows that name it link
    ///   to its new key, and every link of the run is resolved again.
    ///   [`PlanAfterTaken::Replanned`] holds the new plan, and the same place of its
    ///   order is written again. The keys of the other rows stand: if one of them was
    ///   taken too, it meets this question when it is written. A key with no next one
    ///   is [`PlanAfterTaken::NoNextKey`].
    ///
    /// The one place this is decided. The shelf is not read again, so a key it
    /// chooses is free as the shelf knows it, as [`Shelf::next_key`]'s is. A place
    /// that is not in the order is answered as [`PlanAfterTaken::Asked`]: nothing is
    /// chosen again for it.
    pub fn replan_after_taken(&self, mut plan: NewPlan, at: usize) -> PlanAfterTaken {
        let Some(&at_row) = plan.order.get(at) else {
            return PlanAfterTaken::Asked;
        };
        let Some(row) = plan.rows.get(at_row) else {
            return PlanAfterTaken::Asked;
        };
        let search = KeySearch {
            first: row.first.clone(),
            current: row.key.clone(),
            tries: row.tries,
            highest: plan.highest_in(&row.key.key.book),
        };
        let moved = match self.key_after_taken(search) {
            AfterTaken::Asked => return PlanAfterTaken::Asked,
            AfterTaken::NoFreeKey(problem) => return PlanAfterTaken::NoFreeKey(problem),
            AfterTaken::Next(search) => Ok(search),
            AfterTaken::NoNextKey(problem) => Err(problem),
        };
        let stuck = row.request.name.as_ref().and_then(|name| {
            plan.order[..at]
                .iter()
                .filter_map(|written| plan.rows.get(*written))
                .find(|written| written.names(name))
        });
        if let Some(written) = stuck {
            return PlanAfterTaken::Stuck {
                row: row.request.row,
                written_row: written.request.row,
            };
        }
        let moved = match moved {
            Ok(search) => search,
            Err(problem) => return PlanAfterTaken::NoNextKey(problem),
        };

        if let Some(row) = plan.rows.get_mut(at_row) {
            row.key = moved.current;
            row.tries = moved.tries;
        }
        let made = {
            let seeds: Vec<Seed<'_>> = plan
                .rows
                .iter()
                .map(|row| Seed {
                    request: &row.request,
                    id: row.id,
                    key: Some(&row.key),
                })
                .collect();
            self.realize(&seeds, &plan.artifacts)
        };
        for (row, made) in plan.rows.iter_mut().zip(made) {
            match made {
                Some(Ok(new)) => row.new = new,
                Some(Err(problem)) => {
                    return PlanAfterTaken::Refused {
                        row: row.request.row,
                        problem,
                    };
                }
                // Every row of a plan has a key.
                None => {}
            }
        }
        PlanAfterTaken::Replanned(plan)
    }

    /// The next free key of a prefixed book for a row that asks for none: from
    /// `next` on, the first number whose key neither the run asks for nor a file
    /// writes. `next` is left after the key given.
    fn chosen_key(
        &self,
        book: &BookName,
        next: &mut u64,
        asked: &BTreeMap<DocKey, Vec<usize>>,
    ) -> Result<NewKey, NextKeyProblem> {
        let schema = self
            .schema
            .book(book)
            .ok_or_else(|| NextKeyProblem::UnknownBook { book: book.clone() })?;
        loop {
            let key = numbered(schema, book, *next)?;
            *next = next.saturating_add(1);
            if !asked.contains_key(&key.key) && self.key_writers(&key.key).writers().is_none() {
                return Ok(key);
            }
        }
    }

    /// The row of each seed that has a key made a [`NewDoc`] by the one function
    /// [`Shelf::check_new`] calls (steps 4 and 5 of the module page, and a run made
    /// again): first judged against the shelf's index, then, when every row is valid,
    /// again against the index with the run's documents in it, so that a link to a
    /// row of the run is resolved whichever comes first. A hash covers no link, so the
    /// second making changes only the states of links.
    // The problems are the answer, read once per run; it is not a hot path.
    #[allow(clippy::result_large_err)]
    fn realize(
        &self,
        seeds: &[Seed<'_>],
        artifacts: &ArtifactReadings,
    ) -> Vec<Option<Result<NewDoc, NewDocProblem>>> {
        let keys_by_name: BTreeMap<&RunName, String> = seeds
            .iter()
            .filter_map(|seed| {
                let name = seed.request.name.as_ref()?;
                let key = seed.key?;
                Some((name, key.key.form.to_string()))
            })
            .collect();
        let make = |index: &KeyIndex| -> Vec<Option<Result<NewDoc, NewDocProblem>>> {
            seeds
                .iter()
                .map(|seed| {
                    seed.key.map(|key| {
                        self.new_doc_in(
                            index,
                            seed.id,
                            key,
                            entries_of(seed.request, &keys_by_name),
                            seed.request.body.clone(),
                            artifacts,
                        )
                    })
                })
                .collect()
        };
        let alone = make(&self.index);
        if alone.iter().flatten().any(Result::is_err) {
            return alone;
        }
        let mut index = self.index.clone();
        for new in alone.iter().flatten().flatten() {
            let doc = new.doc();
            index.writers.add(
                doc.key().clone(),
                KeyWriter::new(doc.origin().clone(), Some(doc.id())),
            );
            index.valid.insert(doc.id(), *doc.hash());
        }
        make(&index)
    }
}
