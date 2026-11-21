//! The answer of `status` ([`StatusOut`]): the documents that pass the filters, the
//! filters as given, and what is wrong around the documents of the whole project.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::StatusReport;
use whyshelf_core::domain::shelf::filter::FieldWant;

use super::{HealthOut, Namer, RowOut, readers, standing};

/// The answer of `status`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    /// The documents that pass the filters; every document when none is given.
    pub docs: Vec<RowOut>,
    /// The filters as given: what the documents were chosen by.
    pub filters: FiltersOut,
    /// What is wrong around the documents of the whole project, not only those listed.
    pub health: HealthOut,
}

/// The filters of `status` as given, each kind empty when none was.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FiltersOut {
    /// The books asked, in the order of their names and each once; a document of
    /// any of them passes.
    pub books: Vec<String>,
    /// The standings asked, by word and in a fixed order; a document at any of them
    /// passes.
    pub standings: Vec<String>,
    /// How many names read an approval, asked by the word of `readers.count`, in a
    /// fixed order; an approved document read by any of them passes.
    pub readers: Vec<String>,
    /// The field conditions on a value, in the order given; a document holds every
    /// one.
    #[serde(rename = "where")]
    pub where_: Vec<WhereOut>,
    /// The fields asked to hold no value (`--without`), in the order given; a
    /// document holds none of them.
    pub without: Vec<String>,
    /// The fields asked to hold a value, whichever (`--with`), in the order given; a
    /// document holds every one.
    pub with: Vec<String>,
}

/// One `--where <field>=<value>` condition as given.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WhereOut {
    /// The field.
    pub field: String,
    /// The value, as typed.
    pub value: String,
}

/// The answer of `status`.
pub fn status(report: &StatusReport) -> StatusOut {
    let namer = Namer(&report.names);
    StatusOut {
        docs: report.rows.iter().map(|r| namer.row(r)).collect(),
        filters: FiltersOut {
            books: report
                .filters
                .books
                .iter()
                .map(ToString::to_string)
                .collect(),
            standings: report
                .filters
                .standings
                .iter()
                .map(|kind| standing(*kind).to_owned())
                .collect(),
            readers: report
                .filters
                .readers
                .iter()
                .map(|count| readers(*count).to_owned())
                .collect(),
            where_: report
                .filters
                .fields
                .iter()
                .filter_map(|ask| match &ask.value {
                    FieldWant::Value(value) => Some(WhereOut {
                        field: ask.field.to_string(),
                        value: value.clone(),
                    }),
                    FieldWant::NoValue | FieldWant::AnyValue => None,
                })
                .collect(),
            without: report
                .filters
                .fields
                .iter()
                .filter(|ask| ask.value == FieldWant::NoValue)
                .map(|ask| ask.field.to_string())
                .collect(),
            with: report
                .filters
                .fields
                .iter()
                .filter(|ask| ask.value == FieldWant::AnyValue)
                .map(|ask| ask.field.to_string())
                .collect(),
        },
        health: namer.health(&report.health),
    }
}
