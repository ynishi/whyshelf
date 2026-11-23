//! Reading back the content a document's file had, by the address an approval kept.

use whyshelf_core::domain::shelf::artifact::ContentAddress;

use super::{DocsError, MarkdownDocs};

impl MarkdownDocs {
    /// The content a document's file had at `address`, or none when it cannot be
    /// read back; an error when Git could not be run or took too long.
    pub async fn read_back(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, DocsError> {
        Ok(self.git.read_blob(address).await?)
    }
}
