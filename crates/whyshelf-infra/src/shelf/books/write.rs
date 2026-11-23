//! Writing the declaration, once, where none is, when a project is started.

use whyshelf_core::app::shelf::{Creation, DeclarationText, DeclarationWrite};
use whyshelf_core::domain::location::RelPath;

use super::super::create;
use super::{BooksError, BooksToml};

/// The file is created only where nothing is (`create_new`), with its directory, and
/// the text is written as it is, comments and all; a write that fails removes the
/// file it made, and only it.
impl DeclarationWrite for BooksToml {
    type Error = BooksError;

    fn file(&self) -> RelPath {
        self.file.clone()
    }

    async fn in_the_way(&self) -> Result<Option<RelPath>, BooksError> {
        create::in_the_way(&self.root, &self.file)
            .await
            .map_err(|source| BooksError::Io {
                path: self.path.clone(),
                source,
            })
    }

    async fn create(&self, text: &DeclarationText) -> Result<Creation, BooksError> {
        create::create_new(&self.path, text.as_str().as_bytes())
            .await
            .map_err(|failed| BooksError::Write {
                step: failed.step,
                source: failed.source,
            })
    }
}
