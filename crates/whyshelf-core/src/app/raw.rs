//! Raw values: what an adapter reads from a file, before references are
//! resolved.
//!
//! A file names its upstream by alias (`grounds`, `spec`) and its revisions by
//! text. An adapter can parse the shape of a file but cannot know whether an alias
//! names anything, nor whether a revision exists: that is the application's
//! business, done through the store index and the revision port. So the adapter
//! hands over a *raw* value, with every reference still a [`Ref`] and every
//! revision still a [`RawRevision`], and the application builds the domain
//! entity from it (see `Workspace` for how the id and the hash come about).
//!
//! A file that does not parse at all is a [`Candidate`] with the reasons; it is
//! stored as a version of its own (a raw hash only) and evaluated as Invalid. The
//! adapter may add the `id:` it could still see in such a file
//! ([`ParseOutcome::Unparsed`]).
//!
//! The adapter, not the application, normalizes text (line breaks to LF, trailing
//! spaces and tabs removed, the file ends in exactly one LF); the
//! application computes the `raw` hash of the normalized text itself.

use std::collections::BTreeMap;
use std::fmt;

use crate::domain::actor::MeasuredAt;
use crate::domain::corpus::Reading;
use crate::domain::design::{Crux, Design, WorkItem};
use crate::domain::error::{EntityError, ValueError};
use crate::domain::event::DocKind;
use crate::domain::finding::{
    EvidenceLabel, Finding, FindingSource, MaturityLevel, ObservationMethod, Reproducibility,
};
use crate::domain::front::{FileClassification, Guess};
use crate::domain::hash::{ContentHash, TextHash};
use crate::domain::id::{DesignId, DocId, FindingId, ItemNumber, SpecId};
use crate::domain::location::{CodeLocation, RelPath, SourceUri};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::pin::{HypId, Hypothesis, PinContract, PinId};
use crate::domain::reference::Ref;
use crate::domain::revision::RevisionId;
use crate::domain::spec::Spec;
use crate::domain::state::UnparsedReason;

/// A revision identifier as the file wrote it, not yet checked.
///
/// Why a type: the application must pass it through `RevisionPort::validate`
/// before a domain value is made, and the type keeps a raw text from being used
/// in its place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawRevision(String);

impl RawRevision {
    /// A revision text; blank is refused.
    pub fn new(text: impl Into<String>) -> Result<Self, ValueError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(ValueError::RevisionEmpty);
        }
        Ok(Self(text))
    }

    /// The text as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RawRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The text of a file after the adapter's normalization.
///
/// It is the content of a stored object, and its `raw` hash is the object's key.
/// Making one is the adapter's promise that the normalization rule was applied;
/// the application never normalizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedText(String);

impl NormalizedText {
    /// Wrap text the adapter has normalized.
    pub fn new(text: String) -> Self {
        Self(text)
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `raw` hash: the key of the stored object.
    pub fn raw_hash(&self) -> ContentHash {
        ContentHash::raw(self.0.as_bytes())
    }

    /// The [`TextHash`]: the key of the object of this version in the corpus
    /// model, and what a gate is told it read.
    pub fn text_hash(&self) -> TextHash {
        TextHash::of(&self.0)
    }
}

/// One file as a [`DocumentSource`](super::ports::DocumentSource) read it: what
/// reading it gave, and its text.
///
/// The [`Reading`] is the typed value the adapter made of the Markdown and the front
/// matter (a document, a file with no `id:`, or one that cannot be read); the
/// [`NormalizedText`] is the same file's text after the adapter's normalization, which
/// is what an object keeps and what a [`TextHash`] is
/// computed from. The two come from one read of one file, so they cannot describe
/// different versions of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDocument {
    reading: Reading,
    text: NormalizedText,
}

impl RawDocument {
    /// The reading of a file and its normalized text.
    pub fn new(reading: Reading, text: NormalizedText) -> Self {
        Self { reading, text }
    }

    /// What reading the file gave.
    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    /// The normalized text.
    pub fn text(&self) -> &NormalizedText {
        &self.text
    }

    /// The reading and the text apart.
    pub fn into_parts(self) -> (Reading, NormalizedText) {
        (self.reading, self.text)
    }
}

/// The metadata of a Finding, with the revision still unchecked.
///
/// The variants mirror [`FindingSource`]; only `revision` differs in type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawFindingSource {
    /// A measurement.
    MeasuredData {
        /// When the measurement was taken.
        measured_at: MeasuredAt,
        /// The revision of the measured system, unchecked.
        revision: RawRevision,
        /// The environment, free text.
        environment: NonBlank,
    },
    /// What users did and what went wrong.
    UsageObservation {
        /// The situation, free text.
        context: NonBlank,
        /// Whether the friction reproduces.
        reproducibility: Reproducibility,
    },
    /// A document from outside.
    ExternalSource {
        /// Where to read it.
        source: SourceUri,
        /// The version it applies to, free text.
        version: NonBlank,
        /// How mature it is.
        maturity: MaturityLevel,
    },
    /// A look at existing code.
    CodeObservation {
        /// Where, at the time of looking.
        location: CodeLocation,
        /// The revision looked at, unchecked.
        revision: RawRevision,
        /// How it was confirmed.
        method: ObservationMethod,
    },
    /// A limit set from outside the code.
    DomainConstraint {
        /// Who decided, free text.
        authority: NonBlank,
        /// What it applies to, free text.
        scope: NonBlank,
        /// When to look again, free text.
        review_condition: NonBlank,
    },
}

/// A parsed Finding file whose revision is not yet checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFinding {
    title: NonBlank,
    source: RawFindingSource,
    evidence: EvidenceLabel,
    body: String,
}

impl RawFinding {
    /// A raw Finding from the parts of a file.
    pub fn new(
        title: NonBlank,
        source: RawFindingSource,
        evidence: EvidenceLabel,
        body: String,
    ) -> Self {
        Self {
            title,
            source,
            evidence,
            body,
        }
    }

    /// The revision that must be checked before the Finding can be built, if
    /// its type has one.
    pub fn revision(&self) -> Option<&RawRevision> {
        match &self.source {
            RawFindingSource::MeasuredData { revision, .. }
            | RawFindingSource::CodeObservation { revision, .. } => Some(revision),
            _ => None,
        }
    }

    /// Build the Finding. `revision` is the checked form of
    /// [`RawFinding::revision`]; a type that has a revision and gets `None` is
    /// reported as a missing field.
    pub fn build(
        self,
        id: FindingId,
        revision: Option<RevisionId>,
    ) -> Result<Finding, UnparsedReason> {
        let missing = || UnparsedReason::MissingField {
            field: "revision".to_owned(),
        };
        let source = match self.source {
            RawFindingSource::MeasuredData {
                measured_at,
                environment,
                ..
            } => FindingSource::MeasuredData {
                measured_at,
                revision: revision.ok_or_else(missing)?,
                environment,
            },
            RawFindingSource::UsageObservation {
                context,
                reproducibility,
            } => FindingSource::UsageObservation {
                context,
                reproducibility,
            },
            RawFindingSource::ExternalSource {
                source,
                version,
                maturity,
            } => FindingSource::ExternalSource {
                source,
                version,
                maturity,
            },
            RawFindingSource::CodeObservation {
                location, method, ..
            } => FindingSource::CodeObservation {
                location,
                revision: revision.ok_or_else(missing)?,
                method,
            },
            RawFindingSource::DomainConstraint {
                authority,
                scope,
                review_condition,
            } => FindingSource::DomainConstraint {
                authority,
                scope,
                review_condition,
            },
        };
        Ok(Finding::new(
            id,
            self.title,
            source,
            self.evidence,
            self.body,
        ))
    }
}

/// A parsed Spec file whose grounds are not yet resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSpec {
    grounds: NonEmptyVec<Ref>,
    contract: NonBlank,
    pins: BTreeMap<PinId, PinContract>,
    hyps: BTreeMap<HypId, Hypothesis>,
}

impl RawSpec {
    /// A raw Spec from the parts of a file.
    pub fn new(
        grounds: NonEmptyVec<Ref>,
        contract: NonBlank,
        pins: BTreeMap<PinId, PinContract>,
        hyps: BTreeMap<HypId, Hypothesis>,
    ) -> Self {
        Self {
            grounds,
            contract,
            pins,
            hyps,
        }
    }

    /// The grounds as written.
    pub fn grounds(&self) -> &NonEmptyVec<Ref> {
        &self.grounds
    }

    /// Build the Spec from the identifiers its grounds resolved to, in the
    /// order of [`RawSpec::grounds`].
    pub fn build(self, id: SpecId, grounds: NonEmptyVec<FindingId>) -> Result<Spec, EntityError> {
        Spec::new(id, grounds, self.contract, self.pins, self.hyps)
    }
}

/// A parsed Design file whose Spec is not yet resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDesign {
    spec: Ref,
    crux: Crux,
    items: BTreeMap<ItemNumber, WorkItem>,
}

impl RawDesign {
    /// A raw Design from the parts of a file.
    pub fn new(spec: Ref, crux: Crux, items: BTreeMap<ItemNumber, WorkItem>) -> Self {
        Self { spec, crux, items }
    }

    /// The Spec as written.
    pub fn spec(&self) -> &Ref {
        &self.spec
    }

    /// Build the Design from the identifier its Spec resolved to.
    pub fn build(self, id: DesignId, spec: SpecId) -> Design {
        Design::new(id, spec, self.crux, self.items)
    }
}

/// A parsed file of any of the three kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawDoc {
    /// A Finding file.
    Finding(RawFinding),
    /// A Spec file.
    Spec(RawSpec),
    /// A Design file.
    Design(RawDesign),
}

impl RawDoc {
    /// The kind of file.
    pub fn kind(&self) -> DocKind {
        match self {
            Self::Finding(_) => DocKind::Finding,
            Self::Spec(_) => DocKind::Spec,
            Self::Design(_) => DocKind::Design,
        }
    }
}

/// What reading one file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    /// The file parsed.
    Parsed(RawDoc),
    /// The file did not parse.
    Unparsed {
        /// Each problem found.
        reasons: NonEmptyVec<UnparsedReason>,
        /// The `id:` of the front matter, when a minimal scan of its lines still
        /// found one, and only one.
        ///
        /// It is a sighting and not a reading: nothing else of the file is trusted,
        /// and the file is no document for it. It lets a document that was judged
        /// under that identity be told from one whose file is gone.
        seen_id: Option<DocId>,
    },
}

/// A file read by the adapter: where it is, its normalized text, and what
/// parsing it gave.
///
/// The application takes the hash from the text itself, so an adapter cannot
/// give a hash that does not match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    path: RelPath,
    text: NormalizedText,
    outcome: ParseOutcome,
    guess: Guess,
    classification: FileClassification,
}

impl Candidate {
    /// A file that parsed.
    pub fn parsed(path: RelPath, text: NormalizedText, doc: RawDoc) -> Self {
        Self {
            path,
            text,
            outcome: ParseOutcome::Parsed(doc),
            guess: Guess::None,
            classification: FileClassification::default(),
        }
    }

    /// A file that did not parse, with the `id:` the adapter could still see in it
    /// (see [`ParseOutcome::Unparsed`]).
    pub fn unparsed(
        path: RelPath,
        text: NormalizedText,
        reasons: NonEmptyVec<UnparsedReason>,
        seen_id: Option<DocId>,
    ) -> Self {
        Self {
            path,
            text,
            outcome: ParseOutcome::Unparsed { reasons, seen_id },
            guess: Guess::None,
            classification: FileClassification::default(),
        }
    }

    /// The same candidate with what its keys say about its kind
    /// ([`DocKind::guess`]), which only a file with no registered kind needs.
    pub fn with_guess(mut self, guess: Guess) -> Self {
        self.guess = guess;
        self
    }

    /// The same candidate with the classification keys of its front matter.
    pub fn with_classification(mut self, classification: FileClassification) -> Self {
        self.classification = classification;
        self
    }

    /// What the keys of the file say about its kind.
    pub fn guess(&self) -> Guess {
        self.guess
    }

    /// The classification keys of the front matter (`tags`, `kind`, `owner`,
    /// `alias`), as the file wrote them.
    pub fn classification(&self) -> &FileClassification {
        &self.classification
    }

    /// The path.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// The normalized text.
    pub fn text(&self) -> &NormalizedText {
        &self.text
    }

    /// The `raw` hash of the text.
    pub fn raw_hash(&self) -> ContentHash {
        self.text.raw_hash()
    }

    /// The [`TextHash`] of the text.
    pub fn text_hash(&self) -> TextHash {
        self.text.text_hash()
    }

    /// What parsing gave.
    pub fn outcome(&self) -> &ParseOutcome {
        &self.outcome
    }

    /// The kind of file, if it parsed.
    pub fn kind(&self) -> Option<DocKind> {
        match &self.outcome {
            ParseOutcome::Parsed(doc) => Some(doc.kind()),
            ParseOutcome::Unparsed { .. } => None,
        }
    }
}
