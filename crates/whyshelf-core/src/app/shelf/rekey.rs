//! The use case that gives a document a new key: `rekey`, and what became of the
//! links that wrote the old one.
//!
//! ```text
//!   load, once ─────────────► the shelf and the ledger as they are
//!   the references ─────────► the target, and a word (--follow, --stay, --both) for each
//!                             document that writes the old key in a link
//!   rekey::plan ────────────► a RekeyPlan, or a RekeyProblem that names every document not
//!                             decided (nothing is written; the domain decides, here only
//!                             the references are read)
//!   DocSource::forms ───────► each file the plan writes, as it was read
//!   RekeyPlan::entries_of ──► the entries each file is to have
//!   form_write ─────────────► the text of each in the form, or why not (the rule of `fmt`)
//!   DocWrite::in_the_way ───► something at the new file: refused
//!   RekeyPlan::confirm ─────► the files, read as the adapter says, built by Shelf::build
//!        │                    and held against the plan, or PlanDiffers (nothing is written)
//!        ├─ --dry-run ──────► the answer, with nothing written
//!        └─ RunWrites::edit ► create ► replace.. ► persist ► remove ► persist_gone, or none
//!   load, assess ───────────► the standing of the target and of each document whose link
//!                             was read, after the write: what became of the approvals
//! ```
//!
//! The ledger is not written: a decision pins the identity of what it stood on, and an
//! identity does not change, so there is nothing in it to correct; whether each approval
//! holds is what [`assess`](crate::domain::shelf::freshness::assess) says of the files as
//! they are after the write, and the answer carries that and nothing more.
//!
//! Nothing here decides a key, which link means which document, whether a file may be
//! written in the form or whether the files read as planned: [`plan_rekey`],
//! [`form_write`] and [`RekeyPlan::confirm`] say, and this use case does what they say,
//! in the order above. Every write is [`RunWrites::edit`], which is all or none.

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::shelf::build::{DocReading, DropNames};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::rekey::{
    Meaning, RekeyAsk, RekeyPlan, Rewriting, Side, plan as plan_rekey,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::fmt::{FormWrite, Held, form_write, parsed};
use super::load::{View, load};
use super::ports::{DocSource, DocWrite, ShelfPort, ShelfWrite, port};
use super::read::identity;
use super::report::{Names, Row};
use super::run_writes::{Create, Edit, Edited, Remove, Replace, RunWrites};
use super::state;

/// What `rekey` is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyRequest {
    /// The document that takes the new key: a reference that names it (a key that two
    /// files write names two documents, so the identity or a short one names it).
    pub target: Reference,
    /// The new key, as typed; `None` is the next key of the book.
    pub key: Option<String>,
    /// The documents that write the old key in a link, and whose link means the
    /// target: it is rewritten to the new key.
    pub follow: Vec<Reference>,
    /// The documents whose link means the other document that writes the old key: it is
    /// left as it is.
    pub stay: Vec<Reference>,
    /// The documents whose link means both: it keeps the old key and gains the new one.
    pub both: Vec<Reference>,
    /// Plan and check, and write nothing.
    pub dry_run: bool,
}

/// The document that took the new key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rekeyed {
    /// Its identity, which stays.
    pub id: DocId,
    /// The key it had.
    pub from_key: DocKey,
    /// The key it has.
    pub to_key: DocKey,
    /// The file it was in.
    pub from_file: RelPath,
    /// The file it is in.
    pub to_file: RelPath,
}

/// What `rekey` did, or with `--dry-run` would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyReport {
    /// The document that took the new key.
    pub moved: Rekeyed,
    /// The documents whose links were rewritten to the new key, with the items of each
    /// and why they follow the target.
    pub rewritten: Vec<Rewriting>,
    /// The documents whose links stay with the old key, and why.
    pub stayed: Vec<Rewriting>,
    /// The documents whose links keep the old key and gained the new one.
    pub both: Vec<Rewriting>,
    /// The documents whose body names the old key, the document that took the new key
    /// among them when its own body does: not changed (the body is covered by the
    /// hash), for a person to read.
    pub mentions: Vec<DocId>,
    /// The target and each document that writes the old key in a link, as the files are
    /// after the write: whether each approval holds. Empty with `--dry-run`, which
    /// writes nothing to read them from.
    pub approvals: Vec<Row>,
    /// The files written; none with `--dry-run`.
    pub written: Option<Edited>,
    /// The keys of the valid documents after the write (before it, with `--dry-run`).
    pub names: Names,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Give the document `request.target` a new key, and rewrite the links that mean it
    /// (the module page).
    ///
    /// Every document that writes the old key in a link must be decided, by the
    /// approval that stands about it or by a word in the request; any that is not is
    /// refused with all of them listed ([`ShelfError::Rekey`]), and nothing is written.
    /// The files are written all or none ([`ShelfError::PartWay`] and
    /// [`ShelfError::NotDurable`] are the ways that is not so, told with the files they
    /// leave). A document named in two words is [`ShelfError::SaidTwice`].
    pub async fn rekey(&mut self, request: RekeyRequest) -> Result<RekeyReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let target = identity(&loaded, &request.target)?;
        let mut words = BTreeMap::new();
        for (side, references) in [
            (Side::Follow, &request.follow),
            (Side::Stay, &request.stay),
            (Side::Both, &request.both),
        ] {
            for reference in references {
                let doc = identity(&loaded, reference)?;
                if words.insert(doc, side).is_some() {
                    return Err(ShelfError::SaidTwice { doc });
                }
            }
        }
        let ask = RekeyAsk {
            target,
            key: request.key.clone(),
            words,
        };
        let mut plan = plan_rekey(&loaded.shelf, &loaded.ledger, &ask)?;
        let moved = Rekeyed {
            id: plan.target().id,
            from_key: plan.target().key.clone(),
            to_key: plan.new_key().key().clone(),
            from_file: plan.target().file.clone(),
            to_file: plan.new_file().clone(),
        };

        // Nothing is written before every file is read, rendered and checked.
        if let Some(path) = self
            .ports
            .doc_writer()
            .in_the_way(plan.new_file())
            .await
            .map_err(port(ShelfPort::DocWrite))?
        {
            return Err(ShelfError::FileTaken { path });
        }
        let schema = loaded.shelf.schema();
        let forms = self
            .ports
            .docs()
            .forms(state::Side::After, &schema.ranges())
            .await
            .map_err(port(ShelfPort::Docs))?;
        let readings: Vec<DocReading> = forms.iter().map(|found| found.reading.clone()).collect();
        let files = plan.files();
        let mut planned = Vec::new();
        let mut made = None;
        let mut removed = None;
        let mut replace = Vec::new();
        let none = DropNames::none();
        for file in std::iter::once(&files.from).chain(&files.rewritten) {
            let Some(found) = forms.iter().find(|found| found.reading.origin() == file) else {
                return Err(ShelfError::Changed { file: file.clone() });
            };
            let (_, entries, body) =
                parsed(schema, found.reading.clone()).map_err(|left| ShelfError::NotInForm {
                    file: left.file,
                    why: left.why,
                })?;
            let moves = *file == files.from;
            if moves {
                // The target's own body: an invalid document has none in the shelf.
                plan.mention_own_body(&body);
            }
            let origin = if moves { &files.to } else { file };
            let held = Held {
                file,
                read: &found.read,
                comment: found.comment,
            };
            let written = match form_write(
                schema,
                &none,
                self.ports.docs(),
                held,
                origin,
                &plan.entries_of(file, &entries),
                &body,
            )
            .await?
            {
                FormWrite::Leave(why) => {
                    return Err(ShelfError::NotInForm {
                        file: file.clone(),
                        why,
                    });
                }
                FormWrite::InForm(written) | FormWrite::Write(written) => written,
            };
            planned.push(written.reads_as.clone());
            if moves {
                made = Some(Create {
                    path: files.to.clone(),
                    entries: written.form,
                    body,
                    text: written.text,
                });
                removed = Some(Remove {
                    path: file.clone(),
                    read: found.read.clone(),
                });
            } else if written.text != found.read {
                replace.push(Replace {
                    path: file.clone(),
                    read: found.read.clone(),
                    text: written.text,
                });
            }
        }
        let confirmed = plan.confirm(&loaded.shelf, readings, planned, &loaded.artifacts)?;

        if request.dry_run {
            return Ok(report(&plan, moved, Vec::new(), None, loaded.names()));
        }
        // The target's file is the first of the files, so both are there.
        let (Some(create), Some(remove)) = (made, removed) else {
            return Err(ShelfError::Changed {
                file: files.from.clone(),
            });
        };
        let written = RunWrites::edit(
            &self.ports,
            &confirmed,
            Edit {
                create,
                replace,
                remove,
            },
        )
        .await?;

        let after = load(&self.ports).await?;
        // The answer tells approvals, not covers over artifacts: no listing is asked.
        let assessment = after.assess(&crate::domain::shelf::artifact::Listings::NotAsked);
        let view = View {
            loaded: &after,
            assessment: &assessment,
        };
        let mut ids = vec![moved.id];
        for writer in plan.writers() {
            if let Some(id) = writer.id()
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        let approvals = ids.into_iter().map(|id| view.row(id)).collect();
        Ok(report(
            &plan,
            moved,
            approvals,
            Some(written),
            after.names(),
        ))
    }
}

/// The answer: what each document that writes the old key in a link came to, by its
/// meaning.
fn report(
    plan: &RekeyPlan,
    moved: Rekeyed,
    approvals: Vec<Row>,
    written: Option<Edited>,
    names: Names,
) -> RekeyReport {
    let of = |wanted: fn(&Meaning) -> bool| -> Vec<Rewriting> {
        plan.writers()
            .iter()
            .filter(|writer| wanted(&writer.meaning()))
            .cloned()
            .collect()
    };
    RekeyReport {
        moved,
        rewritten: of(|meaning| matches!(meaning, Meaning::Follows(_))),
        stayed: of(|meaning| matches!(meaning, Meaning::Stays(_))),
        both: of(|meaning| matches!(meaning, Meaning::Both)),
        mentions: plan.mentions().to_vec(),
        approvals,
        written,
        names,
    }
}
