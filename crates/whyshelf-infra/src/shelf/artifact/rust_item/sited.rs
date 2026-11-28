//! Where the docs of one side were built from and into: the top of the side's tree
//! and the build directory, each as a path of the file system in the form it is given
//! and with every symbolic link resolved ([`Sited`]). What a page of the source names
//! by a whole path is told by these ([`Crates::file_of`](super::crates::Crates::file_of)).
//!
//! It is made in one place, from the tree of the docs of the side
//! ([`DocsTree`](super::docs_tree::DocsTree), made from where `docs_at` of the space,
//! the one owner of where the build directory is, says they are): the build directory
//! is the parent of the docs, and the root the top of the tree. The resolved form is asked of the tree
//! ([`ProjectTree::real_path`]) or, for docs outside the project, of what reads them
//! ([`OutsideDocs`](super::outside_docs::OutsideDocs)); this module reads nothing of
//! the file system itself.

use std::path::{Component, Path, PathBuf};

use whyshelf_core::app::shelf::{Beyond, Side};
use whyshelf_core::domain::location::RelPath;

use super::super::super::state::{ProjectTree, StateError};
use super::docs_tree::DocsTree;

/// One place in its two forms: as given, and with its links resolved (none when it
/// cannot be, or the side is no directory of the file system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Forms {
    given: PathBuf,
    real: Option<PathBuf>,
}

impl Forms {
    /// The place in these two forms.
    #[cfg(test)]
    pub(super) fn new(given: PathBuf, real: Option<PathBuf>) -> Self {
        Self { given, real }
    }

    /// The rest of `parts` after the parts of this place, when `parts` starts with
    /// them in either form: a form that is no absolute path of the file system with a
    /// part (the place of a side of Git, written as Git writes it) leads none.
    pub(super) fn rest<'a>(&self, parts: &'a [String]) -> Option<&'a [String]> {
        [Some(&self.given), self.real.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|form| {
                let lead = parts_of(form)?;
                parts.starts_with(&lead).then(|| &parts[lead.len()..])
            })
    }
}

/// The parts of an absolute path below its root, when each is one and there is one
/// at least.
fn parts_of(path: &Path) -> Option<Vec<String>> {
    if !path.is_absolute() {
        return None;
    }
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part.to_str().map(str::to_owned)),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (!parts.is_empty()).then_some(parts)
}

/// The top of the tree of one side and the build directory its docs are in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Sited {
    pub(super) root: Forms,
    pub(super) build: Forms,
}

impl Sited {
    /// Where the docs of `side` that `docs` reads were built: the one place a [`Sited`]
    /// is made, from the one value that says where the docs are (a directory of the
    /// project, or one outside it) and what reads them.
    pub(super) async fn of(
        state: &ProjectTree,
        side: Side,
        docs: &DocsTree,
    ) -> Result<Self, StateError> {
        let top = Beyond {
            dir: None,
            parts: Vec::new(),
        };
        let root = Forms {
            given: state.beyond_path(side, &top)?,
            real: state.real_path(side, &top).await,
        };
        let build = match docs {
            DocsTree::Outside(outside) => {
                let (given, real) = outside.docs();
                Forms {
                    given: given.parent().unwrap_or(given).to_path_buf(),
                    real: real.map(|real| real.parent().unwrap_or(real).to_path_buf()),
                }
            }
            DocsTree::Under(under) => {
                let dir = Beyond {
                    dir: parent_of(under.docs()),
                    parts: Vec::new(),
                };
                Forms {
                    given: state.beyond_path(side, &dir)?,
                    real: state.real_path(side, &dir).await,
                }
            }
        };
        Ok(Self { root, build })
    }
}

/// The directory above `path`, none when it is at the top.
fn parent_of(path: &RelPath) -> Option<RelPath> {
    path.as_str()
        .rsplit_once('/')
        .and_then(|(above, _)| above.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(text: &str) -> Vec<String> {
        text.split('/').map(str::to_owned).collect()
    }

    #[test]
    fn a_page_is_under_a_place_in_the_form_given_or_the_form_resolved_and_never_under_a_relative_one()
     {
        let place = Forms::new(
            PathBuf::from("/var/t/build"),
            Some(PathBuf::from("/private/var/t/build")),
        );
        let given = parts("var/t/build/debug/out/x.rs");
        assert_eq!(place.rest(&given), Some(&given[3..]));
        let real = parts("private/var/t/build/debug/out/x.rs");
        assert_eq!(place.rest(&real), Some(&real[4..]));
        assert_eq!(place.rest(&parts("var/t/other/x.rs")), None);
        // The place of a side of Git, and the root of the file system, lead nothing.
        for form in ["HEAD:target", "/"] {
            let none = Forms::new(PathBuf::from(form), None);
            assert_eq!(none.rest(&parts("HEAD:target/x.rs")), None, "{form}");
            assert_eq!(none.rest(&parts("x.rs")), None, "{form}");
        }
    }

    #[test]
    fn the_directory_above_a_path_is_none_at_the_top() {
        let path = |text: &str| text.parse::<RelPath>().unwrap();
        assert_eq!(parent_of(&path("target/doc")), Some(path("target")));
        assert_eq!(parent_of(&path("doc")), None);
    }
}
