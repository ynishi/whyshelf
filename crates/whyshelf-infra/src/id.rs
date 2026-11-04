//! New entity identifiers: [`IdPort`] over UUID version 7.
//!
//! Why v7: it sorts roughly by creation time, so listings are stable, and it is
//! the surrogate key the architecture baseline asks of a persisted entity. The
//! domain never makes one (no clock, no randomness), so it is made here.

use uuid::Uuid;
use whyshelf_core::app::ports::IdPort;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::EntityId;

/// Why an identifier could not be made.
#[derive(Debug, thiserror::Error)]
#[error("the domain refused a freshly made UUID v7")]
pub struct IdError(#[source] ValueError);

/// Makes UUID v7 identifiers from the system clock and the system's randomness.
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidV7Ids;

impl IdPort for UuidV7Ids {
    type Error = IdError;

    async fn new_entity_id(&self) -> Result<EntityId, IdError> {
        EntityId::new(Uuid::now_v7()).map_err(IdError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn identifiers_are_v7_and_distinct() {
        let a = UuidV7Ids.new_entity_id().await.unwrap();
        let b = UuidV7Ids.new_entity_id().await.unwrap();
        assert_eq!(a.as_uuid().get_version_num(), 7);
        assert_ne!(a, b);
    }
}
