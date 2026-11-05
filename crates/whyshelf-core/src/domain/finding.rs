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
//! # A Finding file
//!
//! A Finding is a Markdown file: a YAML front matter between two `---` lines, then
//! the body, free Markdown that says what was seen and how it is known. The keys
//! below are read by the `parse` module of the `whyshelf-infra` crate, which lists
//! what it reports when one is missing or wrong. `title`, `evidence` and the keys
//! of the type are required; the body must not be blank (gate CR). The label
//! `evidence` is `measured`, `theoretical` or `estimated`, and a `measured_data`
//! Finding must say `measured`.
//!
//! The words of these keys (the label, the maturity, the method, the
//! reproducibility) are English, and only English: a value in another language is
//! refused. Each is a closed set in the domain with `ALL`, `name()` and `FromStr`
//! ([`EvidenceLabel`], [`MaturityLevel`], [`ObservationMethod`], [`Reproducibility`]),
//! and the parser reads them from there.
//!
//! One complete file for each type:
//!
//! ```markdown
//! ---
//! finding_type: measured_data
//! title: "Median login time on a cold start"
//! evidence: measured
//! measured_at: "2026-11-04T09:30:00Z"   # RFC 3339
//! revision: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"   # 40 hex digits
//! environment: "mid-range phone, release build, office network"
//! ---
//! The median of 40 cold starts was 8.2 s (p95 11.0 s) [measured].
//! ```
//!
//! ```markdown
//! ---
//! finding_type: usage_observation
//! title: "Testers sign in again after every app switch"
//! evidence: measured
//! context: "demo app, sign in, switch to the mail app and back"
//! reproducibility: reproducible   # or: not-reproducible
//! ---
//! Three of three testers were asked to sign in again within a minute.
//! ```
//!
//! ```markdown
//! ---
//! finding_type: external_source
//! title: "Session lifetimes in the platform guidelines"
//! evidence: theoretical
//! source: "https://example.org/guidelines/sessions"   # a URI, or a citation as text
//! version: "2026 edition"
//! maturity: established   # experimental | emerging | established | standard
//! ---
//! The guidelines advise sessions of at least five minutes on mobile.
//! ```
//!
//! ```markdown
//! ---
//! finding_type: code_observation
//! title: "The session is dropped when the app is backgrounded"
//! evidence: measured
//! ref: "src/session.rs:42"   # path:line, at the revision below
//! revision: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
//! method: read   # or: executed
//! ---
//! `Session::on_background` clears the token unconditionally.
//! ```
//!
//! ```markdown
//! ---
//! finding_type: domain_constraint
//! title: "Tokens must not outlive one hour"
//! evidence: theoretical
//! authority: "the security team"
//! scope: "all clients of the sign-in service"
//! review_condition: "when the sign-in provider changes"
//! ---
//! The policy forbids tokens that live longer than one hour.
//! ```
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
use super::vocabulary::closed_vocabulary;

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

closed_vocabulary!(EvidenceLabel, "evidence label", {
    Measured => "measured",
    Theoretical => "theoretical",
    Estimated => "estimated",
});

impl EvidenceLabel {
    /// The literal the hash is computed from. Its own, apart from [`name`](Self::name).
    fn canon_name(self) -> &'static str {
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

closed_vocabulary!(MaturityLevel, "maturity level", {
    Experimental => "experimental",
    Emerging => "emerging",
    Established => "established",
    Standard => "standard",
});

impl MaturityLevel {
    /// The literal the hash is computed from. Its own, apart from [`name`](Self::name).
    fn canon_name(self) -> &'static str {
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

closed_vocabulary!(ObservationMethod, "observation method", {
    Read => "read",
    Executed => "executed",
});

impl ObservationMethod {
    /// The literal the hash is computed from. Its own, apart from [`name`](Self::name).
    fn canon_name(self) -> &'static str {
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

closed_vocabulary!(Reproducibility, "reproducibility", {
    Reproducible => "reproducible",
    NotReproducible => "not-reproducible",
});

impl Reproducibility {
    /// The literal the hash is computed from. Its own, apart from [`name`](Self::name).
    fn canon_name(self) -> &'static str {
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

    /// The `finding_type` name of this variant, as written in a file: the one
    /// [`FindingType::name`] holds. The hash covers it too, so changing a name is
    /// a change of format (`golden_finding_hash` pins it).
    pub fn type_name(&self) -> &'static str {
        self.finding_type().name()
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
                ("reproducibility", Canon::str(reproducibility.canon_name())),
            ]),
            Self::ExternalSource {
                source,
                version,
                maturity,
            } => Canon::map([
                type_name,
                ("source", source.to_canon()),
                ("version", version.to_canon()),
                ("maturity", Canon::str(maturity.canon_name())),
            ]),
            Self::CodeObservation {
                location,
                revision,
                method,
            } => Canon::map([
                type_name,
                ("location", location.to_canon()),
                ("revision", revision.to_canon()),
                ("method", Canon::str(method.canon_name())),
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
                ("evidence", Canon::str(evidence.canon_name())),
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

    #[test]
    fn every_name_reads_back_to_its_value() {
        fn round<T: Copy + PartialEq + std::fmt::Debug + std::str::FromStr>(
            all: &[T],
            name: fn(T) -> &'static str,
        ) where
            T::Err: std::fmt::Debug,
        {
            for value in all {
                assert_eq!(name(*value).parse::<T>().unwrap(), *value);
            }
        }
        round(&EvidenceLabel::ALL, EvidenceLabel::name);
        round(&MaturityLevel::ALL, MaturityLevel::name);
        round(&ObservationMethod::ALL, ObservationMethod::name);
        round(&Reproducibility::ALL, Reproducibility::name);
        assert_eq!(
            "実測".parse::<EvidenceLabel>(),
            Err(crate::domain::error::ValueError::UnknownName {
                what: "evidence label",
                found: "実測".to_owned()
            }),
            "the names are English only"
        );
    }

    /// The hash keeps literals of its own: a word may be renamed for display, and no
    /// identifier moves. This is the table the hash reads.
    #[test]
    fn golden_canon_literals() {
        for (label, literal) in [
            (EvidenceLabel::Measured, "measured"),
            (EvidenceLabel::Theoretical, "theoretical"),
            (EvidenceLabel::Estimated, "estimated"),
        ] {
            assert_eq!(label.canon_name(), literal);
        }
        for (level, literal) in [
            (MaturityLevel::Experimental, "experimental"),
            (MaturityLevel::Emerging, "emerging"),
            (MaturityLevel::Established, "established"),
            (MaturityLevel::Standard, "standard"),
        ] {
            assert_eq!(level.canon_name(), literal);
        }
        for (method, literal) in [
            (ObservationMethod::Read, "read"),
            (ObservationMethod::Executed, "executed"),
        ] {
            assert_eq!(method.canon_name(), literal);
        }
        for (r, literal) in [
            (Reproducibility::Reproducible, "reproducible"),
            (Reproducibility::NotReproducible, "not-reproducible"),
        ] {
            assert_eq!(r.canon_name(), literal);
        }
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
