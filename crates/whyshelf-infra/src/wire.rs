//! The wire format of an event file, version 1.
//!
//! The domain's [`Event`] knows no file format and no format version, so that a
//! change of the format can never change an event identifier (the identifier is
//! computed from domain values). This module owns the format: [`EventV1`] is the
//! shape of one JSON file, [`encode`] writes a record (an event and the time it was
//! written) as that shape and [`decode`] reads one back through a reader chosen by
//! the `v` key. A file is one record; the event of several records is put together
//! by the store that reads them.
//!
//! ```json
//! {
//!   "v": 1,
//!   "id": "sha256:…",
//!   "target": "0192e3a4-7b5c-7d3e-8f01-23456789abcd",
//!   "heads": [],
//!   "at": "2026-11-04T21:40:00+09:00",
//!   "kind": "Revised",
//!   "old": { "content": "sha256:…", "raw": "sha256:…" },
//!   "new": { "content": "sha256:…", "raw": "sha256:…" }
//! }
//! ```
//!
//! # Reading
//!
//! A file whose `v` is not known is refused with [`WireError::UnsupportedVersion`]
//! and not guessed at. A future version adds its own DTO and its own arm in
//! [`decode`], converting into the same domain [`Recorded`]; an old file stays
//! readable without being rewritten.
//!
//! A pin may carry a `kid`, the key a keyed pin was made under, on the pin of an
//! upstream only (the grounds of CS, the Spec of CI, the Design of CA). On the pin of
//! the entity the gate is run on (and on the Finding of CR, the Spec of CM) it is
//! refused with [`WireError::KidOnTargetPin`]; it is not dropped, since the file would
//! then read as other than it says.
//!
//! The `id` stored in the file is a check, not a source: the event is rebuilt
//! from its content with the domain's constructor, and a file whose stored `id`
//! differs from the one computed is refused ([`WireError::IdMismatch`]) because it
//! was edited or corrupted. Revisions are only checked for their outline here: the
//! revision port was already asked when the approval was made.
//!
//! # Compatibility
//!
//! These carry a version. Which change moves which, and what happens to data
//! written before:
//!
//! | what | where it is written | moves when | older data |
//! |---|---|---|---|
//! | settings `version` | `.whyshelf.toml` (`config::SUPPORTED_VERSION`) | a key is renamed, removed, or means something else | a build reads its own version only; another is `ConfigError::Version`, naming both |
//! | event `v` | every event file ([`VERSION`]) | the JSON of an event changes in a way an old reader would misread (a key renamed, retyped or made required) | [`decode`] picks the reader by `v`; a version it does not know is `UnsupportedVersion`, never guessed at. A new version adds a reader and keeps the old one, so an old file is read without being rewritten |
//! | store layout | the `layout` file at the root of each store ([`crate::store_fs::LAYOUT`]) | the names or the places of the files of a store change (what [`crate::store_fs`] documents) | a store with events and no `layout` file, or with another number, is refused whole with `StoreError::Layout`, which names both and points at `whyshelf migrate`; a store with no events and no file is empty |
//! | map `version` | `map.json` and its `map.schema.json` | the shape of the map changes; the schema of the old version is kept | the map is generated and never read back as input: a newer build writes it again (`whyshelf rebuild`) |
//! | cache `schema_version` | the `meta` table of `index.db` | the tables change | a cache of another version is dropped and built again |
//! | hash domain tag | `whyshelf/<kind>/v1` at the start of every digest | the canonical form of a value, or the scope of a hash, changes | see below |
//!
//! An event file's `v` is about the file. It is not in the event identifier, so
//! moving to a new `v` does not change any identifier. A key that a reader does not
//! know is ignored, which is how a key can be added without a new `v`; an old
//! build then simply does not see it.
//!
//! The hash tag is the one that cannot be handled by choosing a reader. The
//! identifier of an event, and the content hash an approval records, are computed
//! from the domain's values under the `v1` tags, and a file whose stored identifier
//! is not the one recomputed is refused. A change of the canonical form or of a hash
//! scope would be a new tag, and every identifier and approval already written
//! would then disagree with the recomputed ones; it would need a migration that
//! rewrites them with their new hashes. There is none: `whyshelf migrate` counts the
//! versions found against the ones this build reads and rewrites nothing.
//!
//! ```
//! use whyshelf_infra::wire::{WireError, decode};
//!
//! // A file of a version this build does not read is refused, not guessed at.
//! let future = br#"{"v": 2}"#;
//! assert!(matches!(
//!     decode(future),
//!     Err(WireError::UnsupportedVersion { found: Some(2) })
//! ));
//! // A file with no version is refused the same way.
//! assert!(matches!(
//!     decode(b"{}"),
//!     Err(WireError::UnsupportedVersion { found: None })
//! ));
//! ```

use std::collections::BTreeSet;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use whyshelf_core::domain::actor::{Actor, Timestamp};
use whyshelf_core::domain::approval::{Attestation, CaChange, CmResult, Pinned, TestOutcome};
use whyshelf_core::domain::error::{EventError, ValueError};
use whyshelf_core::domain::event::{
    ClassChange, ClassSnapshot, DocKind, DocRegistration, Event, EventBody, EventId, Publication,
    Recorded, Registration, StoreKind, Version,
};
use whyshelf_core::domain::hash::{ContentHash, HunkId};
use whyshelf_core::domain::id::{Alias, DesignId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use whyshelf_core::domain::integration::{ChangeFingerprint, HunkScheme};
use whyshelf_core::domain::keyring::{KeyedPin, UpstreamHash};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec};
use whyshelf_core::domain::policy::GatePolicy;
use whyshelf_core::domain::revision::{Revision, RevisionId};
use whyshelf_core::domain::tag::{Tag, TagChange};

/// The version this module writes.
pub const VERSION: u32 = 1;

/// Why an event file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The file is not JSON, or not of the shape of its version.
    #[error("the event file is not of a known shape")]
    Json(#[source] serde_json::Error),
    /// The file has no `v`, or one this build cannot read.
    #[error(
        "the event file has format version {found:?}; this build reads version {VERSION} (`whyshelf migrate` counts the versions of every file)"
    )]
    UnsupportedVersion {
        /// The `v` found, if there was a number.
        found: Option<u64>,
    },
    /// A field holds a value that the rule of its type refuses.
    #[error("the field `{field}` of the event file is invalid")]
    Field {
        /// The key.
        field: &'static str,
        /// The rule that was broken.
        #[source]
        source: ValueError,
    },
    /// A field holds a name this version does not define (a store or a kind).
    #[error("the field `{field}` of the event file holds `{found}`, which is not a known name")]
    UnknownName {
        /// The key.
        field: &'static str,
        /// The text found.
        found: String,
    },
    /// A pin of the entity a gate is run on carries a `kid`. Only the pin of an
    /// upstream can be keyed; on the target it is not a thing this version writes,
    /// and dropping it would read a file as other than it says.
    #[error(
        "the field `{field}` of the event file names a key (`kid`), which only the pin of an upstream may"
    )]
    KidOnTargetPin {
        /// The key of the pin that carries the `kid`.
        field: &'static str,
    },
    /// The fields do not make an event.
    #[error("the event file does not describe an event")]
    Event(#[source] EventError),
    /// The identifier in the file is not the one its content gives.
    #[error("the event file says it is {stored} but its content gives {computed}")]
    IdMismatch {
        /// The identifier written in the file.
        stored: String,
        /// The identifier computed from the content.
        computed: EventId,
    },
}

fn field<T>(name: &'static str, result: Result<T, ValueError>) -> Result<T, WireError> {
    result.map_err(|source| WireError::Field {
        field: name,
        source,
    })
}

fn parse_field<T: FromStr<Err = ValueError>>(
    name: &'static str,
    text: &str,
) -> Result<T, WireError> {
    field(name, text.parse())
}

fn parse_list<T: FromStr<Err = ValueError>>(
    name: &'static str,
    texts: &[String],
) -> Result<Vec<T>, WireError> {
    texts.iter().map(|text| parse_field(name, text)).collect()
}

// ---- the shapes ----------------------------------------------------------

/// One event file, version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventV1 {
    v: u32,
    id: String,
    target: String,
    heads: Vec<String>,
    at: String,
    #[serde(flatten)]
    body: BodyV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum BodyV1 {
    Registered(RegisteredV1),
    Revised {
        old: VersionV1,
        new: VersionV1,
    },
    Reclassified {
        changes: Vec<ChangeV1>,
    },
    Moved {
        old: String,
        new: String,
    },
    Approved {
        by: String,
        attestation: AttestationV1,
    },
    Revoked {
        revokes: String,
        reason: String,
        by: String,
    },
    Purged {
        raw: String,
        reason: String,
    },
    Published(PublishedV1),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entity", rename_all = "lowercase")]
enum RegisteredV1 {
    Finding(DocRegV1),
    Spec(DocRegV1),
    Design(DocRegV1),
    Item { design: String, number: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DocRegV1 {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    tags: Vec<String>,
    first: VersionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
enum ChangeV1 {
    AddTag { tag: String },
    RemoveTag { tag: String },
    SetKind { kind: String },
    SetOwner { owner: String },
    ClearOwner,
    SetAlias { alias: String },
    ClearAlias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "side", rename_all = "lowercase")]
enum PublishedV1 {
    Outgoing { to: String },
    Incoming { from: String, snapshot: SnapshotV1 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SnapshotV1 {
    kind: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    tags: Vec<String>,
    version: VersionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PinnedV1 {
    id: String,
    /// The hash; for a keyed pin (`kid` is there), the MAC.
    hash: String,
    /// The key a keyed pin was made under. Absent for a plain pin, so a file that
    /// pins nothing private has the bytes it always had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "gate")]
enum AttestationV1 {
    #[serde(rename = "CR")]
    Cr { finding: PinnedV1 },
    #[serde(rename = "CS")]
    Cs {
        spec: PinnedV1,
        hyp: String,
        grounds: Vec<PinnedV1>,
        #[serde(default, skip_serializing_if = "GatePolicyV1::is_default")]
        policy: GatePolicyV1,
    },
    #[serde(rename = "CI")]
    Ci { design: PinnedV1, spec: PinnedV1 },
    #[serde(rename = "CA")]
    Ca {
        item: PinnedV1,
        design: PinnedV1,
        revision: RevisionV1,
        tests: TestsV1,
        /// What the approval records of the approved change. Absent when it records
        /// nothing ([`CaChange::NotRecorded`]), so a CA written before fingerprints
        /// were recorded has the bytes it always had. Boxed, as in the domain, so the
        /// other gates stay small.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change: Option<Box<CaChangeV1>>,
    },
    #[serde(rename = "CM")]
    Cm {
        spec: PinnedV1,
        cs: String,
        result: CmResultV1,
    },
}

/// What a CS approval says the check demanded. The default policy is not written, and
/// a file without the key is read as it: it is the one every approval was made under
/// before the policy was recorded, so such a file has the bytes it always had. Any
/// other policy is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct GatePolicyV1 {
    cs_requires_pins: bool,
    cs_requires_hypotheses: bool,
}

impl GatePolicyV1 {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for GatePolicyV1 {
    fn default() -> Self {
        gate_policy_dto(GatePolicy::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "lowercase")]
enum RevisionV1 {
    Commit { id: String },
    Patch { base: String, patch: String },
}

/// What a CA records of the change it approved (`CaChange`, but for the one case that
/// writes no key). A tagged case has a `kind`; the fingerprint's own keys have none, as
/// in the canonical form, so the two are told apart by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum CaChangeV1 {
    /// A case with a tag.
    Tagged(TaggedCaChangeV1),
    /// The change, cut into hunks.
    Fingerprint(FingerprintV1),
}

/// The cases of `CaChange` that are a word and not a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum TaggedCaChangeV1 {
    /// There was nothing to fingerprint.
    Nothing,
    /// The approver's clone could not cut the change into hunks.
    Unavailable {
        /// The name of the reason.
        why: String,
    },
}

/// The fingerprint of the change a CA approved (`ChangeFingerprint`). It is in the
/// identifier of the approval, so it is written whole; a scheme this build does not
/// know is read into `HunkScheme::Unknown` by its name, not refused and not read as
/// another, so that the identifier is still the one the file names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FingerprintV1 {
    base: RevisionV1,
    baseline: RevisionV1,
    hunks: Vec<String>,
    strong: bool,
    scheme: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TestsV1 {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
enum CmResultV1 {
    Pass,
    FailWiring { item_hint: String },
    FailHyp { finding: String },
}

// ---- domain -> wire ------------------------------------------------------

fn version_dto(version: &Version) -> VersionV1 {
    VersionV1 {
        content: version.content().map(ToString::to_string),
        raw: version.raw().to_string(),
    }
}

fn tags_dto(tags: &BTreeSet<Tag>) -> Vec<String> {
    tags.iter().map(ToString::to_string).collect()
}

fn pinned_dto<I: ToString>(pinned: &Pinned<I>) -> PinnedV1 {
    PinnedV1 {
        id: pinned.id().to_string(),
        hash: pinned.hash().to_string(),
        kid: None,
    }
}

fn upstream_dto<I: ToString>(pinned: &Pinned<I, UpstreamHash>) -> PinnedV1 {
    let (hash, kid) = match pinned.pin() {
        UpstreamHash::Plain(hash) => (hash.to_string(), None),
        UpstreamHash::Keyed(pin) => (pin.mac().to_string(), Some(pin.kid().to_string())),
    };
    PinnedV1 {
        id: pinned.id().to_string(),
        hash,
        kid,
    }
}

/// The name of a store in the wire format.
///
/// A literal of its own, apart from `StoreKind::as_str` (the display) and the
/// canonical form (the hash): a stored event must stay readable when the display
/// is renamed, so the wire format is an anti-corruption layer with its own
/// words. `a_store_is_written_with_the_wire_formats_own_names` pins them.
fn store_name(store: StoreKind) -> &'static str {
    match store {
        StoreKind::Repo => "repo",
        StoreKind::Private => "private",
    }
}

fn change_dto(change: &ClassChange) -> ChangeV1 {
    match change {
        ClassChange::Tag(TagChange::Add(tag)) => ChangeV1::AddTag {
            tag: tag.to_string(),
        },
        ClassChange::Tag(TagChange::Remove(tag)) => ChangeV1::RemoveTag {
            tag: tag.to_string(),
        },
        ClassChange::SetKind(kind) => ChangeV1::SetKind {
            kind: kind.to_string(),
        },
        ClassChange::SetOwner(owner) => ChangeV1::SetOwner {
            owner: owner.to_string(),
        },
        ClassChange::ClearOwner => ChangeV1::ClearOwner,
        ClassChange::SetAlias(alias) => ChangeV1::SetAlias {
            alias: alias.to_string(),
        },
        ClassChange::ClearAlias => ChangeV1::ClearAlias,
    }
}

fn gate_policy_dto(policy: GatePolicy) -> GatePolicyV1 {
    GatePolicyV1 {
        cs_requires_pins: policy.cs_requires_pins(),
        cs_requires_hypotheses: policy.cs_requires_hypotheses(),
    }
}

fn revision_dto(revision: &Revision) -> RevisionV1 {
    match revision {
        Revision::Commit(id) => RevisionV1::Commit { id: id.to_string() },
        Revision::Patch { base, patch } => RevisionV1::Patch {
            base: base.to_string(),
            patch: patch.to_string(),
        },
    }
}

fn fingerprint_dto(fingerprint: &ChangeFingerprint) -> FingerprintV1 {
    FingerprintV1 {
        base: revision_dto(fingerprint.base()),
        baseline: revision_dto(fingerprint.baseline_at_approval()),
        hunks: fingerprint
            .hunks()
            .iter()
            .map(ToString::to_string)
            .collect(),
        strong: fingerprint.is_strong(),
        scheme: fingerprint.scheme().name().to_owned(),
    }
}

fn ca_change_dto(change: &CaChange) -> Option<CaChangeV1> {
    match change {
        CaChange::Fingerprinted(fingerprint) => {
            Some(CaChangeV1::Fingerprint(fingerprint_dto(fingerprint)))
        }
        CaChange::NothingToFingerprint => Some(CaChangeV1::Tagged(TaggedCaChangeV1::Nothing)),
        CaChange::NotFingerprinted(why) => {
            Some(CaChangeV1::Tagged(TaggedCaChangeV1::Unavailable {
                why: why.name().to_owned(),
            }))
        }
        CaChange::NotRecorded => None,
    }
}

fn ca_change_from(dto: &CaChangeV1) -> Result<CaChange, WireError> {
    Ok(match dto {
        CaChangeV1::Fingerprint(fingerprint) => {
            CaChange::Fingerprinted(fingerprint_from(fingerprint)?)
        }
        CaChangeV1::Tagged(TaggedCaChangeV1::Nothing) => CaChange::NothingToFingerprint,
        CaChangeV1::Tagged(TaggedCaChangeV1::Unavailable { why }) => {
            CaChange::NotFingerprinted(parse_field("why", why)?)
        }
    })
}

fn attestation_dto(attestation: &Attestation) -> AttestationV1 {
    match attestation {
        Attestation::Cr { finding } => AttestationV1::Cr {
            finding: pinned_dto(finding),
        },
        Attestation::Cs {
            spec,
            hyp,
            grounds,
            policy,
        } => AttestationV1::Cs {
            spec: pinned_dto(spec),
            hyp: hyp.to_string(),
            grounds: grounds.iter().map(upstream_dto).collect(),
            policy: gate_policy_dto(*policy),
        },
        Attestation::Ci { design, spec } => AttestationV1::Ci {
            design: pinned_dto(design),
            spec: upstream_dto(spec),
        },
        Attestation::Ca {
            item,
            design,
            revision,
            tests,
            change,
        } => AttestationV1::Ca {
            item: pinned_dto(item),
            design: upstream_dto(design),
            revision: revision_dto(revision),
            tests: match tests {
                TestOutcome::Passed => TestsV1::Passed,
                TestOutcome::Failed => TestsV1::Failed,
            },
            change: ca_change_dto(change).map(Box::new),
        },
        Attestation::Cm { spec, cs, result } => AttestationV1::Cm {
            spec: pinned_dto(spec),
            cs: cs.to_string(),
            result: match result {
                CmResult::Pass => CmResultV1::Pass,
                CmResult::FailWiring { item_hint } => CmResultV1::FailWiring {
                    item_hint: item_hint.to_string(),
                },
                CmResult::FailHyp { finding } => CmResultV1::FailHyp {
                    finding: finding.to_string(),
                },
            },
        },
    }
}

fn body_dto(body: &EventBody) -> BodyV1 {
    match body {
        EventBody::Registered(Registration::Doc(doc)) => {
            let reg = DocRegV1 {
                path: doc.path().to_string(),
                alias: doc.alias().map(ToString::to_string),
                owner: doc.owner().map(ToString::to_string),
                tags: tags_dto(doc.tags()),
                first: version_dto(doc.first()),
            };
            BodyV1::Registered(match doc.kind() {
                DocKind::Finding => RegisteredV1::Finding(reg),
                DocKind::Spec => RegisteredV1::Spec(reg),
                DocKind::Design => RegisteredV1::Design(reg),
            })
        }
        EventBody::Registered(Registration::Item { design, number }) => {
            BodyV1::Registered(RegisteredV1::Item {
                design: design.to_string(),
                number: number.get(),
            })
        }
        EventBody::Revised { old, new } => BodyV1::Revised {
            old: version_dto(old),
            new: version_dto(new),
        },
        EventBody::Reclassified { changes } => BodyV1::Reclassified {
            changes: changes.iter().map(change_dto).collect(),
        },
        EventBody::Moved { old, new } => BodyV1::Moved {
            old: old.to_string(),
            new: new.to_string(),
        },
        EventBody::Approved { by, attestation } => BodyV1::Approved {
            by: by.to_string(),
            attestation: attestation_dto(attestation),
        },
        EventBody::Revoked {
            revokes,
            reason,
            by,
        } => BodyV1::Revoked {
            revokes: revokes.to_string(),
            reason: reason.to_string(),
            by: by.to_string(),
        },
        EventBody::Purged { raw, reason } => BodyV1::Purged {
            raw: raw.to_string(),
            reason: reason.to_string(),
        },
        EventBody::Published(Publication::Outgoing { to }) => {
            BodyV1::Published(PublishedV1::Outgoing {
                to: store_name(*to).to_owned(),
            })
        }
        EventBody::Published(Publication::Incoming { from, snapshot }) => {
            BodyV1::Published(PublishedV1::Incoming {
                from: store_name(*from).to_owned(),
                snapshot: SnapshotV1 {
                    kind: snapshot.kind().to_string(),
                    path: snapshot.path().to_string(),
                    alias: snapshot.alias().map(ToString::to_string),
                    owner: snapshot.owner().map(ToString::to_string),
                    tags: tags_dto(snapshot.tags()),
                    version: version_dto(snapshot.version()),
                },
            })
        }
    }
}

impl EventV1 {
    /// The version-1 shape of a record: an event and the time it was written.
    pub fn of(event: &Event, at: &Timestamp) -> Self {
        Self {
            v: VERSION,
            id: event.id().to_string(),
            target: event.target().to_string(),
            heads: event.heads().iter().map(ToString::to_string).collect(),
            at: at.to_string(),
            body: body_dto(event.body()),
        }
    }
}

// ---- wire -> domain ------------------------------------------------------

fn hash(name: &'static str, text: &str) -> Result<ContentHash, WireError> {
    parse_field(name, text)
}

fn version_from(dto: &VersionV1) -> Result<Version, WireError> {
    let raw = hash("raw", &dto.raw)?;
    Ok(match &dto.content {
        Some(content) => Version::Parsed {
            content: hash("content", content)?,
            raw,
        },
        None => Version::Unparsed { raw },
    })
}

fn tags_from(texts: &[String]) -> Result<BTreeSet<Tag>, WireError> {
    Ok(parse_list::<Tag>("tags", texts)?.into_iter().collect())
}

fn optional<T: FromStr<Err = ValueError>>(
    name: &'static str,
    text: &Option<String>,
) -> Result<Option<T>, WireError> {
    text.as_deref()
        .map(|text| parse_field(name, text))
        .transpose()
}

fn actor(name: &'static str, text: &str) -> Result<Actor, WireError> {
    field(name, Actor::new(text))
}

fn non_blank(name: &'static str, text: &str) -> Result<NonBlank, WireError> {
    field(name, NonBlank::new(text))
}

fn entity(name: &'static str, text: &str) -> Result<EntityId, WireError> {
    parse_field(name, text)
}

fn pinned_from<I>(
    name: &'static str,
    dto: &PinnedV1,
    wrap: fn(EntityId) -> I,
) -> Result<Pinned<I>, WireError> {
    if dto.kid.is_some() {
        return Err(WireError::KidOnTargetPin { field: name });
    }
    Ok(Pinned::new(
        wrap(entity(name, &dto.id)?),
        hash(name, &dto.hash)?,
    ))
}

fn upstream_from<I>(
    name: &'static str,
    dto: &PinnedV1,
    wrap: fn(EntityId) -> I,
) -> Result<Pinned<I, UpstreamHash>, WireError> {
    let pin = match &dto.kid {
        None => UpstreamHash::Plain(hash(name, &dto.hash)?),
        Some(kid) => UpstreamHash::Keyed(KeyedPin::new(
            parse_field("kid", kid)?,
            parse_field(name, &dto.hash)?,
        )),
    };
    Ok(Pinned::new(wrap(entity(name, &dto.id)?), pin))
}

/// The store a wire name stands for: the inverse of `store_name`, with the same
/// literals and the same reason for keeping them apart from the display.
fn store_from(name: &'static str, text: &str) -> Result<StoreKind, WireError> {
    match text {
        "repo" => Ok(StoreKind::Repo),
        "private" => Ok(StoreKind::Private),
        _ => Err(WireError::UnknownName {
            field: name,
            found: text.to_owned(),
        }),
    }
}

fn doc_kind(name: &'static str, text: &str) -> Result<DocKind, WireError> {
    DocKind::from_name(text).ok_or_else(|| WireError::UnknownName {
        field: name,
        found: text.to_owned(),
    })
}

fn change_from(dto: &ChangeV1) -> Result<ClassChange, WireError> {
    Ok(match dto {
        ChangeV1::AddTag { tag } => ClassChange::Tag(TagChange::Add(parse_field("tag", tag)?)),
        ChangeV1::RemoveTag { tag } => {
            ClassChange::Tag(TagChange::Remove(parse_field("tag", tag)?))
        }
        ChangeV1::SetKind { kind } => ClassChange::SetKind(doc_kind("kind", kind)?),
        ChangeV1::SetOwner { owner } => ClassChange::SetOwner(actor("owner", owner)?),
        ChangeV1::ClearOwner => ClassChange::ClearOwner,
        ChangeV1::SetAlias { alias } => {
            ClassChange::SetAlias(parse_field::<Alias>("alias", alias)?)
        }
        ChangeV1::ClearAlias => ClassChange::ClearAlias,
    })
}

fn revision_id(name: &'static str, text: &str) -> Result<RevisionId, WireError> {
    field(name, RevisionId::from_validated(text.to_owned()))
}

fn revision_from(name: &'static str, dto: &RevisionV1) -> Result<Revision, WireError> {
    Ok(match dto {
        RevisionV1::Commit { id } => Revision::Commit(revision_id(name, id)?),
        RevisionV1::Patch { base, patch } => Revision::Patch {
            base: revision_id(name, base)?,
            patch: hash("patch", patch)?,
        },
    })
}

fn fingerprint_from(dto: &FingerprintV1) -> Result<ChangeFingerprint, WireError> {
    let hunks = parse_list::<HunkId>("change.hunks", &dto.hunks)?
        .into_iter()
        .collect();
    // A scheme this build does not know is kept by its name, not refused: the file
    // stays readable and the identifier of the approval, which hashes the name, stays
    // what it was. Only a name that is not the shape of a name is an error.
    let scheme = field("change.scheme", HunkScheme::from_name(&dto.scheme))?;
    Ok(ChangeFingerprint::new(
        revision_from("change.base", &dto.base)?,
        revision_from("change.baseline", &dto.baseline)?,
        field("change.hunks", NonEmptySet::from_set(hunks))?,
        dto.strong,
        scheme,
    ))
}

fn attestation_from(dto: &AttestationV1) -> Result<Attestation, WireError> {
    Ok(match dto {
        AttestationV1::Cr { finding } => Attestation::Cr {
            finding: pinned_from("finding", finding, FindingId::new)?,
        },
        AttestationV1::Cs {
            spec,
            hyp,
            grounds,
            policy,
        } => {
            let grounds = grounds
                .iter()
                .map(|g| upstream_from("grounds", g, FindingId::new))
                .collect::<Result<Vec<_>, _>>()?;
            Attestation::Cs {
                spec: pinned_from("spec", spec, SpecId::new)?,
                hyp: hash("hyp", hyp)?,
                grounds: field("grounds", NonEmptyVec::from_vec(grounds))?,
                policy: GatePolicy::new(policy.cs_requires_pins, policy.cs_requires_hypotheses),
            }
        }
        AttestationV1::Ci { design, spec } => Attestation::Ci {
            design: pinned_from("design", design, DesignId::new)?,
            spec: upstream_from("spec", spec, SpecId::new)?,
        },
        AttestationV1::Ca {
            item,
            design,
            revision,
            tests,
            change,
        } => Attestation::Ca {
            item: pinned_from("item", item, ItemId::new)?,
            design: upstream_from("design", design, DesignId::new)?,
            revision: revision_from("revision", revision)?,
            tests: match tests {
                TestsV1::Passed => TestOutcome::Passed,
                TestsV1::Failed => TestOutcome::Failed,
            },
            change: change
                .as_deref()
                .map(ca_change_from)
                .transpose()?
                .unwrap_or(CaChange::NotRecorded),
        },
        AttestationV1::Cm { spec, cs, result } => Attestation::Cm {
            spec: pinned_from("spec", spec, SpecId::new)?,
            cs: parse_field("cs", cs)?,
            result: match result {
                CmResultV1::Pass => CmResult::Pass,
                CmResultV1::FailWiring { item_hint } => CmResult::FailWiring {
                    item_hint: non_blank("item_hint", item_hint)?,
                },
                CmResultV1::FailHyp { finding } => CmResult::FailHyp {
                    finding: FindingId::new(entity("finding", finding)?),
                },
            },
        },
    })
}

fn doc_registration(kind: DocKind, dto: &DocRegV1) -> Result<EventBody, WireError> {
    Ok(EventBody::Registered(Registration::Doc(
        DocRegistration::new(
            kind,
            parse_field::<RelPath>("path", &dto.path)?,
            optional::<Alias>("alias", &dto.alias)?,
            dto.owner
                .as_deref()
                .map(|owner| actor("owner", owner))
                .transpose()?,
            tags_from(&dto.tags)?,
            version_from(&dto.first)?,
        ),
    )))
}

fn body_from(dto: &BodyV1) -> Result<EventBody, WireError> {
    Ok(match dto {
        BodyV1::Registered(RegisteredV1::Finding(reg)) => doc_registration(DocKind::Finding, reg)?,
        BodyV1::Registered(RegisteredV1::Spec(reg)) => doc_registration(DocKind::Spec, reg)?,
        BodyV1::Registered(RegisteredV1::Design(reg)) => doc_registration(DocKind::Design, reg)?,
        BodyV1::Registered(RegisteredV1::Item { design, number }) => {
            EventBody::Registered(Registration::Item {
                design: DesignId::new(entity("design", design)?),
                number: field("number", ItemNumber::new(*number))?,
            })
        }
        BodyV1::Revised { old, new } => EventBody::Revised {
            old: version_from(old)?,
            new: version_from(new)?,
        },
        BodyV1::Reclassified { changes } => {
            let changes = changes
                .iter()
                .map(change_from)
                .collect::<Result<Vec<_>, _>>()?;
            EventBody::Reclassified {
                changes: field("changes", NonEmptyVec::from_vec(changes))?,
            }
        }
        BodyV1::Moved { old, new } => EventBody::Moved {
            old: parse_field("old", old)?,
            new: parse_field("new", new)?,
        },
        BodyV1::Approved { by, attestation } => EventBody::Approved {
            by: actor("by", by)?,
            attestation: attestation_from(attestation)?,
        },
        BodyV1::Revoked {
            revokes,
            reason,
            by,
        } => EventBody::Revoked {
            revokes: parse_field("revokes", revokes)?,
            reason: non_blank("reason", reason)?,
            by: actor("by", by)?,
        },
        BodyV1::Purged { raw, reason } => EventBody::Purged {
            raw: hash("raw", raw)?,
            reason: non_blank("reason", reason)?,
        },
        BodyV1::Published(PublishedV1::Outgoing { to }) => {
            EventBody::Published(Publication::Outgoing {
                to: store_from("to", to)?,
            })
        }
        BodyV1::Published(PublishedV1::Incoming { from, snapshot }) => {
            EventBody::Published(Publication::Incoming {
                from: store_from("from", from)?,
                snapshot: ClassSnapshot::new(
                    doc_kind("kind", &snapshot.kind)?,
                    parse_field("path", &snapshot.path)?,
                    optional::<Alias>("alias", &snapshot.alias)?,
                    snapshot
                        .owner
                        .as_deref()
                        .map(|owner| actor("owner", owner))
                        .transpose()?,
                    tags_from(&snapshot.tags)?,
                    version_from(&snapshot.version)?,
                ),
            })
        }
    })
}

impl EventV1 {
    /// Rebuild the domain record, checking that the identifier in the file is the
    /// one its content gives.
    pub fn into_recorded(self) -> Result<Recorded, WireError> {
        let heads = self
            .heads
            .iter()
            .map(|head| parse_field::<EventId>("heads", head))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let at = field("at", Timestamp::from_validated(self.at.clone()))?;
        let event = Event::new(
            entity("target", &self.target)?,
            heads,
            body_from(&self.body)?,
        )
        .map_err(WireError::Event)?;
        if event.id().to_string() != self.id {
            return Err(WireError::IdMismatch {
                stored: self.id,
                computed: event.id(),
            });
        }
        Ok(Recorded::new(event, at))
    }
}

// ---- files ---------------------------------------------------------------

/// The text of an event file, the file of one record: pretty JSON with a closing line
/// feed, holding the event and the time it was written.
pub fn encode(event: &Event, at: &Timestamp) -> Result<String, WireError> {
    let mut text =
        serde_json::to_string_pretty(&EventV1::of(event, at)).map_err(WireError::Json)?;
    text.push('\n');
    Ok(text)
}

/// The `v` of an event file, read without reading the rest of the file as an event.
///
/// `None` for a file that is not JSON and for one whose `v` is missing or is no
/// whole number. It is what [`crate::format`] counts: a version that this build cannot
/// read is still a version, and it is counted as such.
pub fn version_of(bytes: &[u8]) -> Option<u64> {
    #[derive(serde::Deserialize)]
    struct Head {
        v: Option<u64>,
    }
    serde_json::from_slice::<Head>(bytes).ok()?.v
}

/// Read an event file, choosing the reader by its `v`: the event, with the one time
/// the file holds.
pub fn decode(bytes: &[u8]) -> Result<Recorded, WireError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(WireError::Json)?;
    match value.get("v").and_then(serde_json::Value::as_u64) {
        Some(1) => serde_json::from_value::<EventV1>(value)
            .map_err(WireError::Json)?
            .into_recorded(),
        found => Err(WireError::UnsupportedVersion { found }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use whyshelf_core::domain::approval::CsRef;

    fn eid(n: u128) -> EntityId {
        let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
        EntityId::new(Uuid::from_u128(bits)).unwrap()
    }

    fn at() -> Timestamp {
        Timestamp::from_validated("2026-11-04T21:40:00+09:00".to_owned()).unwrap()
    }

    fn h(text: &str) -> ContentHash {
        ContentHash::raw(text.as_bytes())
    }

    fn path(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    fn pinned<I>(wrap: fn(EntityId) -> I, n: u128, text: &str) -> Pinned<I> {
        Pinned::new(wrap(eid(n)), h(text))
    }

    fn upstream<I>(wrap: fn(EntityId) -> I, n: u128, text: &str) -> Pinned<I, UpstreamHash> {
        Pinned::new(wrap(eid(n)), UpstreamHash::Plain(h(text)))
    }

    fn keyed<I>(wrap: fn(EntityId) -> I, n: u128, text: &str) -> Pinned<I, UpstreamHash> {
        use whyshelf_core::domain::hash::PinKey;
        use whyshelf_core::domain::keyring::PinKeyring;
        let ring = PinKeyring::new("k1".parse().unwrap(), PinKey::new(&[5; 32]).unwrap());
        let pin = ring.pin(eid(n), &h(text)).unwrap();
        Pinned::new(wrap(eid(n)), UpstreamHash::Keyed(pin))
    }

    /// A fingerprint of two hunks, one strong, made from a base that is a commit and a
    /// baseline tip that is a patch, so every part of it is written.
    fn fingerprint() -> ChangeFingerprint {
        use whyshelf_core::domain::integration::Hunk;
        let commit = |text: &str| RevisionId::from_validated(text.repeat(20)).unwrap();
        ChangeFingerprint::of_hunks(
            Revision::Commit(commit("b0")),
            Revision::Patch {
                base: commit("b1"),
                patch: h("tip"),
            },
            &[
                Hunk::new(path("src/a.rs"), vec![], vec!["}".to_owned()]),
                Hunk::new(
                    path("src/a.rs"),
                    vec!["let timeout = 10;".to_owned()],
                    vec!["let timeout = Duration::from_secs(30);".to_owned()],
                ),
            ],
            HunkScheme::CURRENT,
        )
        .unwrap()
    }

    fn alice() -> Actor {
        Actor::new("alice").unwrap()
    }

    fn event(target: u128, heads: &[EventId], body: EventBody) -> Event {
        Event::new(eid(target), heads.iter().copied().collect(), body).unwrap()
    }

    /// One event of every kind and variant of the domain.
    fn every_event() -> Vec<Event> {
        let version = Version::Parsed {
            content: h("c"),
            raw: h("r"),
        };
        let registered = event(
            1,
            &[],
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                DocKind::Spec,
                path("docs/spec/cache-timeout.md"),
                Some("spec/cache-timeout".parse().unwrap()),
                Some(alice()),
                ["area:auth".parse().unwrap(), "wip".parse().unwrap()].into(),
                version.clone(),
            ))),
        );
        let registered_bare = event(
            2,
            &[],
            EventBody::Registered(Registration::Doc(DocRegistration::new(
                DocKind::Finding,
                path("docs/finding/a.md"),
                None,
                None,
                BTreeSet::new(),
                Version::Unparsed { raw: h("broken") },
            ))),
        );
        let item = event(
            3,
            &[],
            EventBody::Registered(Registration::Item {
                design: DesignId::new(eid(4)),
                number: ItemNumber::new(2).unwrap(),
            }),
        );
        let revised = event(
            1,
            &[registered.id()],
            EventBody::Revised {
                old: version.clone(),
                new: Version::Unparsed { raw: h("r2") },
            },
        );
        let reclassified = event(
            1,
            &[revised.id()],
            EventBody::Reclassified {
                changes: NonEmptyVec::from_vec(vec![
                    ClassChange::Tag(TagChange::Add("area:auth".parse().unwrap())),
                    ClassChange::Tag(TagChange::Remove("wip".parse().unwrap())),
                    ClassChange::SetKind(DocKind::Design),
                    ClassChange::SetOwner(alice()),
                    ClassChange::ClearOwner,
                    ClassChange::SetAlias("spec/x".parse().unwrap()),
                    ClassChange::ClearAlias,
                ])
                .unwrap(),
            },
        );
        let moved = event(
            1,
            &[reclassified.id()],
            EventBody::Moved {
                old: path("a.md"),
                new: path("b.md"),
            },
        );
        let commit = RevisionId::from_validated("a1".repeat(20)).unwrap();
        let approvals = [
            Attestation::Cr {
                finding: pinned(FindingId::new, 5, "f"),
            },
            Attestation::Cs {
                spec: pinned(SpecId::new, 6, "s"),
                hyp: h("hyp"),
                grounds: NonEmptyVec::new(
                    upstream(FindingId::new, 5, "f"),
                    vec![keyed(FindingId::new, 7, "g")],
                ),
                policy: GatePolicy::default(),
            },
            Attestation::Cs {
                spec: pinned(SpecId::new, 6, "s"),
                hyp: h("hyp"),
                grounds: NonEmptyVec::new(upstream(FindingId::new, 5, "f"), vec![]),
                policy: GatePolicy::new(false, true),
            },
            Attestation::Ci {
                design: pinned(DesignId::new, 8, "d"),
                spec: keyed(SpecId::new, 6, "s"),
            },
            Attestation::Ca {
                item: pinned(ItemId::new, 9, "i"),
                design: upstream(DesignId::new, 8, "d"),
                revision: Revision::Commit(commit.clone()),
                tests: TestOutcome::Passed,
                change: CaChange::NotRecorded,
            },
            Attestation::Ca {
                item: pinned(ItemId::new, 9, "i"),
                design: keyed(DesignId::new, 8, "d"),
                revision: Revision::Patch {
                    base: commit,
                    patch: h("patch"),
                },
                tests: TestOutcome::Failed,
                change: CaChange::NotRecorded,
            },
            Attestation::Ca {
                item: pinned(ItemId::new, 9, "i"),
                design: upstream(DesignId::new, 8, "d"),
                revision: Revision::Commit(RevisionId::from_validated("c3".repeat(20)).unwrap()),
                tests: TestOutcome::Passed,
                change: CaChange::Fingerprinted(fingerprint()),
            },
            Attestation::Cm {
                spec: pinned(SpecId::new, 6, "hyp"),
                cs: CsRef::Event(moved.id()),
                result: CmResult::Pass,
            },
            Attestation::Cm {
                spec: pinned(SpecId::new, 6, "hyp"),
                cs: CsRef::Event(moved.id()),
                result: CmResult::FailWiring {
                    item_hint: NonBlank::new("a retry item").unwrap(),
                },
            },
            Attestation::Cm {
                spec: pinned(SpecId::new, 6, "hyp"),
                cs: CsRef::Event(moved.id()),
                result: CmResult::FailHyp {
                    finding: FindingId::new(eid(10)),
                },
            },
        ];
        let approved: Vec<Event> = approvals
            .into_iter()
            .map(|attestation| {
                let target = attestation.subject();
                Event::new(
                    target,
                    BTreeSet::new(),
                    EventBody::Approved {
                        by: alice(),
                        attestation,
                    },
                )
                .unwrap()
            })
            .collect();
        let revoked = event(
            5,
            &[approved[0].id()],
            EventBody::Revoked {
                revokes: approved[0].id(),
                reason: NonBlank::new("the source was retracted").unwrap(),
                by: alice(),
            },
        );
        let purged = event(
            1,
            &[moved.id()],
            EventBody::Purged {
                raw: h("r"),
                reason: NonBlank::new("a secret slipped through").unwrap(),
            },
        );
        let outgoing = event(
            1,
            &[purged.id()],
            EventBody::Published(Publication::Outgoing {
                to: StoreKind::Repo,
            }),
        );
        let incoming = event(
            1,
            &[],
            EventBody::Published(Publication::Incoming {
                from: StoreKind::Private,
                snapshot: ClassSnapshot::new(
                    DocKind::Spec,
                    path("docs/spec/cache-timeout.md"),
                    Some("spec/cache-timeout".parse().unwrap()),
                    None,
                    ["area:auth".parse().unwrap()].into(),
                    version,
                ),
            }),
        );
        let mut all = vec![
            registered,
            registered_bare,
            item,
            revised,
            reclassified,
            moved,
            revoked,
            purged,
            outgoing,
            incoming,
        ];
        all.extend(approved);
        all
    }

    #[test]
    fn every_kind_of_event_survives_a_round_trip() {
        for event in every_event() {
            let text = encode(&event, &at()).unwrap();
            let back = decode(text.as_bytes()).unwrap();
            assert_eq!(back.event(), &event, "{text}");
            assert_eq!(back.event().id(), event.id());
            assert_eq!(back.shown_at(), &at());
            assert_eq!(back.at().len(), 1, "a file is one record");
        }
    }

    #[test]
    fn the_file_is_pretty_json_with_a_version_and_a_closing_line_feed() {
        let event = &every_event()[3];
        let text = encode(event, &at()).unwrap();
        assert!(text.ends_with("}\n"));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["v"], 1);
        assert_eq!(json["kind"], "Revised");
        assert_eq!(json["id"], event.id().to_string());
        assert!(text.contains("\n  \"target\""), "two-space indent: {text}");
        // Encoding twice gives the same bytes.
        assert_eq!(text, encode(event, &at()).unwrap());
    }

    #[test]
    fn the_registration_and_attestation_carry_their_tags() {
        let all = every_event();
        let registered: serde_json::Value =
            serde_json::from_str(&encode(&all[0], &at()).unwrap()).unwrap();
        assert_eq!(registered["kind"], "Registered");
        assert_eq!(registered["entity"], "spec");
        assert_eq!(registered["first"]["raw"], h("r").to_string());
        let ca = all
            .iter()
            .find(|e| {
                matches!(
                    e.body(),
                    EventBody::Approved {
                        attestation: Attestation::Ca { .. },
                        ..
                    }
                )
            })
            .unwrap();
        let ca: serde_json::Value = serde_json::from_str(&encode(ca, &at()).unwrap()).unwrap();
        assert_eq!(ca["attestation"]["gate"], "CA");
        assert_eq!(ca["attestation"]["revision"]["form"], "commit");
        assert_eq!(ca["attestation"]["tests"], "passed");
    }

    #[test]
    fn a_cs_writes_its_policy_only_when_it_is_not_the_default() {
        let cs_files: Vec<serde_json::Value> = every_event()
            .iter()
            .filter(|e| {
                matches!(
                    e.body(),
                    EventBody::Approved {
                        attestation: Attestation::Cs { .. },
                        ..
                    }
                )
            })
            .map(|e| serde_json::from_str(&encode(e, &at()).unwrap()).unwrap())
            .collect();
        let [strict, relaxed] = cs_files.as_slice() else {
            panic!("every_event has two CS approvals");
        };
        // A file of the default policy is the file written before the policy was
        // recorded, and it still reads with the identifier it was named by.
        assert!(strict["attestation"].get("policy").is_none());
        assert_eq!(
            relaxed["attestation"]["policy"],
            serde_json::json!({"cs_requires_pins": false, "cs_requires_hypotheses": true})
        );
        let back = decode(strict.to_string().as_bytes()).unwrap();
        let EventBody::Approved { attestation, .. } = back.event().body() else {
            panic!("an approval");
        };
        assert_eq!(
            attestation.gate_basis(),
            whyshelf_core::domain::approval::GateBasis::Cs {
                policy: GatePolicy::default()
            }
        );
        // The policy enters the identifier: a relaxed approval edited to look strict
        // no longer matches its name.
        let mut edited = relaxed.clone();
        edited["attestation"]
            .as_object_mut()
            .unwrap()
            .remove("policy");
        assert!(matches!(
            decode(edited.to_string().as_bytes()),
            Err(WireError::IdMismatch { .. })
        ));
    }

    /// The fingerprint of a CA is written, not dropped: it is in the identifier, and a
    /// file without it would read as another approval.
    #[test]
    fn a_ca_writes_its_fingerprint_and_one_without_it_has_the_bytes_it_had() {
        let ca_files: Vec<(Event, serde_json::Value)> = every_event()
            .into_iter()
            .filter(|e| {
                matches!(
                    e.body(),
                    EventBody::Approved {
                        attestation: Attestation::Ca { .. },
                        ..
                    }
                )
            })
            .map(|e| {
                let file = serde_json::from_str(&encode(&e, &at()).unwrap()).unwrap();
                (e, file)
            })
            .collect();
        let [(_, plain), (_, patch), (with, file)] = ca_files.as_slice() else {
            panic!("every_event has three CA approvals");
        };
        assert!(plain["attestation"].get("change").is_none());
        assert!(patch["attestation"].get("change").is_none());

        let change = &file["attestation"]["change"];
        assert_eq!(change["scheme"], "hunks-v1");
        assert_eq!(change["strong"], true);
        assert_eq!(change["base"]["form"], "commit");
        assert_eq!(change["baseline"]["form"], "patch");
        assert_eq!(change["hunks"].as_array().unwrap().len(), 2);
        let back = decode(file.to_string().as_bytes()).unwrap();
        assert_eq!(back.event(), with);
        let EventBody::Approved { attestation, .. } = back.event().body() else {
            panic!("an approval");
        };
        assert_eq!(attestation.fingerprint(), Some(&fingerprint()));

        // Taken out, the file names an approval its content no longer gives.
        let mut stripped = file.clone();
        stripped["attestation"]
            .as_object_mut()
            .unwrap()
            .remove("change");
        assert!(matches!(
            decode(stripped.to_string().as_bytes()),
            Err(WireError::IdMismatch { .. })
        ));
        // A name that is not the shape of a name is refused.
        let mut shapeless = file.clone();
        shapeless["attestation"]["change"]["scheme"] = serde_json::json!("Hunks V9");
        assert!(matches!(
            decode(shapeless.to_string().as_bytes()),
            Err(WireError::Field {
                field: "change.scheme",
                ..
            })
        ));
        // A change of no hunk is no fingerprint.
        let mut empty = file.clone();
        empty["attestation"]["change"]["hunks"] = serde_json::json!([]);
        assert!(matches!(
            decode(empty.to_string().as_bytes()),
            Err(WireError::Field {
                field: "change.hunks",
                ..
            })
        ));
    }

    /// The cases of the change that are a word are written with their tag, and read
    /// back to the same approval under the same identifier.
    #[test]
    fn a_ca_that_says_why_it_has_no_fingerprint_round_trips() {
        use whyshelf_core::domain::approval::CaChange;
        use whyshelf_core::domain::integration::FingerprintUnavailable;
        let ca_with = |change: CaChange| {
            let attestation = Attestation::Ca {
                item: pinned(ItemId::new, 9, "i"),
                design: upstream(DesignId::new, 8, "d"),
                revision: Revision::Commit(RevisionId::from_validated("c3".repeat(20)).unwrap()),
                tests: TestOutcome::Passed,
                change,
            };
            Event::new(
                attestation.subject(),
                BTreeSet::new(),
                EventBody::Approved {
                    by: alice(),
                    attestation,
                },
            )
            .unwrap()
        };
        let cases = [(CaChange::NothingToFingerprint, "nothing", None)]
            .into_iter()
            .chain(FingerprintUnavailable::ALL.map(|why| {
                (
                    CaChange::NotFingerprinted(why),
                    "unavailable",
                    Some(why.name()),
                )
            }));
        let mut ids = BTreeSet::new();
        for (change, kind, why) in cases {
            let event = ca_with(change.clone());
            assert!(
                ids.insert(event.id()),
                "each case is an approval of its own"
            );
            let file: serde_json::Value =
                serde_json::from_str(&encode(&event, &at()).unwrap()).unwrap();
            assert_eq!(file["attestation"]["change"]["kind"], kind);
            assert_eq!(
                file["attestation"]["change"]
                    .get("why")
                    .and_then(|v| v.as_str()),
                why
            );
            let back = decode(file.to_string().as_bytes()).unwrap();
            assert_eq!(back.event(), &event);
            let EventBody::Approved { attestation, .. } = back.event().body() else {
                panic!("an approval");
            };
            assert_eq!(attestation.change(), Some(&change));
        }
        // A reason this build has no name for is refused, not read as another.
        let event = ca_with(CaChange::NotFingerprinted(FingerprintUnavailable::Shallow));
        let mut file: serde_json::Value =
            serde_json::from_str(&encode(&event, &at()).unwrap()).unwrap();
        file["attestation"]["change"]["why"] = serde_json::json!("elsewhere");
        assert!(matches!(
            decode(file.to_string().as_bytes()),
            Err(WireError::Field { field: "why", .. })
        ));
    }

    /// A CM names the CS approval it stands on by the form of what that is.
    #[test]
    fn a_cm_on_a_judgement_of_a_ledger_round_trips_and_is_not_an_event() {
        use whyshelf_core::domain::judgement::JudgementId;
        let on_judgement = Attestation::Cm {
            spec: pinned(SpecId::new, 6, "hyp"),
            cs: CsRef::Judgement(JudgementId::new(eid(70))),
            result: CmResult::Pass,
        };
        let event = Event::new(
            on_judgement.subject(),
            BTreeSet::new(),
            EventBody::Approved {
                by: alice(),
                attestation: on_judgement.clone(),
            },
        )
        .unwrap();
        let file: serde_json::Value =
            serde_json::from_str(&encode(&event, &at()).unwrap()).unwrap();
        assert_eq!(
            file["attestation"]["cs"],
            JudgementId::new(eid(70)).to_string()
        );
        let back = decode(file.to_string().as_bytes()).unwrap();
        assert_eq!(back.event(), &event);
    }

    /// A fingerprint made under a scheme this build does not have is read back whole,
    /// under the same identifier, and what it says of the landing is that it cannot be
    /// told: the file is not refused, and the approval is not read as another.
    #[test]
    fn a_fingerprint_of_an_unknown_scheme_round_trips_with_the_same_id_and_is_unknown() {
        use whyshelf_core::domain::integration::{
            ApprovedChange, BaselineFacts, Integration, UnknownWhy,
        };
        use whyshelf_core::domain::knowable::Knowable;

        let known = fingerprint();
        let future = ChangeFingerprint::new(
            known.base().clone(),
            known.baseline_at_approval().clone(),
            known.hunks().clone(),
            known.is_strong(),
            HunkScheme::from_name("hunks-v9").unwrap(),
        );
        let approved = |fingerprint: ChangeFingerprint| {
            let attestation = Attestation::Ca {
                item: pinned(ItemId::new, 9, "i"),
                design: upstream(DesignId::new, 8, "d"),
                revision: Revision::Commit(RevisionId::from_validated("c3".repeat(20)).unwrap()),
                tests: TestOutcome::Passed,
                change: CaChange::Fingerprinted(fingerprint),
            };
            Event::new(
                attestation.subject(),
                BTreeSet::new(),
                EventBody::Approved {
                    by: alice(),
                    attestation,
                },
            )
            .unwrap()
        };
        let event = approved(future);
        assert_ne!(
            event.id(),
            approved(known).id(),
            "the scheme is in the identifier"
        );

        let file: serde_json::Value =
            serde_json::from_str(&encode(&event, &at()).unwrap()).unwrap();
        assert_eq!(file["attestation"]["change"]["scheme"], "hunks-v9");
        let back = decode(file.to_string().as_bytes()).expect("an unknown scheme is not refused");
        assert_eq!(back.event(), &event);
        assert_eq!(back.event().id(), event.id());
        let EventBody::Approved { attestation, .. } = back.event().body() else {
            panic!("an approval");
        };
        let fingerprint = attestation.fingerprint().expect("a fingerprint");
        assert_eq!(fingerprint.scheme().name(), "hunks-v9");
        assert!(!fingerprint.scheme().is_known());

        // Its landing cannot be told, whatever the port says of the hunks.
        let verdict = Integration::decide([ApprovedChange::checked(
            Revision::Commit(RevisionId::from_validated("c3".repeat(20)).unwrap()),
            CaChange::Fingerprinted(fingerprint.clone()),
            BaselineFacts::new(Knowable::Known(false), Knowable::Known(true)),
            UnknownWhy::Shallow,
        )]);
        assert_eq!(
            verdict,
            Integration::Unknown {
                approved: Revision::Commit(RevisionId::from_validated("c3".repeat(20)).unwrap()),
                why: UnknownWhy::FingerprintVersion
            }
        );
    }

    /// Golden: the store names a stored `Published` event carries. They are
    /// written out here, not derived from `StoreKind::as_str`, so renaming the
    /// display cannot break a stored event.
    #[test]
    fn a_store_is_written_with_the_wire_formats_own_names() {
        let all = every_event();
        let published = |pick: fn(&Publication) -> bool| -> serde_json::Value {
            let event = all
                .iter()
                .find(|e| matches!(e.body(), EventBody::Published(p) if pick(p)))
                .unwrap();
            serde_json::from_str(&encode(event, &at()).unwrap()).unwrap()
        };
        let outgoing = published(|p| matches!(p, Publication::Outgoing { .. }));
        assert_eq!(outgoing["to"], "repo");
        let incoming = published(|p| matches!(p, Publication::Incoming { .. }));
        assert_eq!(incoming["from"], "private");
        for (store, name) in [(StoreKind::Repo, "repo"), (StoreKind::Private, "private")] {
            assert_eq!(store_name(store), name);
            assert_eq!(store_from("to", name).unwrap(), store);
        }
        assert!(store_from("to", "public").is_err());
    }

    #[test]
    fn a_file_of_another_version_is_refused_not_guessed() {
        let text = encode(&every_event()[0], &at())
            .unwrap()
            .replace("\"v\": 1", "\"v\": 2");
        assert!(matches!(
            decode(text.as_bytes()),
            Err(WireError::UnsupportedVersion { found: Some(2) })
        ));
        assert!(matches!(
            decode(b"{\"kind\": \"Revised\"}"),
            Err(WireError::UnsupportedVersion { found: None })
        ));
        assert!(matches!(decode(b"not json"), Err(WireError::Json(_))));
    }

    #[test]
    fn an_edited_file_is_refused_because_its_identifier_no_longer_matches() {
        let event = &every_event()[5];
        let text = encode(event, &at()).unwrap().replace("b.md", "c.md");
        assert!(matches!(
            decode(text.as_bytes()),
            Err(WireError::IdMismatch { .. })
        ));
    }

    #[test]
    fn the_time_is_not_part_of_the_identifier() {
        let event = &every_event()[3];
        let text = encode(event, &at())
            .unwrap()
            .replace("2026-11-04T21:40:00+09:00", "2027-01-01T00:00:00Z");
        let back = decode(text.as_bytes()).unwrap();
        assert_eq!(back.event().id(), event.id());
        assert_eq!(back.shown_at().as_str(), "2027-01-01T00:00:00Z");
    }

    #[test]
    fn the_person_of_a_judgement_is_part_of_the_identifier() {
        let approved = every_event()
            .into_iter()
            .find(|e| matches!(e.body(), EventBody::Approved { .. }))
            .unwrap();
        let text = encode(&approved, &at()).unwrap().replace("alice", "bob");
        assert!(matches!(
            decode(text.as_bytes()),
            Err(WireError::IdMismatch { .. })
        ));
    }

    #[test]
    fn a_bad_value_names_its_field() {
        let text = encode(&every_event()[5], &at())
            .unwrap()
            .replace("\"old\": \"a.md\"", "\"old\": \"/abs.md\"");
        let Err(WireError::Field { field, .. }) = decode(text.as_bytes()) else {
            panic!("accepted");
        };
        assert_eq!(field, "old");
    }

    /// The pin of the entity a gate is run on is plain. A `kid` there is refused where
    /// it is, not dropped: the file would otherwise read as other than it says.
    #[test]
    fn a_kid_on_a_target_pin_is_refused_not_dropped() {
        // The target of each gate, and the key of its pin in the file.
        type IsGate = fn(&Attestation) -> bool;
        let targets: [(IsGate, &str); 4] = [
            (|a| matches!(a, Attestation::Cr { .. }), "finding"),
            (|a| matches!(a, Attestation::Cs { .. }), "spec"),
            (|a| matches!(a, Attestation::Ci { .. }), "design"),
            (|a| matches!(a, Attestation::Ca { .. }), "item"),
        ];
        for (is_gate, name) in targets {
            let event = every_event()
                .into_iter()
                .find(|e| {
                    matches!(
                        e.body(),
                        EventBody::Approved { attestation, .. } if is_gate(attestation)
                    )
                })
                .unwrap();
            let mut file: serde_json::Value =
                serde_json::from_str(&encode(&event, &at()).unwrap()).unwrap();
            file["attestation"][name]["kid"] = serde_json::json!("k1");
            let Err(WireError::KidOnTargetPin { field }) = decode(file.to_string().as_bytes())
            else {
                panic!("a kid on the {name} pin was accepted");
            };
            assert_eq!(field, name);
        }
    }
}
