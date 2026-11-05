//! From the text of a document to a raw value of the application.
//!
//! The file formats (a Finding, a Spec, a Design) are written down below; the
//! front matter is YAML read with `serde_norway`, and the sections after it are
//! Markdown. Nothing here resolves a reference or checks a revision: that is the
//! application's business through its ports, so a value leaves this module with
//! its `grounds` and `spec` as [`Ref`]s and its `revision` as a [`RawRevision`].
//!
//! **Scalars are read as the text they were written as.** A revision such as
//! `1234567890123456789012345678901234567890` or `123456e789…`, or a version such
//! as `1.10`, would be a number to a YAML reader that guesses types, and a number
//! loses digits (`1.10` becomes `1.1`). Every field below is declared as text, so
//! its characters reach the hash exactly as typed.
//!
//! A problem in a file is a [`UnparsedReason`], never a panic and never the first
//! one only: every key that is missing or wrong is reported, so a person fixes
//! the file in one pass.
//!
//! **A file is read as the kind it is registered as** ([`read`]), whatever else its
//! keys hold. A file nobody registered yet has no such kind, and [`parse`] tells it by
//! its keys (`finding_type` makes a Finding, `grounds` a Spec, `spec` a Design) when
//! they name exactly one; a file whose keys name two or none is not registered on a
//! guess, and the person is asked for `--kind`.
//!
//! **No key is dropped without a word.** Every key of the front matter is one of four
//! things (see [`whyshelf_core::domain::front`]):
//!
//! | the key | is | what happens |
//! |---|---|---|
//! | one the kind reads | content | it is read, and it is what the hash covers |
//! | `tags`, `kind`, `owner`, `alias` | the file's classification | returned with the file; taken in once at registration, then compared with the store and shown as a difference when they disagree. The tool never rewrites the file |
//! | a key another kind reads (`crux` in a Spec) | a foreign section | `ForeignSection`: the file is Invalid |
//! | none of the above (`evidance`) | a typing mistake | `UnknownKey`: the file is Invalid |
//!
//! Classification keys and the text of sections the kind does not read are not in any
//! hash, so changing them cannot remove an approval.
//!
//! | what is wrong | reported as | the key it names |
//! |---|---|---|
//! | a line starts with seven `<` or seven `>` (what a failed merge leaves) | `MergeConflictMarker` | none; nothing else is read |
//! | no front matter, YAML that does not parse, or a front matter that is not a mapping | `MalformedFrontmatter` | none; nothing else is read |
//! | a required key is absent | `MissingField` | the key, such as `context` or `crux.question`; `## Contract` for the section |
//! | a key is present and refused | `InvalidField` | the key with its position, such as `accept.pins[1].id` or `crux.options[0].reason` |
//! | a key of another kind, or of none | `ForeignSection`, `UnknownKey` | the key |
//! | the file is of another kind than the one it is registered as | `KindChanged` | none |
//! | a reference names nothing, names several, or the wrong kind | `DanglingRef`, `AmbiguousRef`, `WrongKind` | raised by the application after parsing, not here |
//!
//! # The keys of a Finding
//!
//! Every Finding has `finding_type`, `title` and `evidence`; the rest depends on the
//! type. The body, everything after the front matter, is the text of the Finding.
//!
//! | `finding_type` | further keys (all required) |
//! |---|---|
//! | `measured_data` | `measured_at` (RFC 3339, the calendar is checked), `revision`, `environment` |
//! | `usage_observation` | `context`, `reproducibility` (`reproducible` or `not-reproducible`) |
//! | `external_source` | `source` (a URI, or a citation as text), `version`, `maturity` (`experimental`, `emerging`, `established` or `standard`) |
//! | `code_observation` | `ref` (`path:line`), `revision`, `method` (`read` or `executed`) |
//! | `domain_constraint` | `authority`, `scope`, `review_condition` |
//!
//! `evidence` is `measured`, `theoretical` or `estimated`. These words, and the
//! others of the table, are the names the domain gives them (`ALL` and `name()` of
//! each type): this module keeps no table of its own, and reads English only. A `revision` is read here
//! as text only; whether it names a revision is asked of the version-control port
//! when the entity is built, so a revision that does not exist makes the file
//! unparsed there.
//!
//! # Examples
//!
//! The complete files below are the examples of the model documentation
//! (`whyshelf_core::domain::finding`, `spec` and `design`); this checks that they
//! read.
//!
//! ```
//! use whyshelf_core::domain::event::DocKind;
//! use whyshelf_infra::{markdown::normalize, parse::parse};
//!
//! let findings = [
//!     "---\nfinding_type: measured_data\ntitle: \"Median login time on a cold start\"\nevidence: measured\nmeasured_at: \"2026-11-04T09:30:00Z\"\nrevision: \"a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\"\nenvironment: \"mid-range phone, release build, office network\"\n---\nThe median of 40 cold starts was 8.2 s.\n",
//!     "---\nfinding_type: usage_observation\ntitle: \"Testers sign in again after every app switch\"\nevidence: measured\ncontext: \"demo app, sign in, switch to the mail app and back\"\nreproducibility: reproducible\n---\nThree of three testers were asked to sign in again.\n",
//!     "---\nfinding_type: external_source\ntitle: \"Session lifetimes in the platform guidelines\"\nevidence: theoretical\nsource: \"https://example.org/guidelines/sessions\"\nversion: \"2026 edition\"\nmaturity: established\n---\nThe guidelines advise sessions of at least five minutes.\n",
//!     "---\nfinding_type: code_observation\ntitle: \"The session is dropped when the app is backgrounded\"\nevidence: measured\nref: \"src/session.rs:42\"\nrevision: \"a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\"\nmethod: read\n---\n`Session::on_background` clears the token.\n",
//!     "---\nfinding_type: domain_constraint\ntitle: \"Tokens must not outlive one hour\"\nevidence: theoretical\nauthority: \"the security team\"\nscope: \"all clients of the sign-in service\"\nreview_condition: \"when the sign-in provider changes\"\n---\nThe policy forbids tokens that live longer than one hour.\n",
//! ];
//! for text in findings {
//!     let doc = parse(&normalize(text)).expect("a complete Finding file reads");
//!     assert_eq!(doc.kind(), DocKind::Finding);
//! }
//!
//! let spec = "---\ngrounds:\n  - finding/slow-login\n  - finding/session-dropped\naccept:\n  pins:\n    - id: PIN-LOGIN-CACHE\n      contract: \"The session is reused for five minutes\"\n    - id: UT-LOGIN-EXPIRY\n      contract: \"An expired session is not reused\"\n  hyp:\n    - id: HYP-FASTER-LOGIN\n      metric: \"median login time\"\n      target: \"under 2 s\"\n---\n\n## Contract\nA session younger than five minutes is reused.\n\n## Design Notes\nFree notes.\n";
//! assert_eq!(parse(&normalize(spec)).unwrap().kind(), DocKind::Spec);
//!
//! let design = "---\nspec: spec/session-reuse\ncrux:\n  question: \"Where is the session kept?\"\n  options:\n    - id: opt-memory\n      summary: \"Keep it in memory\"\n      chosen: true\n      reason: \"No new storage to secure\"\n    - id: opt-disk\n      summary: \"Keep it on disk\"\n      rejected: \"A file to protect and to clean up\"\n    - id: opt-server\n      summary: \"Keep it on the server only\"\n      rejected: \"Every start would need a round trip\"\nitems:\n  - number: 1\n    summary: \"Reuse the session inside the window\"\n    pins: [PIN-LOGIN-CACHE]\n  - number: 2\n    summary: \"Drop the session when it expires\"\n    pins: [UT-LOGIN-EXPIRY]\n---\n\n## Design Notes\nFree notes.\n";
//! assert_eq!(parse(&normalize(design)).unwrap().kind(), DocKind::Design);
//!
//! // Every problem is reported, not the first: two keys are missing here.
//! let broken = "---\nfinding_type: usage_observation\ntitle: \"t\"\nevidence: measured\n---\nbody\n";
//! let reasons = parse(&normalize(broken)).unwrap_err();
//! assert_eq!(reasons.iter().count(), 2);
//! ```
//!
//! # Finding
//!
//! ```yaml
//! finding_type: usage_observation   # measured_data | usage_observation | external_source
//!                                   # | code_observation | domain_constraint
//! title: "Login feels slow on first open"
//! evidence: measured                # measured | theoretical | estimated
//! context: "Cold start of the mobile app, three testers"   # usage_observation
//! reproducibility: reproducible     # reproducible | not-reproducible
//! ```
//!
//! The other types ask for their own keys: `measured_data` has `measured_at`
//! (RFC 3339), `revision` and `environment`; `external_source` has `source` (a
//! URI, or a citation in free text), `version` and `maturity` (`experimental |
//! emerging | established | standard`); `code_observation` has `ref`
//! (`path:line`), `revision` and `method` (`read | executed`);
//! `domain_constraint` has `authority`, `scope` and `review_condition`. The body
//! is everything after the front matter.
//!
//! # Spec
//!
//! ```yaml
//! grounds: [finding/slow-login]     # at least one reference
//! accept:
//!   pins:
//!     - id: PIN-LOGIN-CACHE
//!       contract: "The session is reused for five minutes"
//!   hyp:
//!     - id: HYP-FASTER-LOGIN
//!       metric: "median login time"
//!       target: "under 2 s"
//! ```
//!
//! The Contract is the text under `## Contract`; a `## Design Notes` section and
//! any other section are kept in the file but are not part of the content.
//!
//! # Design
//!
//! ```yaml
//! spec: spec/session-reuse
//! crux:
//!   question: "Where is the session kept?"
//!   options:
//!     - id: opt-memory
//!       summary: "Keep it in memory"
//!       chosen: true
//!       reason: "No new storage to secure"
//!     - id: opt-disk
//!       summary: "Keep it on disk"
//!       rejected: "A file to protect and to clean up"
//! items:
//!   - number: 1
//!     summary: "Reuse the session inside the window"
//!     pins: [PIN-LOGIN-CACHE]
//! ```
//!
//! Exactly one option is `chosen` (with a `reason`) and at least one is
//! `rejected` (with its reason).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::str::FromStr;

use serde::Deserialize;
use serde_norway::Value;
use whyshelf_core::app::raw::{
    NormalizedText, RawDesign, RawDoc, RawFinding, RawFindingSource, RawRevision, RawSpec,
};
use whyshelf_core::domain::actor::{Actor, MeasuredAt};
use whyshelf_core::domain::design::{ChosenOption, Crux, RejectedOption, WorkItem};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::event::DocKind;
use whyshelf_core::domain::finding::{
    EvidenceLabel, FindingType, MaturityLevel, ObservationMethod, Reproducibility,
};
use whyshelf_core::domain::front::{FileClassification, Guess, SortedKeys};
use whyshelf_core::domain::id::{Alias, ItemNumber};
use whyshelf_core::domain::location::{CodeLocation, SourceUri};
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec};
use whyshelf_core::domain::pin::{HypId, Hypothesis, PinContract, PinId};
use whyshelf_core::domain::reference::Ref;
use whyshelf_core::domain::state::UnparsedReason;
use whyshelf_core::domain::tag::Tag;

use crate::clock::validate_rfc3339;
use crate::markdown;

/// Parse a normalized document of a file that is not registered, or say everything
/// that is wrong with it. The kind is told by the keys ([`DocKind::guess`]). A file
/// that is registered is read with [`read`], as the kind it is registered as.
pub fn parse(text: &NormalizedText) -> Result<RawDoc, NonEmptyVec<UnparsedReason>> {
    read(text, None).outcome
}

/// Everything reading one file gave.
#[derive(Debug)]
pub struct Read {
    /// What the keys said about the kind (all that matters for a file not registered).
    pub guess: Guess,
    /// The classification keys of the front matter, as written.
    pub classification: FileClassification,
    /// The document, or every problem with it.
    pub outcome: Result<RawDoc, NonEmptyVec<UnparsedReason>>,
}

/// Read a normalized document as `kind`, or, for a file with no registered kind
/// (`None`), as the kind its keys name when they name exactly one.
///
/// A file registered as one kind is read as that kind whatever else it holds. A key
/// that another kind reads is a problem of its own (a Spec with a `crux`), and so is a
/// key no kind reads; none is dropped without a word. The exception is a file whose
/// one naming key is another kind's (`finding_type` in a registered Spec): it is not
/// a Spec with extra sections but another document, and says so
/// ([`UnparsedReason::KindChanged`]).
pub fn read(text: &NormalizedText, kind: Option<DocKind>) -> Read {
    let mut problems = Problems::default();
    let mut seen = Seen::default();
    let doc = read_into(text.as_str(), kind, &mut problems, &mut seen);
    let outcome = match (doc, NonEmptyVec::from_vec(problems.0)) {
        (Some(doc), Err(_)) => Ok(doc),
        (_, Ok(reasons)) => Err(reasons),
        (None, Err(_)) => Err(NonEmptyVec::new(
            UnparsedReason::MalformedFrontmatter {
                detail: "the file could not be read".to_owned(),
            },
            Vec::new(),
        )),
    };
    Read {
        guess: seen.guess,
        classification: seen.classification,
        outcome,
    }
}

/// What reading found out besides the document.
#[derive(Default)]
struct Seen {
    guess: Guess,
    classification: FileClassification,
}

/// The classification keys, as the file writes them.
#[derive(Deserialize)]
struct ClassFront {
    tags: Option<Vec<String>>,
    kind: Option<String>,
    owner: Option<String>,
    alias: Option<String>,
}

fn classification(front: ClassFront, problems: &mut Problems) -> FileClassification {
    let tags = front.tags.map(|tags| {
        problems
            .parsed_list::<Tag>(Some(tags), "tags")
            .into_iter()
            .collect()
    });
    let kind = front.kind.and_then(|name| {
        let kind = DocKind::from_name(&name);
        if kind.is_none() {
            let offered = DocKind::names().collect::<Vec<_>>().join(", ");
            problems.invalid("kind", format!("`{name}` is none of {offered}"));
        }
        kind
    });
    let owner = front.owner.and_then(|name| {
        Actor::new(&name)
            .map_err(|error| problems.invalid("owner", error))
            .ok()
    });
    let alias = front.alias.and_then(|alias| {
        alias
            .parse::<Alias>()
            .map_err(|error| problems.invalid("alias", error))
            .ok()
    });
    FileClassification {
        tags,
        kind,
        owner,
        alias,
    }
}

fn read_into(
    text: &str,
    registered: Option<DocKind>,
    problems: &mut Problems,
    seen: &mut Seen,
) -> Option<RawDoc> {
    if markdown::conflict_marker(text) {
        problems.0.push(UnparsedReason::MergeConflictMarker);
        return None;
    }
    let parts = match markdown::split_front_matter(text) {
        Ok(parts) => parts,
        Err(error) => {
            problems.malformed(error);
            return None;
        }
    };
    // First read: only to see which keys there are.
    let probe: Value = match serde_norway::from_str(parts.yaml) {
        Ok(value) => value,
        Err(error) => {
            problems.malformed(error);
            return None;
        }
    };
    let Value::Mapping(mapping) = probe else {
        problems.malformed("the front matter is not a mapping of keys to values");
        return None;
    };
    let mut names: Vec<&str> = Vec::new();
    for key in mapping.keys() {
        match key.as_str() {
            Some(name) => names.push(name),
            None => problems.malformed("a key of the front matter is not text"),
        }
    }
    seen.guess = DocKind::guess(names.iter().copied());

    let kind = match (registered, seen.guess) {
        (Some(registered), Guess::One(file)) if file != registered => {
            problems
                .0
                .push(UnparsedReason::KindChanged { registered, file });
            return None;
        }
        (Some(registered), _) => registered,
        (None, Guess::One(kind)) => kind,
        (None, Guess::Several) => {
            problems.invalid(
                "kind",
                "the keys of the file fit more than one kind of document",
            );
            return None;
        }
        (None, Guess::None) => {
            problems.missing("finding_type");
            return None;
        }
    };

    if let Some(front) = typed::<ClassFront>(parts.yaml, problems) {
        seen.classification = classification(front, problems);
    }
    problems
        .0
        .extend(SortedKeys::of(kind, names.iter().copied()).reasons(kind));

    match kind {
        DocKind::Finding => {
            let front = typed::<FindingFront>(parts.yaml, problems)?;
            parse_finding(front, parts.body, problems).map(RawDoc::Finding)
        }
        DocKind::Spec => {
            let front = typed::<SpecFront>(parts.yaml, problems)?;
            parse_spec(front, parts.body, problems).map(RawDoc::Spec)
        }
        DocKind::Design => {
            let front = typed::<DesignFront>(parts.yaml, problems)?;
            parse_design(front, problems).map(RawDoc::Design)
        }
    }
}

/// Second read: into the shape of the kind of file, scalars as text.
fn typed<'a, T: Deserialize<'a>>(yaml: &'a str, problems: &mut Problems) -> Option<T> {
    match serde_norway::from_str(yaml) {
        Ok(front) => Some(front),
        Err(error) => {
            problems.malformed(error);
            None
        }
    }
}

// ---- the shapes of the front matter -------------------------------------

#[derive(Deserialize)]
struct FindingFront {
    finding_type: Option<String>,
    title: Option<String>,
    evidence: Option<String>,
    measured_at: Option<String>,
    revision: Option<String>,
    environment: Option<String>,
    context: Option<String>,
    reproducibility: Option<String>,
    source: Option<String>,
    version: Option<String>,
    maturity: Option<String>,
    #[serde(rename = "ref")]
    location: Option<String>,
    method: Option<String>,
    authority: Option<String>,
    scope: Option<String>,
    review_condition: Option<String>,
}

#[derive(Deserialize)]
struct SpecFront {
    grounds: Option<Vec<String>>,
    accept: Option<AcceptFront>,
}

#[derive(Deserialize)]
struct AcceptFront {
    pins: Option<Vec<PinFront>>,
    hyp: Option<Vec<HypFront>>,
}

#[derive(Deserialize)]
struct PinFront {
    id: Option<String>,
    contract: Option<String>,
}

#[derive(Deserialize)]
struct HypFront {
    id: Option<String>,
    metric: Option<String>,
    target: Option<String>,
}

#[derive(Deserialize)]
struct DesignFront {
    spec: Option<String>,
    crux: Option<CruxFront>,
    items: Option<Vec<ItemFront>>,
}

#[derive(Deserialize)]
struct CruxFront {
    question: Option<String>,
    options: Option<Vec<OptionFront>>,
}

#[derive(Deserialize)]
struct OptionFront {
    id: Option<String>,
    summary: Option<String>,
    chosen: Option<bool>,
    reason: Option<String>,
    rejected: Option<String>,
}

#[derive(Deserialize)]
struct ItemFront {
    number: Option<String>,
    summary: Option<String>,
    pins: Option<Vec<String>>,
}

// ---- problems and field helpers -----------------------------------------

/// The problems found so far.
#[derive(Default)]
struct Problems(Vec<UnparsedReason>);

impl Problems {
    fn malformed(&mut self, detail: impl Display) {
        self.0.push(UnparsedReason::MalformedFrontmatter {
            detail: detail.to_string(),
        });
    }

    fn missing(&mut self, field: &str) {
        self.0.push(UnparsedReason::MissingField {
            field: field.to_owned(),
        });
    }

    fn invalid(&mut self, field: &str, detail: impl Display) {
        self.0.push(UnparsedReason::InvalidField {
            field: field.to_owned(),
            detail: detail.to_string(),
        });
    }

    /// The value of a required key.
    fn need<T>(&mut self, value: Option<T>, field: &str) -> Option<T> {
        if value.is_none() {
            self.missing(field);
        }
        value
    }

    fn non_blank(&mut self, value: Option<String>, field: &str) -> Option<NonBlank> {
        let text = self.need(value, field)?;
        NonBlank::new(text)
            .map_err(|error| self.invalid(field, error))
            .ok()
    }

    /// A text with a rule of its own (`FromStr`).
    fn parsed<T: FromStr<Err = ValueError>>(
        &mut self,
        value: Option<String>,
        field: &str,
    ) -> Option<T> {
        let text = self.need(value, field)?;
        text.parse()
            .map_err(|error| self.invalid(field, error))
            .ok()
    }

    /// A text that must be one of a few names.
    fn name<T: Copy>(
        &mut self,
        value: Option<String>,
        field: &str,
        names: &[(&str, T)],
    ) -> Option<T> {
        let text = self.need(value, field)?;
        match names.iter().find(|(name, _)| *name == text) {
            Some((_, found)) => Some(*found),
            None => {
                let allowed: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
                self.invalid(field, format!("`{text}` is none of {}", allowed.join(", ")));
                None
            }
        }
    }

    fn revision(&mut self, value: Option<String>, field: &str) -> Option<RawRevision> {
        let text = self.need(value, field)?;
        RawRevision::new(text)
            .map_err(|error| self.invalid(field, error))
            .ok()
    }

    /// A list of texts, each with a rule of its own.
    fn parsed_list<T: FromStr<Err = ValueError>>(
        &mut self,
        values: Option<Vec<String>>,
        field: &str,
    ) -> Vec<T> {
        values
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter_map(|(index, text)| {
                text.parse()
                    .map_err(|error| self.invalid(&format!("{field}[{index}]"), error))
                    .ok()
            })
            .collect()
    }
}

/// Insert into a map, reporting a key that is already there.
fn insert_unique<K: Ord + Display, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    field: &str,
    problems: &mut Problems,
) {
    let name = key.to_string();
    if map.insert(key, value).is_some() {
        problems.invalid(field, format!("`{name}` appears twice"));
    }
}

// ---- Finding -------------------------------------------------------------

/// The names of a closed vocabulary of the domain, with the values they stand for:
/// the parser keeps no table of its own, so a word added to the domain is read here
/// the day it is added.
fn names<T: Copy>(all: &[T], name: fn(T) -> &'static str) -> Vec<(&'static str, T)> {
    all.iter().map(|value| (name(*value), *value)).collect()
}

fn parse_finding(front: FindingFront, body: &str, problems: &mut Problems) -> Option<RawFinding> {
    let finding_type = problems.name(
        front.finding_type,
        "finding_type",
        &names(&FindingType::ALL, FindingType::name),
    );
    let title = problems.non_blank(front.title, "title");
    let evidence = problems.name(
        front.evidence,
        "evidence",
        &names(&EvidenceLabel::ALL, EvidenceLabel::name),
    );
    let source = match finding_type? {
        FindingType::MeasuredData => {
            let measured_at = problems
                .need(front.measured_at, "measured_at")
                .and_then(|text| match validate_rfc3339(&text) {
                    Ok(()) => MeasuredAt::from_validated(text)
                        .map_err(|error| problems.invalid("measured_at", error))
                        .ok(),
                    Err(error) => {
                        problems.invalid("measured_at", format!("not an RFC 3339 time: {error}"));
                        None
                    }
                });
            let revision = problems.revision(front.revision, "revision");
            let environment = problems.non_blank(front.environment, "environment");
            match (measured_at, revision, environment) {
                (Some(measured_at), Some(revision), Some(environment)) => {
                    Some(RawFindingSource::MeasuredData {
                        measured_at,
                        revision,
                        environment,
                    })
                }
                _ => None,
            }
        }
        FindingType::UsageObservation => {
            let context = problems.non_blank(front.context, "context");
            let reproducibility = problems.name(
                front.reproducibility,
                "reproducibility",
                &names(&Reproducibility::ALL, Reproducibility::name),
            );
            match (context, reproducibility) {
                (Some(context), Some(reproducibility)) => {
                    Some(RawFindingSource::UsageObservation {
                        context,
                        reproducibility,
                    })
                }
                _ => None,
            }
        }
        FindingType::ExternalSource => {
            let source = problems.parsed::<SourceUri>(front.source, "source");
            let version = problems.non_blank(front.version, "version");
            let maturity = problems.name(
                front.maturity,
                "maturity",
                &names(&MaturityLevel::ALL, MaturityLevel::name),
            );
            match (source, version, maturity) {
                (Some(source), Some(version), Some(maturity)) => {
                    Some(RawFindingSource::ExternalSource {
                        source,
                        version,
                        maturity,
                    })
                }
                _ => None,
            }
        }
        FindingType::CodeObservation => {
            let location = problems.parsed::<CodeLocation>(front.location, "ref");
            let revision = problems.revision(front.revision, "revision");
            let method = problems.name(
                front.method,
                "method",
                &names(&ObservationMethod::ALL, ObservationMethod::name),
            );
            match (location, revision, method) {
                (Some(location), Some(revision), Some(method)) => {
                    Some(RawFindingSource::CodeObservation {
                        location,
                        revision,
                        method,
                    })
                }
                _ => None,
            }
        }
        FindingType::DomainConstraint => {
            let authority = problems.non_blank(front.authority, "authority");
            let scope = problems.non_blank(front.scope, "scope");
            let review_condition = problems.non_blank(front.review_condition, "review_condition");
            match (authority, scope, review_condition) {
                (Some(authority), Some(scope), Some(review_condition)) => {
                    Some(RawFindingSource::DomainConstraint {
                        authority,
                        scope,
                        review_condition,
                    })
                }
                _ => None,
            }
        }
    };
    Some(RawFinding::new(
        title?,
        source?,
        evidence?,
        markdown::body_text(body).to_owned(),
    ))
}

// ---- Spec ----------------------------------------------------------------

fn parse_spec(front: SpecFront, body: &str, problems: &mut Problems) -> Option<RawSpec> {
    let grounds = problems.parsed_list::<Ref>(front.grounds, "grounds");
    let grounds = match NonEmptyVec::from_vec(grounds) {
        Ok(grounds) => Some(grounds),
        Err(_) => {
            problems.invalid("grounds", "a Spec stands on at least one Finding");
            None
        }
    };
    let mut pins: BTreeMap<PinId, PinContract> = BTreeMap::new();
    let mut hyps: BTreeMap<HypId, Hypothesis> = BTreeMap::new();
    if let Some(accept) = front.accept {
        for (index, pin) in accept.pins.unwrap_or_default().into_iter().enumerate() {
            let at = format!("accept.pins[{index}]");
            let id = problems.parsed::<PinId>(pin.id, &format!("{at}.id"));
            let contract = problems.need(pin.contract, &format!("{at}.contract"));
            let contract = contract.and_then(|text| {
                PinContract::new(text)
                    .map_err(|error| problems.invalid(&format!("{at}.contract"), error))
                    .ok()
            });
            if let (Some(id), Some(contract)) = (id, contract) {
                insert_unique(&mut pins, id, contract, "accept.pins", problems);
            }
        }
        for (index, hyp) in accept.hyp.unwrap_or_default().into_iter().enumerate() {
            let at = format!("accept.hyp[{index}]");
            let id = problems.parsed::<HypId>(hyp.id, &format!("{at}.id"));
            let metric = problems.need(hyp.metric, &format!("{at}.metric"));
            let target = problems.need(hyp.target, &format!("{at}.target"));
            if let (Some(id), Some(metric), Some(target)) = (id, metric, target) {
                match Hypothesis::new(metric, target) {
                    Ok(hypothesis) => {
                        insert_unique(&mut hyps, id, hypothesis, "accept.hyp", problems)
                    }
                    Err(error) => problems.invalid(&at, error),
                }
            }
        }
    }
    let contract = match markdown::section(body, "Contract") {
        None => {
            problems.missing("## Contract");
            None
        }
        Some(text) => NonBlank::new(text)
            .map_err(|error| problems.invalid("## Contract", error))
            .ok(),
    };
    Some(RawSpec::new(grounds?, contract?, pins, hyps))
}

// ---- Design --------------------------------------------------------------

fn parse_design(front: DesignFront, problems: &mut Problems) -> Option<RawDesign> {
    let spec = problems.parsed::<Ref>(front.spec, "spec");
    let crux = parse_crux(front.crux, problems);
    let mut items: BTreeMap<ItemNumber, WorkItem> = BTreeMap::new();
    for (index, item) in front.items.unwrap_or_default().into_iter().enumerate() {
        let at = format!("items[{index}]");
        let number = problems.parsed::<ItemNumber>(item.number, &format!("{at}.number"));
        let summary = problems.non_blank(item.summary, &format!("{at}.summary"));
        let pins: BTreeSet<PinId> = problems
            .parsed_list::<PinId>(item.pins, &format!("{at}.pins"))
            .into_iter()
            .collect();
        if let (Some(number), Some(summary)) = (number, summary) {
            insert_unique(
                &mut items,
                number,
                WorkItem::new(summary, pins),
                "items",
                problems,
            );
        }
    }
    Some(RawDesign::new(spec?, crux?, items))
}

fn parse_crux(front: Option<CruxFront>, problems: &mut Problems) -> Option<Crux> {
    let front = problems.need(front, "crux")?;
    let question = problems.non_blank(front.question, "crux.question");
    let mut chosen: Vec<ChosenOption> = Vec::new();
    let mut rejected: Vec<RejectedOption> = Vec::new();
    let options = problems
        .need(front.options, "crux.options")
        .unwrap_or_default();
    for (index, option) in options.into_iter().enumerate() {
        let at = format!("crux.options[{index}]");
        let id = problems.parsed(option.id, &format!("{at}.id"));
        let summary = problems.non_blank(option.summary, &format!("{at}.summary"));
        let is_chosen = option.chosen == Some(true);
        match (is_chosen, option.rejected.is_some()) {
            (true, false) => {
                let reason = problems.non_blank(option.reason, &format!("{at}.reason"));
                if let (Some(id), Some(summary), Some(reason)) = (id, summary, reason) {
                    chosen.push(ChosenOption::new(id, summary, reason));
                }
            }
            (false, true) => {
                let reason = problems.non_blank(option.rejected, &format!("{at}.rejected"));
                if let (Some(id), Some(summary), Some(reason)) = (id, summary, reason) {
                    rejected.push(RejectedOption::new(id, summary, reason));
                }
            }
            _ => problems.invalid(
                &at,
                "an option is either `chosen: true` with a `reason` or `rejected` with its reason",
            ),
        }
    }
    let chosen = match <[ChosenOption; 1]>::try_from(chosen) {
        Ok([only]) => Some(only),
        Err(found) => {
            if found.is_empty() {
                problems.invalid("crux.options", "no option is chosen");
            } else {
                problems.invalid("crux.options", "more than one option is chosen");
            }
            None
        }
    };
    let rejected = match NonEmptyVec::from_vec(rejected) {
        Ok(rejected) => Some(rejected),
        Err(_) => {
            problems.invalid(
                "crux.options",
                "at least one option is rejected, with its reason",
            );
            None
        }
    };
    match Crux::new(question?, chosen?, rejected?) {
        Ok(crux) => Some(crux),
        Err(error) => {
            problems.invalid("crux.options", error);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::normalize;

    const REVISION: &str = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";

    fn parse_text(text: &str) -> Result<RawDoc, Vec<UnparsedReason>> {
        parse(&normalize(text)).map_err(NonEmptyVec::into_vec)
    }

    fn finding_of(text: &str) -> RawFinding {
        match parse_text(text) {
            Ok(RawDoc::Finding(finding)) => finding,
            other => panic!("not a Finding: {other:?}"),
        }
    }

    const FINDING: &str = "finding_type: usage_observation\ntitle: t\nevidence: measured\n\
                           context: c\nreproducibility: reproducible\n";
    const SPEC: &str = "grounds: [finding/a]\naccept:\n  pins:\n    - id: PIN-A\n      contract: c\n  hyp:\n    - id: HYP-A\n      metric: m\n      target: t\n";
    const DESIGN: &str = "spec: spec/a\ncrux:\n  question: q\n  options:\n    - id: opt-a\n      summary: s\n      chosen: true\n      reason: r\n    - id: opt-b\n      summary: s\n      rejected: r\nitems:\n  - number: 1\n    summary: s\n    pins: [PIN-A]\n";

    fn read_as(keys: &str, kind: Option<DocKind>) -> Read {
        read(
            &normalize(&format!("---\n{keys}---\n\n## Contract\nc\n")),
            kind,
        )
    }

    fn reasons(read: Read) -> Vec<UnparsedReason> {
        read.outcome.expect_err("it should not parse").into_vec()
    }

    #[test]
    fn a_key_of_another_kind_in_a_registered_file_stops_it_and_a_typing_mistake_too() {
        // A Spec file with a Design's section: refused, not read as a Spec that
        // happens to carry a `crux`.
        let spec_with_crux = format!("{SPEC}crux:\n  question: q\n");
        assert_eq!(
            reasons(read_as(&spec_with_crux, Some(DocKind::Spec))),
            vec![UnparsedReason::ForeignSection {
                section: DocKind::Design,
                registered: DocKind::Spec,
                key: "crux".to_owned()
            }]
        );
        // A key nobody reads is a mistake to be corrected, not a comment.
        let typo = format!("{FINDING}evidance: measured\n");
        assert_eq!(
            reasons(read_as(&typo, Some(DocKind::Finding))),
            vec![UnparsedReason::UnknownKey {
                key: "evidance".to_owned()
            }]
        );
        // Every problem is reported together with the others of the file.
        let both = format!("{SPEC}crux:\n  question: q\ntypo: 1\n");
        assert_eq!(reasons(read_as(&both, Some(DocKind::Spec))).len(), 2);
        // The same files with nothing extra read.
        for (keys, kind) in [
            (FINDING, DocKind::Finding),
            (SPEC, DocKind::Spec),
            (DESIGN, DocKind::Design),
        ] {
            let read = read_as(keys, Some(kind));
            assert_eq!(read.outcome.expect("it reads").kind(), kind, "{kind}");
        }
    }

    #[test]
    fn a_registered_file_is_read_as_its_kind_and_one_that_names_another_kind_says_so() {
        // `finding_type` in a file registered as a Spec: another document altogether.
        let swapped = format!("{SPEC}{FINDING}");
        let read = read_as(&swapped, Some(DocKind::Spec));
        assert_eq!(read.guess, Guess::Several);
        let only_finding = read_as(FINDING, Some(DocKind::Spec));
        assert_eq!(only_finding.guess, Guess::One(DocKind::Finding));
        assert_eq!(
            reasons(only_finding),
            vec![UnparsedReason::KindChanged {
                registered: DocKind::Spec,
                file: DocKind::Finding
            }]
        );
    }

    #[test]
    fn a_file_with_no_registered_kind_is_told_by_its_keys_when_they_say_one_kind() {
        let read = read_as(SPEC, None);
        assert_eq!(read.guess, Guess::One(DocKind::Spec));
        assert_eq!(read.outcome.expect("it reads").kind(), DocKind::Spec);
        // Two kinds, or none: nothing is registered on a guess.
        let several = read_as(&format!("{SPEC}{DESIGN}"), None);
        assert_eq!(several.guess, Guess::Several);
        assert!(several.outcome.is_err());
        let none = read_as("title: t\n", None);
        assert_eq!(none.guess, Guess::None);
        assert!(none.outcome.is_err());
    }

    #[test]
    fn classification_keys_are_returned_and_cannot_change_what_a_hash_covers() {
        let plain = read_as(SPEC, Some(DocKind::Spec));
        let with_keys = format!(
            "{SPEC}tags: [area:auth, scratch]\nkind: spec\nowner: alice\nalias: spec/session\n"
        );
        let classified = read_as(&with_keys, Some(DocKind::Spec));
        let keys = &classified.classification;
        assert_eq!(keys.tags.as_ref().map(|t| t.len()), Some(2));
        assert_eq!(keys.kind, Some(DocKind::Spec));
        assert_eq!(keys.owner.as_ref().map(Actor::as_str), Some("alice"));
        assert_eq!(
            keys.alias.as_ref().map(ToString::to_string).as_deref(),
            Some("spec/session")
        );
        assert!(plain.classification.is_empty());
        // The document is the same value, so it has the same hash: a classification
        // key never removes an approval.
        assert_eq!(plain.outcome.unwrap(), classified.outcome.unwrap());

        // A value a type refuses is a problem of the file, not a silent drop.
        let bad = format!("{SPEC}tags: [NOT A TAG]\nkind: poem\n");
        let problems = reasons(read_as(&bad, Some(DocKind::Spec)));
        assert!(matches!(&problems[..],
            [UnparsedReason::InvalidField { field: tags, .. }, UnparsedReason::InvalidField { field: kind, .. }]
                if tags == "tags[0]" && kind == "kind"));
    }

    #[test]
    fn every_word_the_domain_names_is_read_and_only_those() {
        let usage = |evidence: &str, reproducibility: &str| {
            format!(
                "---\nfinding_type: usage_observation\ntitle: t\nevidence: {evidence}\n\
                 context: c\nreproducibility: {reproducibility}\n---\nbody\n"
            )
        };
        for label in EvidenceLabel::ALL {
            for state in Reproducibility::ALL {
                finding_of(&usage(label.name(), state.name()));
            }
        }
        for level in MaturityLevel::ALL {
            let text = format!(
                "---\nfinding_type: external_source\ntitle: t\nevidence: theoretical\n\
                 source: \"https://example.org/s\"\nversion: v\nmaturity: {}\n---\nb\n",
                level.name()
            );
            finding_of(&text);
        }
        for method in ObservationMethod::ALL {
            let text = format!(
                "---\nfinding_type: code_observation\ntitle: t\nref: \"a.rs:1\"\n\
                 revision: {REVISION}\nmethod: {}\nevidence: measured\n---\nb\n",
                method.name()
            );
            finding_of(&text);
        }
        // English only: the example of the specification in another language is refused,
        // and the reason names the key and the words that would do.
        let refused = parse_text(&usage("実測", "reproducible")).expect_err("not a label");
        assert!(matches!(
            &refused[..],
            [UnparsedReason::InvalidField { field, detail }]
                if field == "evidence" && detail.contains("measured, theoretical, estimated")
        ));
    }

    #[test]
    fn a_code_observation_with_an_unquoted_revision_parses() {
        let text = format!(
            "---\nfinding_type: code_observation\ntitle: \"The parser skips blank keys\"\n\
             ref: \"src/parser.rs:204\"\nrevision: {REVISION}\nmethod: read\nevidence: measured\n---\n\
             \nIt compares the whole line.\n"
        );
        let finding = finding_of(&text);
        assert_eq!(finding.revision().map(RawRevision::as_str), Some(REVISION));
    }

    #[test]
    fn scalars_that_look_like_numbers_keep_their_digits() {
        for digits in [
            "1234567890123456789012345678901234567890",
            "123456e789012345678901234567890123456789",
            "0000000000000000000000000000000000000001",
        ] {
            let text = format!(
                "---\nfinding_type: code_observation\ntitle: t\nref: \"a.rs:1\"\nrevision: {digits}\n\
                 method: executed\nevidence: measured\n---\nbody\n"
            );
            let finding = finding_of(&text);
            assert_eq!(finding.revision().map(RawRevision::as_str), Some(digits));
        }
        let text = "---\nfinding_type: external_source\ntitle: t\nevidence: theoretical\n\
                    source: \"https://example.org/spec\"\nversion: 1.10\nmaturity: standard\n---\nb\n";
        let expected = RawFinding::new(
            NonBlank::new("t").unwrap(),
            RawFindingSource::ExternalSource {
                source: "https://example.org/spec".parse().unwrap(),
                version: NonBlank::new("1.10").unwrap(),
                maturity: MaturityLevel::Standard,
            },
            EvidenceLabel::Theoretical,
            "b\n".to_owned(),
        );
        assert_eq!(finding_of(text), expected);
    }

    #[test]
    fn a_usage_observation_keeps_its_body() {
        let text = "---\nfinding_type: usage_observation\ntitle: Slow login\nevidence: estimated\n\
                    context: \"three testers, cold start\"\nreproducibility: not-reproducible\n---\n\n\nBody line.\n";
        let finding = finding_of(text);
        let again = finding_of(&text.replace("Body line.", "Body line.  "));
        assert_eq!(
            finding, again,
            "trailing blanks are not part of the content"
        );
        assert!(finding.revision().is_none());
    }

    #[test]
    fn a_measurement_needs_a_real_date() {
        let text = format!(
            "---\nfinding_type: measured_data\ntitle: t\nevidence: measured\n\
             measured_at: \"2026-13-45T10:00:00Z\"\nrevision: {REVISION}\nenvironment: laptop\n---\nb\n"
        );
        let reasons = parse_text(&text).unwrap_err();
        assert!(matches!(
            reasons.as_slice(),
            [UnparsedReason::InvalidField { field, .. }] if field == "measured_at"
        ));
    }

    #[test]
    fn an_unquoted_timestamp_is_still_text() {
        let text = format!(
            "---\nfinding_type: measured_data\ntitle: t\nevidence: measured\n\
             measured_at: 2026-11-04T12:40:00Z\nrevision: {REVISION}\nenvironment: laptop\n---\nb\n"
        );
        finding_of(&text);
    }

    #[test]
    fn every_missing_key_is_reported_at_once() {
        let text = "---\nfinding_type: domain_constraint\n---\nb\n";
        let reasons = parse_text(text).unwrap_err();
        let fields: Vec<&str> = reasons
            .iter()
            .filter_map(|r| match r {
                UnparsedReason::MissingField { field } => Some(field.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            fields,
            [
                "title",
                "evidence",
                "authority",
                "scope",
                "review_condition"
            ]
        );
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_a_malformed_front_matter() {
        let text = "---\nfinding_type: domain_constraint\ntitle: [a, b]\n---\nb\n";
        let reasons = parse_text(text).unwrap_err();
        assert!(matches!(
            reasons.as_slice(),
            [UnparsedReason::MalformedFrontmatter { .. }]
        ));
    }

    #[test]
    fn a_spec_parses_with_pins_hypotheses_and_a_contract() {
        let text = "---\ngrounds:\n  - finding/slow-login\n  - private:0192e3a4-7b5c-7d3e-8f01-23456789abcd\n\
                    accept:\n  pins:\n    - id: PIN-LOGIN-CACHE\n      contract: \"The session is reused for five minutes\"\n\
                    \x20 hyp:\n    - id: HYP-FASTER-LOGIN\n      metric: \"median login time\"\n      target: \"under 2 s\"\n---\n\n\
                    ## Contract\nThe session is reused.\n\n## Design Notes\nNot part of the content.\n";
        let Ok(RawDoc::Spec(spec)) = parse_text(text) else {
            panic!("not a Spec");
        };
        assert_eq!(spec.grounds().len(), 2);
        let other = text.replace("Not part of the content.", "Changed notes.");
        let Ok(RawDoc::Spec(again)) = parse_text(&other) else {
            panic!("not a Spec");
        };
        assert_eq!(
            spec, again,
            "a supplementary section is outside the content"
        );
    }

    #[test]
    fn a_spec_without_a_contract_section_is_reported() {
        let text = "---\ngrounds: [finding/a]\n---\n\n## Design Notes\nx\n";
        let reasons = parse_text(text).unwrap_err();
        assert_eq!(
            reasons,
            vec![UnparsedReason::MissingField {
                field: "## Contract".to_owned()
            }]
        );
    }

    #[test]
    fn a_spec_with_a_repeated_pin_or_no_grounds_is_reported() {
        let text = "---\ngrounds: []\naccept:\n  pins:\n    - {id: PIN-A, contract: x}\n    - {id: PIN-A, contract: y}\n---\n## Contract\nc\n";
        let reasons = parse_text(text).unwrap_err();
        assert!(reasons.iter().any(|r| matches!(
            r, UnparsedReason::InvalidField { field, .. } if field == "grounds"
        )));
        assert!(reasons.iter().any(|r| matches!(
            r, UnparsedReason::InvalidField { field, detail } if field == "accept.pins" && detail.contains("twice")
        )));
    }

    fn design_text(options: &str) -> String {
        format!(
            "---\nspec: spec/session-reuse\ncrux:\n  question: \"Where is the session kept?\"\n  options:\n{options}\
             items:\n  - number: 1\n    summary: \"Reuse inside the window\"\n    pins: [PIN-LOGIN-CACHE]\n---\n\n## Design Notes\nx\n"
        )
    }

    const GOOD_OPTIONS: &str = "    - id: opt-memory\n      summary: \"Keep it in memory\"\n      chosen: true\n      reason: \"No new storage\"\n\
                                \x20   - id: opt-disk\n      summary: \"Keep it on disk\"\n      rejected: \"A file to protect\"\n";

    #[test]
    fn a_design_parses() {
        let Ok(RawDoc::Design(design)) = parse_text(&design_text(GOOD_OPTIONS)) else {
            panic!("not a Design");
        };
        assert_eq!(design.spec().to_string(), "spec/session-reuse");
    }

    #[test]
    fn a_crux_needs_one_choice_and_one_rejection() {
        let none_chosen = "    - id: opt-a\n      summary: a\n      rejected: no\n";
        let reasons = parse_text(&design_text(none_chosen)).unwrap_err();
        assert!(reasons.iter().any(|r| matches!(
            r,
            UnparsedReason::InvalidField { detail, .. } if detail.contains("no option is chosen")
        )));
        let none_rejected =
            "    - id: opt-a\n      summary: a\n      chosen: true\n      reason: yes\n";
        let reasons = parse_text(&design_text(none_rejected)).unwrap_err();
        assert!(reasons.iter().any(|r| matches!(
            r,
            UnparsedReason::InvalidField { detail, .. } if detail.contains("rejected")
        )));
    }

    #[test]
    fn a_conflict_marker_makes_the_file_unparsed_for_that_reason_alone() {
        let text = "---\nfinding_type: usage_observation\n<<<<<<< HEAD\ntitle: a\n=======\ntitle: b\n>>>>>>> topic\n---\nx\n";
        assert_eq!(
            parse_text(text).unwrap_err(),
            vec![UnparsedReason::MergeConflictMarker]
        );
    }

    #[test]
    fn broken_and_missing_front_matter_are_malformed() {
        for text in [
            "# Just a note\n",
            "---\na: [unclosed\n---\nx\n",
            "---\n- a\n- b\n---\nx\n",
        ] {
            let reasons = parse_text(text).unwrap_err();
            assert!(
                matches!(
                    reasons.as_slice(),
                    [UnparsedReason::MalformedFrontmatter { .. }]
                ),
                "{text}: {reasons:?}"
            );
        }
    }

    #[test]
    fn a_file_with_no_kind_key_names_the_missing_one() {
        let reasons = parse_text("---\ntitle: x\n---\nbody\n").unwrap_err();
        assert_eq!(
            reasons,
            vec![UnparsedReason::MissingField {
                field: "finding_type".to_owned()
            }]
        );
    }
}
