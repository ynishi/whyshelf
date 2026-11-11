//! What a checked declaration says, gathered for one picture: every book and every
//! link, in the order the declaration writes them.
//!
//! [`ShelfMap::of`] reads a [`Schema`] and nothing else: no document, no ledger. It
//! adds nothing the declaration does not say (the model knows no kind of document,
//! so no meaning of a book is guessed here); a book is its name, directory, key
//! grammar, description and fields, and a link its holder, name, target, meaning,
//! count, cover and, for artifacts, its comparison and the preset it was expanded
//! from. Drawing it is the adapter's.

use super::artifact::{Comparison, Space};
use super::schema::{
    BookDescription, BookDir, BookName, Cardinality, CoverSeverity, DeclaredLink, EntryName,
    FieldDecl, KeyPattern, PresetOrigin, Propagation, Schema,
};

/// Every book and every link of a declaration, in the order it writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfMap {
    /// The books, as declared, one after another.
    pub books: Vec<BookMap>,
    /// The links of every book, book by book in the order of [`ShelfMap::books`],
    /// and within a book as declared.
    pub links: Vec<LinkMap>,
}

/// One book of a [`ShelfMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookMap {
    /// The name.
    pub name: BookName,
    /// The directory.
    pub dir: BookDir,
    /// The grammar of its keys.
    pub key: KeyPattern,
    /// What it is for, when the declaration says.
    pub description: Option<BookDescription>,
    /// Its fields, as declared.
    pub fields: Vec<(EntryName, FieldDecl)>,
}

/// One link of a [`ShelfMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMap {
    /// The book that holds it.
    pub from: BookName,
    /// Its name.
    pub name: EntryName,
    /// What it points at.
    pub target: MapTarget,
    /// What it means for freshness.
    pub propagation: Propagation,
    /// How many a document writes.
    pub cardinality: Cardinality,
}

/// What a link of a [`ShelfMap`] points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapTarget {
    /// The documents of a book, which may be the book that holds the link.
    Book {
        /// The book.
        book: BookName,
        /// How strongly every pending or approved document of it is expected to be
        /// reached, or `None` ([`LinkKind::cover`](super::schema::LinkKind::cover)).
        cover: Option<CoverSeverity>,
    },
    /// Artifacts, by name in a space.
    Artifact {
        /// The space.
        space: Space,
        /// What of each artifact is compared.
        comparison: Comparison,
        /// The preset the link was expanded from, or none
        /// ([`BookSchema::preset_of`](super::schema::BookSchema::preset_of)). The
        /// values above are the link's, whichever it is.
        preset: Option<PresetOrigin>,
    },
}

impl ShelfMap {
    /// The map of a checked declaration.
    pub fn of(schema: &Schema) -> Self {
        let books = schema
            .books()
            .values()
            .map(|book| BookMap {
                name: book.name().clone(),
                dir: book.dir().clone(),
                key: book.key().clone(),
                description: book.description().cloned(),
                fields: book
                    .fields()
                    .iter()
                    .map(|(name, decl)| (name.clone(), decl.clone()))
                    .collect(),
            })
            .collect();
        let links = schema
            .books()
            .values()
            .flat_map(|book| {
                book.all_links().iter().map(|(name, link)| {
                    let (target, propagation, cardinality) = match link {
                        DeclaredLink::Book(kind) => (
                            MapTarget::Book {
                                book: kind.target.clone(),
                                cover: kind.cover,
                            },
                            kind.propagation,
                            kind.cardinality,
                        ),
                        DeclaredLink::Artifact(kind) => (
                            MapTarget::Artifact {
                                space: kind.space.clone(),
                                comparison: kind.comparison.clone(),
                                preset: book.preset_of(name).cloned(),
                            },
                            kind.propagation.propagation(),
                            kind.cardinality,
                        ),
                    };
                    LinkMap {
                        from: book.name().clone(),
                        name: name.clone(),
                        target,
                        propagation,
                        cardinality,
                    }
                })
            })
            .collect();
        Self { books, links }
    }
}

#[cfg(test)]
mod tests;
