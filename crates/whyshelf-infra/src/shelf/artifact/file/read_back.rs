//! Reading back the content a file had, by the address an approval kept.

use whyshelf_core::domain::shelf::artifact::ContentAddress;

use super::{ArtifactError, FileSpace};

impl FileSpace {
    /// The content a file had at `address`, or none when it cannot be read back: Git
    /// holds no such blob (the file was never added), or it is larger than the most
    /// bytes of a file a reading reads.
    pub async fn read_back(
        &self,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, ArtifactError> {
        let cap = usize::try_from(self.max_file).unwrap_or(usize::MAX);
        Ok(self.git.read_blob_within(address, cap).await?)
    }
}
