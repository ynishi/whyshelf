//! The use case that tells what the covering links to artifacts expect and how each
//! stands: `cover`.
//!
//! ```text
//!   schema_of ----------------------------------------> Schema, or its problems (an answer)
//!   load_with, listings_of (ArtifactSource::list) ------> the shelf, the ledger, and the names
//!                                                         each selection takes now
//!   Loaded::assess(&Listings::Asked(..)) --------------> Assessment::cover(): per link, the names
//!                                                         selected and which are reached,
//!                                                         exempt or a gap, and how complete the
//!                                                         listing is
//!   artifact_lists --------------------------------------> the gaps and the exemptions, read
//! ```
//!
//! It decides nothing: what is a gap, how complete a listing is and what an exemption
//! reads as are the assessment of coverage's ([`coverage::assess`]), told here per
//! link. It reads and writes nothing else, and `check` tells the same from the same
//! assessment ([`artifact_lists`]); the difference is that this one asks about one
//! link when it is asked to, and fails nothing: a gap, a listing that is not the whole
//! and a selection that takes nothing are its answer, and the gate on them is
//! `check`'s ([`BooksCheck::is_clean`](super::BooksCheck::is_clean)).
//!
//! [`coverage::assess`]: crate::domain::shelf::coverage::assess

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::ArtifactKey;
use crate::domain::shelf::coverage::{CoverAssessment, NameCover};
use crate::domain::shelf::exemption::{ExemptionReading, artifact_cover_of, artifact_space_of};
use crate::domain::shelf::ledger::Ledger;
use crate::domain::shelf::schema::{BookName, EntryName};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{View, listings_of, load_with, schema_of};
use super::ports::ShelfRead;
use super::report::{
    ArtifactCoverLists, CoverReport, ExemptArtifact, RepeatedArtifact, UncoveredArtifact,
    UnneededArtifact,
};

/// A covering link to artifacts by the book that declares it and its name: what
/// `cover` is asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverLinkAsk {
    /// The book that declares the link.
    pub book: BookName,
    /// The link.
    pub link: EntryName,
}

impl CoverLinkAsk {
    fn is(&self, book: &BookName, link: &EntryName) -> bool {
        self.book == *book && self.link == *link
    }
}

/// The gaps and the exemptions of the covering links to artifacts, as the assessment of
/// coverage read them, of the link `only` or of every one: the one place they are
/// made, for `check` and for `cover`. An exemption that cannot be told is in none of
/// them.
pub(super) fn artifact_lists(
    ledger: &Ledger,
    cover: &CoverAssessment,
    only: Option<&CoverLinkAsk>,
) -> ArtifactCoverLists {
    let asked = |book: &BookName, link: &EntryName| only.is_none_or(|ask| ask.is(book, link));
    let mut lists = ArtifactCoverLists::default();
    for link in cover.links().iter().filter(|l| asked(l.book(), l.link())) {
        for (name, state) in link.names() {
            if *state == NameCover::Gap {
                lists.uncovered.push(UncoveredArtifact {
                    book: link.book().clone(),
                    link: link.link().clone(),
                    artifact: ArtifactKey::new(link.space().clone(), name.clone()),
                    severity: link.severity(),
                });
            }
        }
    }
    for group in cover.exemptions() {
        let of = group.of();
        let Some(artifact) = of.artifact() else {
            continue;
        };
        if !asked(of.book(), of.link()) {
            continue;
        }
        for read in group.reads() {
            let Some(record) = ledger.exemption(read.id()) else {
                continue;
            };
            match read.reading() {
                ExemptionReading::Needed(severity) => lists.exempt.push(ExemptArtifact {
                    artifact: artifact.clone(),
                    severity: *severity,
                    record: record.clone(),
                }),
                ExemptionReading::Unneeded(why) => lists.unneeded.push(UnneededArtifact {
                    artifact: artifact.clone(),
                    record: record.clone(),
                    why: why.clone(),
                }),
                ExemptionReading::CannotTell(_) => {}
            }
        }
        if let Some(repeated) = group.repeated() {
            lists.repeated.push(RepeatedArtifact {
                artifact: artifact.clone(),
                book: of.book().clone(),
                link: of.link().clone(),
                exemptions: repeated.clone(),
            });
        }
    }
    lists
}

impl<P: ShelfRead> ShelfApp<P> {
    /// What the covering links to artifacts expect, and how each stands: for each, the
    /// selection it declares, how complete the listing of the names it takes is, and
    /// each name that is reached, exempt or a gap; with the exemptions of those names.
    ///
    /// With `link`, only that link: a link that is declared with no cover over
    /// artifacts (not declared, a link into a book, or `cover` not written) is
    /// [`ShelfError::NotACoverLink`]. A declaration that is not sound is an answer, as
    /// for [`books_check`](Self::books_check). The names are asked of the adapters
    /// once for each (space, selection): two links that declare the same selection
    /// share one listing and show the same completeness. It exits as every reading
    /// does: what it tells is not a failure.
    pub async fn cover(&self, link: Option<CoverLinkAsk>) -> Result<CoverReport, ShelfError> {
        let schema = match schema_of(&self.ports).await? {
            Ok(schema) => schema,
            Err(problems) => return Ok(CoverReport::SchemaInvalid(problems)),
        };
        // The selections to list: the asked link's alone, or every one.
        let asks = match &link {
            Some(ask) => {
                let not_a_cover = |miss| ShelfError::NotACoverLink {
                    book: ask.book.clone(),
                    link: ask.link.clone(),
                    miss,
                };
                let space =
                    artifact_space_of(&schema, &ask.book, &ask.link).map_err(not_a_cover)?;
                let cover =
                    artifact_cover_of(&schema, &ask.book, &ask.link, space).map_err(not_a_cover)?;
                BTreeSet::from([(space.clone(), cover.selection().clone())])
            }
            None => schema.cover_selections(),
        };
        let loaded = load_with(&self.ports, &schema).await?;
        let listings = listings_of(self.ports.artifacts(), asks).await?;
        let assessment = loaded.assess(&listings);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let cover = assessment.cover();
        Ok(CoverReport::Covered {
            links: cover
                .links()
                .iter()
                .filter(|l| link.as_ref().is_none_or(|ask| ask.is(l.book(), l.link())))
                .cloned()
                .collect(),
            lists: artifact_lists(&loaded.ledger, cover, link.as_ref()),
            names: loaded.names(),
            health: view.health(),
        })
    }
}
