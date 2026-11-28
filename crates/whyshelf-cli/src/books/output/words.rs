//! The words more than one answer writes the same way: a plural, a problem of the
//! declaration, a text as a word of a shell, what an artifact is now, the word of
//! a standing, how strongly an approval does not hold, whether one holds at an end of a
//! change and what the change made of it, how a change reaches a document and how one
//! is inside the reach, why an upstream holds an approval back, which approval by name
//! an origin is told of, why
//! a link cannot be followed to what it names, how many names read an approval, and
//! who ran the command that made a record.

use whyshelf_core::domain::actor::{Actor, Attribution};
use whyshelf_core::domain::shelf::artifact::ArtifactState;
use whyshelf_core::domain::shelf::doc::Unfollowed;
use whyshelf_core::domain::shelf::freshness::{
    FreshnessLevel, Holding, HoldsAs, Severity, StandingKind,
};
use whyshelf_core::domain::shelf::impact::{Made, Sides};
use whyshelf_core::domain::shelf::ledger::ReaderCount;
use whyshelf_core::domain::shelf::order::Hold;
use whyshelf_core::domain::shelf::schema::SchemaProblem;
use whyshelf_core::domain::shelf::settle::Approval;
use whyshelf_core::domain::shelf::within::{InsideBy, Witness};

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
/// documents whose links cannot be read, the files left in a merge conflict that may
/// hold a document of the answer, and the files whose identity cannot be read, a count
/// of none left unsaid. The one place the three phrases are written; each warning
/// writes what they may do.
pub(super) fn unread(invalid: usize, sided: usize, unreadable: usize) -> String {
    let mut parts = Vec::new();
    if invalid > 0 {
        parts.push(format!(
            "{invalid} document{} whose links cannot be read",
            plural(invalid)
        ));
    }
    if sided > 0 {
        parts.push(format!(
            "{sided} file{} left in a merge conflict",
            plural(sided)
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

/// The word of whether an approval holds at one end of a change: the one place the
/// words are written. The match is exhaustive, so a new holding does not build until
/// it has a word.
pub(super) fn holding_word(holding: &Holding) -> &'static str {
    match holding {
        Holding::NotThere => "not_there",
        Holding::Pending => "pending",
        Holding::Against(_) => "against",
        Holding::Holds(_) => "holds",
        Holding::CannotTell { .. } => "cannot_tell",
        Holding::DoesNotHold { .. } => "does_not_hold",
    }
}

/// The level beside a holding: how one that holds holds, the decision against one,
/// how strongly one does not hold, in the words of a standing and a severity.
pub(super) fn holding_level(holding: &Holding) -> Option<&'static str> {
    let holds = |holds_as: &HoldsAs| {
        standing(StandingKind::Approved(match holds_as {
            HoldsAs::Fresh => FreshnessLevel::Fresh,
            HoldsAs::Awaiting => FreshnessLevel::Awaiting,
        }))
    };
    match holding {
        Holding::NotThere | Holding::Pending => None,
        Holding::Against(status) => Some(standing(StandingKind::from(*status))),
        Holding::Holds(holds_as) | Holding::CannotTell { holds_as, .. } => Some(holds(holds_as)),
        Holding::DoesNotHold { level, .. } => Some(severity(*level)),
    }
}

/// The word of what a change made of an approval: the one place the words are
/// written.
pub(super) fn made_word(made: &Made) -> &'static str {
    match made {
        Made::Yes(_) => "yes",
        Made::No => "no",
        Made::CannotTell(_) => "cannot_tell",
    }
}

/// The word of which end of a change could not tell, for `impact` read either way
/// ([`Backward::side`](whyshelf_core::domain::shelf::within::Backward::side) turns the
/// change read backwards into the change's own words first).
pub(super) fn sides_word(sides: Sides) -> &'static str {
    match sides {
        Sides::Before => "before",
        Sides::After => "after",
        Sides::Both => "both",
    }
}

/// The word of one way a change is shown to reach a document: the one place the words
/// are written.
pub(super) fn witness_word(witness: Witness) -> &'static str {
    match witness {
        Witness::AddsCause => "adds_cause",
        Witness::RemovesCause => "removes_cause",
        Witness::Decided => "decided",
        Witness::OnShelf => "on_shelf",
        Witness::Edited => "edited",
        Witness::Recorded => "recorded",
    }
}

/// The word of how a document is inside the reach before a change.
pub(super) fn inside_word(by: InsideBy) -> &'static str {
    match by {
        InsideBy::Named => "named",
        InsideBy::Reached => "reached",
    }
}

/// The word of how strongly an approval does not hold: the one place the words are
/// written.
pub(super) fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Expired => "expired",
        Severity::Stale => "stale",
        Severity::Broken => "broken",
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

/// The word of which approval by name `stale` tells of, as a value of an answer: the
/// one place the words are written. The model's [`Approval`] has no serde, and these
/// words are the answer's, so the model's value is turned into them here. The match is
/// exhaustive, so a new kind does not build until it has a word.
pub(super) fn approve_word(approval: &Approval) -> &'static str {
    match approval {
        Approval::Pending(_) => "pending",
        Approval::AsAnother(_) => "as_another",
        Approval::Until(_) => "until",
    }
}

/// The phrase of which approval by name `stale` tells of, as a line a person
/// reads: the one place the phrases are written, beside [`approve_word`], whose kinds
/// they say.
pub(super) fn approve_phrase(approval: &Approval) -> &'static str {
    match approval {
        Approval::Pending(_) => "as pending",
        Approval::AsAnother(_) => "by another name",
        Approval::Until(_) => "with a new expiry",
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
