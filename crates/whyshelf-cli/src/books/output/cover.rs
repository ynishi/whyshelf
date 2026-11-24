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
    ArtifactCoverLists, CoverReport, RepeatedArtifact, UncoveredOf, UnneededArtifact,
};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactKey, ArtifactName, Completeness, ListingPart, PartRead,
};
use whyshelf_core::domain::shelf::coverage::{Assessed, LinkCover, NameCover, PartCover};
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
    /// What it is of the link's listing: `name` (a name the listing holds, reached by
    /// a document that names it under `link`) or `part` (a part the cover takes
    /// whole, reached by a document that names it under `whole`).
    pub of: &'static str,
    /// For a `part`, the link of the book it is reached through (the link of
    /// `parts = { whole = "<link>" }`); null for a `name`.
    pub whole: Option<String>,
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
    /// covers the names of another space), `not_listed` (the selection of the link
    /// does not list the name now, or holds the part no longer; for a name that was
    /// moved, `judge moved` leaves the exemption of the old name here) or
    /// `names_found` (names are found in the part now: an exemption of a part stands
    /// only while none is, so the part is a gap again until a document names it under
    /// the link of parts).
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
    /// listed, and others may be there, or parts of it are not listed whole),
    /// `cannot_list` (the names cannot be listed here) or `not_asked`. A gap is told
    /// only of a listing that is `all`, or that is in parts the cover takes whole
    /// (`assessed_whole`).
    pub completeness: &'static str,
    /// Why the listing is not the whole, for `partial` and `cannot_list`; null
    /// otherwise. This answer is a flat form of the domain's `Completeness` (the
    /// counts below are null unless the cover is `assessed_whole`), kept flat so that
    /// fields are only ever added within a version.
    pub why: Option<String>,
    /// The link of the book through which the cover takes each part of a listing
    /// whole (`parts = { whole = "<link>" }`); null when it declares none.
    pub parts_whole: Option<String>,
    /// Whether everything the selection takes is assessed: the listing is `all`, or
    /// it is in parts and the cover takes each part whole through `parts_whole`, and
    /// the link has no `problems` (a link with one is assessed in nothing).
    pub assessed_whole: bool,
    /// Whether the selection takes nothing of what is assessed whole: a cover that
    /// expects nothing, which is a mistake of the declaration more than a fulfilled
    /// one.
    pub selects_nothing: bool,
    /// How many names and parts the selection takes; null unless `assessed_whole`.
    pub selected: Option<usize>,
    /// How many of them a document reaches through the link (a part, through
    /// `parts_whole`); null unless `assessed_whole`.
    pub reached: Option<usize>,
    /// How many are exempt from it; null unless `assessed_whole`.
    pub exempt: Option<usize>,
    /// How many are gaps of it; null unless `assessed_whole`.
    pub gaps: Option<usize>,
    /// What keeps the link from being assessed as it declares (a part in another
    /// space than that of `parts_whole`), each a sentence; any of them fails `check`,
    /// whatever the strength, and leaves the link not `assessed_whole`, its counts
    /// null.
    pub problems: Vec<String>,
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

/// How a name or a part of a covering link stands, in one word, as the answers write
/// it: `reached`, `exempt` or `gap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stands {
    Reached,
    Exempt,
    Gap,
}

impl Stands {
    fn of_name(state: NameCover) -> Self {
        match state {
            NameCover::Reached => Self::Reached,
            NameCover::Exempt(_) => Self::Exempt,
            NameCover::Gap => Self::Gap,
        }
    }

    fn of_part(state: PartCover) -> Self {
        match state {
            PartCover::Reached => Self::Reached,
            PartCover::Exempt(_) => Self::Exempt,
            PartCover::Gap => Self::Gap,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Reached => "reached",
            Self::Exempt => "exempt",
            Self::Gap => "gap",
        }
    }
}

/// The names and the parts of a link with how each stands, when it is assessed whole.
type Standing<'a> = (
    &'a [(ArtifactName, NameCover)],
    &'a [(ListingPart, PartCover)],
);

/// The names and the parts of a link, when it is assessed whole: the one place the
/// answers read [`Assessed`].
fn whole_of(link: &LinkCover) -> Option<Standing<'_>> {
    match link.assessed() {
        Assessed::Whole { names, parts } => Some((names, parts)),
        Assessed::NotWhole | Assessed::Unsound(_) => None,
    }
}

/// The problems of a link, as sentences: none unless it is unsound.
fn problems_of(link: &LinkCover) -> Vec<String> {
    match link.assessed() {
        Assessed::Unsound(problems) => problems.iter().map(ToString::to_string).collect(),
        Assessed::Whole { .. } | Assessed::NotWhole => Vec::new(),
    }
}

/// How many names and parts of a link stand in this way; none unless the link is
/// assessed whole.
fn counted(link: &LinkCover, stands: Stands) -> Option<usize> {
    whole_of(link).map(|(names, parts)| {
        let names = names.iter().map(|(_, s)| Stands::of_name(*s));
        let parts = parts.iter().map(|(_, s)| Stands::of_part(*s));
        names.chain(parts).filter(|s| *s == stands).count()
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
    let problems = problems_of(link);
    if !problems.is_empty() {
        return format!("{at}: {}", problems.join("; "));
    }
    let Some((names, parts)) = whole_of(link) else {
        return format!("{at}: {}", link.completeness());
    };
    if link.selects_nothing() {
        return format!("{at}: its selection takes no name, so it expects nothing");
    }
    let gaps = counted(link, Stands::Gap).unwrap_or_default();
    let names = names.len();
    match (parts.len(), link.parts_whole()) {
        (0, _) | (_, None) => format!(
            "{at}: its selection takes {names} name{}, {gaps} reached by no document and not exempt",
            plural(names)
        ),
        (parts, Some(whole)) => format!(
            "{at}: its selection takes {names} name{} and {parts} part{} taken whole through `{whole}`, {gaps} reached by no document and not exempt",
            plural(names),
            plural(parts)
        ),
    }
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
        parts_whole: link.parts_whole().map(ToString::to_string),
        assessed_whole: link.is_whole(),
        selects_nothing: link.selects_nothing(),
        selected: whole_of(link).map(|(names, parts)| names.len() + parts.len()),
        reached: counted(link, Stands::Reached),
        exempt: counted(link, Stands::Exempt),
        gaps: counted(link, Stands::Gap),
        problems: problems_of(link),
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
                of: match u.of {
                    UncoveredOf::Name => "name",
                    UncoveredOf::Part { .. } => "part",
                },
                whole: match &u.of {
                    UncoveredOf::Name => None,
                    UncoveredOf::Part { whole } => Some(whole.to_string()),
                },
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

/// A part of a listing a covering link takes whole, with how it stands.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PartStandOut {
    /// The name of the part, the artifact it is as a whole; for the space `file`, a
    /// path under the root.
    pub name: String,
    /// The space of the part.
    pub space: String,
    /// What was read of it: `found` (read, with the names in `found`) or `unread`
    /// (it could not be read).
    pub read: &'static str,
    /// The names found in it, when `found`; empty otherwise.
    pub found: Vec<String>,
    /// Why it is not listed whole, as the adapter says it.
    pub why: String,
    /// How it stands: `reached` (a document that counts writes it under the link the
    /// cover takes parts whole through), `exempt`, or `gap` (nobody reaches it,
    /// whatever was read of it: one read with no name found in it may still hold one
    /// the adapter does not see).
    pub stands: &'static str,
    /// For `exempt`, the exemption's record id; null otherwise.
    pub exemption: Option<String>,
}

/// A covering link of `cover`: as `check` tells it, with each name it takes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoverLinkNamesOut {
    /// The link, its selection and whether the listing is the whole.
    #[serde(flatten)]
    pub link: CoverLinkOut,
    /// Each name the selection takes, in the order of the names, with how it stands;
    /// empty unless it is assessed whole.
    pub names: Vec<NameStandOut>,
    /// Each part of the listing the cover takes whole, in the adapter's order, with
    /// how it stands; empty unless it is assessed whole in parts.
    pub parts: Vec<PartStandOut>,
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
    let (names, parts) = whole_of(link).unwrap_or((&[], &[]));
    CoverLinkNamesOut {
        link: cover_link(link),
        names: names
            .iter()
            .map(|(name, state)| NameStandOut {
                name: name.to_string(),
                stands: Stands::of_name(*state).word(),
                exemption: match state {
                    NameCover::Exempt(id) => Some(id.to_string()),
                    NameCover::Reached | NameCover::Gap => None,
                },
            })
            .collect(),
        parts: parts
            .iter()
            .map(|(part, state)| {
                let (read, found) = match part.read() {
                    PartRead::Found(names) => {
                        ("found", names.iter().map(ToString::to_string).collect())
                    }
                    PartRead::Unread => ("unread", Vec::new()),
                };
                PartStandOut {
                    name: part.key().name().to_string(),
                    space: part.key().space().to_string(),
                    read,
                    found,
                    why: part.why().to_string(),
                    stands: Stands::of_part(*state).word(),
                    exemption: match state {
                        PartCover::Exempt(id) => Some(id.to_string()),
                        PartCover::Reached | PartCover::Gap => None,
                    },
                }
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
            // A part is reached only through the link the cover takes parts whole
            // through, which is named so that the line says where to write it.
            let through = match &u.of {
                UncoveredOf::Name => String::new(),
                UncoveredOf::Part { whole } => {
                    format!(", a part taken whole through {}.{whole}", u.book)
                }
            };
            format!(
                "warning: {} is reached by no {}.{} (cover = \"warn\"){through}",
                artifact_label(&u.artifact),
                u.book,
                u.link
            )
        });
    let unsound = links
        .iter()
        .filter(|link| link.severity() == CoverSeverity::Warn)
        .filter_map(|link| {
            if matches!(link.assessed(), Assessed::Unsound(_)) {
                // It fails the check whatever its strength, and the answer is shown.
                None
            } else if !link.is_whole() {
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
