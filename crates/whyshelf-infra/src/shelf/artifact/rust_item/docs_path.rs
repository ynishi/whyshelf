//! The name of a place in the built docs: [`DocsPath`].

use std::path::{Component, Path, PathBuf};

use whyshelf_core::domain::location::RelPath;

/// A place in the built docs, by its parts below the directory rustdoc writes them
/// in: the name of the tree of the built docs
/// ([`StateRead`](whyshelf_core::app::shelf::StateRead)`<DocsPath>`). No part is
/// empty, `.` or `..`, or holds `/`, `\` or a control character, as a part of a path
/// of the project ([`RelPath`]); no part at all is the directory of the docs itself.
///
/// It is a name of this crate's: the core reads the tree through the generic port and
/// never sees one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct DocsPath(Vec<String>);

impl DocsPath {
    /// The directory of the docs itself.
    pub(crate) fn top() -> Self {
        Self(Vec::new())
    }

    /// The place `part` names under this one, when `part` is one part of a path.
    pub(crate) fn join(&self, part: &str) -> Option<Self> {
        let path: RelPath = part.parse().ok()?;
        if path.as_str().contains('/') {
            return None;
        }
        let mut parts = self.0.clone();
        parts.push(part.to_owned());
        Some(Self(parts))
    }

    /// Its parts, from the directory of the docs.
    pub(crate) fn parts(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// The last part, when it has one.
    pub(crate) fn file_name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }

    /// The place above it, when it is not the top.
    pub(crate) fn parent(&self) -> Option<Self> {
        let (_, above) = self.0.split_last()?;
        Some(Self(above.to_vec()))
    }

    /// The path of the file system of this place, under `docs`, the directory of the
    /// docs: what a reader names in what it says.
    pub(crate) fn under(&self, docs: &Path) -> PathBuf {
        self.parts()
            .fold(docs.to_path_buf(), |path, part| path.join(part))
    }

    /// The place a path relative to the directory of the docs is, when each of its
    /// parts is one.
    pub(crate) fn of_relative(path: &Path) -> Option<Self> {
        path.components()
            .try_fold(Self::top(), |at, part| match part {
                Component::Normal(part) => at.join(part.to_str()?),
                _ => None,
            })
    }

    /// The path of the project this place is, under `docs`, the directory of the docs
    /// in the project.
    pub(crate) fn in_project(&self, docs: &RelPath) -> Option<RelPath> {
        let mut text = docs.as_str().to_owned();
        for part in self.parts() {
            text.push('/');
            text.push_str(part);
        }
        text.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_parts_each_a_part_of_a_path_and_the_top_has_none() {
        let page = DocsPath::top()
            .join("demo")
            .and_then(|p| p.join("struct.A.html"))
            .unwrap();
        assert_eq!(page.parts().collect::<Vec<_>>(), ["demo", "struct.A.html"]);
        assert_eq!(page.file_name(), Some("struct.A.html"));
        assert_eq!(page.parent(), DocsPath::top().join("demo"));
        for bad in ["", ".", "..", "a/b", "a\\b"] {
            assert_eq!(DocsPath::top().join(bad), None, "{bad:?}");
        }
        assert_eq!(DocsPath::top().parent(), None);
        assert_eq!(
            page.in_project(&"target/doc".parse().unwrap()),
            Some("target/doc/demo/struct.A.html".parse().unwrap())
        );
        assert_eq!(
            DocsPath::of_relative(Path::new("demo/struct.A.html")),
            Some(page)
        );
    }
}
