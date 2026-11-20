//! The answer of `init`: the declaration and the files and documents written beside
//! it, and the commands to run next.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::Initialized;

use super::NextRoot;

/// The answer of `init`: what was written, and what to run next.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitOut {
    /// The file of the declaration, relative to the project root.
    pub declaration: String,
    /// The other files written beside it, relative to the project root, in the
    /// order they were written: what the documents name as artifacts (the table of
    /// the template `experiment`); none for the other templates.
    pub files: Vec<String>,
    /// The documents made beside it, in the order they were written; none for
    /// the template `minimal`.
    pub documents: Vec<InitDocOut>,
    /// The commands to run next, in order, as they are typed, each with
    /// `--root=<dir>` when `init` ran with one; none when that root is no UTF-8
    /// text.
    pub next: Vec<String>,
}

/// A document `init` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitDocOut {
    /// Its key (`book:KEY`).
    pub key: String,
    /// Its file, relative to the project root.
    pub path: String,
}

/// The answer of `init`. `book` is the book `next` makes a first document of, when
/// `init` made none; `root` the `--root` it ran with, which every command of `next`
/// carries.
pub fn initialized(done: &Initialized, book: Option<&str>, root: NextRoot<'_>) -> InitOut {
    // A book's directory may hold documents of the person's own: `next` names the
    // documents `init` made that no other of them stands on.
    let heads: Vec<String> = if done.docs.is_empty() {
        Vec::new()
    } else {
        done.heads.iter().map(|k| k.form.to_string()).collect()
    };
    let next = super::next::after_init(book, &heads, root);
    InitOut {
        declaration: done.declaration.to_string(),
        files: done.files.iter().map(ToString::to_string).collect(),
        documents: done
            .docs
            .iter()
            .map(|d| InitDocOut {
                key: d.key.to_string(),
                path: d.file.to_string(),
            })
            .collect(),
        next,
    }
}
