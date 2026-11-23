//! Reading the declaration: the text of the file, and the declaration a text holds.
//!
//! The file is read through the state of the project at the side asked, every link on
//! its path followed as the system follows one, out of the root too (`read_allowed`).
//! Nothing there (a broken link too) is [`BooksError::Missing`]; a directory there, a
//! text that is not UTF-8, a link the system cannot resolve (a loop) and a file that
//! cannot be read are [`BooksError::Io`]; a kind of file that is neither (a pipe, a
//! socket, a device) is [`BooksError::Untold`].

use std::io;

use whyshelf_core::app::shelf::{BookSchemaSource, DeclarationText, Side};
use whyshelf_core::domain::shelf::schema::ShelfDecl;

use super::super::state::paths::{Failure, Read, read_allowed};
use super::{BooksError, BooksToml, parse};

impl BookSchemaSource for BooksToml {
    type Error = BooksError;

    fn declarations_in(&self, text: &DeclarationText) -> Result<ShelfDecl, BooksError> {
        parse(text.as_str()).map_err(|source| BooksError::Shape {
            path: self.path.clone(),
            source,
        })
    }

    async fn declarations(&self, side: Side) -> Result<ShelfDecl, BooksError> {
        let path = self.state.path_of(side, &self.file)?;
        let io = |source| BooksError::Io {
            path: path.clone(),
            source,
        };
        let bytes = match read_allowed(&self.state, side, &self.file).await {
            Read::Bytes(bytes) => bytes,
            Read::Absent => return Err(BooksError::Missing { path }),
            Read::Dir => return Err(io(io::ErrorKind::IsADirectory.into())),
            Read::Untold(why) => return Err(BooksError::Untold { path, why }),
            Read::Failed(Failure::Io(source)) => return Err(io(source)),
            Read::Failed(Failure::State(error)) => return Err(BooksError::State(error)),
        };
        let text = String::from_utf8(bytes).map_err(|_| {
            io(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            ))
        })?;
        self.declarations_in(&DeclarationText::new(text))
    }
}
