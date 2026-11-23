//! Writing a new file of the project, for `init`.

use whyshelf_core::app::shelf::{Creation, FileWrite};
use whyshelf_core::domain::location::RelPath;

use super::super::super::create;
use super::{FileSpace, WriteError};

/// A new file of the project, by its path under the root, written whole where
/// nothing is (`create_new`, with its directories), as the declaration and a new
/// document are; a write that fails removes the file it made, and only it.
impl FileWrite for FileSpace {
    type Error = WriteError;

    async fn in_the_way(&self, path: &RelPath) -> Result<Option<RelPath>, WriteError> {
        create::in_the_way(&self.root, path)
            .await
            .map_err(|source| WriteError::Io {
                path: self.root.join(path.as_str()),
                source,
            })
    }

    async fn create(&self, path: &RelPath, bytes: &[u8]) -> Result<Creation, WriteError> {
        create::create_new(&self.root.join(path.as_str()), bytes)
            .await
            .map_err(|failed| WriteError::Write {
                step: failed.step,
                source: failed.source,
            })
    }
}
