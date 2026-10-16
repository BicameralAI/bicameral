//! In-memory event-store adapter (testing only).

use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventStoreError, SubstrateCapability, SubstrateHealth,
    SubstrateKind,
};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::atomic_event_store::{AtomicEventBatchV1, AtomicEventStoreAdapter};

/// In-memory event store for testing.
pub struct MemoryAdapter {
    entries: Arc<Mutex<Vec<EventStoreEntry>>>,
}

impl MemoryAdapter {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for MemoryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStoreAdapter for MemoryAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        self.entries
            .lock()
            .map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?
            .push(entry);
        Ok(())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let entries = self
            .entries
            .lock()
            .map_err(|e| EventStoreError::Io {
                reason: e.to_string(),
            })?
            .clone();
        Ok(entries)
    }

    fn substrate_kind(&self) -> SubstrateKind {
        SubstrateKind::Memory
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        let entry_count = self.entries.lock().map(|e| e.len()).unwrap_or(0);
        Ok(SubstrateHealth {
            kind: SubstrateKind::Memory,
            reachable: true,
            last_synced: None,
            entry_count,
            freshness: None,
            warnings: vec!["In-memory adapter — not persistent".to_string()],
            capabilities: self.capabilities(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::new()
    }
}

impl AtomicEventStoreAdapter for MemoryAdapter {
    fn append_atomic_batch(&self, batch: AtomicEventBatchV1) -> Result<(), EventStoreError> {
        batch.validate()?;
        let mut entries = self.entries.lock().map_err(|error| EventStoreError::Io {
            reason: error.to_string(),
        })?;
        entries.extend(batch.entries);
        Ok(())
    }
}
