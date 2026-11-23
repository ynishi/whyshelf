//! Reading the declaration: the text of the file, and the declaration a text holds.

use std::io;

use whyshelf_core::app::shelf::{BookSchemaSource, DeclarationText};
use whyshelf_core::domain::shelf::schema::ShelfDecl;

use super::{BooksError, BooksToml, parse};

impl BookSchemaSource for BooksToml {
    type Error = BooksError;

    fn declarations_in(&self, text: &DeclarationText) -> Result<ShelfDecl, BooksError> {
        parse(text.as_str()).map_err(|source| BooksError::Shape {
            path: self.path.clone(),
            source,
        })
    }

    async fn declarations(&self) -> Result<ShelfDecl, BooksError> {
        let text = match tokio::fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(BooksError::Missing {
                    path: self.path.clone(),
                });
            }
            Err(source) => {
                return Err(BooksError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        self.declarations_in(&DeclarationText::new(text))
    }
}
