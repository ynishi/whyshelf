//! Names put in a row, for a sentence.
//!
//! A message that names several things (books, spaces, keys, files, problems)
//! says them one way, here, so no two messages join names differently: [`listed`]
//! is the one rule. How each name is marked ([`Mark`]) and how the names are
//! joined ([`Join`]) are arguments of it.
//!
//! | mark | join | three names |
//! |---|---|---|
//! | [`Mark::Plain`] | [`Join::And`] | `a, b and c` |
//! | [`Mark::Plain`] | [`Join::Or`] | `a, b or c` |
//! | [`Mark::Plain`] | [`Join::Comma`] | `a, b, c` |
//! | [`Mark::Plain`] | [`Join::Semicolon`] | `a; b; c` |
//! | [`Mark::Backtick`] | [`Join::And`] | `` `a`, `b` and `c` `` |
//! | [`Mark::Quote`] | [`Join::Or`] | `"a", "b" or "c"` |
//!
//! No names make no text; the sentence that has no names to say decides what to
//! say instead (a message may say `none`).

use std::fmt::Display;

/// How each name is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// As it is: `a`.
    Plain,
    /// In backticks, as a name of the problem's own is: `` `a` ``.
    Backtick,
    /// In double quotes, as a word written in a declaration is: `"a"`.
    Quote,
}

/// How the names are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Commas, and `and` before the last of two or more: `a, b and c`.
    And,
    /// Commas, and `or` before the last of two or more: `a, b or c`.
    Or,
    /// Commas only: `a, b, c`.
    Comma,
    /// Semicolons only, for items that hold commas of their own: `a; b; c`.
    Semicolon,
}

/// The names said in a row: each marked as `mark` says, joined as `join` says.
///
/// One name is that name alone, whatever the join. No names are an empty text.
///
/// ```
/// use whyshelf_core::domain::words::{listed, Join, Mark};
///
/// assert_eq!(listed(["a", "b", "c"], Mark::Backtick, Join::And), "`a`, `b` and `c`");
/// assert_eq!(listed(["a", "b"], Mark::Quote, Join::Or), "\"a\" or \"b\"");
/// assert_eq!(listed(Vec::<&str>::new(), Mark::Plain, Join::Comma), "");
/// ```
pub fn listed<T: Display>(items: impl IntoIterator<Item = T>, mark: Mark, join: Join) -> String {
    let marked: Vec<String> = items
        .into_iter()
        .map(|item| match mark {
            Mark::Plain => item.to_string(),
            Mark::Backtick => format!("`{item}`"),
            Mark::Quote => format!("\"{item}\""),
        })
        .collect();
    let (word, separator) = match join {
        Join::And => (Some("and"), ", "),
        Join::Or => (Some("or"), ", "),
        Join::Comma => (None, ", "),
        Join::Semicolon => (None, "; "),
    };
    match (marked.as_slice(), word) {
        ([], _) => String::new(),
        ([only], _) => only.clone(),
        ([rest @ .., last], Some(word)) => format!("{} {word} {last}", rest.join(separator)),
        (_, None) => marked.join(separator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKS: [Mark; 3] = [Mark::Plain, Mark::Backtick, Mark::Quote];
    const JOINS: [Join; 4] = [Join::And, Join::Or, Join::Comma, Join::Semicolon];

    fn names(n: usize) -> Vec<&'static str> {
        ["a", "b", "c"][..n].to_vec()
    }

    #[test]
    fn no_names_are_no_text_for_every_mark_and_join() {
        for mark in MARKS {
            for join in JOINS {
                assert_eq!(listed(names(0), mark, join), "");
            }
        }
    }

    #[test]
    fn one_name_is_itself_marked_for_every_join() {
        for join in JOINS {
            assert_eq!(listed(names(1), Mark::Plain, join), "a");
            assert_eq!(listed(names(1), Mark::Backtick, join), "`a`");
            assert_eq!(listed(names(1), Mark::Quote, join), "\"a\"");
        }
    }

    #[test]
    fn two_names_are_joined_as_the_join_says() {
        assert_eq!(listed(names(2), Mark::Plain, Join::And), "a and b");
        assert_eq!(listed(names(2), Mark::Plain, Join::Or), "a or b");
        assert_eq!(listed(names(2), Mark::Plain, Join::Comma), "a, b");
        assert_eq!(listed(names(2), Mark::Plain, Join::Semicolon), "a; b");
    }

    #[test]
    fn three_names_are_joined_as_the_join_says() {
        assert_eq!(listed(names(3), Mark::Plain, Join::And), "a, b and c");
        assert_eq!(listed(names(3), Mark::Plain, Join::Or), "a, b or c");
        assert_eq!(listed(names(3), Mark::Plain, Join::Comma), "a, b, c");
        assert_eq!(listed(names(3), Mark::Plain, Join::Semicolon), "a; b; c");
    }

    #[test]
    fn each_mark_marks_every_name() {
        assert_eq!(
            listed(names(3), Mark::Backtick, Join::And),
            "`a`, `b` and `c`"
        );
        assert_eq!(
            listed(names(3), Mark::Quote, Join::Or),
            "\"a\", \"b\" or \"c\""
        );
        assert_eq!(
            listed(names(3), Mark::Quote, Join::Comma),
            "\"a\", \"b\", \"c\""
        );
        assert_eq!(
            listed(names(2), Mark::Backtick, Join::Semicolon),
            "`a`; `b`"
        );
    }

    #[test]
    fn any_display_item_and_any_iterator_will_do() {
        assert_eq!(
            listed([1, 2, 3].iter(), Mark::Plain, Join::And),
            "1, 2 and 3"
        );
        assert_eq!(
            listed(["x".to_owned()].into_iter(), Mark::Quote, Join::Comma),
            "\"x\""
        );
    }
}
