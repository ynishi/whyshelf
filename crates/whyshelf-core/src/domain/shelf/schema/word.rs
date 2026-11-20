//! The words of a declaration: the types a declaration writes as one word
//! ([`Words`], [`DeclWord`], [`BookWord`]) and the one way such a word is read
//! ([`read_word`], [`read_book_word`]). The words themselves are each type's own.

use crate::domain::words::{Join, Mark, listed};

use super::{BookEntry, SchemaProblem, WordAt, WordOf};

/// A type that a declaration writes as one word: the values it has (`all`) and the
/// word of each (`word`). `find_word` is the one search of them by a word, for both
/// kinds of word there are: [`DeclWord`] (a count, a type, a meaning), and
/// [`BookWord`] (what a book's entry says).
pub trait Words: Copy + 'static {
    /// Every value of the type: the `ALL` of the type.
    fn all() -> &'static [Self];

    /// The word of the value: the `word` of the type.
    fn word(self) -> &'static str;
}

/// A type that a declaration writes as one word in a link or a field: [`ScalarType`](super::ScalarType),
/// [`Cardinality`](super::Cardinality) and [`Propagation`](super::Propagation). [`read_word`] reads such a word, and a problem
/// lists the words of the type from its `ALL`.
pub trait DeclWord: Words {
    /// What the word of this type says in a problem.
    const OF: WordOf;
}

/// A type that a declaration writes as one word in an entry of a book: what a file
/// with no front matter is ([`WithoutFrontMatter`](super::WithoutFrontMatter)) and the grammar of the keys
/// ([`KeyWord`](super::KeyWord)). [`read_book_word`] reads such a word, and a problem lists the words
/// the entry takes ([`BookEntry::accepted`](super::BookEntry::accepted)).
pub trait BookWord: Words {
    /// The entry of the book that writes the word.
    const ENTRY: BookEntry;
}

/// The value of a type whose word is `word`: the one search of `all` by `word` that
/// every `from_word` and every read of a word is.
pub(super) fn find_word<T: Words>(word: &str) -> Option<T> {
    T::all().iter().copied().find(|value| value.word() == word)
}

/// The words of a type, each in quotes, joined as `join` says: `"a", "b"` or
/// `"a" or "b"`.
pub(super) fn quoted<T: Words>(join: Join) -> String {
    listed(T::all().iter().map(|value| value.word()), Mark::Quote, join)
}

/// The value a word of a declaration names, or why the word is refused.
///
/// The one reader of the words of a declaration (a count, a type, a meaning):
/// whatever puts one in a declaration as written keeps the word as written, and
/// whatever uses it as a value reads it here, and so refuses a word the type does
/// not have the same way, saying where it was written.
// The problem is the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn read_word<T: DeclWord>(word: &str, at: WordAt) -> Result<T, SchemaProblem> {
    find_word(word).ok_or_else(|| SchemaProblem::UnknownWord {
        at,
        word: word.to_owned(),
        of: T::OF,
    })
}

/// The value a word of an entry of a book names, or why the word is refused.
///
/// The reader of the words of the entries of a book (what a file with no front matter
/// is, the grammar of the keys), beside [`read_word`] and by the same search: whatever
/// puts one in a declaration as written keeps the word as written, and whatever uses
/// it as a value reads it here, and so refuses a word the entry does not take the
/// same way, naming the book and the entry and saying what the entry takes.
// The problem is the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn read_book_word<T: BookWord>(word: &str, book: &str) -> Result<T, SchemaProblem> {
    find_word(word).ok_or_else(|| SchemaProblem::UnknownBookWord {
        book: book.to_owned(),
        entry: T::ENTRY,
        word: word.to_owned(),
    })
}

/// A read word that is kept, or its problem that is reported: a refused word leaves
/// the entry it was written in out, and the problem is told with the others.
pub(super) fn report<T>(
    read: Result<T, SchemaProblem>,
    problems: &mut Vec<SchemaProblem>,
) -> Option<T> {
    match read {
        Ok(value) => Some(value),
        Err(problem) => {
            problems.push(problem);
            None
        }
    }
}
