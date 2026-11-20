//! The warning lines of names that cannot be told now: one line per cause, as `check`
//! and `stale` write them on stderr, in the words each of them gives ([`UntoldWords`]).

use whyshelf_core::app::shelf::Unknown;
use whyshelf_core::domain::shelf::untold::{Held, UntoldCause, Writers};

use super::words::plural;

/// How many names of one cause, and how many of its documents, a warning line
/// writes before it says how many more.
const UNTOLD_NAMES_SHOWN: usize = 3;

/// The words in which `check` and `stale` differ when they write one cause of names
/// that cannot be told now.
pub(super) struct UntoldWords {
    /// What the document does with the name, for one name: `names`, `pins`.
    verb: &'static str,
    /// What follows `which cannot be told`, for one name.
    one_after: &'static str,
    /// Before the number of documents, for many names of many documents.
    by: &'static str,
    /// What follows `cannot be told`, for many names of one document. Like
    /// `one_after`, it comes before the reason, which is the adapter's own text.
    writer_after: &'static str,
    /// What follows `cannot be told`, for many names of many documents.
    many_after: &'static str,
}

pub(super) const CHECK_UNTOLD: UntoldWords = UntoldWords {
    verb: "names",
    one_after: "",
    by: "named by",
    writer_after: "",
    many_after: "",
};

pub(super) const STALE_UNTOLD: UntoldWords = UntoldWords {
    verb: "pins",
    one_after: " now, so whether its approval holds cannot be said",
    by: "pinned by the approvals of",
    writer_after: " now, so whether its approval holds cannot be said",
    many_after: " now, so whether those approvals hold cannot be said",
};

/// The one line for one cause of names that cannot be told now, as `words` word it.
/// The one place a cause is written: for one (document, name) pair the line says
/// them; for more it says the number of names, the reason once, the document (one,
/// by its key) or the number of documents with their first [`UNTOLD_NAMES_SHOWN`]
/// keys, and the first [`UNTOLD_NAMES_SHOWN`] names.
pub(super) fn untold_line(cause: &UntoldCause<'_, Unknown>, words: &UntoldWords) -> String {
    let (space, why) = (cause.space(), cause.why());
    match cause.held() {
        Held::One(u) => format!(
            "warning: {} {} {} in {space}, which cannot be told{}: {why}",
            document(u),
            words.verb,
            u.artifact.name(),
            words.one_after
        ),
        Held::Many { names, docs } => {
            let n = names.len();
            let names = first_few(names.iter().map(|name| name.as_str().to_owned()), n);
            match docs {
                Writers::One(u) => format!(
                    "warning: {} {} {n} names in {space}, which cannot be told{}, each for this reason: {why}; names: {names}",
                    document(u),
                    words.verb,
                    words.writer_after
                ),
                Writers::Many(writers) => {
                    let d = writers.len();
                    let keys = first_few(writers.iter().map(|u| document(u)), d);
                    format!(
                        "warning: {n} name{} in {space}, {} {d} documents ({keys}), cannot be told{}, each for this reason: {why}; names: {names}",
                        plural(n),
                        words.by,
                        words.many_after
                    )
                }
            }
        }
    }
}

/// The document of a name as a line writes it: its key, else its identity.
fn document(u: &Unknown) -> String {
    u.key
        .as_ref()
        .map_or_else(|| u.doc.to_string(), ToString::to_string)
}

/// The first [`UNTOLD_NAMES_SHOWN`] of `total` items joined by `, `, then `, and N
/// more` when there are more. The one list a line writes, of names and of
/// documents.
fn first_few(items: impl Iterator<Item = String>, total: usize) -> String {
    let shown = items
        .take(UNTOLD_NAMES_SHOWN)
        .collect::<Vec<String>>()
        .join(", ");
    match total.checked_sub(UNTOLD_NAMES_SHOWN) {
        Some(more) if more > 0 => format!("{shown}, and {more} more"),
        _ => shown,
    }
}
