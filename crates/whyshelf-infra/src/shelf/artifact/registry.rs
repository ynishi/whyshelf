//! The registry of the adapters of a project: the list the dispatch of the core
//! ([`Spaces`]) hands each ask to, by the number of an adapter.
//!
//! It holds no rule and names no space: each adapter names its own space and presets
//! ([`SpaceAdapter`]), and the dispatch keys them, refuses two of one space, and
//! answers for a space with none. What is here is the list and the two matches that
//! hand an ask to the adapter at a number, whatever its type. An adapter is added by
//! a variant of [`Adapter`], an arm of each match, a variant of [`SpacesError`], and a
//! place in [`Registry::of_project`].
//!
//! [`Spaces`]: whyshelf_core::app::shelf::Spaces
//! [`SpaceAdapter`]: whyshelf_core::app::shelf::SpaceAdapter

use whyshelf_core::app::shelf::{AsyncVisit, SpaceRegistry, Visit};
use whyshelf_core::domain::error::ValueError;

use super::file::{ArtifactError, FileSpace};
use super::js_test::{JsTestError, JsTestSpace};
use super::rust_item::{RustItemError, RustItemSpace};
use super::rust_test::{RustTestError, RustTestSpace};

/// One adapter of a space.
#[derive(Debug, Clone)]
pub enum Adapter {
    /// The files of the project ([`file`](mod@super::file)).
    File(FileSpace),
    /// The items of the Rust crates of the project ([`rust_item`](super::rust_item)).
    RustItem(RustItemSpace),
    /// The test functions of the Rust source of the project
    /// ([`rust_test`](super::rust_test)).
    RustTest(RustTestSpace),
    /// The tests of the JavaScript and TypeScript source of the project
    /// ([`js_test`](super::js_test)).
    JsTest(JsTestSpace),
}

impl Adapter {
    /// The answer of this adapter to an ask with no I/O.
    fn visit<V: Visit<SpacesError>>(&self, visit: V) -> V::Output {
        match self {
            Self::File(files) => visit.visit(files),
            Self::RustItem(items) => visit.visit(items),
            Self::RustTest(tests) => visit.visit(tests),
            Self::JsTest(tests) => visit.visit(tests),
        }
    }

    /// The answer of this adapter to an ask that reads.
    async fn visit_async<V: AsyncVisit<SpacesError>>(&self, visit: V) -> V::Output {
        match self {
            Self::File(files) => visit.visit(files).await,
            Self::RustItem(items) => visit.visit(items).await,
            Self::RustTest(tests) => visit.visit(tests).await,
            Self::JsTest(tests) => visit.visit(tests).await,
        }
    }
}

/// Why an adapter could not do its work.
#[derive(Debug, thiserror::Error)]
pub enum SpacesError {
    /// The adapter of the space `file`.
    #[error(transparent)]
    File(#[from] ArtifactError),
    /// The adapter of the space `rust-item`.
    #[error(transparent)]
    RustItem(#[from] RustItemError),
    /// The adapter of the space `rust-test`.
    #[error(transparent)]
    RustTest(#[from] RustTestError),
    /// The adapter of the space `js-test`.
    #[error(transparent)]
    JsTest(#[from] JsTestError),
    /// The domain refused a reason the dispatch gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the artifact source")]
    Reason(#[from] ValueError),
}

/// The adapters, in a list.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    adapters: Vec<Adapter>,
}

impl Registry {
    /// The adapters of a project at its root: the files under it, the items of its
    /// Rust crates, the test functions of its Rust source, and the tests of its
    /// JavaScript and TypeScript source.
    pub fn of_project(
        files: FileSpace,
        rust_items: RustItemSpace,
        rust_tests: RustTestSpace,
        js_tests: JsTestSpace,
    ) -> Self {
        Self {
            adapters: vec![
                Adapter::File(files),
                Adapter::RustItem(rust_items),
                Adapter::RustTest(rust_tests),
                Adapter::JsTest(js_tests),
            ],
        }
    }

    /// `adapter` alone: for the tests of an adapter, which ask it through the
    /// dispatch as a project does.
    #[cfg(test)]
    pub(super) fn of_one(adapter: Adapter) -> Self {
        Self {
            adapters: vec![adapter],
        }
    }
}

impl SpaceRegistry for Registry {
    type Error = SpacesError;

    fn visit<V: Visit<SpacesError>>(&self, at: usize, visit: V) -> Option<V::Output> {
        self.adapters.get(at).map(|adapter| adapter.visit(visit))
    }

    async fn visit_async<V: AsyncVisit<SpacesError>>(
        &self,
        at: usize,
        visit: V,
    ) -> Option<V::Output> {
        match self.adapters.get(at) {
            Some(adapter) => Some(adapter.visit_async(visit).await),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    // arch-lint: allow(ports-by-the-dispatch) reason="the test asks the dispatch through the port it answers, and implements none"
    use whyshelf_core::app::shelf::{ArtifactSource, Side, Spaces};
    use whyshelf_core::domain::shelf::artifact::{
        ArtifactAsks, ArtifactKey, ArtifactName, Comparison, Space,
    };
    use whyshelf_core::domain::shelf::ledger::PATH_ONLY_SPACE;

    use super::super::{file, js_test, rust_item, rust_test};
    use super::*;

    fn space(text: &str) -> Space {
        Space::new(text).unwrap()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn of_project(root: &std::path::Path) -> Spaces<Registry> {
        Spaces::new(Registry::of_project(
            FileSpace::new(root.to_path_buf()),
            RustItemSpace::new(root.to_path_buf(), None),
            RustTestSpace::new(root.to_path_buf()),
            JsTestSpace::new(root.to_path_buf()),
        ))
        .unwrap()
    }

    #[test]
    fn the_four_adapters_of_a_project_name_four_spaces_and_six_presets() {
        let source = of_project(std::path::Path::new("."));
        assert_eq!(
            source.spaces(),
            BTreeSet::from([
                space("file"),
                space("js-test"),
                space("rust-item"),
                space("rust-test")
            ])
        );
        let mut presets = file::presets();
        presets.extend(rust_item::presets());
        presets.extend(rust_test::presets());
        presets.extend(js_test::presets());
        assert_eq!(source.presets(), presets);
        assert_eq!(presets.len(), 6);
    }

    #[tokio::test]
    async fn each_space_is_resolved_by_its_own_adapter() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.rs"), "one\n")
            .await
            .unwrap();
        let source = of_project(dir.path());
        let mut asks = ArtifactAsks::new();
        asks.ask(space("file"), name("a.rs"), Comparison::Bytes);
        asks.ask(space("rust-test"), name("a.rs"), Comparison::Bytes);
        let found = source.resolve(Side::After, &asks).await.unwrap();
        assert!(
            found
                .get(&space("file"), &name("a.rs"))
                .is_some_and(|reading| reading.is_present())
        );
        // The same text in another space is another artifact, read by that space's
        // adapter: no test is named.
        assert!(
            found
                .get(&space("rust-test"), &name("a.rs"))
                .is_some_and(|reading| reading.not_a_name_why().is_some())
        );
        let key = ArtifactKey::new(space("file"), name("a.rs"));
        assert_eq!(
            source.current(Side::After, &key).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
    }

    #[test]
    fn a_record_written_before_spaces_names_a_space_the_files_answer_for() {
        let source = of_project(std::path::Path::new("."));
        let old = Space::new(PATH_ONLY_SPACE).unwrap();
        assert!(source.spaces().contains(&old));
    }
}
