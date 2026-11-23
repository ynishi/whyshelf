//! The words more than one answer writes the same way: a plural, a problem of the
//! declaration, a text as a word of a shell, what an artifact is now, the word of
//! a standing, why an upstream holds an approval back, why a link cannot be followed
//! to what it names, how many names read an approval, and who ran the command that
//! made a record.

use whyshelf_core::domain::actor::{Actor, Attribution};
use whyshelf_core::domain::shelf::artifact::ArtifactState;
use whyshelf_core::domain::shelf::doc::Unfollowed;
use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};
use whyshelf_core::domain::shelf::ledger::ReaderCount;
use whyshelf_core::domain::shelf::order::Hold;
use whyshelf_core::domain::shelf::schema::SchemaProblem;

/// Who ran the command that made a record, as every answer that shows the record's
/// actor shows it beside it: null when the actor ran it. The one place it is written.
pub(super) fn ran_by(by: &Attribution) -> Option<String> {
    by.ran_by().map(|name: &Actor| name.as_str().to_owned())
}

/// `s` unless the number is one.
pub(super) fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// What could not be read, counted, as the warnings of `reach` and `units` say it: the
/// documents whose links cannot be read and the files whose identity cannot be read,
/// a count of none left unsaid. The one place the two phrases are written; each
/// warning writes what they may do.
pub(super) fn unread(invalid: usize, unreadable: usize) -> String {
    let mut parts = Vec::new();
    if invalid > 0 {
        parts.push(format!(
            "{invalid} document{} whose links cannot be read",
            plural(invalid)
        ));
    }
    if unreadable > 0 {
        parts.push(format!(
            "{unreadable} file{} whose identity cannot be read",
            plural(unreadable)
        ));
    }
    parts.join(" and ")
}

/// A problem of the declaration as every answer gives it: its text, then the text
/// of each reason under it (`Error::source`), joined by `: `, so a problem that
/// keeps its reason as a source (a comparison that is none, a directory that is no
/// relative path) says why. The one place a problem of the declaration is written.
pub(super) fn schema_problem(problem: &SchemaProblem) -> String {
    let mut text = problem.to_string();
    let mut source = std::error::Error::source(problem);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// A text as one word of a POSIX shell: as it is when it holds nothing the shell
/// reads, otherwise in single quotes (a `'` in it as `'\''`).
pub(super) fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// The word for what an artifact is now: `present`, `absent` or `unknown`.
pub(super) fn state_word(state: &ArtifactState) -> &'static str {
    match state {
        ArtifactState::Present(_) => "present",
        ArtifactState::Absent => "absent",
        ArtifactState::Unknown(_) => "unknown",
    }
}

/// The word of a standing: the one place the words are written. The match is
/// exhaustive, so a new kind does not build until it has a word. `status --standing`
/// reads a word back over [`StandingKind::ALL`], so what is printed is what can be
/// asked.
pub fn standing(kind: StandingKind) -> &'static str {
    match kind {
        StandingKind::Pending => "pending",
        StandingKind::Denied => "denied",
        StandingKind::Deprecated => "deprecated",
        StandingKind::Retired => "retired",
        StandingKind::Approved(FreshnessLevel::Fresh) => "fresh",
        StandingKind::Approved(FreshnessLevel::Awaiting) => "awaiting",
        StandingKind::Approved(FreshnessLevel::Expired) => "expired",
        StandingKind::Approved(FreshnessLevel::Stale) => "stale",
        StandingKind::Approved(FreshnessLevel::Broken) => "broken",
    }
}

/// The word for why an upstream holds an approval back: the one place the words are
/// written, for the approvals a run leaves out and the upstreams a unit waits on.
pub(super) fn hold_word(hold: Hold) -> &'static str {
    match hold {
        Hold::Broken => "broken",
        Hold::Pending => "pending",
        Hold::Stale => "stale",
        Hold::Awaiting => "awaiting",
        Hold::LeftOut => "left_out",
    }
}

/// The word for why a link cannot be followed to the document it names: the one place
/// the words are written. The match is exhaustive, so a new reason does not build until
/// it has a word.
pub(super) fn unfollowed(why: Unfollowed) -> &'static str {
    match why {
        Unfollowed::TargetInvalid => "invalid",
        Unfollowed::Ambiguous => "ambiguous",
    }
}

/// The word of how many names read an approval: the one place the words are
/// written. The match is exhaustive, so a new count does not build until it has a
/// word. `status --readers` reads a word back over [`ReaderCount::ALL`], so what is
/// printed is what can be asked.
pub fn readers(count: ReaderCount) -> &'static str {
    match count {
        ReaderCount::Nobody => "nobody",
        ReaderCount::One => "one",
        ReaderCount::Several => "several",
    }
}
