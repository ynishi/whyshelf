//! The use case that finds the documents that are not linked: `unlinked`.
//!
//! It only reads. It asks [`linkage`] which list each valid document is in, if any
//! (the rule is on [`Linkage`]), and [`Standing::stands`] whether a decision went
//! against it, to list it apart; it shows each document it lists as a row, with its
//! book. It is maintenance, not a gate: a document it lists is no fault of the
//! shelf.
//!
//! ```text
//!   schema_of --unsound--> SchemaInvalid (as check answers it)
//!       | sound
//!   load_with --> Shelf --assess--> Assessment
//!                   |                    |
//!                   +------linkage-------+--> Linkage per valid document
//!                                         | filter: the book, the questions asked
//!                                         | Standing::stands: which half
//!                                         v
//!                   standing { isolated, no_incoming, no_outgoing }
//!                   decided_against { isolated, no_incoming, no_outgoing }
//! ```
//!
//! [`linkage`]: crate::domain::shelf::trace::linkage
//! [`Linkage`]: crate::domain::shelf::trace::Linkage
//! [`Standing::stands`]: crate::domain::shelf::freshness::Standing::stands

use crate::domain::shelf::schema::BookName;
use crate::domain::shelf::trace::{Linkage, linkage};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{View, load_with, schema_of};
use super::ports::ShelfRead;
use super::report::{UnlinkedEntry, UnlinkedLists, UnlinkedReport};

/// Which of the two one-way questions of `unlinked` to answer; the documents linked
/// neither way are answered whichever is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnlinkedAsk {
    /// Both.
    #[default]
    Both,
    /// Only the documents no link reaches.
    NoIncoming,
    /// Only the documents that hold no link.
    NoOutgoing,
}

impl UnlinkedAsk {
    /// Whether the documents no link reaches are asked for.
    fn incoming(self) -> bool {
        matches!(self, Self::Both | Self::NoIncoming)
    }

    /// Whether the documents that hold no link are asked for.
    fn outgoing(self) -> bool {
        matches!(self, Self::Both | Self::NoOutgoing)
    }
}

/// What `unlinked` is asked: in one book or in all, and which questions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnlinkedFilter {
    /// The book the answer is limited to; none is every book.
    pub book: Option<BookName>,
    /// The questions to answer.
    pub ask: UnlinkedAsk,
}

impl UnlinkedLists {
    /// Empty lists for the questions asked, and none for the others.
    fn asked(ask: UnlinkedAsk) -> Self {
        Self {
            isolated: Vec::new(),
            no_incoming: ask.incoming().then(Vec::new),
            no_outgoing: ask.outgoing().then(Vec::new),
        }
    }

    /// Put a document in the list it fits, if that list was asked for (the
    /// answers of [`Linkage`] are disjoint); its entry is made only then.
    fn add(&mut self, linkage: Linkage, entry: impl FnOnce() -> UnlinkedEntry) {
        let list = if linkage.isolated() {
            Some(&mut self.isolated)
        } else if linkage.unreached() {
            self.no_incoming.as_mut()
        } else if linkage.holds_none() {
            self.no_outgoing.as_mut()
        } else {
            None
        };
        if let Some(list) = list {
            list.push(entry());
        }
    }
}

impl<P: ShelfRead> ShelfApp<P> {
    /// The valid documents linked neither way, those that no link of another
    /// document reaches, and those that hold no link, those decided against apart
    /// (the module page says what is asked of whom).
    ///
    /// A declaration that is not sound is an answer, as for
    /// [`books_check`](Self::books_check), with the lists asked for empty; a book
    /// that is not declared is [`ShelfError::UnknownBook`], with no reference.
    pub async fn unlinked(&self, filter: &UnlinkedFilter) -> Result<UnlinkedReport, ShelfError> {
        let schema = match schema_of(&self.ports).await? {
            Ok(schema) => schema,
            Err(problems) => {
                return Ok(UnlinkedReport::SchemaInvalid {
                    problems,
                    lists: UnlinkedLists::asked(filter.ask),
                });
            }
        };
        if let Some(book) = &filter.book
            && schema.book(book).is_none()
        {
            return Err(ShelfError::UnknownBook {
                reference: None,
                book: book.clone(),
            });
        }
        let loaded = load_with(&self.ports, &schema).await?;
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let all = linkage(&loaded.shelf, &assessment);
        let mut standing = UnlinkedLists::asked(filter.ask);
        let mut decided_against = UnlinkedLists::asked(filter.ask);
        let books = schema
            .books()
            .keys()
            .filter(|name| filter.book.as_ref().is_none_or(|asked| asked == *name));
        for book in books {
            for (id, linked) in &all {
                let in_book = loaded
                    .shelf
                    .doc(*id)
                    .is_some_and(|doc| doc.key().book == *book);
                if !in_book {
                    continue;
                }
                let lists = if assessment.standing_of(*id).stands() {
                    &mut standing
                } else {
                    &mut decided_against
                };
                lists.add(*linked, || UnlinkedEntry {
                    book: book.clone(),
                    row: view.row(*id),
                });
            }
        }
        Ok(UnlinkedReport::Answered {
            standing,
            decided_against,
            names: loaded.names(),
            health: view.health(),
        })
    }
}
