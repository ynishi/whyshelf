//! The answers of `judge` and its subcommands: the records written, each as it is in
//! the ledger, the approvals left out with why, and the lines written on stderr.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use whyshelf_core::app::shelf::{
    ChosenApproved, Declared, DeclaredAbout, Exempted, Judged, JudgedAll, LeftOut, LeftOutWhy,
    MovedPrefix, Names,
};
use whyshelf_core::domain::shelf::ledger::{Moved, Record};
use whyshelf_core::domain::shelf::order::{Hold, Rule};
use whyshelf_infra::shelf::ledger::to_json;

use super::cause::absent_detail;
use super::words::hold_word;
use super::{CauseOut, DocRef, Namer};

/// The answer of `judge`: every record written, and every approval left out, each in
/// the order the documents were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedAllOut {
    /// The judgements.
    pub judged: Vec<JudgedOut>,
    /// The approvals not made, each `repeats`: one made now would be by one who
    /// already approved the content that stands, and lapse when that reading and the
    /// approval that stands lapse (`--again` makes it anyway). Another actor's approval of that content is a
    /// second reading, and is made. Empty for every verb but `approve`.
    pub left_out: Vec<LeftOutOut>,
}

/// The answer of `judge approve --stale` and `--pending`: every approval made, upstream
/// first, and those left out.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChosenApprovedOut {
    /// The judgements, upstream first, each as `judge` answers it with the rule that
    /// took its document; empty when nothing could be approved.
    pub judged: Vec<ChosenJudgedOut>,
    /// The documents taken and not approved, by identity, each with the rule that
    /// took it.
    pub left_out: Vec<ChosenLeftOutOut>,
}

/// The rule that took a document into a run of `judge approve --stale` or
/// `--pending`.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleOut {
    /// Its approval was expired, stale or broken.
    Stale,
    /// No decision stood about it.
    Pending,
}

/// One approval of `judge approve --stale` or `--pending`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChosenJudgedOut {
    /// The judgement, as `judge` answers it.
    #[serde(flatten)]
    pub judged: JudgedOut,
    /// The rule that took the document.
    pub rule: RuleOut,
}

/// One document `judge approve --stale` or `--pending` took and did not approve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChosenLeftOutOut {
    /// The document and why, as `judge` answers it.
    #[serde(flatten)]
    pub left: LeftOutOut,
    /// The rule that took the document.
    pub rule: RuleOut,
}

/// An approval that `judge approve`, `judge approve --stale` or `--pending` did not
/// make.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftOutOut {
    /// The document.
    pub doc: DocRef,
    /// Why: of `--stale`, `broken` (its approval is broken now); of `--stale` and
    /// `--pending`, `refused` (the model refuses its approval now) or `stands_on` (a
    /// `depends` link names a document that is broken, pending or stale and not in
    /// the run, or left out); of every one, `repeats` (one made now would be by one
    /// who already approved the content that stands, and lapse when that reading
    /// and the approval that stands lapse; of `--stale`, one stale only through an upstream's upstream the run
    /// approved again on a new hash); of `--stale`, `second_reading` (one made now
    /// would be of the content that stands by an actor who has not approved it, which
    /// a run that chose the document by a rule does not make: name the document); of
    /// `--stale` and `--pending`, `not_admitted` (who may decide does not admit the
    /// actor to approve in the document's book: the run approves the others, and
    /// what stands on it is held back as `left_out`).
    pub reason: &'static str,
    /// A sentence for a person.
    pub detail: String,
    /// For `stands_on`, the document that holds it back.
    pub upstream: Option<DocRef>,
    /// For `stands_on`, the link that names it.
    pub link: Option<String>,
    /// For `stands_on`, why that document holds it back: `broken`, `pending` (and
    /// not in the run), `stale` (and not in the run), `awaiting` (its approval awaits
    /// the approvals of other names, at any freshness) or `left_out` (refused, broken
    /// or `not_admitted` in the run; one that `repeats`, or is a `second_reading`,
    /// stands, and holds nothing back).
    pub upstream_is: Option<&'static str>,
    /// For `repeats` and `second_reading`, the approval that stands (its record's
    /// id).
    pub approval: Option<String>,
    /// For `broken`, the kinds of its causes, each once, in the order of `causes`;
    /// empty otherwise.
    pub kinds: Vec<&'static str>,
    /// For `broken`, why its approval does not hold; empty otherwise.
    pub causes: Vec<CauseOut>,
}

/// The answer of `judge same`: every declaration written, in the order the
/// documents and files were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeclaredAllOut {
    /// The declarations.
    pub declared: Vec<DeclaredOut>,
}

/// One declaration that an edit keeps the meaning.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeclaredOut {
    /// The document it is about, if it is about one.
    pub doc: Option<DocRef>,
    /// The name of the artifact it is about, if it is about one: for the space
    /// `file`, a path under the root.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The record written, as it is in the ledger (its `from` lists the earlier
    /// hashes, its `to` the hash now).
    pub record: Value,
}

/// The answer of `judge moved`: every declaration of a move written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedAllOut {
    /// The declarations.
    pub moved: Vec<MovedOut>,
}

/// The answer of `judge moved --prefix`: every declaration of a move written, one for
/// each name under the prefix.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedPrefixOut {
    /// The declarations, as `judge moved` answers them, in the order of the spaces
    /// and then of the names.
    pub moved: Vec<MovedOut>,
    /// The spaces names are pinned in whose adapter takes no prefix: names of them may
    /// be under the prefix, and none was moved.
    pub not_asked: Vec<String>,
}

/// One declaration that an artifact moved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedOut {
    /// The space both names are of.
    pub space: String,
    /// The old name: for the space `file`, a path under the root.
    pub from: String,
    /// The new name.
    pub to: String,
    /// The record written, as it is in the ledger (its `id` is what `judge withdraw`
    /// takes).
    pub record: Value,
}

/// The answer of `judge exempt`: every exemption written, in the order the
/// documents were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExemptedAllOut {
    /// The exemptions.
    pub exempt: Vec<ExemptedOut>,
}

/// One exemption written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExemptedOut {
    /// The document; null for an artifact.
    pub doc: Option<DocRef>,
    /// The name of the artifact (for the space `file`, a path under the root); null
    /// for a document.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The book that declares the link.
    pub book: String,
    /// The link whose cover the document or the artifact is exempt from.
    pub link: String,
    /// The record written, as it is in the ledger (its `id` is what `judge withdraw`
    /// takes).
    pub record: Value,
}

/// The answer of `judge withdraw`: the revocation written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WithdrawnOut {
    /// The record written, as it is in the ledger (its `targets` are the records
    /// withdrawn).
    pub record: Value,
}

/// One judgement written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedOut {
    /// The document judged.
    pub doc: DocRef,
    /// The record written, as it is in the ledger.
    pub record: Value,
    /// For a deprecation, what replaces the document.
    pub successors: Vec<DocRef>,
    /// For an approval, the names of the document's links to artifacts with nothing
    /// there; a `depends` link pinned each as absent.
    pub absent: Vec<AbsentNameOut>,
}

/// A name of a link to artifacts with nothing there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentNameOut {
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

/// The answer of `judge`.
pub fn judged(all: &JudgedAll) -> JudgedAllOut {
    let names = Names::default();
    let namer = Namer(&names);
    JudgedAllOut {
        judged: all
            .judged
            .iter()
            .map(|judged| namer.judged(judged))
            .collect(),
        left_out: all
            .left_out
            .iter()
            .map(|left| namer.left_out(left))
            .collect(),
    }
}

/// The lines `judge` writes on stderr: one per approval it left out, in the order of
/// `left_out`, each with the flag that makes it anyway.
pub fn judged_warnings(all: &JudgedAll) -> Vec<String> {
    let names = Names::default();
    let namer = Namer(&names);
    all.left_out
        .iter()
        .map(|left| {
            format!(
                "{}; pass --again to approve it anyway",
                left_out_warning(&namer, left, true)
            )
        })
        .collect()
}

/// The rule that took a document into a run of approvals, as the answer writes it.
fn rule_out(rule: Rule) -> RuleOut {
    match rule {
        Rule::Stale => RuleOut::Stale,
        Rule::Pending => RuleOut::Pending,
    }
}

/// The answer of `judge approve --stale` and `--pending`.
pub fn chosen_approved(done: &ChosenApproved) -> ChosenApprovedOut {
    let namer = Namer(&done.names);
    ChosenApprovedOut {
        judged: done
            .judged
            .iter()
            .map(|one| ChosenJudgedOut {
                judged: namer.judged(&one.judged),
                rule: rule_out(one.rule),
            })
            .collect(),
        left_out: done
            .left_out
            .iter()
            .map(|one| ChosenLeftOutOut {
                left: namer.left_out(&one.left),
                rule: rule_out(one.rule),
            })
            .collect(),
    }
}

/// The line on stderr for an approval left out: `approved again` of one that had an
/// approval (`again`), `approved` of a pending one.
fn left_out_warning(namer: &Namer<'_>, left: &LeftOut, again: bool) -> String {
    let doc = left
        .key
        .as_ref()
        .map_or_else(|| left.doc.to_string(), ToString::to_string);
    let approved = if again { "approved again" } else { "approved" };
    match &left.why {
        LeftOutWhy::Broken(_) => format!(
            "warning: {doc} is broken and was not {approved} ({})",
            left_out_kinds(namer, left).join(", ")
        ),
        LeftOutWhy::Refused(_)
        | LeftOutWhy::StandsOn { .. }
        | LeftOutWhy::Repeats { .. }
        | LeftOutWhy::SecondReading { .. }
        | LeftOutWhy::NotAdmitted(_) => {
            format!(
                "warning: {doc} was not {approved}: {}",
                left_out_detail(namer, left)
            )
        }
    }
}

/// The lines `judge approve --stale` and `--pending` write on stderr: one per
/// document left out, in the order of `left_out`, then one per name with nothing
/// there that an approval made pinned as absent, in the order of `judged`.
pub fn chosen_approved_warnings(done: &ChosenApproved) -> Vec<String> {
    let namer = Namer(&done.names);
    let left = done
        .left_out
        .iter()
        .map(|one| left_out_warning(&namer, &one.left, one.rule == Rule::Stale));
    let absent = done
        .judged
        .iter()
        .map(|one| &one.judged)
        .flat_map(|judged| {
            let doc = judged
                .key
                .as_ref()
                .map_or_else(|| judged.subject.to_string(), ToString::to_string);
            judged.absent.iter().map(move |(_, artifact)| {
                format!(
                    "warning: {doc} pins {} in {}, which has nothing behind it",
                    artifact.name(),
                    artifact.space()
                )
            })
        });
    left.chain(absent).collect()
}

/// The kinds of the causes of a broken approval left out, each once, in the order
/// found; none for one left out for another reason.
fn left_out_kinds(namer: &Namer<'_>, left: &LeftOut) -> Vec<&'static str> {
    let LeftOutWhy::Broken(degradation) = &left.why else {
        return Vec::new();
    };
    let mut kinds: Vec<&'static str> = Vec::new();
    for cause in degradation.causes().iter() {
        let kind = namer.cause(cause).kind;
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// The sentence for an approval left out.
fn left_out_detail(namer: &Namer<'_>, left: &LeftOut) -> String {
    match &left.why {
        LeftOutWhy::Broken(_) => format!(
            "its approval is broken ({})",
            left_out_kinds(namer, left).join(", ")
        ),
        LeftOutWhy::Refused(refusal) => refusal.to_string(),
        LeftOutWhy::Repeats { approval } => format!(
            "an approval now would pin what its approval {approval} pins, by one who already approved that content, and lapse when that reading and the approval that stands lapse: it would only repeat it"
        ),
        LeftOutWhy::SecondReading { approval } => format!(
            "an approval now would pin what its approval {approval} pins, by one who has not approved it: a second reading is made by naming the document, with `judge approve`"
        ),
        LeftOutWhy::NotAdmitted(not) => not.why(),
        LeftOutWhy::StandsOn { held, key } => {
            let upstream = key
                .as_ref()
                .map_or_else(|| held.upstream.to_string(), ToString::to_string);
            let link = &held.link;
            match held.hold {
                Hold::Broken => format!("the link `{link}` names {upstream}, which is broken"),
                Hold::Pending => format!(
                    "the link `{link}` names {upstream}, which is pending: approve it first with `judge approve`"
                ),
                Hold::Stale => format!(
                    "the link `{link}` names {upstream}, which is stale: approve it again first, with `--stale`"
                ),
                Hold::Awaiting => format!(
                    "the link `{link}` names {upstream}, whose approval awaits the approvals of other names its rule asks for: another name is to approve it first, with `judge approve`"
                ),
                Hold::LeftOut => {
                    format!("the link `{link}` names {upstream}, which was left out of this run")
                }
            }
        }
    }
}

impl Namer<'_> {
    fn left_out(&self, left: &LeftOut) -> LeftOutOut {
        let (upstream, link, upstream_is) = match &left.why {
            LeftOutWhy::StandsOn { held, .. } => (
                Some(self.doc(held.upstream)),
                Some(held.link.to_string()),
                Some(hold_word(held.hold)),
            ),
            LeftOutWhy::Broken(_)
            | LeftOutWhy::Refused(_)
            | LeftOutWhy::Repeats { .. }
            | LeftOutWhy::SecondReading { .. }
            | LeftOutWhy::NotAdmitted(_) => (None, None, None),
        };
        LeftOutOut {
            doc: DocRef {
                id: left.doc.to_string(),
                key: left.key.as_ref().map(ToString::to_string),
            },
            reason: match left.why {
                LeftOutWhy::Broken(_) => "broken",
                LeftOutWhy::Refused(_) => "refused",
                LeftOutWhy::StandsOn { .. } => "stands_on",
                LeftOutWhy::Repeats { .. } => "repeats",
                LeftOutWhy::SecondReading { .. } => "second_reading",
                LeftOutWhy::NotAdmitted(_) => "not_admitted",
            },
            detail: left_out_detail(self, left),
            upstream,
            link,
            upstream_is,
            approval: match &left.why {
                LeftOutWhy::Repeats { approval } | LeftOutWhy::SecondReading { approval } => {
                    Some(approval.to_string())
                }
                LeftOutWhy::Broken(_)
                | LeftOutWhy::Refused(_)
                | LeftOutWhy::StandsOn { .. }
                | LeftOutWhy::NotAdmitted(_) => None,
            },
            kinds: left_out_kinds(self, left),
            causes: match &left.why {
                LeftOutWhy::Broken(degradation) => {
                    degradation.causes().iter().map(|c| self.cause(c)).collect()
                }
                LeftOutWhy::Refused(_)
                | LeftOutWhy::StandsOn { .. }
                | LeftOutWhy::Repeats { .. }
                | LeftOutWhy::SecondReading { .. }
                | LeftOutWhy::NotAdmitted(_) => Vec::new(),
            },
        }
    }

    fn judged(&self, judged: &Judged) -> JudgedOut {
        JudgedOut {
            doc: DocRef {
                id: judged.subject.to_string(),
                key: judged.key.as_ref().map(ToString::to_string),
            },
            record: to_json(&judged.record.to_canon()),
            successors: judged
                .successors
                .as_ref()
                .map(|s| self.successors(s))
                .unwrap_or_default(),
            absent: judged
                .absent
                .iter()
                .map(|(link, artifact)| AbsentNameOut {
                    link: link.to_string(),
                    path: artifact.name().to_string(),
                    name: artifact.name().to_string(),
                    space: artifact.space().to_string(),
                    detail: absent_detail(artifact),
                })
                .collect(),
        }
    }
}

/// The answer of `judge same`.
pub fn same_meaning(all: &[Declared]) -> DeclaredAllOut {
    DeclaredAllOut {
        declared: all
            .iter()
            .map(|one| {
                let (doc, artifact) = match &one.about {
                    DeclaredAbout::Doc { id, key } => (
                        Some(DocRef {
                            id: id.to_string(),
                            key: key.as_ref().map(ToString::to_string),
                        }),
                        None,
                    ),
                    DeclaredAbout::Artifact { artifact } => (None, Some(artifact)),
                };
                DeclaredOut {
                    doc,
                    artifact: artifact.map(|a| a.name().to_string()),
                    space: artifact.map(|a| a.space().to_string()),
                    record: to_json(&one.record.to_canon()),
                }
            })
            .collect(),
    }
}

/// The answer of `judge moved`.
pub fn moved(all: &[Moved]) -> MovedAllOut {
    MovedAllOut {
        moved: all.iter().map(moved_out).collect(),
    }
}

/// One declaration of a move, as `judge moved` and `judge moved --prefix` answer it.
fn moved_out(one: &Moved) -> MovedOut {
    MovedOut {
        space: one.of().space().to_string(),
        from: one.of().from_name().to_string(),
        to: one.of().to_name().to_string(),
        record: to_json(&Record::Moved(one.clone()).to_canon()),
    }
}

/// The answer of `judge moved --prefix`: the answer of `judge moved`, and the spaces
/// whose adapter takes no prefix.
pub fn moved_prefix(done: &MovedPrefix) -> MovedPrefixOut {
    MovedPrefixOut {
        moved: done.moved.iter().map(moved_out).collect(),
        not_asked: done.not_asked.iter().map(ToString::to_string).collect(),
    }
}

/// The answer of `judge exempt`.
pub fn exempted(all: &[Exempted]) -> ExemptedAllOut {
    ExemptedAllOut {
        exempt: all
            .iter()
            .map(|one| ExemptedOut {
                doc: one.doc().map(|id| DocRef {
                    id: id.to_string(),
                    key: one.key.as_ref().map(ToString::to_string),
                }),
                artifact: one.artifact().map(|a| a.name().to_string()),
                space: one.artifact().map(|a| a.space().to_string()),
                book: one.record.of().book().to_string(),
                link: one.record.of().link().to_string(),
                record: to_json(&Record::Exempt(one.record.clone()).to_canon()),
            })
            .collect(),
    }
}

/// The answer of `judge withdraw`.
pub fn withdrawn(record: &Record) -> WithdrawnOut {
    WithdrawnOut {
        record: to_json(&record.to_canon()),
    }
}
