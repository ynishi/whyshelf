//! The artifacts of a project: adapters registered by space, behind one
//! [`ArtifactSource`].
//!
//! The domain knows a space only by its name. This module is where a name of a space
//! meets the adapter that resolves names in it, and where the words of the outside
//! (a path, a blob) are turned into the values of the domain (a name, an
//! [`ArtifactReading`]). Every space goes through the same port and answers in the
//! same four ways.
//!
//! ```text
//!   ArtifactAsks ──per space──► the adapter registered for it ──► SpaceReadings ─┐
//!                └─ a space no adapter is registered for ──► cannot tell, each name ┤
//!                                                                    ArtifactReadings
//!   (space, ContentAddress) ──► the adapter of the space ──► content, or not readable
//! ```
//!
//! | space | adapter | its names | the presets it offers | suggested moves ([`MoveSource`]) |
//! |---|---|---|---|---|
//! | `file` | [`file::FileSpace`] | paths under the root of the project | `file`: `space = "file"`, `compare = "bytes"`, `means = "depends"`; `code`: `space = "file"`, `compare = "presence"`, `means = "depends"` | the same content, then Git's renames ([`file`](mod@file#suggesting-moves)) |
//! | `rust-item` | [`rust_item::RustItemSpace`] | Rust paths (`crate::module::Item`, `Type::member`), looked up in the docs `cargo doc` wrote | `rustdoc`: `space = "rust-item"`, `compare = "presence"`, `means = "depends"` | cannot suggest |
//! | `rust-test` | [`rust_test::RustTestSpace`] | test functions by their file and path in it (`<file>.rs::<module>::<function>`), parsed from the source | `test`: `space = "rust-test"`, `compare = "bytes"`, `means = "depends"` | cannot suggest |
//! | `js-test` | [`js_test::JsTestSpace`] | JavaScript and TypeScript tests by their file and titles (`<file>.ts::<describe>::<test>`), lexed from the source | `js-test`: `space = "js-test"`, `compare = "bytes"`, `means = "depends"` | cannot suggest |
//!
//! The same adapters answer [`MoveSource`]: a space with no adapter cannot suggest.
//!
//! Each adapter offers its presets of links to artifacts for its own space, as
//! values ([`ArtifactSource::presets`]); the domain knows none. A project may write
//! `[defaults] artifact = "file"` so that a link to artifacts that names neither a
//! preset nor a space is one to files.
//!
//! [`ArtifactSource`]: whyshelf_core::app::shelf::ArtifactSource
//! [`MoveSource`]: whyshelf_core::app::shelf::MoveSource
//! [`ArtifactReading`]: whyshelf_core::domain::shelf::artifact::ArtifactReading

pub mod file;
pub mod js_test;
pub mod rust_item;
pub mod rust_test;

use std::collections::{BTreeMap, BTreeSet};

use whyshelf_core::app::shelf::{ArtifactSource, MoveSource};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactAsks, ArtifactKey, ArtifactReading, ArtifactReadings, ContentAddress, Space, SpaceAsks,
    SpaceReadings,
};
use whyshelf_core::domain::shelf::ledger::{GoneName, SpaceSuggestions};
use whyshelf_core::domain::shelf::schema::BuiltinPresets;

use file::{ArtifactError, FileSpace};
use js_test::{JsTestError, JsTestSpace};
use rust_item::{RustItemError, RustItemSpace};
use rust_test::{RustTestError, RustTestSpace};

/// One adapter of a space.
#[derive(Debug, Clone)]
pub enum Adapter {
    /// The files of the project ([`file`](mod@file)).
    File(FileSpace),
    /// The items of the Rust crates of the project ([`rust_item`]).
    RustItem(RustItemSpace),
    /// The test functions of the Rust source of the project ([`rust_test`]).
    RustTest(RustTestSpace),
    /// The tests of the JavaScript and TypeScript source of the project
    /// ([`js_test`]).
    JsTest(JsTestSpace),
}

impl Adapter {
    /// The presets it offers when it resolves the names of `space`.
    fn presets(&self, space: &Space) -> BuiltinPresets {
        match self {
            Self::File(_) => file::presets(space),
            Self::RustItem(_) => rust_item::presets(space),
            Self::RustTest(_) => rust_test::presets(space),
            Self::JsTest(_) => js_test::presets(space),
        }
    }

    async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, SpacesError> {
        match self {
            Self::File(files) => Ok(files.resolve(asks).await?),
            Self::RustItem(items) => Ok(items.resolve(asks).await?),
            Self::RustTest(tests) => Ok(tests.resolve(asks).await?),
            Self::JsTest(tests) => Ok(tests.resolve(asks).await?),
        }
    }

    async fn current(&self, key: &ArtifactKey) -> Result<Option<Vec<u8>>, SpacesError> {
        match self {
            Self::File(files) => Ok(files.current(key.name()).await?),
            Self::RustItem(items) => Ok(items.current(key.name()).await?),
            Self::RustTest(tests) => Ok(tests.current(key.name()).await?),
            Self::JsTest(tests) => Ok(tests.current(key.name()).await?),
        }
    }

    /// What it suggests the gone names of its space moved to: the files look; the
    /// items and the tests of the Rust crates, and the JavaScript tests, cannot
    /// suggest yet.
    async fn suggest(&self, gone: &[GoneName]) -> Result<SpaceSuggestions, SpacesError> {
        match self {
            Self::File(files) => Ok(SpaceSuggestions::Suggested(files.suggest(gone).await?)),
            Self::RustItem(_) | Self::RustTest(_) | Self::JsTest(_) => {
                Ok(SpaceSuggestions::CannotSuggest)
            }
        }
    }

    /// The content at `address`, if this adapter can read it back. An adapter that
    /// keeps nothing by address answers none.
    async fn read_back(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, SpacesError> {
        match self {
            Self::File(files) => Ok(files.read_back(address).await?),
            Self::RustItem(_) | Self::RustTest(_) | Self::JsTest(_) => Ok(None),
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
    /// The domain refused a reason this module gave (a bug if it ever happens: the
    /// reason is never blank).
    #[error("the domain refused a reason of the artifact source")]
    Reason(#[source] ValueError),
}

/// The adapters, by the space each resolves names in.
#[derive(Debug, Clone, Default)]
pub struct ArtifactSpaces {
    adapters: BTreeMap<Space, Adapter>,
}

impl ArtifactSpaces {
    /// No space.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same, with `adapter` resolving the names of `space` (in the place of one
    /// registered for it before).
    pub fn register(mut self, space: Space, adapter: Adapter) -> Self {
        self.adapters.insert(space, adapter);
        self
    }

    /// The spaces of a project at its root: `file`, the files under it,
    /// `rust-item`, the items of its Rust crates, `rust-test`, the test
    /// functions of its Rust source, and `js-test`, the tests of its JavaScript and
    /// TypeScript source.
    pub fn of_project(
        files: FileSpace,
        rust_items: RustItemSpace,
        rust_tests: RustTestSpace,
        js_tests: JsTestSpace,
    ) -> Self {
        let adapters = [
            (file::FILE_SPACE, Adapter::File(files)),
            (rust_item::RUST_ITEM_SPACE, Adapter::RustItem(rust_items)),
            (rust_test::RUST_TEST_SPACE, Adapter::RustTest(rust_tests)),
            (js_test::JS_TEST_SPACE, Adapter::JsTest(js_tests)),
        ];
        adapters
            .into_iter()
            .fold(Self::new(), |spaces, (name, adapter)| {
                match Space::new(name) {
                    Ok(space) => spaces.register(space, adapter),
                    // Not a space: no adapter, so every link that names it is
                    // refused. The test of this module holds each to be one.
                    Err(_) => spaces,
                }
            })
    }

    /// The content of an artifact of `space` at `address`, read back by the adapter
    /// of the space: none when no adapter is registered for it, the adapter cannot
    /// read back, or nothing is behind the address; an error when the adapter could
    /// not ask its store (Git could not be run, or took too long).
    pub async fn read_back(
        &self,
        space: &Space,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, SpacesError> {
        match self.adapters.get(space) {
            Some(adapter) => adapter.read_back(address).await,
            None => Ok(None),
        }
    }
}

impl ArtifactSource for ArtifactSpaces {
    type Error = SpacesError;

    fn spaces(&self) -> BTreeSet<Space> {
        self.adapters.keys().cloned().collect()
    }

    /// The presets of every adapter, each for the space it is registered for. Of two
    /// adapters that offer a preset of one name, the one of the later space, in the
    /// order of the names of the spaces, is kept.
    fn presets(&self) -> BuiltinPresets {
        self.adapters
            .iter()
            .flat_map(|(space, adapter)| adapter.presets(space))
            .collect()
    }

    async fn resolve(&self, asks: &ArtifactAsks) -> Result<ArtifactReadings, SpacesError> {
        let mut readings = ArtifactReadings::new();
        for (space, names) in asks.spaces() {
            let found = match self.adapters.get(space) {
                Some(adapter) => adapter.resolve(names).await?,
                None => {
                    let why = NonBlank::new(format!("no adapter offers the space `{space}`"))
                        .map_err(SpacesError::Reason)?;
                    names
                        .keys()
                        .map(|name| (name.clone(), ArtifactReading::cannot_tell(why.clone())))
                        .collect()
                }
            };
            readings.extend_space(space.clone(), found);
        }
        Ok(readings)
    }

    async fn current(&self, artifact: &ArtifactKey) -> Result<Option<Vec<u8>>, SpacesError> {
        match self.adapters.get(artifact.space()) {
            Some(adapter) => adapter.current(artifact).await,
            None => Ok(None),
        }
    }
}

/// Suggested moves, by the adapter of each space: a space with no adapter, or whose
/// adapter has no way to look, cannot suggest.
impl MoveSource for ArtifactSpaces {
    type Error = SpacesError;

    async fn suggest(
        &self,
        space: &Space,
        gone: &[GoneName],
    ) -> Result<SpaceSuggestions, SpacesError> {
        match self.adapters.get(space) {
            Some(adapter) => adapter.suggest(gone).await,
            None => Ok(SpaceSuggestions::CannotSuggest),
        }
    }
}

#[cfg(test)]
mod tests {
    use whyshelf_core::domain::shelf::artifact::{ArtifactName, ArtifactState, Comparison};

    use super::*;

    fn space(text: &str) -> Space {
        Space::new(text).unwrap()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn of_project(root: &std::path::Path) -> ArtifactSpaces {
        ArtifactSpaces::of_project(
            FileSpace::new(root.to_path_buf()),
            RustItemSpace::new(root.to_path_buf(), None),
            RustTestSpace::new(root.to_path_buf()),
            JsTestSpace::new(root.to_path_buf()),
        )
    }

    #[tokio::test]
    async fn each_space_is_resolved_by_its_adapter_and_one_with_none_cannot_be_told() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.rs"), "one\n")
            .await
            .unwrap();
        let source = of_project(dir.path());
        assert_eq!(
            source.spaces(),
            BTreeSet::from([
                space("file"),
                space("js-test"),
                space("rust-item"),
                space("rust-test")
            ])
        );
        let mut presets = file::presets(&space("file"));
        presets.extend(rust_item::presets(&space("rust-item")));
        presets.extend(rust_test::presets(&space("rust-test")));
        presets.extend(js_test::presets(&space("js-test")));
        assert_eq!(source.presets(), presets);
        assert_eq!(presets.len(), 5);

        let mut asks = ArtifactAsks::new();
        asks.ask(space("file"), name("a.rs"), Comparison::Bytes);
        asks.ask(space("other"), name("a.rs"), Comparison::Bytes);
        let found = source.resolve(&asks).await.unwrap();
        assert!(
            found
                .get(&space("file"), &name("a.rs"))
                .is_some_and(ArtifactReading::is_present)
        );
        // The same name in another space is another artifact, and that space has no
        // adapter here.
        let other = found.get(&space("other"), &name("a.rs")).unwrap();
        assert!(matches!(
            other.state(&Comparison::Bytes),
            ArtifactState::Unknown(why) if why.as_str().contains("`other`")
        ));
        let key = ArtifactKey::new(space("file"), name("a.rs"));
        assert_eq!(
            source.current(&key).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
        let elsewhere = ArtifactKey::new(space("other"), name("a.rs"));
        assert_eq!(source.current(&elsewhere).await.unwrap(), None);
        // Read back by the space's adapter; a space with none reads nothing back.
        let address = ContentAddress::from_validated("a".repeat(40)).unwrap();
        assert_eq!(
            source.read_back(&space("other"), &address).await.unwrap(),
            None
        );
    }

    #[test]
    fn a_record_written_before_spaces_names_a_space_the_files_answer_for() {
        let source = of_project(std::path::Path::new("."));
        let old = Space::new(whyshelf_core::domain::shelf::ledger::PATH_ONLY_SPACE).unwrap();
        assert!(source.spaces().contains(&old));
    }
}
