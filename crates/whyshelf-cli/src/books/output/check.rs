//! The answer of `check` ([`BooksCheckOut`]): the books counted, what is wrong around
//! the documents, and the gaps, names and exemptions it lists; and the lines it writes
//! on stderr beside it ([`check_warnings`], [`check_notes`]).

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{BooksCheck, Names};
use whyshelf_core::domain::shelf::schema::CoverSeverity;
use whyshelf_core::domain::shelf::untold::untold_causes;

use super::cause::{absent_detail, moved_detail, unneeded_word};
use super::untold::{CHECK_UNTOLD, untold_line};
use super::words::{plural, schema_problem};
use super::{
    AbsentOut, DocRef, ExemptOut, HealthOut, MovedNameOut, Namer, RepeatedOut, UnknownOut,
    UnneededOut,
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
/// `exemption_repeated`. A gap of strength `error` has none: it fails the check,
/// and the answer is shown. None of them changes the exit.
pub fn check_warnings(check: &BooksCheck) -> Vec<String> {
    let BooksCheck::Checked {
        uncovered,
        unknown,
        absent,
        moved,
        unneeded,
        repeated,
        names,
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
    gaps.chain(untold)
        .chain(nothing)
        .chain(moved_away)
        .chain(not_needed)
        .chain(twice)
        .collect()
}

/// The lines `check` writes on stderr beside its answer, one per book that skipped
/// files, in the order of the books: `note: book spec skipped 3 files with no front
/// matter`. A skipped file is no failure, and the lines change no exit; they are
/// there so that a run that shows the answer only when it fails still says that
/// files were left out.
pub fn check_notes(check: &BooksCheck) -> Vec<String> {
    let BooksCheck::Checked { books, .. } = check else {
        return Vec::new();
    };
    books
        .iter()
        .filter(|book| !book.skipped.is_empty())
        .map(|book| {
            let count = book.skipped.len();
            format!(
                "note: book {} skipped {count} file{} with no front matter",
                book.name,
                plural(count)
            )
        })
        .collect()
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
            names,
        } => {
            let namer = Namer(names);
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
            }
        }
    }
}
