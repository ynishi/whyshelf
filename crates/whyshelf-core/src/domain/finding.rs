//! Stage 1: the Finding, the ground a Spec stands on.
//!
//! A Finding records a cause, an observation or a premise that sits *under* an
//! intent: with it in hand the intent is largely settled. The name works for
//! development and for business; a budget or a regulation is one more kind of
//! Finding. Every change starts from a Finding, and a Spec that does not refer
//! to one is refused by machine.
//!
//! # Types and their required metadata
//!
//! The kind of Finding ([`FindingSource`]) decides which metadata gate CR demands.
//! The type is an enum whose variants *hold* that metadata, so a Finding of a
//! given type cannot exist without it.
//!
//! | variant | records | required metadata |
//! |---|---|---|
//! | `MeasuredData` | a measurement, usage-log aggregate, benchmark, earlier CM result | when measured, revision and environment, and the evidence label `Measured` |
//! | `UsageObservation` | what users did, friction, a field report | the situation (repository, steps) and whether it reproduces |
//! | `ExternalSource` | an outside specification, paper, trend or established practice | the source (URI or citation), the version it applies to, its maturity |
//! | `CodeObservation` | a behavior or defect seen in existing code, a probe result | the location (`file:line`), the revision looked at, how it was confirmed |
//! | `DomainConstraint` | a business goal, regulation, hardware or budget limit | who decided, the scope, the condition for review |
//!
//! A `CodeObservation` location and revision describe the moment of looking. They
//! are not kept in step with later code; a fact that must follow the code is a
//! PIN instead.
//!
//! # What the hash covers
//!
//! The hash covers the type, its metadata, the title, the evidence label and the
//! body. It does not cover the identifier or any classification (tags, owner).
//! The Finding is checked by gate CR: a reviewer judges whether the source or
//! method of measurement is trustworthy and whether several viewpoints are
//! independent.

use super::actor::MeasuredAt;
use super::canon::{Canon, ToCanon};
use super::hash::{ContentHash, HashKind};
use super::id::FindingId;
use super::location::{CodeLocation, SourceUri};
use super::nonempty::NonBlank;
use super::revision::RevisionId;

/// How strongly a statement in a Finding is backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceLabel {
    /// Observed or measured directly.
    Measured,
    /// Derived from a model or a specification, not observed.
    Theoretical,
    /// A guess with some basis.
    Estimated,
}

impl EvidenceLabel {
    fn name(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Theoretical => "theoretical",
            Self::Estimated => "estimated",
        }
    }
}

/// How mature the practice described by an external source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaturityLevel {
    /// Proposed or tried by few.
    Experimental,
    /// Used in more than a few places, still moving.
    Emerging,
    /// Used in production by many, stable.
    Established,
    /// Written down as a standard.
    Standard,
}

impl MaturityLevel {
    fn name(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Emerging => "emerging",
            Self::Established => "established",
            Self::Standard => "standard",
        }
    }
}

/// How a code observation was confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservationMethod {
    /// By reading the code.
    Read,
    /// By running it.
    Executed,
}

impl ObservationMethod {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Executed => "executed",
        }
    }
}

/// Whether a reported friction can be reproduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reproducibility {
    /// It happens again when the steps are repeated.
    Reproducible,
    /// It was seen and did not happen again.
    NotReproducible,
}

impl Reproducibility {
    fn name(self) -> &'static str {
        match self {
            Self::Reproducible => "reproducible",
            Self::NotReproducible => "not-reproducible",
        }
    }
}

/// The kind of a Finding together with the metadata that kind requires.
///
/// The `NonBlank` fields are free explanatory text (an environment, a
/// situation, a person or body that decided); the other fields are values with
/// rules and have their own types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingSource {
    /// A measurement.
    MeasuredData {
        /// When the measurement was taken.
        measured_at: MeasuredAt,
        /// The revision of the measured system.
        revision: RevisionId,
        /// The environment of the measurement, free text.
        environment: NonBlank,
    },
    /// What users did and what went wrong.
    UsageObservation {
        /// The situation: repository, steps taken. Free text.
        context: NonBlank,
        /// Whether the friction reproduces.
        reproducibility: Reproducibility,
    },
    /// A document from outside.
    ExternalSource {
        /// Where to read it.
        source: SourceUri,
        /// The version of the source this applies to, free text.
        version: NonBlank,
        /// How mature the practice is.
        maturity: MaturityLevel,
    },
    /// A look at existing code.
    CodeObservation {
        /// Where in the code, at the time of looking.
        location: CodeLocation,
        /// The revision that was looked at.
        revision: RevisionId,
        /// How it was confirmed.
        method: ObservationMethod,
    },
    /// A limit set from outside the code.
    DomainConstraint {
        /// Who decided, free text.
        authority: NonBlank,
        /// What the constraint applies to, free text.
        scope: NonBlank,
        /// When it should be looked at again, free text.
        review_condition: NonBlank,
    },
}

/// The kind of a Finding without its metadata.
///
/// Why: a new Finding is started from a type name (`finding new --type ..`)
/// before any of the metadata exists, so the choice needs a value of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingType {
    /// A measurement.
    MeasuredData,
    /// What users did and what went wrong.
    UsageObservation,
    /// A document from outside.
    ExternalSource,
    /// A look at existing code.
    CodeObservation,
    /// A limit set from outside the code.
    DomainConstraint,
}

impl FindingType {
    /// Every type, in the order of the table in the module documentation.
    pub const ALL: [FindingType; 5] = [
        FindingType::MeasuredData,
        FindingType::UsageObservation,
        FindingType::ExternalSource,
        FindingType::CodeObservation,
        FindingType::DomainConstraint,
    ];

    /// The `finding_type` name as written in a file.
    pub fn name(self) -> &'static str {
        match self {
            Self::MeasuredData => "measured_data",
            Self::UsageObservation => "usage_observation",
            Self::ExternalSource => "external_source",
            Self::CodeObservation => "code_observation",
            Self::DomainConstraint => "domain_constraint",
        }
    }

    /// The type with this file name; `None` for any other text.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

impl FindingSource {
    /// The type of this variant.
    pub fn finding_type(&self) -> FindingType {
        match self {
            Self::MeasuredData { .. } => FindingType::MeasuredData,
            Self::UsageObservation { .. } => FindingType::UsageObservation,
            Self::ExternalSource { .. } => FindingType::ExternalSource,
            Self::CodeObservation { .. } => FindingType::CodeObservation,
            Self::DomainConstraint { .. } => FindingType::DomainConstraint,
        }
    }

    /// The `finding_type` name of this variant, as written in a file.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::MeasuredData { .. } => "measured_data",
            Self::UsageObservation { .. } => "usage_observation",
            Self::ExternalSource { .. } => "external_source",
            Self::CodeObservation { .. } => "code_observation",
            Self::DomainConstraint { .. } => "domain_constraint",
        }
    }
}

impl ToCanon for FindingSource {
    fn to_canon(&self) -> Canon {
        let type_name = ("type", Canon::str(self.type_name()));
        match self {
            Self::MeasuredData {
                measured_at,
                revision,
                environment,
            } => Canon::map([
                type_name,
                ("measured_at", measured_at.to_canon()),
                ("revision", revision.to_canon()),
                ("environment", environment.to_canon()),
            ]),
            Self::UsageObservation {
                context,
                reproducibility,
            } => Canon::map([
                type_name,
                ("context", context.to_canon()),
                ("reproducibility", Canon::str(reproducibility.name())),
            ]),
            Self::ExternalSource {
                source,
                version,
                maturity,
            } => Canon::map([
                type_name,
                ("source", source.to_canon()),
                ("version", version.to_canon()),
                ("maturity", Canon::str(maturity.name())),
            ]),
            Self::CodeObservation {
                location,
                revision,
                method,
            } => Canon::map([
                type_name,
                ("location", location.to_canon()),
                ("revision", revision.to_canon()),
                ("method", Canon::str(method.name())),
            ]),
            Self::DomainConstraint {
                authority,
                scope,
                review_condition,
            } => Canon::map([
                type_name,
                ("authority", authority.to_canon()),
                ("scope", scope.to_canon()),
                ("review_condition", review_condition.to_canon()),
            ]),
        }
    }
}

/// A Finding: the first stage of the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    id: FindingId,
    title: NonBlank,
    source: FindingSource,
    evidence: EvidenceLabel,
    body: String,
    hash: ContentHash,
}

impl Finding {
    /// Build a Finding and compute its hash.
    ///
    /// `body` is the normalized text after the front matter. It may be empty
    /// here; gate CR's machine check refuses an empty body.
    pub fn new(
        id: FindingId,
        title: NonBlank,
        source: FindingSource,
        evidence: EvidenceLabel,
        body: String,
    ) -> Self {
        let hash = ContentHash::of(
            HashKind::Finding,
            &Canon::map([
                ("title", title.to_canon()),
                ("source", source.to_canon()),
                ("evidence", Canon::str(evidence.name())),
                ("body", Canon::str(body.as_str())),
            ]),
        );
        Self {
            id,
            title,
            source,
            evidence,
            body,
            hash,
        }
    }

    /// The identifier.
    pub fn id(&self) -> FindingId {
        self.id
    }

    /// The title.
    pub fn title(&self) -> &NonBlank {
        &self.title
    }

    /// The kind and its metadata.
    pub fn source(&self) -> &FindingSource {
        &self.source
    }

    /// How strongly the body is backed.
    pub fn evidence(&self) -> EvidenceLabel {
        self.evidence
    }

    /// The body text.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The hash that approvals refer to.
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::id::test_support::fid;

    pub(crate) fn usage_source() -> FindingSource {
        FindingSource::UsageObservation {
            context: NonBlank::new("login page, enter a wrong password twice").unwrap(),
            reproducibility: Reproducibility::Reproducible,
        }
    }

    pub(crate) fn finding(n: u128, body: &str) -> Finding {
        Finding::new(
            fid(n),
            NonBlank::new("slow login").unwrap(),
            usage_source(),
            EvidenceLabel::Measured,
            body.to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::id::test_support::fid;

    fn all_sources() -> Vec<FindingSource> {
        let rev = RevisionId::from_validated("a1b2c3".into()).unwrap();
        vec![
            FindingSource::MeasuredData {
                measured_at: MeasuredAt::from_validated("2026-11-04T12:40:00Z".into()).unwrap(),
                revision: rev.clone(),
                environment: NonBlank::new("4 cores, warm cache").unwrap(),
            },
            usage_source(),
            FindingSource::ExternalSource {
                source: "https://example.org/spec".parse().unwrap(),
                version: NonBlank::new("2.1").unwrap(),
                maturity: MaturityLevel::Established,
            },
            FindingSource::CodeObservation {
                location: "src/cache.rs:204".parse().unwrap(),
                revision: rev,
                method: ObservationMethod::Read,
            },
            FindingSource::DomainConstraint {
                authority: NonBlank::new("product owner").unwrap(),
                scope: NonBlank::new("all regions").unwrap(),
                review_condition: NonBlank::new("when the budget changes").unwrap(),
            },
        ]
    }

    #[test]
    fn every_source_has_a_distinct_type_name_and_canonical_form() {
        let sources = all_sources();
        let names: std::collections::BTreeSet<_> = sources.iter().map(|s| s.type_name()).collect();
        assert_eq!(names.len(), 5);
        let forms: std::collections::BTreeSet<_> =
            sources.iter().map(|s| s.to_canon().to_bytes()).collect();
        assert_eq!(forms.len(), 5);
    }

    #[test]
    fn the_hash_ignores_the_identifier() {
        assert_eq!(finding(1, "text").hash(), finding(2, "text").hash());
    }

    #[test]
    fn the_hash_follows_every_content_field() {
        let base = finding(1, "text");
        assert_ne!(base.hash(), finding(1, "other").hash());
        let retitled = Finding::new(
            fid(1),
            NonBlank::new("another title").unwrap(),
            usage_source(),
            EvidenceLabel::Measured,
            "text".into(),
        );
        assert_ne!(base.hash(), retitled.hash());
        let relabeled = Finding::new(
            fid(1),
            NonBlank::new("slow login").unwrap(),
            usage_source(),
            EvidenceLabel::Estimated,
            "text".into(),
        );
        assert_ne!(base.hash(), relabeled.hash());
        let other_source = Finding::new(
            fid(1),
            NonBlank::new("slow login").unwrap(),
            FindingSource::UsageObservation {
                context: NonBlank::new("login page, enter a wrong password twice").unwrap(),
                reproducibility: Reproducibility::NotReproducible,
            },
            EvidenceLabel::Measured,
            "text".into(),
        );
        assert_ne!(base.hash(), other_source.hash());
    }

    // Golden vector: fixed Finding, fixed digest.
    #[test]
    fn golden_finding_hash() {
        let f = finding(1, "Login takes 8 s after two failed attempts.\n");
        assert_eq!(
            f.hash().to_string(),
            "sha256:84ef612fe79e50cc57e9280f5c0059bce911fca49d77222f42e0c7a3faddb612"
        );
    }
}
