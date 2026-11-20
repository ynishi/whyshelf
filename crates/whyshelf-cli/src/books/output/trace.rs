//! The answer of `trace` ([`TraceOut`]): a document, the links it follows and those
//! that reach it, transitively, each with where its other end stands.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{DownstreamEdge, TraceReport, UpstreamEdge, UpstreamEnd};
use whyshelf_core::domain::shelf::doc::LinkState;

use super::doc::artifact_target;
use super::{DocRef, HealthOut, LinkTargetOut, Namer, RowOut, standing};

/// One link followed in `trace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeOut {
    /// The document that holds the link.
    pub from: DocRef,
    /// The link.
    pub link: String,
    /// `depends`, `informs` or `replaces`.
    pub means: &'static str,
    /// How many links away from the document asked about.
    pub depth: usize,
    /// What it names.
    pub to: LinkTargetOut,
    /// Where the other end stands (the end away from the document asked about).
    pub standing: Option<&'static str>,
}

/// The answer of `trace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TraceOut {
    /// The document asked about.
    pub root: RowOut,
    /// What it links to, transitively.
    pub upstream: Vec<EdgeOut>,
    /// What links to it, transitively.
    pub downstream: Vec<EdgeOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

impl Namer<'_> {
    fn upstream(&self, edge: &UpstreamEdge) -> EdgeOut {
        let (to, standing) = match &edge.to {
            UpstreamEnd::Doc(row) => (
                LinkTargetOut::Resolved {
                    doc: self.doc(row.id),
                },
                Some(self::standing(row.standing.kind())),
            ),
            UpstreamEnd::Unresolved(u) => (self.target(&LinkState::Unresolved(u.clone())), None),
            UpstreamEnd::Artifact(a) => (artifact_target(a), None),
        };
        EdgeOut {
            from: self.doc(edge.from),
            link: edge.link.to_string(),
            means: edge.propagation.word(),
            depth: edge.depth,
            to,
            standing,
        }
    }

    fn downstream(&self, edge: &DownstreamEdge) -> EdgeOut {
        EdgeOut {
            from: self.doc(edge.holder.id),
            link: edge.link.to_string(),
            means: edge.propagation.word(),
            depth: edge.depth,
            to: LinkTargetOut::Resolved {
                doc: self.doc(edge.to),
            },
            standing: Some(standing(edge.holder.standing.kind())),
        }
    }
}

/// The answer of `trace`.
pub fn trace(report: &TraceReport) -> TraceOut {
    let namer = Namer(&report.names);
    TraceOut {
        root: namer.row(&report.root),
        upstream: report.upstream.iter().map(|e| namer.upstream(e)).collect(),
        downstream: report
            .downstream
            .iter()
            .map(|e| namer.downstream(e))
            .collect(),
        health: namer.health(&report.health),
    }
}
