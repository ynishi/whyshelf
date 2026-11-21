//! What an answer says of why an approval does not hold and of what is worth knowing
//! beside it: one [`CauseOut`] for each cause and one [`NoticeOut`] for each notice,
//! each the one translation of its domain value, and the sentences of exemptions and
//! of names with nothing there, or that cannot be told now, or that were moved.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::nonempty::TwoOrMore;
use whyshelf_core::domain::shelf::artifact::{ArtifactKey, Comparison};
use whyshelf_core::domain::shelf::build::{Detail, FailureTag};
use whyshelf_core::domain::shelf::doc::DocLabel;
use whyshelf_core::domain::shelf::exemption::{CoverMiss, Unneeded};
use whyshelf_core::domain::shelf::freshness::{
    Cause, Notice, Severity, Successors, UpstreamStatus,
};
use whyshelf_core::domain::shelf::ledger::{ArtifactPin, FramePin, JudgementId, ReadPin};
use whyshelf_core::domain::shelf::schema::{BookName, EntryName};
use whyshelf_core::domain::words::{Join, Mark, listed};

use super::{DocRef, Namer, standing};

/// A cause of an approval that does not hold.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CauseOut {
    /// What kind of cause (`self_changed`, `upstream_changed`, `upstream_declared`,
    /// `via`, `expired`, ...).
    pub kind: &'static str,
    /// How strong: `expired`, `stale` or `broken`.
    pub severity: &'static str,
    /// The upstream document the cause names, if it names one.
    pub upstream: Option<DocRef>,
    /// The link it was reached through, if any.
    pub link: Option<String>,
    /// The name of the artifact the cause names, if it names one
    /// (`artifact_changed`, `artifact_gone`, `artifact_arrived`,
    /// `artifact_comparison_changed`): for the space `file`, its path.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The name `artifact` is read as now, when people declared it moved since it
    /// was pinned (`judge moved`); none when it is read as written. For
    /// `artifact_moved`, `artifact` is the name the link writes and `read_as` the
    /// name it is to write.
    pub read_as: Option<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// Something worth knowing that changes no freshness.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoticeOut {
    /// What kind (`informs_not_fresh`, `informs_unresolved`, `upstream_expired`,
    /// `self_same_meaning`, `upstream_same_meaning`, `artifact_same_meaning`,
    /// `not_covered`, `exempt`, `exemption_unneeded`, `exemption_repeated`,
    /// `artifact_absent`, `artifact_unknown`).
    pub kind: &'static str,
    /// For `not_covered` and `exempt`, how strongly the link expects to reach the
    /// document: `warn` or `error`; null for any other kind.
    pub severity: Option<&'static str>,
    /// The link; empty for a notice about the document itself.
    pub link: String,
    /// The document it names, if it names one.
    pub target: Option<DocRef>,
    /// The name of the artifact it names, if it names one: for the space `file`, a
    /// path under the root.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// For `artifact_same_meaning`, the name the pin of `artifact` is read as now,
    /// when people declared it moved since (`judge moved`); none otherwise.
    pub read_as: Option<String>,
    /// For an edit declared to keep the meaning, the declarations (record ids) that
    /// join the hash pinned to the hash now, in order; for `exempt` and
    /// `exemption_unneeded`, the exemption (`judge withdraw` takes it); for
    /// `exemption_repeated`, every exemption of the document from the link; empty
    /// otherwise.
    pub declared: Vec<String>,
    /// For `not_covered`, `exempt`, `exemption_unneeded` and `exemption_repeated`,
    /// the book that declares
    /// `link`; null for any other kind.
    pub book: Option<String>,
    /// For `exemption_unneeded`, why the exemption is not needed: `reached` (the
    /// link reaches the document now), `not_declared` (no such link into a book is
    /// declared now), `no_cover` (the link is declared with no cover), `other_book`
    /// (it points into another book than the document's), `not_valid` (the document
    /// is not valid now) or `decided_against` (it is denied, deprecated or retired);
    /// null for any other kind. (An exemption of an artifact is told by `check` and
    /// `cover` with the words `reached`, `not_declared`, `no_cover`, `other_space`
    /// and `not_listed`, as their own lists give them.)
    pub why: Option<&'static str>,
    /// A sentence for a person.
    pub detail: String,
}

/// An exemption that stands and takes the place of a gap.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExemptOut {
    /// The document.
    pub doc: DocRef,
    /// The book that declares the link.
    pub book: String,
    /// The link whose cover the document is exempt from.
    pub link: String,
    /// How strongly the link expects to reach the documents of its book: `warn` or
    /// `error`.
    pub severity: &'static str,
    /// The exemption's record id (`judge withdraw` takes it).
    pub exemption: String,
    /// Who declared it.
    pub actor: String,
    /// Who ran the command, when not the actor; null when the actor ran it.
    pub ran_by: Option<String>,
    /// When (unix milliseconds).
    pub at: i64,
    /// Why.
    pub reason: String,
}

/// An exemption that stands and is no longer needed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnneededOut {
    /// The document.
    pub doc: DocRef,
    /// The book that declares the link.
    pub book: String,
    /// The link the exemption names.
    pub link: String,
    /// The exemption's record id (`judge withdraw` takes it).
    pub exemption: String,
    /// Who declared it.
    pub actor: String,
    /// Who ran the command, when not the actor; null when the actor ran it.
    pub ran_by: Option<String>,
    /// When (unix milliseconds).
    pub at: i64,
    /// Why it was declared.
    pub reason: String,
    /// Why it is not needed now, as the notice `exemption_unneeded` gives it.
    pub why: &'static str,
    /// A sentence for a person.
    pub detail: String,
}

/// Two or more exemptions that stand of one document from one link.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepeatedOut {
    /// The document.
    pub doc: DocRef,
    /// The book that declares the link.
    pub book: String,
    /// The link the exemptions name.
    pub link: String,
    /// The exemptions' record ids, in the order of identity: withdraw all but one.
    pub exemptions: Vec<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a link writes that an approval pinned, declared moved since.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedNameOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written, the one pinned.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// The name the pin is read as now, which the link is to write.
    pub moved_to: String,
    /// The declarations of the moves (record ids), in order.
    pub declared: Vec<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a link to artifacts writes with nothing there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written (kept from when every name was a path; the same as
    /// `name`).
    pub path: String,
    /// The name as written.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a link to artifacts writes that its adapter cannot tell about now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnknownOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// Why, as the adapter says.
    pub why: String,
}

impl Namer<'_> {
    fn pin(&self, pin: &FramePin) -> DocRef {
        DocRef {
            id: pin.target().to_string(),
            key: Some(pin.key().to_string()),
        }
    }

    pub(super) fn cause(&self, cause: &Cause) -> CauseOut {
        let severity = severity(cause.severity());
        let out = |kind, upstream: Option<&FramePin>, detail: String| CauseOut {
            kind,
            severity,
            upstream: upstream.map(|p| self.pin(p)),
            link: upstream.map(|p| p.link().to_string()),
            artifact: None,
            space: None,
            read_as: None,
            detail,
        };
        // What was pinned, and beside it the name it is read as after a move.
        let artifact = |kind, read: &ReadPin, detail: String| CauseOut {
            kind,
            severity,
            upstream: None,
            link: Some(read.pin().link().to_string()),
            artifact: Some(read.pin().name().to_string()),
            space: Some(read.pin().space().to_string()),
            read_as: read.moved_away().map(|a| a.name().to_string()),
            detail: match read.moved_away() {
                Some(_) => format!(
                    "{detail} (pinned as {}, declared moved since)",
                    read.pin().artifact()
                ),
                None => detail,
            },
        };
        match cause {
            Cause::SelfChanged { approved, now } => out(
                "self_changed",
                None,
                format!("the document changed since it was approved ({approved} -> {now})"),
            ),
            Cause::SelfNotValid(failure) => out(
                match failure.tag() {
                    FailureTag::Invalid => "self_invalid",
                    FailureTag::Gone => "self_gone",
                    FailureTag::Unknown => "self_unknown",
                },
                None,
                failure.said_of(&"the document", Detail::Full),
            ),
            Cause::UpstreamNotValid { pin, failure } => out(
                match failure.tag() {
                    FailureTag::Invalid => "upstream_invalid",
                    FailureTag::Gone => "upstream_gone",
                    FailureTag::Unknown => "upstream_unknown",
                },
                Some(pin),
                failure.said_of(pin.key(), Detail::Short),
            ),
            Cause::UpstreamChanged { pin, now } => out(
                "upstream_changed",
                Some(pin),
                format!(
                    "{} changed since the approval pinned it ({} -> {now})",
                    pin.key(),
                    pin.hash()
                ),
            ),
            Cause::UpstreamDeclared { pin, status } => out(
                "upstream_declared",
                Some(pin),
                format!("{} is {}", pin.key(), upstream_status(status)),
            ),
            Cause::Via { pin, severity } => out(
                "via",
                Some(pin),
                format!(
                    "{} does not hold ({})",
                    pin.key(),
                    self::severity(*severity)
                ),
            ),
            Cause::LinksChanged {
                link,
                added,
                removed,
                unresolved,
            } => CauseOut {
                kind: "links_changed",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: None,
                space: None,
                read_as: None,
                detail: format!(
                    "the link `{link}` changed since the approval: {} added, {} removed, {} unresolved",
                    added.len(),
                    removed.len(),
                    unresolved.len()
                ),
            },
            Cause::ArtifactChanged { pin, now } => artifact(
                "artifact_changed",
                pin,
                format!(
                    "{} changed since the approval pinned it ({} -> {now})",
                    pin.read_as(),
                    pinned_hash(pin.pin())
                ),
            ),
            Cause::ArtifactComparisonChanged { pin, now } => artifact(
                "artifact_comparison_changed",
                pin,
                format!(
                    "the link `{}` compares {} as {} now, and it was pinned as {}: the hashes are not compared",
                    pin.pin().link(),
                    pin.read_as(),
                    comparison(now),
                    comparison(pin.pin().comparison())
                ),
            ),
            // The name the link writes is the cause's `artifact`; the name it is to
            // write, the one the pin is read as, is `read_as`.
            Cause::ArtifactMoved { link, written, pin } => CauseOut {
                kind: "artifact_moved",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: Some(written.name().to_string()),
                space: Some(written.space().to_string()),
                read_as: Some(pin.read_as().name().to_string()),
                detail: moved_detail(link, written, pin.read_as()),
            },
            Cause::ArtifactGone { pin } => artifact(
                "artifact_gone",
                pin,
                format!(
                    "{} was there when it was pinned and has nothing behind it now",
                    pin.read_as()
                ),
            ),
            Cause::ArtifactArrived { pin, now } => artifact(
                "artifact_arrived",
                pin,
                format!(
                    "{} was not there when it was pinned and is now ({now}): look again",
                    pin.read_as()
                ),
            ),
            Cause::ArtifactLinksChanged {
                link,
                added,
                removed,
            } => CauseOut {
                kind: "links_changed",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: None,
                // The space of the names, when they all agree; none when the link
                // moved to another space since (the detail names each name's space).
                space: {
                    let mut spaces = added.iter().chain(removed).map(|a| a.space());
                    let first = spaces.next();
                    match first {
                        Some(first) if spaces.all(|s| s == first) => Some(first.to_string()),
                        Some(_) | None => None,
                    }
                },
                read_as: None,
                detail: format!(
                    "the link `{link}` to artifacts changed since the approval: {} added ({}), {} removed ({})",
                    added.len(),
                    listed(added.iter(), Mark::Plain, Join::Semicolon),
                    removed.len(),
                    listed(removed.iter(), Mark::Plain, Join::Semicolon)
                ),
            },
            Cause::InCycle { cycle } => out(
                "in_cycle",
                None,
                format!(
                    "the document is in a cycle of {} (see health.cycles)",
                    cycle.source
                ),
            ),
            Cause::Expired { since } => out(
                "expired",
                None,
                format!("the approval lapsed at {} (unix ms)", since.millis()),
            ),
        }
    }

    pub(super) fn notice(&self, notice: &Notice) -> NoticeOut {
        match notice {
            Notice::InformsNotFresh {
                link,
                target,
                standing: kind,
            } => NoticeOut {
                kind: "informs_not_fresh",
                severity: None,
                link: link.to_string(),
                target: Some(self.doc(*target)),
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                book: None,
                why: None,
                detail: format!("a document it informs of is {}", standing(*kind)),
            },
            Notice::InformsUnresolved { link, unresolved } => NoticeOut {
                kind: "informs_unresolved",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                book: None,
                why: None,
                detail: unresolved.to_string(),
            },
            Notice::UpstreamExpired { pin, since } => NoticeOut {
                kind: "upstream_expired",
                severity: None,
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                book: None,
                why: None,
                detail: format!(
                    "the approval of {} lapsed at {} (unix ms)",
                    pin.key(),
                    since.millis()
                ),
            },
            Notice::SelfSameMeaning {
                approved,
                now,
                declared,
            } => NoticeOut {
                kind: "self_same_meaning",
                severity: None,
                link: String::new(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                book: None,
                why: None,
                detail: format!(
                    "the document was edited since its approval ({approved} -> {now}), and the edit was declared to keep the meaning"
                ),
            },
            Notice::UpstreamSameMeaning { pin, now, declared } => NoticeOut {
                kind: "upstream_same_meaning",
                severity: None,
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
                space: None,
                read_as: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                book: None,
                why: None,
                detail: format!(
                    "{} was edited since it was pinned ({} -> {now}), and the edit was declared to keep the meaning",
                    pin.key(),
                    pin.hash()
                ),
            },
            Notice::ArtifactSameMeaning { pin, now, declared } => NoticeOut {
                kind: "artifact_same_meaning",
                severity: None,
                link: pin.pin().link().to_string(),
                target: None,
                artifact: Some(pin.pin().name().to_string()),
                space: Some(pin.pin().space().to_string()),
                read_as: pin.moved_away().map(|a| a.name().to_string()),
                declared: declared.iter().map(ToString::to_string).collect(),
                book: None,
                why: None,
                detail: format!(
                    "{} changed since it was pinned (now {now}), and the change was declared to keep the meaning",
                    pin.read_as()
                ),
            },
            Notice::ArtifactAbsent { link, artifact } => NoticeOut {
                kind: "artifact_absent",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: Some(artifact.name().to_string()),
                space: Some(artifact.space().to_string()),
                read_as: None,
                declared: Vec::new(),
                book: None,
                why: None,
                detail: absent_detail(artifact),
            },
            Notice::ArtifactUnknown {
                link,
                artifact,
                why,
            } => NoticeOut {
                kind: "artifact_unknown",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: Some(artifact.name().to_string()),
                space: Some(artifact.space().to_string()),
                read_as: None,
                declared: Vec::new(),
                book: None,
                why: None,
                detail: unknown_detail(artifact, why.as_str()),
            },
            Notice::NotCovered {
                book,
                link,
                severity,
            } => NoticeOut {
                kind: "not_covered",
                severity: Some(severity.word()),
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                book: Some(book.to_string()),
                why: None,
                detail: format!(
                    "no document of `{book}` reaches this one through `{link}`, which is declared to cover its book"
                ),
            },
            Notice::Exempt {
                book,
                link,
                severity,
                exemption,
            } => NoticeOut {
                kind: "exempt",
                severity: Some(severity.word()),
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: vec![exemption.to_string()],
                book: Some(book.to_string()),
                why: None,
                detail: format!(
                    "no document of `{book}` reaches this one through `{link}`, and {exemption} declares it is not expected to"
                ),
            },
            Notice::ExemptionUnneeded {
                book,
                link,
                exemption,
                why,
            } => NoticeOut {
                kind: "exemption_unneeded",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: vec![exemption.to_string()],
                book: Some(book.to_string()),
                why: Some(unneeded_word(why)),
                detail: self.unneeded(None, book, link, *exemption, why),
            },
            Notice::ExemptionRepeated {
                book,
                link,
                exemptions,
            } => NoticeOut {
                kind: "exemption_repeated",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: exemptions.iter().map(ToString::to_string).collect(),
                book: Some(book.to_string()),
                why: None,
                detail: self.repeated(None, book, link, exemptions),
            },
        }
    }

    pub(super) fn label(&self, id: DocId) -> String {
        DocLabel::new(id, self.0.key_of(id).cloned()).to_string()
    }
}

/// The word of why an exemption is not needed, as `exemption_unneeded` gives it.
pub(super) fn unneeded_word(why: &Unneeded) -> &'static str {
    match why {
        Unneeded::Reached { .. } => "reached",
        Unneeded::Cover(CoverMiss::NotDeclared | CoverMiss::NotDeclaredToArtifacts) => {
            "not_declared"
        }
        Unneeded::Cover(CoverMiss::NoCover) => "no_cover",
        Unneeded::Cover(CoverMiss::OtherBook { .. }) => "other_book",
        Unneeded::Cover(CoverMiss::OtherSpace { .. }) => "other_space",
        Unneeded::Cover(CoverMiss::NotListed) => "not_listed",
        Unneeded::NotValid(_) => "not_valid",
        Unneeded::DecidedAgainst => "decided_against",
    }
}

impl Namer<'_> {
    /// The sentence of an exemption that is not needed: the one place it is
    /// written, for the notice, `check`'s list and its warning. The document is
    /// named by its key (its identity when it has none); a notice, which a row of the
    /// document carries, names it `it` (`doc` none). The documents the link reaches
    /// it from are named by their keys.
    pub(super) fn unneeded(
        &self,
        doc: Option<DocId>,
        book: &BookName,
        link: &EntryName,
        exemption: JudgementId,
        why: &Unneeded,
    ) -> String {
        let doc = doc.map_or_else(|| "it".to_owned(), |id| self.label(id));
        self.unneeded_of(&doc, book, link, exemption, why)
    }

    /// The sentence of an exemption that is not needed, of the subject `subject` as
    /// the sentence names it (a document by its key, an artifact by its name in its
    /// space).
    pub(super) fn unneeded_of(
        &self,
        subject: &str,
        book: &BookName,
        link: &EntryName,
        exemption: JudgementId,
        why: &Unneeded,
    ) -> String {
        let because = match why {
            Unneeded::Reached { by } => format!(
                "`{book}.{link}` reaches it now, from {}",
                listed(
                    by.iter().map(|id| self.label(*id)),
                    Mark::Plain,
                    Join::Comma
                )
            ),
            Unneeded::Cover(CoverMiss::NotDeclared) => {
                format!("no link `{book}.{link}` into a book is declared now")
            }
            Unneeded::Cover(CoverMiss::NoCover) => {
                format!("`{book}.{link}` is declared with no cover now")
            }
            Unneeded::Cover(CoverMiss::OtherBook { target, doc_book }) => format!(
                "`{book}.{link}` points into `{target}` now, and the document is of `{doc_book}`"
            ),
            Unneeded::Cover(CoverMiss::NotDeclaredToArtifacts) => {
                format!("no link `{book}.{link}` to artifacts is declared now")
            }
            Unneeded::Cover(CoverMiss::OtherSpace {
                link_space,
                artifact_space,
            }) => format!(
                "`{book}.{link}` covers the names of `{link_space}` now, and the artifact is in `{artifact_space}`"
            ),
            Unneeded::Cover(CoverMiss::NotListed) => {
                format!("the selection of `{book}.{link}` does not list it now")
            }
            Unneeded::NotValid(tag) => tag.said_of(&"the document"),
            Unneeded::DecidedAgainst => {
                "the document is decided against, and no covering link expects to reach it"
                    .to_owned()
            }
        };
        format!("{exemption} exempts {subject} from `{book}.{link}` and is not needed: {because}")
    }

    /// The sentence of two or more exemptions of one document from one link.
    pub(super) fn repeated(
        &self,
        doc: Option<DocId>,
        book: &BookName,
        link: &EntryName,
        exemptions: &TwoOrMore<JudgementId>,
    ) -> String {
        let doc = doc.map_or_else(|| "it".to_owned(), |id| self.label(id));
        Self::repeated_of(&doc, book, link, exemptions)
    }

    /// The sentence of two or more exemptions of one subject from one link, as the
    /// sentence names it.
    pub(super) fn repeated_of(
        subject: &str,
        book: &BookName,
        link: &EntryName,
        exemptions: &TwoOrMore<JudgementId>,
    ) -> String {
        format!(
            "{} exemptions exempt {subject} from `{book}.{link}` ({}): one is enough, withdraw the others",
            exemptions.len(),
            listed(exemptions.iter(), Mark::Plain, Join::Comma)
        )
    }
}

/// The sentence for a name with nothing there.
pub(super) fn absent_detail(artifact: &ArtifactKey) -> String {
    format!(
        "{artifact} has nothing behind it now: the evidence is not written yet, or the name is wrong"
    )
}

/// The sentence for a link that still writes a name its pin was moved away from.
pub(super) fn moved_detail(
    link: &EntryName,
    written: &ArtifactKey,
    read_as: &ArtifactKey,
) -> String {
    format!(
        "the link `{link}` writes {written}, which was declared moved away since the approval pinned it: the pin is read as `{}` now, and the link is to write that name",
        read_as.name()
    )
}

/// The sentence for a name its adapter cannot tell about now.
fn unknown_detail(artifact: &ArtifactKey, why: &str) -> String {
    format!(
        "{artifact} cannot be told about now ({why}): it changes no freshness, and no approval can be made on it"
    )
}

fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Expired => "expired",
        Severity::Stale => "stale",
        Severity::Broken => "broken",
    }
}

fn upstream_status(status: &UpstreamStatus) -> &'static str {
    match status {
        UpstreamStatus::Pending => "pending (its approval was withdrawn)",
        UpstreamStatus::Unseconded => {
            "unseconded (its approval awaits the approvals of other names its rule asks for)"
        }
        UpstreamStatus::Denied => "denied",
        UpstreamStatus::Retired => "retired",
        UpstreamStatus::Deprecated {
            successors: Successors::None,
        } => "deprecated with no successor",
        UpstreamStatus::Deprecated {
            successors: Successors::Some(_),
        } => "deprecated; follow its successor",
    }
}

/// The hash an artifact pin kept, or `absent`.
fn pinned_hash(pin: &ArtifactPin) -> String {
    pin.seen()
        .state()
        .hash()
        .map_or_else(|| "absent".to_owned(), ToString::to_string)
}

/// A comparison as a person reads it: `bytes`, or `lines` with the prefixes it
/// leaves out.
fn comparison(comparison: &Comparison) -> String {
    let ignored: Vec<&str> = comparison.ignored().map(|p| p.as_str()).collect();
    if ignored.is_empty() {
        comparison.kind_word().to_owned()
    } else {
        format!(
            "{} (ignoring {})",
            comparison.kind_word(),
            ignored.join(" ")
        )
    }
}
