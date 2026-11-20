//! The answer of `get` ([`GetOut`]): a document as every answer shows it, its
//! content (its fields as JSON values, its links and its body), where the failure of
//! its approval starts, and what is wrong around the documents.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use whyshelf_core::app::shelf::GetReport;
use whyshelf_core::domain::shelf::doc::{Doc, FieldValue};

use super::doc::artifact_target;
use super::{HealthOut, LinkOut, Namer, RootOut, RowOut};

/// A document's content.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocOut {
    /// The fields, by name.
    pub fields: serde_json::Map<String, Value>,
    /// The links.
    pub links: Vec<LinkOut>,
    /// The body.
    pub body: String,
}

/// The answer of `get`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GetOut {
    /// The document.
    pub row: RowOut,
    /// Its content, when it is valid.
    pub content: Option<DocOut>,
    /// Where the failure of its approval starts.
    pub roots: Vec<RootOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

impl Namer<'_> {
    fn content(&self, doc: &Doc) -> DocOut {
        DocOut {
            fields: doc
                .fields()
                .iter()
                .map(|(name, value)| (name.to_string(), field(value)))
                .collect(),
            links: doc
                .links()
                .iter()
                .map(|(name, link)| LinkOut {
                    name: name.to_string(),
                    targets: link.states().iter().map(|s| self.target(s)).collect(),
                })
                .chain(doc.artifact_links().iter().map(|(name, link)| LinkOut {
                    name: name.to_string(),
                    targets: link.artifacts().iter().map(artifact_target).collect(),
                }))
                .collect(),
            body: doc.body().to_owned(),
        }
    }
}

/// The answer of `get`.
pub fn get(report: &GetReport) -> GetOut {
    let namer = Namer(&report.names);
    GetOut {
        row: namer.row(&report.row),
        content: report.doc.as_ref().map(|d| namer.content(d)),
        roots: report.roots.iter().map(|r| namer.root(r)).collect(),
        health: namer.health(&report.health),
    }
}

fn field(value: &FieldValue) -> Value {
    match value {
        FieldValue::Text(s) => Value::String(s.as_str().to_owned()),
        FieldValue::Line(s) => Value::String(s.as_str().to_owned()),
        FieldValue::Integer(n) => Value::from(*n),
        FieldValue::Bool(b) => Value::Bool(*b),
        FieldValue::TextList(items) => Value::Array(
            items
                .iter()
                .map(|item| Value::String(item.as_str().to_owned()))
                .collect(),
        ),
        FieldValue::Enum(word) => Value::String(word.as_str().to_owned()),
    }
}
