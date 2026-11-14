//! Which documents `status` shows: a filter by book, by standing and by field, and
//! the one rule of whether a document passes it.
//!
//! The people who read `status` want a page of it, not all of it. What they ask is a
//! [`StatusFilterAsk`]; [`StatusFilter::new`] checks it against the declaration and
//! answers a [`StatusFilter`] or every way it does not fit ([`FilterProblem`]);
//! [`StatusFilter::admits`] then says, per document, whether it passes. A caller
//! reads arguments into an ask and drops the rows that are not admitted; it decides
//! neither.
//!
//! ```text
//!   StatusFilterAsk ──► StatusFilter::new(&Schema) ──► StatusFilter
//!    (books, standings,        │                          │
//!     field=value, as given)   └─► every FilterProblem    └─► admits(&Shelf, id, standing)
//!                                                              ──► whether a document passes
//!
//!   admits:  Shelf::state_of ──► DocState (valid, invalid, gone, unknown)
//!            Schema::membership ──► the book of a file, through one private function
//!            FieldValue::read ──► the value of a condition, read once per declaring book
//! ```
//!
//! # The rule
//!
//! - **Kinds of condition combine by AND, a kind repeated by OR.** A document passes
//!   when it is in one of the books asked (if any), stands as one of the standings
//!   asked (if any), and holds every field condition. A kind with nothing asked is
//!   no condition, so an empty ask admits every document. `--book a --book b
//!   --standing stale` is the stale documents of the books `a` and `b`; each
//!   `--where` is a condition of its own.
//! - **The book of a file is the one `Schema::membership` puts it in, valid or
//!   not** (the rule [`Shelf::tally_by_book`] counts by). A file under no book's
//!   directory has no book, so it passes no book condition and no field condition.
//!   A valid document is in the book of its file; an invalid identity passes the
//!   book condition when any of its files is in an asked book (its files can lie in
//!   different books); an identity no file carries ([`DocState::Gone`]) or that may
//!   be in a file that could not be read ([`DocState::Unknown`]) has no book to ask.
//! - **The standing** is the [`StandingKind`] the caller gives, from
//!   [`Standing::kind`](super::freshness::Standing::kind); any document has one,
//!   whatever its state.
//! - **A field condition** `<field>=<value>` holds only for a valid document whose
//!   book declares the field and writes a value that equals the condition. An
//!   invalid document, a document of a book that does not declare the field, and a
//!   document that does not write it do not pass. The value is compared by the type
//!   the book declares: read as that type by [`FieldValue::read`], and equal to the
//!   document's value; for a list field, the list holds the text as one item (it is
//!   not split at commas).
//! - **What fits the declaration** ([`StatusFilter::new`]): every book asked is
//!   declared, every field is declared by some book, and the value can be read as the
//!   type of at least one book that declares the field (any text is an item of a list
//!   field). A book that declares the field with a type the value cannot be read as
//!   is not an error: its documents do not pass that condition.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;

use super::build::{DocState, Shelf};
use super::doc::{Doc, FieldValue};
use super::freshness::StandingKind;
use super::schema::{BookName, EntryName, FieldType, Membership, Schema};

/// What a person asked `status` to show, as given and before it is checked against
/// the declaration. A caller that reads arguments makes this and nothing more, so the
/// rule of what it means is one, in [`StatusFilter`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusFilterAsk {
    /// The books the documents are of; a document of any of them passes. Empty: every
    /// book.
    pub books: BTreeSet<BookName>,
    /// The standings the documents are at; a document at any of them passes. Empty:
    /// every standing.
    pub standings: BTreeSet<StandingKind>,
    /// The field conditions, each of which a document must hold, in the order given.
    /// Empty: no field condition.
    pub fields: Vec<FieldAsk>,
}

/// One `<field>=<value>` condition, as given: the value stays a text until the
/// declaration says what type it is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAsk {
    /// The field.
    pub field: EntryName,
    /// The value, as typed.
    pub value: String,
}

/// A [`StatusFilterAsk`] checked against the declaration, so that
/// [`admits`](Self::admits) has no case of a book, a field or a value that makes no
/// sense: the rule of the page is in this type, and there is one way to make it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusFilter {
    ask: StatusFilterAsk,
    /// One per field condition of the ask, in its order.
    conditions: Vec<Condition>,
}

/// A field condition, with the value it wants read once for each book that declares
/// the field.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Condition {
    field: EntryName,
    /// Per book that declares the field and can read the value; a book that is not
    /// here passes nothing.
    wanted: BTreeMap<BookName, Wanted>,
}

/// What a book that declares a field wants of its value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wanted {
    /// The field is a list: it holds this text as one item.
    Item(String),
    /// The field is of another type: it is equal to this value.
    Equal(FieldValue),
}

impl Wanted {
    /// Whether a value a document wrote is what is wanted.
    fn is_met_by(&self, value: &FieldValue) -> bool {
        match (self, value) {
            (Self::Item(item), FieldValue::TextList(items)) => items.contains(item),
            (Self::Equal(want), value) => want == value,
            (Self::Item(_), _) => false,
        }
    }
}

/// Why a filter does not fit the declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FilterProblem {
    /// A book that is not declared is asked for. The words are those of
    /// `ShelfError::UnknownBook` without a reference, so one mistake reads one way
    /// whichever command makes it.
    #[error("there is no book `{0}`")]
    UnknownBook(BookName),
    /// No book declares the field.
    #[error("no book declares a field `{0}`")]
    UnknownField(EntryName),
    /// The value is of no type any book that declares the field gives it, so nothing
    /// could pass.
    #[error("`{value}` is no value of the field `{field}` in any book that declares it")]
    ValueNotOfField {
        /// The field.
        field: EntryName,
        /// The value, as typed.
        value: String,
    },
}

impl StatusFilter {
    /// The filter, or every way it does not fit the declaration (the page says what
    /// fits). All the problems are collected, so one run tells a person all they
    /// have to mend.
    pub fn new(schema: &Schema, ask: StatusFilterAsk) -> Result<Self, NonEmptyVec<FilterProblem>> {
        let mut problems: Vec<FilterProblem> = ask
            .books
            .iter()
            .filter(|book| schema.book(*book).is_none())
            .cloned()
            .map(FilterProblem::UnknownBook)
            .collect();
        let mut conditions = Vec::with_capacity(ask.fields.len());
        for want in &ask.fields {
            let mut declared = false;
            let mut wanted = BTreeMap::new();
            for book in schema.books().values() {
                let Some(decl) = book.fields().get(&want.field) else {
                    continue;
                };
                declared = true;
                let read = match &decl.ty {
                    FieldType::TextList => Some(Wanted::Item(want.value.clone())),
                    ty => FieldValue::read(ty, &want.value).map(Wanted::Equal),
                };
                if let Some(read) = read {
                    wanted.insert(book.name().clone(), read);
                }
            }
            if !declared {
                problems.push(FilterProblem::UnknownField(want.field.clone()));
            } else if wanted.is_empty() {
                problems.push(FilterProblem::ValueNotOfField {
                    field: want.field.clone(),
                    value: want.value.clone(),
                });
            }
            conditions.push(Condition {
                field: want.field.clone(),
                wanted,
            });
        }
        match NonEmptyVec::from_vec(problems) {
            Ok(problems) => Err(problems),
            Err(_) => Ok(Self { ask, conditions }),
        }
    }

    /// The filter as given: what an answer shows beside the rows it kept.
    pub fn ask(&self) -> &StatusFilterAsk {
        &self.ask
    }

    /// Whether the document of this identity passes every condition, read from the
    /// shelf the filter was checked against (the page says the rule).
    ///
    /// The shelf is the one place that knows whether the identity is valid, invalid,
    /// gone or unknown, and which files it has, so the caller does not pass that in;
    /// it passes the standing the model gave the document.
    pub fn admits(&self, shelf: &Shelf, id: DocId, standing: StandingKind) -> bool {
        if !self.ask.standings.is_empty() && !self.ask.standings.contains(&standing) {
            return false;
        }
        // Nothing but the standing asked: the shelf need not be read (an empty ask,
        // the plain `status`, passes every identity this way).
        if self.ask.books.is_empty() && self.conditions.is_empty() {
            return true;
        }
        let state = shelf.state_of(id);
        let schema = shelf.schema();
        if !self.ask.books.is_empty() && !self.is_in_asked_book(schema, &state) {
            return false;
        }
        if self.conditions.is_empty() {
            return true;
        }
        // A field condition holds only for a valid document in a book, read once.
        let DocState::Valid(doc) = state else {
            return false;
        };
        let Some(book) = book_of(schema, doc.origin()) else {
            return false;
        };
        self.conditions
            .iter()
            .all(|condition| condition.holds(book, doc))
    }

    /// Whether a file of the document is in a book asked.
    fn is_in_asked_book(&self, schema: &Schema, state: &DocState<'_>) -> bool {
        let in_asked = |origin: &RelPath| {
            book_of(schema, origin).is_some_and(|book| self.ask.books.contains(book))
        };
        match state {
            DocState::Valid(doc) => in_asked(doc.origin()),
            DocState::Invalid(files) => files.iter().any(|file| in_asked(file.origin())),
            DocState::Gone | DocState::Unknown { .. } => false,
        }
    }
}

impl Condition {
    /// Whether a valid document of this book holds the condition: the book declares
    /// the field and can read the value, and the value it wrote is the one wanted.
    fn holds(&self, book: &BookName, doc: &Doc) -> bool {
        match (self.wanted.get(book), doc.fields().get(&self.field)) {
            (Some(wanted), Some(value)) => wanted.is_met_by(value),
            _ => false,
        }
    }
}

/// The book a file is in, for a valid or an invalid one: the one entry of the
/// question for the book condition and for the field condition, so that the two
/// cannot disagree. A file under no book's directory has none.
fn book_of<'a>(schema: &'a Schema, origin: &RelPath) -> Option<&'a BookName> {
    match schema.membership(origin) {
        Membership::In(book) => Some(book.name()),
        Membership::Outside => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::build::{DocReading, RawProblem, RawValue, ReadId};
    use crate::domain::shelf::fixtures_tests::{docid, list, path, reading, text};
    use crate::domain::shelf::freshness::FreshnessLevel;
    use crate::domain::shelf::schema::{
        BookSchemaDecl, Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
        LinkEntryDecl, LinkTargetDecl, Propagation,
    };

    fn field(name: &str, ty: FieldTypeDecl) -> FieldEntryDecl {
        FieldEntryDecl {
            name: name.to_owned(),
            ty,
            presence: if name == "title" {
                FieldPresence::Required
            } else {
                FieldPresence::Optional
            },
        }
    }

    fn book(name: &str, dir: &str, prefix: &str, fields: Vec<FieldEntryDecl>) -> BookSchemaDecl {
        BookSchemaDecl {
            name: name.to_owned(),
            dir: dir.to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: prefix.to_owned(),
            },
            description: None,
            fields,
            links: vec![LinkEntryDecl {
                name: "base".to_owned(),
                target: LinkTargetDecl::Book(name.to_owned()),
                cardinality: Cardinality::Many,
                propagation: Propagation::Informs,
                space: None,
                compare: None,
                cover: None,
            }],
        }
    }

    fn words(of: &[&str]) -> FieldTypeDecl {
        FieldTypeDecl::Enum(of.iter().map(|w| (*w).to_owned()).collect())
    }

    /// Three books. `topic` is a field of `decision` (a list) and `note` (a text), and
    /// not of `plain`; `size` is an integer in `decision` and an enum in `note`;
    /// `level`, `count` and `flag` are of `decision` alone.
    fn schema() -> Schema {
        let decls = vec![
            book(
                "decision",
                "decisions",
                "DEC",
                vec![
                    field("title", FieldTypeDecl::Text),
                    field("topic", FieldTypeDecl::TextList),
                    field("size", FieldTypeDecl::Integer),
                    field("level", words(&["low", "high"])),
                    field("count", FieldTypeDecl::Integer),
                    field("flag", FieldTypeDecl::Bool),
                ],
            ),
            book(
                "note",
                "notes",
                "N",
                vec![
                    field("title", FieldTypeDecl::Text),
                    field("topic", FieldTypeDecl::Text),
                    field("size", words(&["big", "small"])),
                ],
            ),
            book(
                "plain",
                "plain",
                "P",
                vec![field("title", FieldTypeDecl::Text)],
            ),
        ];
        Schema::validate(decls, &Default::default()).expect("a sound declaration")
    }

    fn shelf_of(readings: Vec<DocReading>) -> Shelf {
        Shelf::build(&schema(), readings, &Default::default())
    }

    fn ask() -> StatusFilterAsk {
        StatusFilterAsk::default()
    }

    fn book_name(text: &str) -> BookName {
        BookName::new(text).expect("a book name")
    }

    fn field_ask(field: &str, value: &str) -> FieldAsk {
        FieldAsk {
            field: EntryName::new(field).expect("a field name"),
            value: value.to_owned(),
        }
    }

    fn books(names: &[&str]) -> BTreeSet<BookName> {
        names.iter().map(|n| book_name(n)).collect()
    }

    fn filter(ask: StatusFilterAsk) -> StatusFilter {
        StatusFilter::new(&schema(), ask).expect("a filter that fits")
    }

    fn problems(ask: StatusFilterAsk) -> Vec<FilterProblem> {
        StatusFilter::new(&schema(), ask)
            .expect_err("a filter that does not fit")
            .into_vec()
    }

    const FRESH: StandingKind = StandingKind::Approved(FreshnessLevel::Fresh);
    const STALE: StandingKind = StandingKind::Approved(FreshnessLevel::Stale);

    /// A decision `DEC-<n>`, identity `n`, with these extra entries.
    fn dec(n: u128, extra: &[(&str, RawValue)]) -> DocReading {
        let mut entries = vec![("title", text("t"))];
        entries.extend(extra.iter().cloned());
        reading(
            &format!("decisions/dec-{n}.md"),
            n,
            &format!("DEC-{n}"),
            &entries,
        )
    }

    fn note(n: u128, extra: &[(&str, RawValue)]) -> DocReading {
        let mut entries = vec![("title", text("t"))];
        entries.extend(extra.iter().cloned());
        reading(&format!("notes/n-{n}.md"), n, &format!("N-{n}"), &entries)
    }

    fn plain(n: u128) -> DocReading {
        reading(
            &format!("plain/p-{n}.md"),
            n,
            &format!("P-{n}"),
            &[("title", text("t"))],
        )
    }

    /// A file of no valid document, as far as an adapter could read: it names this
    /// identity.
    fn broken(file: &str, id: u128) -> DocReading {
        DocReading::Unparsed {
            origin: path(file),
            id: ReadId::Read(docid(id)),
            why: RawProblem::NotAMapping,
        }
    }

    fn admits(filter: &StatusFilter, shelf: &Shelf, n: u128, standing: StandingKind) -> bool {
        filter.admits(shelf, docid(n), standing)
    }

    fn int(n: i64) -> RawValue {
        RawValue::Integer(n)
    }

    #[test]
    fn an_empty_ask_admits_every_document_in_every_state() {
        let shelf = shelf_of(vec![dec(1, &[]), broken("notes/x.md", 2)]);
        let all = filter(ask());
        assert!(admits(&all, &shelf, 1, FRESH));
        assert!(admits(&all, &shelf, 2, StandingKind::Pending));
        // An identity no file carries.
        assert!(admits(&all, &shelf, 3, StandingKind::Retired));
    }

    #[test]
    fn the_books_asked_are_one_or_the_other() {
        let shelf = shelf_of(vec![dec(1, &[]), note(2, &[]), plain(3)]);
        let one = filter(StatusFilterAsk {
            books: books(&["decision"]),
            ..ask()
        });
        assert!(admits(&one, &shelf, 1, FRESH));
        assert!(!admits(&one, &shelf, 2, FRESH));
        let two = filter(StatusFilterAsk {
            books: books(&["decision", "note"]),
            ..ask()
        });
        assert!(admits(&two, &shelf, 1, FRESH));
        assert!(admits(&two, &shelf, 2, FRESH));
        assert!(!admits(&two, &shelf, 3, FRESH));
    }

    #[test]
    fn the_standings_asked_are_one_or_the_other() {
        let shelf = shelf_of(vec![dec(1, &[])]);
        let some = filter(StatusFilterAsk {
            standings: [STALE, StandingKind::Pending].into(),
            ..ask()
        });
        assert!(admits(&some, &shelf, 1, STALE));
        assert!(admits(&some, &shelf, 1, StandingKind::Pending));
        assert!(!admits(&some, &shelf, 1, FRESH));
        assert!(!admits(&some, &shelf, 1, StandingKind::Denied));
    }

    #[test]
    fn kinds_of_condition_all_have_to_hold() {
        let shelf = shelf_of(vec![
            dec(1, &[("topic", list(&["#41"]))]),
            dec(2, &[("topic", list(&["#42"]))]),
            note(3, &[("topic", text("#41"))]),
        ]);
        let all_three = filter(StatusFilterAsk {
            books: books(&["decision", "plain"]),
            standings: [STALE].into(),
            fields: vec![field_ask("topic", "#41")],
        });
        assert!(admits(&all_three, &shelf, 1, STALE));
        assert!(!admits(&all_three, &shelf, 1, FRESH), "the standing");
        assert!(!admits(&all_three, &shelf, 2, STALE), "the field");
        assert!(!admits(&all_three, &shelf, 3, STALE), "the book");
    }

    #[test]
    fn each_where_is_a_condition_of_its_own() {
        let shelf = shelf_of(vec![
            dec(1, &[("count", int(2)), ("flag", RawValue::Bool(true))]),
            dec(2, &[("count", int(2)), ("flag", RawValue::Bool(false))]),
            dec(3, &[("count", int(3)), ("flag", RawValue::Bool(true))]),
        ]);
        let both = filter(StatusFilterAsk {
            fields: vec![field_ask("count", "2"), field_ask("flag", "true")],
            ..ask()
        });
        assert!(admits(&both, &shelf, 1, FRESH));
        assert!(!admits(&both, &shelf, 2, FRESH));
        assert!(!admits(&both, &shelf, 3, FRESH));
    }

    #[test]
    fn a_value_is_compared_by_the_type_of_the_field() {
        let shelf = shelf_of(vec![
            dec(
                1,
                &[
                    ("count", int(12)),
                    ("flag", RawValue::Bool(true)),
                    ("level", text("high")),
                    ("topic", list(&["a, b", "c"])),
                ],
            ),
            dec(2, &[("count", int(1))]),
            note(3, &[("topic", text("a, b"))]),
        ]);
        let admitted = |field: &str, value: &str, n: u128| {
            filter(StatusFilterAsk {
                fields: vec![field_ask(field, value)],
                ..ask()
            })
            .admits(&shelf, docid(n), FRESH)
        };
        // An integer is a number, not the text it is written as.
        assert!(admitted("count", "12", 1));
        assert!(!admitted("count", "1", 1));
        assert!(admitted("count", "1", 2));
        // A truth value.
        assert!(admitted("flag", "true", 1));
        assert!(!admitted("flag", "false", 1));
        // An enum word.
        assert!(admitted("level", "high", 1));
        assert!(!admitted("level", "low", 1));
        // A list holds the text as one item, which is not split at a comma.
        assert!(admitted("topic", "c", 1));
        assert!(admitted("topic", "a, b", 1));
        assert!(!admitted("topic", "a", 1));
        // A text is equal as a whole.
        assert!(admitted("topic", "a, b", 3));
        assert!(!admitted("topic", "a", 3));
    }

    #[test]
    fn a_book_that_cannot_read_the_value_passes_nothing_while_one_that_can_does() {
        // `size` is an integer in `decision` and an enum in `note`.
        let shelf = shelf_of(vec![
            dec(1, &[("size", int(3))]),
            note(2, &[("size", text("big"))]),
        ]);
        let by = |value: &str| {
            filter(StatusFilterAsk {
                fields: vec![field_ask("size", value)],
                ..ask()
            })
        };
        let three = by("3");
        assert!(admits(&three, &shelf, 1, FRESH));
        assert!(!admits(&three, &shelf, 2, FRESH));
        let big = by("big");
        assert!(!admits(&big, &shelf, 1, FRESH));
        assert!(admits(&big, &shelf, 2, FRESH));
    }

    #[test]
    fn a_document_of_a_book_without_the_field_or_without_the_value_does_not_pass() {
        let shelf = shelf_of(vec![
            plain(1),
            dec(2, &[]),
            dec(3, &[("topic", list(&["x"]))]),
        ]);
        let by = filter(StatusFilterAsk {
            fields: vec![field_ask("topic", "x")],
            ..ask()
        });
        assert!(!admits(&by, &shelf, 1, FRESH), "the book has no such field");
        assert!(!admits(&by, &shelf, 2, FRESH), "the field is not written");
        assert!(admits(&by, &shelf, 3, FRESH));
    }

    #[test]
    fn an_invalid_document_passes_by_the_book_of_any_of_its_files_and_never_by_a_field() {
        // One identity in two invalid files, of two books.
        let shelf = shelf_of(vec![
            broken("decisions/a.md", 1),
            broken("notes/b.md", 1),
            broken("plain/c.md", 2),
        ]);
        let in_books = |names: &[&str]| {
            filter(StatusFilterAsk {
                books: books(names),
                ..ask()
            })
        };
        assert!(admits(&in_books(&["decision"]), &shelf, 1, FRESH));
        assert!(admits(&in_books(&["note"]), &shelf, 1, FRESH));
        assert!(admits(&in_books(&["note", "plain"]), &shelf, 1, FRESH));
        assert!(!admits(&in_books(&["plain"]), &shelf, 1, FRESH));
        assert!(admits(&in_books(&["plain"]), &shelf, 2, FRESH));
        let by_field = filter(StatusFilterAsk {
            fields: vec![field_ask("topic", "x")],
            ..ask()
        });
        assert!(!admits(&by_field, &shelf, 1, FRESH));
        // The standing of an invalid document is asked as for any.
        let pending = filter(StatusFilterAsk {
            standings: [StandingKind::Pending].into(),
            ..ask()
        });
        assert!(admits(&pending, &shelf, 1, StandingKind::Pending));
        assert!(!admits(&pending, &shelf, 1, FRESH));
    }

    #[test]
    fn an_identity_that_is_gone_or_unknown_passes_by_standing_alone() {
        let gone = shelf_of(vec![dec(1, &[])]);
        let unknown = shelf_of(vec![DocReading::Unparsed {
            origin: path("decisions/lost.md"),
            id: ReadId::Unreadable,
            why: RawProblem::NotAMapping,
        }]);
        let by_book = filter(StatusFilterAsk {
            books: books(&["decision"]),
            ..ask()
        });
        let by_field = filter(StatusFilterAsk {
            fields: vec![field_ask("topic", "x")],
            ..ask()
        });
        let by_standing = filter(StatusFilterAsk {
            standings: [StandingKind::Retired].into(),
            ..ask()
        });
        for (shelf, n) in [(&gone, 9), (&unknown, 1)] {
            assert!(!admits(&by_book, shelf, n, StandingKind::Retired));
            assert!(!admits(&by_field, shelf, n, StandingKind::Retired));
            assert!(admits(&by_standing, shelf, n, StandingKind::Retired));
            assert!(!admits(&by_standing, shelf, n, FRESH));
        }
    }

    #[test]
    fn a_file_under_no_book_passes_neither_the_book_nor_the_field() {
        // `Schema::membership` puts a file outside every directory in no book.
        let shelf = shelf_of(vec![
            broken("elsewhere/a.md", 1),
            reading(
                "elsewhere/b.md",
                2,
                "DEC-2",
                &[("title", text("t")), ("topic", list(&["x"]))],
            ),
        ]);
        let by_book = filter(StatusFilterAsk {
            books: books(&["decision"]),
            ..ask()
        });
        let by_field = filter(StatusFilterAsk {
            fields: vec![field_ask("topic", "x")],
            ..ask()
        });
        for n in [1, 2] {
            assert!(!admits(&by_book, &shelf, n, FRESH));
            assert!(!admits(&by_field, &shelf, n, FRESH));
        }
    }

    #[test]
    fn the_ask_is_kept_as_given() {
        let given = StatusFilterAsk {
            books: books(&["note", "decision"]),
            standings: [STALE].into(),
            fields: vec![field_ask("topic", "x"), field_ask("count", "1")],
        };
        assert_eq!(filter(given.clone()).ask(), &given);
    }

    #[test]
    fn a_book_that_is_not_declared_is_a_problem() {
        assert_eq!(
            problems(StatusFilterAsk {
                books: books(&["nothing"]),
                ..ask()
            }),
            vec![FilterProblem::UnknownBook(book_name("nothing"))]
        );
        assert_eq!(
            FilterProblem::UnknownBook(book_name("nothing")).to_string(),
            "there is no book `nothing`"
        );
    }

    #[test]
    fn a_field_no_book_declares_is_a_problem_and_a_link_is_no_field() {
        for name in ["nothing", "base"] {
            assert_eq!(
                problems(StatusFilterAsk {
                    fields: vec![field_ask(name, "x")],
                    ..ask()
                }),
                vec![FilterProblem::UnknownField(
                    EntryName::new(name).expect("a name")
                )],
                "{name}"
            );
        }
    }

    #[test]
    fn a_value_no_declaring_book_can_read_is_a_problem() {
        // `count` is an integer in the one book that declares it.
        assert_eq!(
            problems(StatusFilterAsk {
                fields: vec![field_ask("count", "many")],
                ..ask()
            }),
            vec![FilterProblem::ValueNotOfField {
                field: EntryName::new("count").expect("a name"),
                value: "many".to_owned(),
            }]
        );
        // `size` is an integer in one book and an enum in the other: neither reads this.
        assert_eq!(
            problems(StatusFilterAsk {
                fields: vec![field_ask("size", "huge")],
                ..ask()
            })
            .len(),
            1
        );
        // A list field reads any text as an item.
        assert!(
            StatusFilter::new(
                &schema(),
                StatusFilterAsk {
                    fields: vec![field_ask("topic", "anything at all")],
                    ..ask()
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn every_problem_is_collected() {
        let all = problems(StatusFilterAsk {
            books: books(&["nothing", "decision", "zero"]),
            standings: [STALE].into(),
            fields: vec![
                field_ask("nothing", "x"),
                field_ask("count", "many"),
                field_ask("topic", "fine"),
            ],
        });
        assert_eq!(
            all,
            vec![
                FilterProblem::UnknownBook(book_name("nothing")),
                FilterProblem::UnknownBook(book_name("zero")),
                FilterProblem::UnknownField(EntryName::new("nothing").expect("a name")),
                FilterProblem::ValueNotOfField {
                    field: EntryName::new("count").expect("a name"),
                    value: "many".to_owned(),
                },
            ]
        );
    }
}
