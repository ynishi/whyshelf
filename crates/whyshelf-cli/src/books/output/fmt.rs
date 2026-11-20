//! The answers of `fmt` and `fmt --check`: the files written in the form, those in it
//! already, those left as they are with why, and the lines written on stderr.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{FmtReport, LeftAloneWhy};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::build::UnheldValue;

/// The answer of `fmt` and `fmt --check`, the files by their path relative to the
/// project root, in the order the documents are read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FmtOut {
    /// The files written in the form; empty for `--check`.
    pub rewritten: Vec<String>,
    /// How many files were in the form already (none of them is written).
    pub in_form: usize,
    /// For `--check`, the files not in the form that `fmt` would write in it; null
    /// without `--check`.
    pub not_in_form: Option<Vec<String>>,
    /// The files left as they are, each with why. They are not in the form either,
    /// and are listed apart from `not_in_form`; `--check` exits 1 when either list
    /// has any file, and `fmt` without it exits 0, with a warning for each.
    pub left_alone: Vec<LeftAloneOut>,
    /// The names `--drop` takes out, per file: for each file in `rewritten` (or, for
    /// `--check`, in `not_in_form`) that loses an entry of a name it was asked to
    /// drop, the names it loses. Empty without `--drop`, and for a file that holds
    /// none of the names.
    pub dropped: Vec<DroppedOut>,
}

/// The names `fmt --drop` takes out of one file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DroppedOut {
    /// The file.
    pub file: String,
    /// The names of the entries taken out of it (or, for `--check`, to be taken out),
    /// each once, in the order of the names.
    pub names: Vec<String>,
}

/// A file `fmt` leaves as it is.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftAloneOut {
    /// The file.
    pub file: String,
    /// Why: `unparsed` (its front matter cannot be read), `comment` (its front
    /// matter holds a comment, which the form would lose), `reads_otherwise` (its
    /// text in the form would not read as the file does), `read_only` (the user
    /// cannot write it), `linked` (it has more than one link, which a file written
    /// in its place would not keep) or `changed` (it changed between the reading and
    /// the write).
    pub reason: &'static str,
    /// For `comment`, the line of the file the first one is on.
    pub line: Option<u32>,
    /// A sentence for a person.
    pub detail: String,
}

/// The answer of `fmt`, or of `fmt --check` when `check`.
pub fn fmt(report: &FmtReport, check: bool) -> FmtOut {
    let paths = |files: &[RelPath]| files.iter().map(ToString::to_string).collect();
    FmtOut {
        rewritten: paths(&report.rewritten),
        in_form: report.in_form.len(),
        not_in_form: check.then(|| paths(&report.not_in_form)),
        left_alone: report
            .left_alone
            .iter()
            .map(|left| LeftAloneOut {
                file: left.file.to_string(),
                reason: match left.why {
                    LeftAloneWhy::Unparsed(_) | LeftAloneWhy::Unheld(_) => "unparsed",
                    LeftAloneWhy::Comment { .. } => "comment",
                    LeftAloneWhy::ReadsOtherwise => "reads_otherwise",
                    LeftAloneWhy::ReadOnly => "read_only",
                    LeftAloneWhy::Linked => "linked",
                    LeftAloneWhy::Changed => "changed",
                },
                line: match left.why {
                    LeftAloneWhy::Comment { line } => Some(line),
                    _ => None,
                },
                detail: left_alone_detail(&left.why),
            })
            .collect(),
        dropped: report
            .dropped
            .iter()
            .map(|dropped| DroppedOut {
                file: dropped.file.to_string(),
                names: dropped.names.iter().map(ToString::to_string).collect(),
            })
            .collect(),
    }
}

/// The lines `fmt` writes on stderr: one per file it leaves as it is, in the order
/// of `left_alone`.
pub fn fmt_warnings(report: &FmtReport) -> Vec<String> {
    report
        .left_alone
        .iter()
        .map(|left| {
            format!(
                "warning: {} is left as it is: {}",
                left.file,
                left_alone_detail(&left.why)
            )
        })
        .collect()
}

/// The sentence for a file `fmt` leaves as it is.
fn left_alone_detail(why: &LeftAloneWhy) -> String {
    match why {
        LeftAloneWhy::Unparsed(problem) => format!("its front matter cannot be read: {problem}"),
        LeftAloneWhy::Unheld(values) => format!(
            "its front matter cannot be read: {}",
            UnheldValue::told_all(values)
        ),
        LeftAloneWhy::Comment { line } => format!(
            "its front matter holds a comment on line {line}, which the form would lose; remove it, or keep it in the body"
        ),
        LeftAloneWhy::ReadsOtherwise => {
            "its text in the form would not read as the file does".to_owned()
        }
        LeftAloneWhy::ReadOnly => "it cannot be written (read-only)".to_owned(),
        LeftAloneWhy::Linked => "it has more than one link (a hard link), which a file written in its place would not keep".to_owned(),
        LeftAloneWhy::Changed => "it changed after it was read; run fmt again".to_owned(),
    }
}
