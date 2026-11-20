//! The answers that read what an approval saw: `get --approved` ([`ApprovedOut`]), each
//! item with its content as it was approved, and `diff` ([`DiffOut`]), each item then
//! and now.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{ApprovedReport, DiffItem, DiffReport};
use whyshelf_core::domain::shelf::artifact::ArtifactState;
use whyshelf_core::domain::shelf::doc::normalize;
use whyshelf_core::domain::shelf::snapshot::{
    ItemNow, ItemRole, Retrieval, SeenHash, SnapshotItem,
};

use super::super::textdiff;
use super::words::state_word;
use super::{DocRef, HealthOut, Namer, RowOut};

/// What an item of an approval is, and which document or file it names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemOut {
    /// `subject` (the approved document), `upstream` (a document it stood on) or
    /// `artifact` (an artifact it stood on).
    pub role: &'static str,
    /// The document, for a subject or an upstream (an upstream's key is the one it
    /// had when it was pinned).
    pub doc: Option<DocRef>,
    /// The link it was pinned through, for an upstream or an artifact.
    pub link: Option<String>,
    /// The name, for an artifact (kept from when every name was a path; the same as
    /// `name`).
    pub path: Option<String>,
    /// The name, for an artifact.
    pub name: Option<String>,
    /// The space, for an artifact.
    pub space: Option<String>,
    /// For an artifact pinned under `name` and declared moved since (`judge moved`),
    /// the name it is read as now, which what it is now is asked of; none when it is
    /// read as written, and for a document.
    pub read_as: Option<String>,
    /// The hash the approval pinned; none for an artifact that was not there.
    pub approved_hash: Option<String>,
    /// Where its content, as it was approved, can be read back; none when the record
    /// keeps no address.
    pub address: Option<String>,
}

/// Whether the content of an item, as it was approved, came back.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RetrievalOut {
    /// `read`, `address_not_readable` (an address, and nothing behind it: the content
    /// was never committed, or is not in this clone) or `no_address` (the record
    /// keeps none: made outside Git, by an older version, or no file was there).
    pub state: &'static str,
    /// The content, when it was read and is UTF-8 text.
    pub text: Option<String>,
    /// How many bytes were read.
    pub bytes: Option<usize>,
}

/// One item of `get --approved`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovedItemOut {
    /// What it is.
    pub item: ItemOut,
    /// Its content as it was approved, or why not.
    pub retrieval: RetrievalOut,
}

/// The answer of `get <ref> --approved`: what the approval that stands saw.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovedOut {
    /// The document, as every answer shows it now.
    pub row: RowOut,
    /// The approval (its record's id).
    pub judgement: String,
    /// Who approved.
    pub actor: String,
    /// When (unix milliseconds).
    pub at: i64,
    /// The commit the approval was made at, when the record keeps one (a hint).
    pub revision: Option<String>,
    /// The approved document first, then each upstream, then each artifact.
    pub items: Vec<ApprovedItemOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// One item of `diff`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffItemOut {
    /// What it is, with the hash that was approved.
    pub item: ItemOut,
    /// The hash it has now; none when it is no valid document, or, for an artifact,
    /// when `now` is not `present`.
    pub current_hash: Option<String>,
    /// Whether it is not known to be what was approved: its hash differs, it is no
    /// valid document now, or it is an artifact that cannot be told now (`now` is
    /// `unknown`).
    pub changed: bool,
    /// For an artifact, what it is now: `present`, `absent`, or `unknown` (its
    /// adapter cannot tell now); none for a document.
    pub now: Option<&'static str>,
    /// What holds it now, if anything does: a document's file, or the name of an
    /// artifact that is there now (for the space `file`, its path).
    pub file: Option<String>,
    /// Whether the approved content came back: `read`, `address_not_readable`,
    /// `no_address` (the content itself is in `diff`).
    pub retrieval: &'static str,
    /// The unified diff from the approved content to the file now, when both are
    /// UTF-8 text; none otherwise. It agrees with `changed`: a document's diff is of
    /// its normalized text (what its hash is taken over) and is empty when `changed`
    /// is false; an artifact's diff is of its bytes as text.
    pub diff: Option<String>,
}

/// The answer of `diff <ref>`: each item of the approval that stands, then and
/// now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffOut {
    /// The document, as every answer shows it now.
    pub row: RowOut,
    /// The approval (its record's id).
    pub judgement: String,
    /// The commit the approval was made at, when the record keeps one (a hint).
    pub revision: Option<String>,
    /// The approved document first, then each upstream, then each artifact.
    pub items: Vec<DiffItemOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

impl Namer<'_> {
    fn item(&self, item: &SnapshotItem) -> ItemOut {
        let (role, doc, link, artifact) = match item.role() {
            ItemRole::Subject { id } => ("subject", Some(self.doc(*id)), None, None),
            ItemRole::Upstream { link, target, key } => (
                "upstream",
                Some(DocRef {
                    id: target.to_string(),
                    key: Some(key.to_string()),
                }),
                Some(link.to_string()),
                None,
            ),
            ItemRole::Artifact { link, artifact, .. } => {
                ("artifact", None, Some(link.to_string()), Some(artifact))
            }
        };
        let approved_hash = match item.seen() {
            SeenHash::Doc(hash) => Some(hash.to_string()),
            SeenHash::Artifact(state) => state.hash().map(ToString::to_string),
        };
        ItemOut {
            role,
            doc,
            link,
            path: artifact.map(|a| a.name().to_string()),
            name: artifact.map(|a| a.name().to_string()),
            space: artifact.map(|a| a.space().to_string()),
            // Only a name other than the one pinned: a way back is read as written.
            read_as: item
                .moved()
                .map(|m| m.artifact())
                .filter(|a| Some(*a) != artifact)
                .map(|a| a.name().to_string()),
            approved_hash,
            address: item.address().map(ToString::to_string),
        }
    }
}

fn retrieval_word(retrieval: &Retrieval) -> &'static str {
    match retrieval {
        Retrieval::Read(_) => "read",
        Retrieval::AddressNotReadable(_) => "address_not_readable",
        Retrieval::NoAddress => "no_address",
    }
}

pub(super) fn retrieval(retrieval: &Retrieval) -> RetrievalOut {
    let read = match retrieval {
        Retrieval::Read(bytes) => Some(bytes),
        Retrieval::AddressNotReadable(_) | Retrieval::NoAddress => None,
    };
    RetrievalOut {
        state: retrieval_word(retrieval),
        text: read.and_then(|b| std::str::from_utf8(b).ok().map(str::to_owned)),
        bytes: read.map(Vec::len),
    }
}

/// The answer of `get <ref> --approved`.
pub fn approved(report: &ApprovedReport) -> ApprovedOut {
    let namer = Namer(&report.names);
    ApprovedOut {
        row: namer.row(&report.row),
        judgement: report.snapshot.judgement().to_string(),
        actor: report.snapshot.actor().as_str().to_owned(),
        at: report.snapshot.at().millis(),
        revision: report.snapshot.revision().map(ToString::to_string),
        items: report
            .items
            .iter()
            .map(|i| ApprovedItemOut {
                item: namer.item(&i.item),
                retrieval: retrieval(&i.retrieval),
            })
            .collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `diff <ref>`.
pub fn diff(report: &DiffReport) -> DiffOut {
    let namer = Namer(&report.names);
    DiffOut {
        row: namer.row(&report.row),
        judgement: report.snapshot.judgement().to_string(),
        revision: report.snapshot.revision().map(ToString::to_string),
        items: report.items.iter().map(|i| diff_item(&namer, i)).collect(),
        health: namer.health(&report.health),
    }
}

pub(super) fn diff_item(namer: &Namer<'_>, item: &DiffItem) -> DiffItemOut {
    let out = namer.item(&item.item);
    let current_hash = match &item.now {
        ItemNow::Doc(hash) => hash.map(|h| h.to_string()),
        ItemNow::Artifact(state) => state.hash().map(ToString::to_string),
    };
    let approved_text = match &item.approved {
        Retrieval::Read(bytes) => std::str::from_utf8(bytes).ok(),
        Retrieval::AddressNotReadable(_) | Retrieval::NoAddress => None,
    };
    let current_text = item
        .current
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok());
    let label = out
        .path
        .clone()
        .or_else(|| item.file.as_ref().map(ToString::to_string))
        .or_else(|| out.doc.as_ref().and_then(|d| d.key.clone()))
        .unwrap_or_default();
    // What is now is at the name a declared move took the artifact to.
    let now_label = out.read_as.clone().unwrap_or_else(|| label.clone());
    // The diff says what `changed` says: empty when the hash is the same. A document's
    // hash is taken over its text after the reader's normalization (line breaks,
    // trailing white space), so its diff is taken over the normalized texts too. An
    // artifact's hash is its link's comparison of the file (every byte, or the lines
    // with their white space collapsed, without blank and ignored lines), so when it is the same the diff is
    // empty, and when it is not, the diff is of the texts as they are.
    let is_doc = matches!(item.now, ItemNow::Doc(_));
    let diff = match (approved_text, current_text) {
        (Some(_), Some(_)) if !item.changed => Some(String::new()),
        (Some(then), Some(now)) if is_doc => Some(textdiff::unified(
            &normalize(then),
            &normalize(now),
            &format!("approved/{label}"),
            &format!("now/{now_label}"),
        )),
        (Some(then), Some(now)) => Some(textdiff::unified(
            then,
            now,
            &format!("approved/{label}"),
            &format!("now/{now_label}"),
        )),
        _ => None,
    };
    // An artifact that is there now is held by its name, as `file` has always said:
    // the name it is read as, after a declared move.
    let file = match (&item.now, item.item.role()) {
        (ItemNow::Artifact(ArtifactState::Present(_)), ItemRole::Artifact { artifact, .. }) => {
            Some(
                item.item
                    .moved()
                    .map_or(artifact, |m| m.artifact())
                    .name()
                    .to_string(),
            )
        }
        _ => item.file.as_ref().map(ToString::to_string),
    };
    let now = match &item.now {
        ItemNow::Doc(_) => None,
        ItemNow::Artifact(state) => Some(state_word(state)),
    };
    DiffItemOut {
        current_hash,
        changed: item.changed,
        now,
        file,
        retrieval: retrieval_word(&item.approved),
        diff,
        item: out,
    }
}
