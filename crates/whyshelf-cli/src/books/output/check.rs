//! The answer of `check` ([`BooksCheckOut`]): the books counted, what is wrong around
//! the documents, and the gaps, names and exemptions it lists; and the lines it writes
//! on stderr beside it ([`check_warnings`], [`check_notes`]).

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{Awaiting, BooksCheck, Names};
use whyshelf_core::domain::shelf::doc::KeyCollision;
use whyshelf_core::domain::shelf::ledger::{SameMeaningEdge, ShortSameMeaning};
use whyshelf_core::domain::shelf::schema::CoverSeverity;
use whyshelf_core::domain::shelf::untold::untold_causes;
use whyshelf_core::domain::words::{Join, Mark, listed};

use super::cause::{absent_detail, moved_detail, unneeded_word};
use super::cover::{
    CoverLinkOut, ExemptArtifactOut, RepeatedArtifactOut, UncoveredArtifactOut,
    UnneededArtifactOut, artifact_lists, cover_link, exemption_lines, warn_lines,
};
use super::untold::{CHECK_UNTOLD, untold_line};
use super::words::{plural, ran_by, schema_problem};
use super::{
    AbsentOut, DocRef, ExemptOut, HealthOut, MovedNameOut, Namer, RepeatedOut, UnknownOut,
    UnneededOut, standing,
};

/// One book, counted.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookOut {
    /// The name.
    pub name: String,
    /// The directory.
    pub dir: String,
    /// Valid documents.
    pub valid: usize,
    /// Files under it that are no valid document.
    pub invalid: usize,
    /// How many files in its range it skipped, which are no document of it
    /// (`without_front_matter = "skip"`); the length of `skipped_files`.
    pub skipped: usize,
    /// The files it skipped, each with why, in the order they were read. A file out
    /// of the range (`include`, `exclude`) is not listed: it is not read.
    pub skipped_files: Vec<SkippedOut>,
}

/// A file a book skipped.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkippedOut {
    /// The file, relative to the project root.
    pub path: String,
    /// Why: `no_front_matter`.
    pub why: &'static str,
}

/// The answer of `check`.
///
/// Beside it, on stderr, `check` writes one line per entry of `uncovered` of
/// strength `warn` ([`check_warnings`]), since such a gap leaves the check clean
/// and a run that shows the answer only when it fails would show it nowhere.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BooksCheckOut {
    /// Whether nothing is wrong.
    pub clean: bool,
    /// The problems of the declaration; when there are any, nothing else was read.
    pub schema_problems: Vec<String>,
    /// The books, in the order the declaration writes them.
    pub books: Vec<BookOut>,
    /// What is wrong around the documents.
    pub health: Option<HealthOut>,
    /// The documents a covering link is expected to reach and does not, each with
    /// the strength of the link; which strength makes `clean` false is
    /// [`BooksCheck::is_clean`]'s rule.
    pub uncovered: Vec<UncoveredOut>,
    /// The names of pending and approved documents with nothing there (for the
    /// space `file`, paths that name no file); they leave `clean` as it is, since a
    /// name may be of evidence not written yet.
    pub absent: Vec<AbsentOut>,
    /// The names of pending and approved documents their adapter cannot tell about
    /// now; they leave `clean` as it is, and no approval can be made on one.
    pub unknown: Vec<UnknownOut>,
    /// The names links still write that an approval pinned and people declared
    /// moved since (`judge moved`): the approval is stale, and the link is to write
    /// `moved_to`. They leave `clean` as it is.
    pub moved: Vec<MovedNameOut>,
    /// The exemptions that stand and take the place of a gap (`judge exempt`), each
    /// with who, when and why: an exempt document is not in `uncovered` for that
    /// link, and leaves `clean` as it is whatever the strength.
    pub exempt: Vec<ExemptOut>,
    /// The exemptions that stand and are no longer needed, each with why (the
    /// notice `exemption_unneeded`); they leave `clean` as it is.
    pub exemption_unneeded: Vec<UnneededOut>,
    /// The documents two or more exemptions that stand exempt from one link (the
    /// notice `exemption_repeated`), as merged branches can leave them; they leave
    /// `clean` as it is.
    pub exemption_repeated: Vec<RepeatedOut>,
    /// The approvals whose content fewer different names read than their rule of
    /// who may decide asks for, at any freshness (`standing` is `awaiting` when it is
    /// fresh): another name is to approve the same content. They leave `clean` as it
    /// is. Left out when there is none, as there never is where no rule asks for more
    /// than one name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub awaiting: Vec<AwaitingEntryOut>,
    /// The edits declared to keep the meaning by fewer different names than they
    /// need, that a pin of an approval that stands meets: they join nothing until
    /// more names declare them. They leave `clean` as it is. Left out when there is
    /// none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub same_meaning_short: Vec<SameMeaningShortOut>,
    /// The names of artifacts a covering link to artifacts is expected to reach and
    /// nobody does, each with the strength of the link (the artifact counterpart of
    /// `uncovered`); which strength makes `clean` false is [`BooksCheck::is_clean`]'s
    /// rule.
    pub artifact_uncovered: Vec<UncoveredArtifactOut>,
    /// The exemptions of artifacts that stand and take the place of a gap
    /// (`judge exempt --artifact`), each with who, when and why: an exempt name is
    /// not in `artifact_uncovered` for that link, and leaves `clean` as it is
    /// whatever the strength.
    pub artifact_exempt: Vec<ExemptArtifactOut>,
    /// The exemptions of artifacts that stand and are no longer needed, each with
    /// why (`reached`, `not_declared`, `no_cover`, `other_space`, `not_listed`); they
    /// leave `clean` as it is. A name that `judge moved` moved is `not_listed` under
    /// the exemption of its old name.
    pub artifact_exemption_unneeded: Vec<UnneededArtifactOut>,
    /// The artifacts two or more exemptions that stand exempt from one link; they
    /// leave `clean` as it is.
    pub artifact_exemption_repeated: Vec<RepeatedArtifactOut>,
    /// Each covering link to artifacts: its selection, and whether the listing of the
    /// names it takes is the whole (`completeness`: `all`, `partial`, `cannot_list`).
    /// A link of strength `error` whose listing is not the whole, or whose selection
    /// takes no name, makes `clean` false: a gap cannot be ruled out, or the cover
    /// expects nothing.
    pub artifact_cover: Vec<CoverLinkOut>,
    /// The keys that two or more files write, one entry for each key, with every
    /// file and the identity of each that could be read. Each of the files is
    /// invalid (`health.invalid` says so with a problem that names the others), so
    /// the entries leave `clean` as it is. Which file is to take another key is not
    /// said: it is for a person to decide. Left out when there is none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<CollisionOut>,
}

/// A key that two or more files write.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CollisionOut {
    /// The key (`book:KEY`).
    pub key: String,
    /// The files that write it, in the order of their paths.
    pub files: Vec<CollisionFileOut>,
    /// A sentence for a person.
    pub detail: String,
}

/// A file that writes a key another file writes too.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CollisionFileOut {
    /// The file, relative to the project root.
    pub path: String,
    /// The identity (UUID v7) the file carries; null when it could not be read.
    pub id: Option<String>,
}

/// The sentence of a key that two or more files write: the key, and every file with
/// the identity it carries, or that it carries none that can be read.
fn collision_detail(collision: &KeyCollision) -> String {
    let files = collision.writers().iter().map(|writer| match writer.id() {
        Some(id) => format!("{} (id {id})", writer.file()),
        None => format!("{} (no identity that can be read)", writer.file()),
    });
    format!(
        "the key `{}` is written by {} files: {}",
        collision.key(),
        collision.writers().len(),
        listed(files, Mark::Plain, Join::Semicolon)
    )
}

/// An approval whose content awaits the approvals of other names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AwaitingEntryOut {
    /// The document.
    pub doc: DocRef,
    /// Where it stands: `awaiting` when it is fresh, else the level of its
    /// freshness (`expired`, `stale`, `broken`).
    pub standing: &'static str,
    /// The names whose reading counts now, each once.
    pub readers: Vec<String>,
    /// How many different names the rule asks for.
    pub need: usize,
    /// A sentence for a person.
    pub detail: String,
}

/// An edit declared to keep the meaning by fewer different names than its
/// declarations need.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SameMeaningShortOut {
    /// The document the edit is of; null for an artifact.
    pub doc: Option<DocRef>,
    /// The name of the artifact the edit is of; null for a document.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The two hashes the declarations say mean the same, the smaller first.
    pub hashes: Vec<String>,
    /// The declarations (record ids), in the order of identity.
    pub declared: Vec<String>,
    /// The names that declared it, each once.
    pub names: Vec<String>,
    /// How many different names it needs.
    pub need: usize,
    /// A sentence for a person.
    pub detail: String,
}

/// A document a covering link is expected to reach and does not.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UncoveredOut {
    /// The document.
    pub doc: DocRef,
    /// The book whose link is expected to reach it.
    pub book: String,
    /// The link.
    pub link: String,
    /// How strongly the link expects to reach it: `warn` or `error`.
    pub severity: &'static str,
}

/// The lines `check` writes on stderr: one per document a covering link of
/// strength `warn` does not reach, in the order of `uncovered`, then one per cause
/// of names that cannot be told now, in the order the causes first appear in
/// `unknown` (the names of one cause are one line, as [`untold_causes`] gathers
/// them, naming the document or documents that write them; the answer on stdout
/// keeps every name), then one per name with nothing
/// there, in the order of `absent`, then one per name a link still writes
/// that was declared moved since an approval pinned it, in the order of `moved`,
/// then one per exemption no longer needed, in the order of `exemption_unneeded`,
/// then one per document exempt from one link more than once, in the order of
/// `exemption_repeated`; and, for the covering links to artifacts, one per name a
/// link of strength `warn` does not reach, one per such link whose listing is not the
/// whole or whose selection takes no name, then one per exemption of an artifact no
/// longer needed (with the `judge withdraw` that withdraws it) and one per artifact
/// exempt from one link more than once. A gap of strength `error` has none: it fails
/// the check, and the answer is shown. None of them changes the exit.
pub fn check_warnings(check: &BooksCheck) -> Vec<String> {
    let BooksCheck::Checked {
        uncovered,
        unknown,
        absent,
        moved,
        unneeded,
        repeated,
        names,
        cover,
        artifacts,
        ..
    } = check
    else {
        return Vec::new();
    };
    let gaps = uncovered
        .iter()
        .filter(|u| u.severity == CoverSeverity::Warn)
        .map(|u| {
            let doc = u
                .key
                .as_ref()
                .map_or_else(|| u.doc.to_string(), ToString::to_string);
            format!(
                "warning: {doc} is reached by no {}.{} (cover = \"warn\")",
                u.book, u.link
            )
        });
    let untold = untold_causes(unknown)
        .into_iter()
        .map(|cause| untold_line(&cause, &CHECK_UNTOLD));
    let nothing = absent.iter().map(|a| {
        let doc = a
            .key
            .as_ref()
            .map_or_else(|| a.doc.to_string(), ToString::to_string);
        format!(
            "warning: {doc} names {} in {}, which has nothing behind it",
            a.artifact.name(),
            a.artifact.space()
        )
    });
    let moved_away = moved.iter().map(|m| {
        let doc = m
            .key
            .as_ref()
            .map_or_else(|| m.doc.to_string(), ToString::to_string);
        format!(
            "warning: {doc} names {} in {} under `{}`, which was declared moved to {}: write the new name",
            m.artifact.name(),
            m.artifact.space(),
            m.link,
            m.moved_to.name()
        )
    });
    let namer = Namer(names);
    let not_needed = unneeded.iter().map(|u| {
        let of = u.record.of();
        format!(
            "warning: {}; withdraw it with `judge withdraw {}`",
            namer.unneeded(Some(u.doc), of.book(), of.link(), u.record.id(), &u.why),
            u.record.id()
        )
    });
    let twice = repeated.iter().map(|r| {
        format!(
            "warning: {}, with `judge withdraw <id>`",
            namer.repeated(Some(r.doc), &r.book, &r.link, &r.exemptions)
        )
    });
    let artifact_gaps = warn_lines(artifacts, cover.links());
    let artifact_exemptions = exemption_lines(artifacts, &namer);
    gaps.chain(untold)
        .chain(nothing)
        .chain(moved_away)
        .chain(not_needed)
        .chain(twice)
        .chain(artifact_gaps)
        .chain(artifact_exemptions)
        .collect()
}

/// The lines `check` writes on stderr beside its answer, one per book that skipped
/// files, in the order of the books: `note: book spec skipped 3 files with no front
/// matter`; then one per approval that awaits the approvals of other names, in the
/// order of `awaiting`, and one per edit declared to keep the meaning by too few
/// names, in the order of `same_meaning_short`, each `note: <detail>`. None of them
/// is a failure, and the lines change no exit; they are there so that a run that
/// shows the answer only when it fails still says what was left out or waits.
pub fn check_notes(check: &BooksCheck) -> Vec<String> {
    let BooksCheck::Checked {
        books,
        awaiting,
        same_meaning_short,
        names,
        ..
    } = check
    else {
        return Vec::new();
    };
    let namer = Namer(names);
    let skipped = books
        .iter()
        .filter(|book| !book.skipped.is_empty())
        .map(|book| {
            let count = book.skipped.len();
            format!(
                "note: book {} skipped {count} file{} with no front matter",
                book.name,
                plural(count)
            )
        });
    let waits = awaiting
        .iter()
        .map(|a| format!("note: {}", namer.awaiting_detail(a)));
    let short = same_meaning_short
        .iter()
        .map(|s| format!("note: {}", namer.same_meaning_short_detail(s)));
    skipped.chain(waits).chain(short).collect()
}

impl Namer<'_> {
    /// The sentence of an approval that awaits other names.
    fn awaiting_detail(&self, awaiting: &Awaiting) -> String {
        format!(
            "{} is approved by {}, and its rule asks for {} different names: another name is to approve the same content",
            self.label(awaiting.doc),
            listed(awaiting.readers.iter(), Mark::Backtick, Join::And),
            awaiting.need
        )
    }

    /// The sentence of an edit declared to keep the meaning by too few names.
    fn same_meaning_short_detail(&self, short: &ShortSameMeaning) -> String {
        let (of, a, b) = match &short.of {
            SameMeaningEdge::Doc { id, a, b } => (self.label(*id), a.to_string(), b.to_string()),
            SameMeaningEdge::Artifact { artifact, a, b } => {
                (artifact.to_string(), a.to_string(), b.to_string())
            }
        };
        format!(
            "the edit of {of} between {a} and {b} is declared to keep the meaning by {}, and its rule asks for {} different names: it joins nothing until another name declares it",
            listed(short.names.iter(), Mark::Backtick, Join::And),
            short.need
        )
    }

    fn awaiting(&self, awaiting: &Awaiting) -> AwaitingEntryOut {
        AwaitingEntryOut {
            doc: self.doc(awaiting.doc),
            standing: standing(awaiting.standing),
            readers: awaiting
                .readers
                .iter()
                .map(|a| a.as_str().to_owned())
                .collect(),
            need: awaiting.need.count(),
            detail: self.awaiting_detail(awaiting),
        }
    }

    fn same_meaning_short(&self, short: &ShortSameMeaning) -> SameMeaningShortOut {
        let (doc, artifact, space, hashes) = match &short.of {
            SameMeaningEdge::Doc { id, a, b } => (
                Some(self.doc(*id)),
                None,
                None,
                vec![a.to_string(), b.to_string()],
            ),
            SameMeaningEdge::Artifact { artifact, a, b } => (
                None,
                Some(artifact.name().to_string()),
                Some(artifact.space().to_string()),
                vec![a.to_string(), b.to_string()],
            ),
        };
        SameMeaningShortOut {
            doc,
            artifact,
            space,
            hashes,
            declared: short.declared.iter().map(ToString::to_string).collect(),
            names: short.names.iter().map(|a| a.as_str().to_owned()).collect(),
            need: short.need.count(),
            detail: self.same_meaning_short_detail(short),
        }
    }
}

/// The answer of `check`.
pub fn books_check(check: &BooksCheck) -> BooksCheckOut {
    match check {
        BooksCheck::SchemaInvalid(problems) => BooksCheckOut {
            clean: false,
            schema_problems: problems.iter().map(schema_problem).collect(),
            books: Vec::new(),
            health: None,
            uncovered: Vec::new(),
            absent: Vec::new(),
            unknown: Vec::new(),
            moved: Vec::new(),
            exempt: Vec::new(),
            exemption_unneeded: Vec::new(),
            exemption_repeated: Vec::new(),
            awaiting: Vec::new(),
            same_meaning_short: Vec::new(),
            collisions: Vec::new(),
            artifact_uncovered: Vec::new(),
            artifact_exempt: Vec::new(),
            artifact_exemption_unneeded: Vec::new(),
            artifact_exemption_repeated: Vec::new(),
            artifact_cover: Vec::new(),
        },
        BooksCheck::Checked {
            books,
            health,
            uncovered,
            absent,
            unknown,
            moved,
            exempt,
            unneeded,
            repeated,
            awaiting,
            same_meaning_short,
            collisions,
            cover,
            artifacts,
            names,
        } => {
            let namer = Namer(names);
            let of_artifacts = artifact_lists(artifacts, &namer);
            BooksCheckOut {
                clean: check.is_clean(),
                schema_problems: Vec::new(),
                books: books
                    .iter()
                    .map(|b| BookOut {
                        name: b.name.to_string(),
                        dir: b.dir.as_rel().to_string(),
                        valid: b.tally.valid,
                        invalid: b.tally.invalid,
                        skipped: b.skipped.len(),
                        skipped_files: b
                            .skipped
                            .iter()
                            .map(|file| SkippedOut {
                                path: file.origin().to_string(),
                                why: file.why().word(),
                            })
                            .collect(),
                    })
                    .collect(),
                health: Some(Namer(&Names::default()).health(health)),
                uncovered: uncovered
                    .iter()
                    .map(|u| UncoveredOut {
                        doc: DocRef {
                            id: u.doc.to_string(),
                            key: u.key.as_ref().map(ToString::to_string),
                        },
                        book: u.book.to_string(),
                        link: u.link.to_string(),
                        severity: u.severity.word(),
                    })
                    .collect(),
                absent: absent
                    .iter()
                    .map(|a| AbsentOut {
                        doc: DocRef {
                            id: a.doc.to_string(),
                            key: a.key.as_ref().map(ToString::to_string),
                        },
                        link: a.link.to_string(),
                        path: a.artifact.name().to_string(),
                        name: a.artifact.name().to_string(),
                        space: a.artifact.space().to_string(),
                        detail: absent_detail(&a.artifact),
                    })
                    .collect(),
                unknown: unknown
                    .iter()
                    .map(|u| UnknownOut {
                        doc: DocRef {
                            id: u.doc.to_string(),
                            key: u.key.as_ref().map(ToString::to_string),
                        },
                        link: u.link.to_string(),
                        name: u.artifact.name().to_string(),
                        space: u.artifact.space().to_string(),
                        why: u.why.to_string(),
                    })
                    .collect(),
                moved: moved
                    .iter()
                    .map(|m| MovedNameOut {
                        doc: DocRef {
                            id: m.doc.to_string(),
                            key: m.key.as_ref().map(ToString::to_string),
                        },
                        link: m.link.to_string(),
                        name: m.artifact.name().to_string(),
                        space: m.artifact.space().to_string(),
                        moved_to: m.moved_to.name().to_string(),
                        declared: m.declared.iter().map(ToString::to_string).collect(),
                        detail: moved_detail(&m.link, &m.artifact, &m.moved_to),
                    })
                    .collect(),
                exempt: exempt
                    .iter()
                    .map(|e| ExemptOut {
                        doc: DocRef {
                            id: e.doc.to_string(),
                            key: e.key.as_ref().map(ToString::to_string),
                        },
                        book: e.record.of().book().to_string(),
                        link: e.record.of().link().to_string(),
                        severity: e.severity.word(),
                        exemption: e.record.id().to_string(),
                        actor: e.record.actor().as_str().to_owned(),
                        ran_by: ran_by(e.record.attribution()),
                        at: e.record.at().millis(),
                        reason: e.record.reason().as_str().to_owned(),
                    })
                    .collect(),
                exemption_unneeded: unneeded
                    .iter()
                    .map(|u| {
                        let of = u.record.of();
                        UnneededOut {
                            doc: DocRef {
                                id: u.doc.to_string(),
                                key: u.key.as_ref().map(ToString::to_string),
                            },
                            book: of.book().to_string(),
                            link: of.link().to_string(),
                            exemption: u.record.id().to_string(),
                            actor: u.record.actor().as_str().to_owned(),
                            ran_by: ran_by(u.record.attribution()),
                            at: u.record.at().millis(),
                            reason: u.record.reason().as_str().to_owned(),
                            why: unneeded_word(&u.why),
                            detail: namer.unneeded(
                                Some(u.doc),
                                of.book(),
                                of.link(),
                                u.record.id(),
                                &u.why,
                            ),
                        }
                    })
                    .collect(),
                exemption_repeated: repeated
                    .iter()
                    .map(|r| RepeatedOut {
                        doc: DocRef {
                            id: r.doc.to_string(),
                            key: r.key.as_ref().map(ToString::to_string),
                        },
                        book: r.book.to_string(),
                        link: r.link.to_string(),
                        exemptions: r.exemptions.iter().map(ToString::to_string).collect(),
                        detail: namer.repeated(Some(r.doc), &r.book, &r.link, &r.exemptions),
                    })
                    .collect(),
                awaiting: awaiting.iter().map(|a| namer.awaiting(a)).collect(),
                same_meaning_short: same_meaning_short
                    .iter()
                    .map(|s| namer.same_meaning_short(s))
                    .collect(),
                collisions: collisions
                    .iter()
                    .map(|collision| CollisionOut {
                        key: collision.key().to_string(),
                        files: collision
                            .writers()
                            .iter()
                            .map(|writer| CollisionFileOut {
                                path: writer.file().to_string(),
                                id: writer.id().map(|id| id.to_string()),
                            })
                            .collect(),
                        detail: collision_detail(collision),
                    })
                    .collect(),
                artifact_uncovered: of_artifacts.uncovered,
                artifact_exempt: of_artifacts.exempt,
                artifact_exemption_unneeded: of_artifacts.unneeded,
                artifact_exemption_repeated: of_artifacts.repeated,
                artifact_cover: cover.links().iter().map(cover_link).collect(),
            }
        }
    }
}
