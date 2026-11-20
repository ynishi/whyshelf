//! The answer of `stats`: the measures of the core's stats as their answer types, and
//! the drawing made from that answer alone. The types (`StatsOut` and its parts) are
//! the parent's, as `summary.rs` draws from them and this file calls it.

use whyshelf_core::app::shelf::StatsReport;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::doc::DocKey;
use whyshelf_core::domain::shelf::schema::BookName;
use whyshelf_core::domain::shelf::stats::Origin as ChangedPin;

use super::{
    ApprovalsOut, ArtifactNamed, BookApprovalsOut, BookPendingOut, ChangedOut, DocApprovalsOut,
    DocNamed, DocRef, Namer, OriginCountOut, RunOut, RunsOut, StatsOut,
};

/// The answer of `stats`, with `--since` as it was given.
pub fn stats(report: &StatsReport, since: Option<&str>) -> StatsOut {
    let namer = Namer(&report.names);
    let stats = &report.stats;
    let doc = |id: DocId, key: Option<&DocKey>| DocRef {
        id: id.to_string(),
        key: key.map(ToString::to_string),
    };
    let book = |b: Option<&BookName>| b.map(ToString::to_string);
    let mut out = StatsOut {
        since: since.map(str::to_owned),
        approvals: ApprovalsOut {
            docs: stats
                .approvals
                .docs
                .iter()
                .map(|d| DocApprovalsOut {
                    doc: doc(d.doc, d.key.as_ref()),
                    approvals: d.approvals,
                    again: d.again,
                    revoked: d.revoked,
                })
                .collect(),
            books: stats
                .approvals
                .books
                .iter()
                .map(|b| BookApprovalsOut {
                    book: book(b.book.as_ref()),
                    docs: b.docs,
                    approvals: b.approvals,
                    again: b.again,
                    revoked: b.revoked,
                })
                .collect(),
        },
        pending: stats
            .pending
            .iter()
            .map(|p| BookPendingOut {
                book: book(p.book.as_ref()),
                count: p.millis.count,
                median_ms: p.millis.median,
                p90_ms: p.millis.p90,
                max_ms: p.millis.max,
            })
            .collect(),
        runs: RunsOut {
            count: stats.runs.sizes.count,
            median: stats.runs.sizes.median,
            p90: stats.runs.sizes.p90,
            max: stats.runs.sizes.max,
            largest: stats
                .runs
                .largest
                .iter()
                .map(|r| RunOut {
                    at: r.at.millis(),
                    actor: r.actor.to_string(),
                    size: r.size,
                })
                .collect(),
        },
        origins: stats
            .origins
            .iter()
            .map(|o| OriginCountOut {
                origin: match &o.origin {
                    ChangedPin::Doc { doc: id, key } => ChangedOut::Doc(DocNamed {
                        doc: doc(*id, key.as_ref()),
                    }),
                    ChangedPin::Artifact(artifact) => {
                        ChangedOut::Artifact(ArtifactNamed::of(artifact))
                    }
                },
                count: o.count,
            })
            .collect(),
        health: namer.health(&report.health),
        drawing: String::new(),
    };
    out.drawing = super::summary::draw(&out);
    out
}
