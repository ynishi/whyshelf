//! The answer of `cover` ([`CoverOut`]): what the covering links to artifacts expect
//! and how each stands, and the parts of it `check` tells beside its own
//! ([`ArtifactListsOut`], [`CoverLinkOut`]).
//!
//! A covering link to artifacts expects the names its selection takes. For each, the
//! answer says whether the listing of those names is the whole (`completeness`), and,
//! when it is, which of the names a document reaches, which an exemption keeps out of
//! the gaps and which are gaps. It says it in words and in numbers; whether it fails
//! is `check`'s to say.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{
    ArtifactCoverLists, CoverReport, RepeatedArtifact, UnneededArtifact,
};
use whyshelf_core::domain::shelf::artifact::{ArtifactKey, Completeness};
use whyshelf_core::domain::shelf::coverage::{LinkCover, NameCover};
use whyshelf_core::domain::shelf::schema::CoverSeverity;

use super::cause::unneeded_word;
use super::map::SelectionOut;
use super::words::{plural, ran_by, schema_problem};
use super::{HealthOut, Namer};

/// A name of an artifact a covering link expects to reach and nobody does.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UncoveredArtifactOut {
    /// The book that declares the link.
    pub book: String,
    /// The covering link.
    pub link: String,
    /// The name; for the space `file`, a path under the root.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// How strongly the link expects to reach it: `warn` or `error`.
    pub severity: &'static str,
}

/// An exemption of an artifact that stands and takes the place of a gap.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExemptArtifactOut {
    /// The name.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// The book that declares the link.
    pub book: String,
    /// The link whose cover the artifact is exempt from.
    pub link: String,
    /// How strongly the link expects to reach the names of its selection: `warn`
    /// or `error`.
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

/// An exemption of an artifact that stands and is no longer needed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnneededArtifactOut {
    /// The name.
    pub name: String,
    /// The space of the name.
    pub space: String,
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
    /// Why it is not needed now: `reached` (a document reaches the name through the
    /// link now), `not_declared` (no such link to artifacts is declared now),
    /// `no_cover` (the link is declared with no cover), `other_space` (the link
    /// covers the names of another space) or `not_listed` (the selection of the link
    /// does not list the name now; for a name that was moved, `judge moved` leaves
    /// the exemption of the old name here).
    pub why: &'static str,
    /// A sentence for a person.
    pub detail: String,
}

/// Two or more exemptions that stand of one artifact from one link.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepeatedArtifactOut {
    /// The name.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// The book that declares the link.
    pub book: String,
    /// The link the exemptions name.
    pub link: String,
    /// The exemptions' record ids, in the order of identity: withdraw all but one.
    pub exemptions: Vec<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// A covering link to artifacts: its selection, and whether the listing of the names
/// it takes is the whole.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoverLinkOut {
    /// The book that declares the link.
    pub book: String,
    /// The link.
    pub link: String,
    /// The space of the names.
    pub space: String,
    /// How strongly the link expects to reach the names its selection takes: `warn`
    /// or `error`.
    pub severity: &'static str,
    /// The selection it declares.
    pub selection: SelectionOut,
    /// Whether every name the selection takes is listed: `all`, `partial` (some are
    /// listed, and others may be there), `cannot_list` (the names cannot be listed
    /// here) or `not_asked`. A gap is told only of a listing that is `all`.
    pub completeness: &'static str,
    /// Why the listing is not the whole, for `partial` and `cannot_list`; null
    /// otherwise. This answer is a flat form of the domain's `Completeness` (the
    /// counts below are null unless the listing is `all`), kept flat so that fields
    /// are only ever added within a version.
    pub why: Option<String>,
    /// Whether the selection takes no name of a listing that is the whole: a cover
    /// that expects nothing, which is a mistake of the declaration more than a
    /// fulfilled one.
    pub selects_nothing: bool,
    /// How many names the selection takes; null when the listing is not the whole.
    pub selected: Option<usize>,
    /// How many of them a document reaches through the link; null when the listing
    /// is not the whole.
    pub reached: Option<usize>,
    /// How many are exempt from it; null when the listing is not the whole.
    pub exempt: Option<usize>,
    /// How many are gaps of it; null when the listing is not the whole.
    pub gaps: Option<usize>,
    /// A sentence for a person.
    pub detail: String,
}

/// What `check` and `cover` tell of the gaps and the exemptions of artifacts.
pub(super) struct ArtifactListsOut {
    pub(super) uncovered: Vec<UncoveredArtifactOut>,
    pub(super) exempt: Vec<ExemptArtifactOut>,
    pub(super) unneeded: Vec<UnneededArtifactOut>,
    pub(super) repeated: Vec<RepeatedArtifactOut>,
}

/// The word of how complete a listing is: the one place the words are written.
fn completeness_word(completeness: &Completeness) -> &'static str {
    match completeness {
        Completeness::All => "all",
        Completeness::Partial(_) => "partial",
        Completeness::CannotList(_) => "cannot_list",
        Completeness::NotAsked => "not_asked",
    }
}

/// Why a listing is not the whole, when it says.
fn completeness_why(completeness: &Completeness) -> Option<String> {
    match completeness {
        Completeness::Partial(why) | Completeness::CannotList(why) => Some(why.to_string()),
        Completeness::All | Completeness::NotAsked => None,
    }
}

/// How many names of a link stand in this way.
fn counted(link: &LinkCover, stands: impl Fn(&NameCover) -> bool) -> Option<usize> {
    link.completeness().is_all().then(|| {
        link.names()
            .iter()
            .filter(|(_, state)| stands(state))
            .count()
    })
}

/// The sentence of a covering link: what its selection takes and where it stands.
fn cover_detail(link: &LinkCover) -> String {
    let at = format!(
        "{}.{} (strength `{}`, space `{}`)",
        link.book(),
        link.link(),
        link.severity().word(),
        link.space()
    );
    if !link.completeness().is_all() {
        return format!("{at}: {}", link.completeness());
    }
    let names = link.names().len();
    if link.selects_nothing() {
        return format!("{at}: its selection takes no name, so it expects nothing");
    }
    let gaps = link
        .names()
        .iter()
        .filter(|(_, state)| *state == NameCover::Gap)
        .count();
    format!(
        "{at}: its selection takes {names} name{}, {gaps} reached by no document and not exempt",
        plural(names)
    )
}

/// A covering link as the answers tell it.
pub(super) fn cover_link(link: &LinkCover) -> CoverLinkOut {
    CoverLinkOut {
        book: link.book().to_string(),
        link: link.link().to_string(),
        space: link.space().to_string(),
        severity: link.severity().word(),
        selection: SelectionOut::of(link.selection()),
        completeness: completeness_word(link.completeness()),
        why: completeness_why(link.completeness()),
        selects_nothing: link.selects_nothing(),
        selected: link.completeness().is_all().then(|| link.names().len()),
        reached: counted(link, |state| *state == NameCover::Reached),
        exempt: counted(link, |state| matches!(state, NameCover::Exempt(_))),
        gaps: counted(link, |state| *state == NameCover::Gap),
        detail: cover_detail(link),
    }
}

/// The gaps and the exemptions of artifacts as the answers tell them.
pub(super) fn artifact_lists(lists: &ArtifactCoverLists, namer: &Namer<'_>) -> ArtifactListsOut {
    ArtifactListsOut {
        uncovered: lists
            .uncovered
            .iter()
            .map(|u| UncoveredArtifactOut {
                book: u.book.to_string(),
                link: u.link.to_string(),
                name: u.artifact.name().to_string(),
                space: u.artifact.space().to_string(),
                severity: u.severity.word(),
            })
            .collect(),
        exempt: lists
            .exempt
            .iter()
            .map(|e| ExemptArtifactOut {
                name: e.artifact.name().to_string(),
                space: e.artifact.space().to_string(),
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
        unneeded: lists
            .unneeded
            .iter()
            .map(|u| {
                let of = u.record.of();
                UnneededArtifactOut {
                    name: u.artifact.name().to_string(),
                    space: u.artifact.space().to_string(),
                    book: of.book().to_string(),
                    link: of.link().to_string(),
                    exemption: u.record.id().to_string(),
                    actor: u.record.actor().as_str().to_owned(),
                    ran_by: ran_by(u.record.attribution()),
                    at: u.record.at().millis(),
                    reason: u.record.reason().as_str().to_owned(),
                    why: unneeded_word(&u.why),
                    detail: unneeded_detail(namer, u),
                }
            })
            .collect(),
        repeated: lists
            .repeated
            .iter()
            .map(|r| RepeatedArtifactOut {
                name: r.artifact.name().to_string(),
                space: r.artifact.space().to_string(),
                book: r.book.to_string(),
                link: r.link.to_string(),
                exemptions: r.exemptions.iter().map(ToString::to_string).collect(),
                detail: repeated_detail(r),
            })
            .collect(),
    }
}

/// How an artifact is named in a sentence: its name, and its space.
fn artifact_label(artifact: &ArtifactKey) -> String {
    format!("`{}` in `{}`", artifact.name(), artifact.space())
}

/// The sentence of an exemption of an artifact that is not needed.
fn unneeded_detail(namer: &Namer<'_>, unneeded: &UnneededArtifact) -> String {
    let of = unneeded.record.of();
    namer.unneeded_of(
        &artifact_label(&unneeded.artifact),
        of.book(),
        of.link(),
        unneeded.record.id(),
        &unneeded.why,
    )
}

/// The sentence of two or more exemptions of one artifact from one link.
fn repeated_detail(repeated: &RepeatedArtifact) -> String {
    Namer::repeated_of(
        &artifact_label(&repeated.artifact),
        &repeated.book,
        &repeated.link,
        &repeated.exemptions,
    )
}

/// A name of a covering link that a selection takes, with how it stands.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NameStandOut {
    /// The name.
    pub name: String,
    /// How it stands: `reached` (a document that counts writes it under the link),
    /// `exempt` (nobody reaches it, and an exemption keeps it out of the gaps) or
    /// `gap`.
    pub stands: &'static str,
    /// For `exempt`, the exemption's record id (`judge withdraw` takes it); null
    /// otherwise.
    pub exemption: Option<String>,
}

/// A covering link of `cover`: as `check` tells it, with each name it takes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoverLinkNamesOut {
    /// The link, its selection and whether the listing is the whole.
    #[serde(flatten)]
    pub link: CoverLinkOut,
    /// Each name the selection takes, in the order of the names, with how it stands;
    /// empty unless the listing is the whole.
    pub names: Vec<NameStandOut>,
}

/// The answer of `cover`: what the covering links to artifacts expect, and how each
/// stands.
///
/// It fails nothing: a gap, a listing that is not the whole and a selection that takes
/// nothing are told here, and `check` is the gate on them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoverOut {
    /// The problems of the declaration, as `check` gives them; when there are any,
    /// nothing else was read and every list is empty.
    pub schema_problems: Vec<String>,
    /// Each covering link to artifacts (the one asked for, with `--link`), in the
    /// order the declaration writes the books and their links.
    pub links: Vec<CoverLinkNamesOut>,
    /// The names a covering link expects and nobody reaches, of either strength.
    pub uncovered: Vec<UncoveredArtifactOut>,
    /// The exemptions that stand and take the place of a gap, each with who, when and
    /// why.
    pub exempt: Vec<ExemptArtifactOut>,
    /// The exemptions that stand and are no longer needed, each with why.
    pub exemption_unneeded: Vec<UnneededArtifactOut>,
    /// The artifacts two or more exemptions that stand exempt from one link.
    pub exemption_repeated: Vec<RepeatedArtifactOut>,
    /// What is wrong around the documents; null when the declaration cannot be used.
    pub health: Option<HealthOut>,
}

/// The answer of `cover`.
pub fn cover(report: &CoverReport) -> CoverOut {
    match report {
        CoverReport::SchemaInvalid(problems) => CoverOut {
            schema_problems: problems.iter().map(schema_problem).collect(),
            links: Vec::new(),
            uncovered: Vec::new(),
            exempt: Vec::new(),
            exemption_unneeded: Vec::new(),
            exemption_repeated: Vec::new(),
            health: None,
        },
        CoverReport::Covered {
            links,
            lists,
            names,
            health,
        } => {
            let namer = Namer(names);
            let lists = artifact_lists(lists, &namer);
            CoverOut {
                schema_problems: Vec::new(),
                links: links.iter().map(link_names).collect(),
                uncovered: lists.uncovered,
                exempt: lists.exempt,
                exemption_unneeded: lists.unneeded,
                exemption_repeated: lists.repeated,
                health: Some(namer.health(health)),
            }
        }
    }
}

/// A covering link with each name it takes.
fn link_names(link: &LinkCover) -> CoverLinkNamesOut {
    CoverLinkNamesOut {
        link: cover_link(link),
        names: link
            .names()
            .iter()
            .map(|(name, state)| NameStandOut {
                name: name.to_string(),
                stands: match state {
                    NameCover::Reached => "reached",
                    NameCover::Exempt(_) => "exempt",
                    NameCover::Gap => "gap",
                },
                exemption: match state {
                    NameCover::Exempt(id) => Some(id.to_string()),
                    NameCover::Reached | NameCover::Gap => None,
                },
            })
            .collect(),
    }
}

/// The lines `check` writes on stderr about a covering link to artifacts of strength
/// `warn`: one per name nobody reaches, then one for each whose listing is not the
/// whole or whose selection takes nothing. A link of strength `error` has none: it
/// fails the check, and the answer is shown.
pub(super) fn warn_lines(lists: &ArtifactCoverLists, links: &[LinkCover]) -> Vec<String> {
    let gaps = lists
        .uncovered
        .iter()
        .filter(|u| u.severity == CoverSeverity::Warn)
        .map(|u| {
            format!(
                "warning: {} is reached by no {}.{} (cover = \"warn\")",
                artifact_label(&u.artifact),
                u.book,
                u.link
            )
        });
    let unsound = links
        .iter()
        .filter(|link| link.severity() == CoverSeverity::Warn)
        .filter_map(|link| {
            if !link.completeness().is_all() {
                Some(format!(
                    "warning: {}.{} cannot say which names it leaves unreached ({})",
                    link.book(),
                    link.link(),
                    link.completeness()
                ))
            } else if link.selects_nothing() {
                Some(format!(
                    "warning: the selection of {}.{} takes no name in `{}`, so it expects nothing",
                    link.book(),
                    link.link(),
                    link.space()
                ))
            } else {
                None
            }
        });
    gaps.chain(unsound).collect()
}

/// The lines `check` writes on stderr about exemptions of artifacts that are no
/// longer needed (each with the command that withdraws it), then those made twice.
pub(super) fn exemption_lines(lists: &ArtifactCoverLists, namer: &Namer<'_>) -> Vec<String> {
    let not_needed = lists.unneeded.iter().map(|u| {
        format!(
            "warning: {}; withdraw it with `judge withdraw {}`",
            unneeded_detail(namer, u),
            u.record.id()
        )
    });
    let twice = lists.repeated.iter().map(|r| {
        format!(
            "warning: {}, with `judge withdraw <id>`",
            repeated_detail(r)
        )
    });
    not_needed.chain(twice).collect()
}
