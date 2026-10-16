//! Atomic event-store batch contract for confirmed CandidateSet selections.
//!
//! A selection may promote several distinct candidates, and owner-policy
//! approval may travel in the same transaction. The event store must expose one
//! all-or-none materialization boundary. Sequential calls to `append` are not a
//! valid fallback because they can leave partially canonical Product state.

use std::collections::BTreeSet;

use bicameral_api::event_store::{EventStoreAdapter, EventStoreEntry, EventStoreError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ATOMIC_EVENT_BATCH_VERSION: u32 = 1;

/// One atomically materialized collection of otherwise distinct replay events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicEventBatchV1 {
    pub version: u32,
    pub batch_id: Uuid,
    pub entries: Vec<EventStoreEntry>,
}

impl AtomicEventBatchV1 {
    pub fn new(batch_id: Uuid, entries: Vec<EventStoreEntry>) -> Result<Self, EventStoreError> {
        if entries.is_empty() {
            return Err(EventStoreError::Conflict {
                reason: "atomic event batch must contain at least one entry".to_string(),
            });
        }

        let ids: BTreeSet<Uuid> = entries.iter().map(|entry| entry.id).collect();
        if ids.len() != entries.len() {
            return Err(EventStoreError::Conflict {
                reason: "atomic event batch contains duplicate event ids".to_string(),
            });
        }

        Ok(Self {
            version: ATOMIC_EVENT_BATCH_VERSION,
            batch_id,
            entries,
        })
    }

    pub fn validate(&self) -> Result<(), EventStoreError> {
        if self.version != ATOMIC_EVENT_BATCH_VERSION {
            return Err(EventStoreError::Conflict {
                reason: format!("unsupported atomic event batch version: {}", self.version),
            });
        }
        Self::new(self.batch_id, self.entries.clone()).map(|_| ())
    }
}

/// Event-store capability required by `candidate_selection.commit`.
///
/// Implementations must either materialize every entry in `batch` or
/// materialize none. They must never fall back to repeated `append` calls.
pub trait AtomicEventStoreAdapter: EventStoreAdapter {
    fn append_atomic_batch(&self, batch: AtomicEventBatchV1) -> Result<(), EventStoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::event_store::EventType;

    #[test]
    fn empty_batch_is_rejected() {
        let error = AtomicEventBatchV1::new(Uuid::from_u128(1), Vec::new()).unwrap_err();
        assert!(error.to_string().contains("at least one"));
    }

    #[test]
    fn duplicate_event_ids_are_rejected() {
        let mut first = EventStoreEntry::new(
            EventType::AcceptCandidate,
            serde_json::json!({"candidate_id": Uuid::from_u128(2)}),
            "actor".to_string(),
        );
        first.id = Uuid::from_u128(3);
        let second = first.clone();
        let error = AtomicEventBatchV1::new(Uuid::from_u128(4), vec![first, second]).unwrap_err();
        assert!(error.to_string().contains("duplicate event ids"));
    }
}
