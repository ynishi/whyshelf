//! The answer of `rekey` ([`RekeyOut`]): the document that took the new key, the
//! documents whose links were rewritten, stayed or keep both, the documents whose body
//! names the old key, whether each approval holds after the write and the files
//! written; and the sentences of what is told when it is refused.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{Edited, RekeyReport};
use whyshelf_core::domain::shelf::rekey::{
    Contradicted, Contradiction, Evidence, FollowWhy, Hints, Meaning, RekeyProblem, Rewriting,
    Side, StayWhy, Undecided,
};

use super::{DocRef, Namer, RowOut};

/// The answer of `rekey`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RekeyOut {
    /// Whether this is a `--dry-run`: the same plan, checked as it would be before a
    /// write, with nothing written (`written` is null and `approvals` is empty).
    pub dry_run: bool,
    /// The document that took the new key.
    pub moved: MovedOut,
    /// The documents whose links that wrote the old key were rewritten to the new key.
    pub rewritten: Vec<LinkedOut>,
    /// The documents whose links keep the old key, which another document has now.
    pub stayed: Vec<LinkedOut>,
    /// The documents whose links keep the old key and gained the new one (`--both`).
    pub both: Vec<LinkedOut>,
    /// The documents whose body names the old key, the one that took the new key among
    /// them when its own body does. Their body is not changed (it
    /// is covered by the hash, so an approval would lapse): read them, and edit the ones
    /// that mean the document that took the new key.
    pub mentions: Vec<DocRef>,
    /// The document that took the new key and each document that writes the old key in a
    /// link, as `status` shows one, read from the files after the write: whether each
    /// approval holds. Empty with `--dry-run`.
    pub approvals: Vec<RowOut>,
    /// The files written; null with `--dry-run`.
    pub written: Option<WrittenOut>,
}

/// The document that took the new key.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedOut {
    /// The document, by its identity and the key it has now (the key it will have, for a
    /// `--dry-run`).
    pub doc: DocRef,
    /// The key it had (`book:KEY`).
    pub from_key: String,
    /// The key it has (`book:KEY`).
    pub to_key: String,
    /// The file it was in.
    pub from_file: String,
    /// The file it is in.
    pub to_file: String,
}

/// A document that writes the old key in a link, and what its links came to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkedOut {
    /// The document: its identity, and its key when it is valid.
    pub doc: Option<DocRef>,
    /// Its file.
    pub file: String,
    /// What decided it: `only_writer` (nobody else writes the old key), `pinned` (the
    /// approval of the document pinned it) or `asked` (`--follow`, `--stay` or
    /// `--both` said so).
    pub why: &'static str,
    /// Each item that wrote the old key.
    pub items: Vec<ItemOut>,
    /// A sentence for a person, with `--both` named where one document can have meant
    /// both.
    pub detail: String,
}

/// An item of a link that wrote the old key.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemOut {
    /// The link.
    pub link: String,
    /// The key as the document wrote it.
    pub written: String,
}

/// The files `rekey` wrote.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WrittenOut {
    /// The file the document was made in, with the new key.
    pub created: String,
    /// The files whose links were written over, in order.
    pub replaced: Vec<String>,
    /// The file the document was in, which is removed.
    pub removed: String,
}

/// What a document's link comes to, as a word of `why`, and a sentence.
fn linked(namer: &Namer<'_>, writer: &Rewriting) -> LinkedOut {
    let (why, detail) = match writer.meaning() {
        Meaning::Follows(FollowWhy::OnlyWriter) => (
            "only_writer",
            "nobody else writes the old key, so its links mean the document that took the new key"
                .to_owned(),
        ),
        Meaning::Follows(FollowWhy::Pinned) => (
            "pinned",
            format!(
                "its approval pinned the document that took the new key; if it means both, say `{}`",
                word(Side::Both)
            ),
        ),
        Meaning::Follows(FollowWhy::Asked) => ("asked", "said to follow".to_owned()),
        Meaning::Stays(StayWhy::Pinned) => (
            "pinned",
            format!(
                "its approval pinned the other document that writes the old key; if it means both, say `{}`",
                word(Side::Both)
            ),
        ),
        Meaning::Stays(StayWhy::Asked) => (
            "asked",
            format!(
                "said to stay with the old key; if it means both, say `{}`",
                word(Side::Both)
            ),
        ),
        Meaning::Both => (
            "asked",
            "said to mean both: it keeps the old key and gains the new one".to_owned(),
        ),
    };
    LinkedOut {
        doc: writer.id().map(|id| namer.doc(id)),
        file: writer.file().to_string(),
        why,
        items: writer
            .items()
            .iter()
            .map(|item| ItemOut {
                link: item.link.to_string(),
                written: item.written.clone(),
            })
            .collect(),
        detail,
    }
}

/// The answer of `rekey`.
pub fn rekey(report: &RekeyReport) -> RekeyOut {
    let namer = Namer(&report.names);
    let all = |writers: &[Rewriting]| writers.iter().map(|w| linked(&namer, w)).collect();
    let moved = &report.moved;
    RekeyOut {
        dry_run: report.written.is_none(),
        moved: MovedOut {
            doc: DocRef {
                id: moved.id.to_string(),
                key: Some(moved.to_key.to_string()),
            },
            from_key: moved.from_key.to_string(),
            to_key: moved.to_key.to_string(),
            from_file: moved.from_file.to_string(),
            to_file: moved.to_file.to_string(),
        },
        rewritten: all(&report.rewritten),
        stayed: all(&report.stayed),
        both: all(&report.both),
        mentions: report.mentions.iter().map(|id| namer.doc(*id)).collect(),
        approvals: report.approvals.iter().map(|row| namer.row(row)).collect(),
        written: report.written.as_ref().map(|edited: &Edited| WrittenOut {
            created: edited.created.to_string(),
            replaced: edited.replaced.iter().map(ToString::to_string).collect(),
            removed: edited.removed.to_string(),
        }),
    }
}

/// How a document is named in a sentence of a refusal: its file, with the identity a
/// word is to name it by.
fn named(file: &impl std::fmt::Display, id: Option<impl std::fmt::Display>) -> String {
    match id {
        Some(id) => format!("{file} (id {id})"),
        None => format!("{file} (no identity that can be read)"),
    }
}

/// The links, as a sentence: `the link `a`` or `the links `a`, `b``.
fn links_said(links: &[whyshelf_core::domain::shelf::schema::EntryName]) -> String {
    match links {
        [one] => format!("the link `{one}`"),
        many => format!(
            "the links {}",
            many.iter()
                .map(|link| format!("`{link}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// What a word names, as it is typed: the one place the flags are written in a
/// sentence (the command line declares them in `args.rs`).
fn word(side: Side) -> &'static str {
    match side {
        Side::Follow => "--follow",
        Side::Stay => "--stay",
        Side::Both => "--both",
    }
}

/// The three words, as a sentence says them.
fn words() -> String {
    format!(
        "`{}`, `{}` or `{}`",
        word(Side::Follow),
        word(Side::Stay),
        word(Side::Both)
    )
}

/// A document that has no evidence and no word, as a sentence.
fn undecided(doc: &Undecided) -> String {
    let who = named(&doc.file, doc.id);
    let links = links_said(&doc.links);
    let words = words();
    match doc.hints {
        Hints::NoApproval => format!(
            "{who} writes the old key in {links}, and no approval stands about it, so nothing says which document it meant: say {words} for it"
        ),
        Hints::NoPin => format!(
            "{who} writes the old key in {links}, and its approval pinned no document that writes the old key for it: say {words} for it"
        ),
        Hints::NoIdentity => format!(
            "{who} writes the old key in {links}, and no word can name it: give the file an identity first"
        ),
    }
}

/// A document whose evidence and word disagree, as a sentence.
fn contradicted(doc: &Contradicted) -> String {
    let who = named(&doc.file, doc.id);
    let links = links_said(&doc.links);
    let Contradiction { evidence, asked } = doc.contradiction;
    match (evidence, asked) {
        (Evidence::OnlyWriter, Some(side)) => format!(
            "{who}: nobody else writes the old key, so {links} can only mean the document that took the new key; `{}` is against that",
            word(side)
        ),
        (Evidence::Pinned(pinned), Some(side)) => format!(
            "{who}: its approval pinned {} for {links}; `{}` is against that: say `{}` if it means both",
            side_said(pinned),
            word(side),
            word(Side::Both)
        ),
        // With no word, only pins on both sides contradict; the only writer always
        // decides alone.
        (Evidence::Pinned(_) | Evidence::OnlyWriter, None) => format!(
            "{who}: the evidence for {links} points at both documents that write the old key: say `{}` if it means both",
            word(Side::Both)
        ),
    }
}

/// Which document an approval pinned, as a sentence.
fn side_said(side: Side) -> &'static str {
    match side {
        Side::Follow => "the document that took the new key",
        Side::Stay => "the other document that writes the old key",
        Side::Both => "both documents that write the old key",
    }
}

/// Each case a refused `rekey` lists, one sentence each: every document that is not
/// decided or is contradicted, and every place that could not be read. Empty for a
/// refusal whose message says all there is.
pub fn rekey_problems(problem: &RekeyProblem) -> Vec<String> {
    match problem {
        RekeyProblem::Unsettled {
            undecided: not_decided,
            contradicted: against,
        } => not_decided
            .iter()
            .map(undecided)
            .chain(against.iter().map(contradicted))
            .collect(),
        RekeyProblem::NotAllRead { keys, links } => keys
            .iter()
            .map(|file| format!("{file} could not be read, and may write the key"))
            .chain(
                links
                    .iter()
                    .map(|place| format!("{place} could not be read, and may link to the key")),
            )
            .collect(),
        RekeyProblem::Gone
        | RekeyProblem::Unknown { .. }
        | RekeyProblem::InSeveralFiles { .. }
        | RekeyProblem::NoKey { .. }
        | RekeyProblem::ClaimKey(_)
        | RekeyProblem::NextKey(_)
        | RekeyProblem::File(_)
        | RekeyProblem::NotAWriter { .. } => Vec::new(),
    }
}
