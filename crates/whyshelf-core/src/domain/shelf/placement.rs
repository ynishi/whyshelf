//! Where the names of a space live in the tree of the project: the files a name is
//! written in, as the adapter of its space says.
//!
//! A name of a space is opaque to the domain; only its adapter can say which files of
//! the project hold the thing it names. [`Placement`] is that answer, one per name.
//! What a document stands on is a document's file or an artifact's place, so this is
//! what a question about the files a change may touch reads of the artifacts.
//!
//! | answer | the files | an example |
//! |---|---|---|
//! | [`Placement::Only`] | the thing is these files and nothing else | `file`: the path written; `rust-test`: the file before `::` |
//! | [`Placement::Partly`] | these files hold the thing, and something outside them does too | `rust-item`: the file an item is defined in, while its docs come from the build |
//! | [`Placement::Unplaced`] | the adapter cannot say, and why | a text that is no name of the space; a name read through a link that leaves the root |
//!
//! The files are paths of the project as written ([`RelPath`]), the files the content
//! of the name is read from at the side asked, by the same rule its reading follows
//! links by: a name with no link on its way is its own path; one read through links
//! inside the root lives in each link entry on its way, as written, and in the name
//! they lead to, since its content depends on each of them; one whose content is read
//! through no link (one that leaves the root, is broken, or loops) has no place.

use std::collections::BTreeSet;

use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptySet};

/// Where one name of a space lives in the tree of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// The thing the name is, is these files, whole.
    Only(NonEmptySet<RelPath>),
    /// These files hold part of the thing the name is; what else it is made of is
    /// outside the files of the project.
    Partly(NonEmptySet<RelPath>),
    /// The adapter cannot say where the name lives, and why. A text that is no name
    /// of the space is one, said by the grammar of the space alone.
    Unplaced(NonBlank),
}

impl Placement {
    /// The files of the placement, whole or in part; none when it is unplaced.
    pub fn files(&self) -> Option<&BTreeSet<RelPath>> {
        match self {
            Self::Only(files) | Self::Partly(files) => Some(files.as_set()),
            Self::Unplaced(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(text: &str) -> RelPath {
        text.parse().expect("a path")
    }

    #[test]
    fn a_placement_whole_or_in_part_has_its_files_and_an_unplaced_one_none() {
        let files = NonEmptySet::new(rel("src/a.rs"), [rel("src/b.rs")]);
        for placement in [
            Placement::Only(files.clone()),
            Placement::Partly(files.clone()),
        ] {
            assert_eq!(placement.files(), Some(files.as_set()));
        }
        let unplaced = Placement::Unplaced(NonBlank::new("no name of the space").unwrap());
        assert_eq!(unplaced.files(), None);
    }
}
