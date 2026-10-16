//! Google Drive API event-store adapter core (Bot #1086).
//!
//! This module preserves the existing Drive event representation while keeping
//! provider networking/credential ownership behind an injected transport.
//! `DriveApiAdapter` itself is synchronous because `EventStoreAdapter` is
//! synchronous; the daemon/provider boundary may implement this transport with
//! whatever async/network machinery it owns.
//!
//! Production success is intentionally stronger than the local synced-folder
//! adapter: an append is not confirmed until the exact provider object is read
//! back and its bytes match the canonical serialized event.

use bicameral_api::event_store::{
    event_store_entry_digest, serialize_event_store_entry, EventStoreAdapter, EventStoreEntry,
    EventStoreError, FreshnessState, SubstrateCapability, SubstrateHealth, SubstrateKind,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::atomic_event_store::AtomicEventBatchV1;

/// Safe metadata for one Google Drive object.
///
/// Provider file ids/revisions are identifiers, not credentials. File content
/// is fetched separately so callers do not accidentally log event bytes while
/// inspecting list results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRemoteObject {
    pub file_id: String,
    pub name: String,
    pub version: Option<String>,
}

/// Provider transport error classes kept separate from canonical event-store
/// authority. `safe_message` must never contain bearer/token/auth-code values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveTransportErrorKind {
    Authentication,
    Unavailable,
    NotFound,
    Conflict,
    InvalidResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveTransportError {
    pub kind: DriveTransportErrorKind,
    pub safe_message: String,
}

impl DriveTransportError {
    pub fn new(kind: DriveTransportErrorKind, safe_message: impl Into<String>) -> Self {
        Self {
            kind,
            safe_message: safe_message.into(),
        }
    }
}

/// Network/provider seam used by the Drive event-store adapter.
///
/// A production implementation must scope every operation to the validated
/// folder and use Google Drive's shared-drive support flags where applicable.
/// This trait deliberately accepts no raw token; credential ownership belongs
/// at the provider transport / daemon connection boundary.
pub trait DriveApiTransport: Send + Sync {
    /// Find direct children in `folder_id` with the exact display name.
    fn find_children_by_name(
        &self,
        folder_id: &str,
        name: &str,
    ) -> Result<Vec<DriveRemoteObject>, DriveTransportError>;

    /// Create one JSON object as a direct child of `folder_id`.
    fn create_json_file(
        &self,
        folder_id: &str,
        name: &str,
        body: &[u8],
    ) -> Result<DriveRemoteObject, DriveTransportError>;

    /// Download the exact bytes for one provider object id.
    fn download_file(&self, file_id: &str) -> Result<Vec<u8>, DriveTransportError>;

    /// List direct children of the validated event-store folder.
    fn list_children(&self, folder_id: &str)
        -> Result<Vec<DriveRemoteObject>, DriveTransportError>;

    /// Verify that the selected folder and current credential are reachable.
    fn probe_folder(&self, folder_id: &str) -> Result<(), DriveTransportError>;
}

/// Exact provider confirmation witness produced only after remote readback.
///
/// This is supporting provider evidence for later ProductCreationCommitProof
/// minting. It is not canonical Product state and carries no credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveAppendConfirmation {
    pub provider_file_id: String,
    pub provider_version: Option<String>,
    pub resource_fingerprint: String,
    pub event_digest: String,
    pub confirmed_at: DateTime<Utc>,
    /// False when the exact provider object already existed and was verified.
    pub created: bool,
}

pub struct DriveApiAdapter {
    folder_id: String,
    transport: Arc<dyn DriveApiTransport>,
}

impl DriveApiAdapter {
    pub fn new(
        folder_id: impl Into<String>,
        transport: Arc<dyn DriveApiTransport>,
    ) -> Result<Self, EventStoreError> {
        let folder_id = folder_id.into();
        if folder_id.trim().is_empty() {
            return Err(EventStoreError::Unreachable {
                reason: "validated Google Drive folder id is required".to_string(),
            });
        }
        Ok(Self {
            folder_id,
            transport,
        })
    }

    /// Append one event and return exact remote confirmation.
    ///
    /// Idempotency is keyed by the canonical event UUID filename. An existing
    /// exact object is accepted only when its bytes exactly match the canonical
    /// serialization. Multiple same-name objects or mismatched bytes are hard
    /// conflicts; this adapter never overwrites them.
    pub fn append_confirmed(
        &self,
        entry: &EventStoreEntry,
    ) -> Result<DriveAppendConfirmation, EventStoreError> {
        let name = format!("{}.json", entry.id);
        let canonical_bytes = serialize_event(entry)?;
        let event_digest = event_store_entry_digest(entry)
            .map_err(|error| EventStoreError::Serialization {
                reason: error.to_string(),
            })?
            .strip_prefix("sha256:")
            .unwrap_or("")
            .to_string();
        let resource_fingerprint =
            sha256_hex(format!("google_drive_folder:{}", self.folder_id).as_bytes());

        let existing = self
            .transport
            .find_children_by_name(&self.folder_id, &name)
            .map_err(map_transport_error)?;

        match existing.as_slice() {
            [] => {
                let created = self
                    .transport
                    .create_json_file(&self.folder_id, &name, &canonical_bytes)
                    .map_err(map_transport_error)?;
                if created.name != name {
                    return Err(EventStoreError::Conflict {
                        reason: "provider created an object with an unexpected event name"
                            .to_string(),
                    });
                }
                self.confirm_exact_object(
                    created,
                    &canonical_bytes,
                    event_digest,
                    resource_fingerprint,
                    true,
                )
            }
            [object] => self.confirm_exact_object(
                object.clone(),
                &canonical_bytes,
                event_digest,
                resource_fingerprint,
                false,
            ),
            _ => Err(EventStoreError::Conflict {
                reason: format!(
                    "multiple provider objects exist for canonical event {}",
                    entry.id
                ),
            }),
        }
    }

    fn confirm_exact_object(
        &self,
        object: DriveRemoteObject,
        canonical_bytes: &[u8],
        event_digest: String,
        resource_fingerprint: String,
        created: bool,
    ) -> Result<DriveAppendConfirmation, EventStoreError> {
        let remote_bytes = self
            .transport
            .download_file(&object.file_id)
            .map_err(map_transport_error)?;
        if remote_bytes != canonical_bytes {
            return Err(EventStoreError::Conflict {
                reason: "provider readback did not match canonical event bytes".to_string(),
            });
        }

        // Parse after the byte comparison as an additional integrity guard.
        // Exact-byte equality remains the provider-confirmation contract.
        serde_json::from_slice::<EventStoreEntry>(&remote_bytes).map_err(|error| {
            EventStoreError::Serialization {
                reason: format!("provider readback was not a valid event: {error}"),
            }
        })?;

        Ok(DriveAppendConfirmation {
            provider_file_id: object.file_id,
            provider_version: object.version,
            resource_fingerprint,
            event_digest,
            confirmed_at: Utc::now(),
            created,
        })
    }
}

/// Append with exact provider readback, keeping the confirmation witness.
///
/// The production create path (bot#1087) requires this seam so the
/// `CreateProduct` append yields the provider facts a
/// `ProductCreationCommitProof` binds. The plain [`EventStoreAdapter::append`]
/// wrapper discards that witness; only provider-backed adapters implement this.
pub trait ConfirmedAppend: Send + Sync {
    fn append_confirmed_entry(
        &self,
        entry: &EventStoreEntry,
    ) -> Result<DriveAppendConfirmation, EventStoreError>;
}

impl ConfirmedAppend for DriveApiAdapter {
    fn append_confirmed_entry(
        &self,
        entry: &EventStoreEntry,
    ) -> Result<DriveAppendConfirmation, EventStoreError> {
        self.append_confirmed(entry)
    }
}

impl EventStoreAdapter for DriveApiAdapter {
    fn append(&self, entry: EventStoreEntry) -> Result<(), EventStoreError> {
        self.append_confirmed(&entry).map(|_| ())
    }

    fn replay(&self) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let objects = self
            .transport
            .list_children(&self.folder_id)
            .map_err(map_transport_error)?;
        let mut entries = Vec::new();
        let mut skipped = 0usize;

        for object in objects {
            if !object.name.ends_with(".json") || object.name.starts_with(".tmp-") {
                continue;
            }

            let bytes = self
                .transport
                .download_file(&object.file_id)
                .map_err(map_transport_error)?;

            if let Ok(batch) = serde_json::from_slice::<AtomicEventBatchV1>(&bytes) {
                batch.validate()?;
                entries.extend(batch.entries);
                continue;
            }

            match serde_json::from_slice::<EventStoreEntry>(&bytes) {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    tracing::warn!(
                        provider_file_id = %object.file_id,
                        error = %error,
                        "Skipping non-event JSON provider object during replay"
                    );
                    skipped += 1;
                }
            }
        }

        if skipped > 0 {
            tracing::info!(skipped, "Skipped non-event provider objects during replay");
        }

        entries.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(entries)
    }

    fn substrate_kind(&self) -> SubstrateKind {
        // The frozen wire contract identifies Google event storage as
        // drive_folder. Provider-vs-local confirmation is carried separately.
        SubstrateKind::DriveFolder
    }

    fn health_check(&self) -> Result<SubstrateHealth, EventStoreError> {
        self.transport
            .probe_folder(&self.folder_id)
            .map_err(map_transport_error)?;
        let objects = self
            .transport
            .list_children(&self.folder_id)
            .map_err(map_transport_error)?;
        let entry_count = objects
            .iter()
            .filter(|object| object.name.ends_with(".json") && !object.name.starts_with(".tmp-"))
            .count();

        Ok(SubstrateHealth {
            kind: SubstrateKind::DriveFolder,
            reachable: true,
            last_synced: None,
            entry_count,
            // This adapter queries the provider directly; Fresh means the
            // health/list requests succeeded at check time, not that a local
            // sync marker exists.
            freshness: Some(FreshnessState::Fresh),
            warnings: Vec::new(),
            capabilities: self.capabilities(),
        })
    }

    fn capabilities(&self) -> BTreeSet<SubstrateCapability> {
        BTreeSet::from([SubstrateCapability::DurableReplay])
    }
}

fn serialize_event(entry: &EventStoreEntry) -> Result<Vec<u8>, EventStoreError> {
    serialize_event_store_entry(entry).map_err(|error| EventStoreError::Serialization {
        reason: error.to_string(),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn map_transport_error(error: DriveTransportError) -> EventStoreError {
    match error.kind {
        DriveTransportErrorKind::Conflict => EventStoreError::Conflict {
            reason: error.safe_message,
        },
        DriveTransportErrorKind::Authentication
        | DriveTransportErrorKind::Unavailable
        | DriveTransportErrorKind::NotFound => EventStoreError::Unreachable {
            reason: error.safe_message,
        },
        DriveTransportErrorKind::InvalidResponse => EventStoreError::Io {
            reason: error.safe_message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bicameral_api::event_store::{EventType, SubstrateCapability};
    use chrono::TimeZone;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Clone)]
    struct FakeStoredObject {
        meta: DriveRemoteObject,
        body: Vec<u8>,
    }

    #[derive(Default)]
    struct FakeDriveTransport {
        objects: Mutex<BTreeMap<String, FakeStoredObject>>,
        create_count: Mutex<u64>,
        fail_probe: Mutex<bool>,
    }

    impl FakeDriveTransport {
        fn insert_named(&self, file_id: &str, name: &str, body: Vec<u8>) {
            self.objects.lock().unwrap().insert(
                file_id.to_string(),
                FakeStoredObject {
                    meta: DriveRemoteObject {
                        file_id: file_id.to_string(),
                        name: name.to_string(),
                        version: Some("1".to_string()),
                    },
                    body,
                },
            );
        }

        fn len(&self) -> usize {
            self.objects.lock().unwrap().len()
        }
    }

    impl DriveApiTransport for FakeDriveTransport {
        fn find_children_by_name(
            &self,
            _folder_id: &str,
            name: &str,
        ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .values()
                .filter(|object| object.meta.name == name)
                .map(|object| object.meta.clone())
                .collect())
        }

        fn create_json_file(
            &self,
            _folder_id: &str,
            name: &str,
            body: &[u8],
        ) -> Result<DriveRemoteObject, DriveTransportError> {
            let mut count = self.create_count.lock().unwrap();
            *count += 1;
            let file_id = format!("provider-file-{count}");
            let meta = DriveRemoteObject {
                file_id: file_id.clone(),
                name: name.to_string(),
                version: Some(count.to_string()),
            };
            self.objects.lock().unwrap().insert(
                file_id,
                FakeStoredObject {
                    meta: meta.clone(),
                    body: body.to_vec(),
                },
            );
            Ok(meta)
        }

        fn download_file(&self, file_id: &str) -> Result<Vec<u8>, DriveTransportError> {
            self.objects
                .lock()
                .unwrap()
                .get(file_id)
                .map(|object| object.body.clone())
                .ok_or_else(|| {
                    DriveTransportError::new(
                        DriveTransportErrorKind::NotFound,
                        "provider object not found",
                    )
                })
        }

        fn list_children(
            &self,
            _folder_id: &str,
        ) -> Result<Vec<DriveRemoteObject>, DriveTransportError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .values()
                .map(|object| object.meta.clone())
                .collect())
        }

        fn probe_folder(&self, _folder_id: &str) -> Result<(), DriveTransportError> {
            if *self.fail_probe.lock().unwrap() {
                return Err(DriveTransportError::new(
                    DriveTransportErrorKind::Authentication,
                    "provider credential unavailable",
                ));
            }
            Ok(())
        }
    }

    fn event(id: &str, second: u32) -> EventStoreEntry {
        EventStoreEntry {
            id: Uuid::parse_str(id).unwrap(),
            event_type: EventType::CreateProduct,
            payload: serde_json::json!({"product_id": id}),
            timestamp: Utc.with_ymd_and_hms(2026, 8, 10, 20, 0, second).unwrap(),
            actor: "acct_test".to_string(),
            governance_result_id: None,
        }
    }

    #[test]
    fn append_requires_exact_provider_readback_and_returns_safe_confirmation() {
        let transport = Arc::new(FakeDriveTransport::default());
        let adapter = DriveApiAdapter::new("folder-alpha", transport.clone()).unwrap();
        let entry = event("00000000-0000-0000-0000-000000000001", 1);

        let confirmation = adapter.append_confirmed(&entry).unwrap();

        assert!(confirmation.created);
        assert_eq!(confirmation.provider_file_id, "provider-file-1");
        assert_eq!(confirmation.provider_version.as_deref(), Some("1"));
        assert_eq!(confirmation.event_digest.len(), 64);
        assert_eq!(confirmation.resource_fingerprint.len(), 64);
        assert!(!confirmation.resource_fingerprint.contains("folder-alpha"));
        assert_eq!(transport.len(), 1);
    }

    #[test]
    fn retry_of_same_exact_event_is_idempotent() {
        let transport = Arc::new(FakeDriveTransport::default());
        let adapter = DriveApiAdapter::new("folder-alpha", transport.clone()).unwrap();
        let entry = event("00000000-0000-0000-0000-000000000002", 2);

        let first = adapter.append_confirmed(&entry).unwrap();
        let second = adapter.append_confirmed(&entry).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.provider_file_id, second.provider_file_id);
        assert_eq!(first.event_digest, second.event_digest);
        assert_eq!(transport.len(), 1);
    }

    #[test]
    fn same_event_id_with_different_content_is_conflict() {
        let transport = Arc::new(FakeDriveTransport::default());
        let adapter = DriveApiAdapter::new("folder-alpha", transport.clone()).unwrap();
        let first = event("00000000-0000-0000-0000-000000000003", 3);
        adapter.append_confirmed(&first).unwrap();

        let mut conflicting = first.clone();
        conflicting.payload = serde_json::json!({"product_id": "different"});
        let error = adapter.append_confirmed(&conflicting).unwrap_err();

        assert!(matches!(error, EventStoreError::Conflict { .. }));
        assert_eq!(transport.len(), 1);
    }

    #[test]
    fn duplicate_provider_objects_for_one_event_are_conflict() {
        let transport = Arc::new(FakeDriveTransport::default());
        let entry = event("00000000-0000-0000-0000-000000000004", 4);
        let bytes = serialize_event(&entry).unwrap();
        let name = format!("{}.json", entry.id);
        transport.insert_named("duplicate-a", &name, bytes.clone());
        transport.insert_named("duplicate-b", &name, bytes);
        let adapter = DriveApiAdapter::new("folder-alpha", transport).unwrap();

        let error = adapter.append_confirmed(&entry).unwrap_err();
        assert!(matches!(error, EventStoreError::Conflict { .. }));
    }

    #[test]
    fn replay_skips_non_event_json_and_orders_deterministically() {
        let transport = Arc::new(FakeDriveTransport::default());
        let earlier = event("00000000-0000-0000-0000-000000000010", 1);
        let later = event("00000000-0000-0000-0000-000000000009", 2);
        transport.insert_named(
            "later",
            &format!("{}.json", later.id),
            serialize_event(&later).unwrap(),
        );
        transport.insert_named(
            "earlier",
            &format!("{}.json", earlier.id),
            serialize_event(&earlier).unwrap(),
        );
        transport.insert_named("notes", "notes.json", br#"{"not":"an event"}"#.to_vec());
        transport.insert_named("text", "readme.txt", b"ignore me".to_vec());
        let adapter = DriveApiAdapter::new("folder-alpha", transport).unwrap();

        let replayed = adapter.replay().unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].id, earlier.id);
        assert_eq!(replayed[1].id, later.id);
    }

    #[test]
    fn health_is_fail_closed_on_provider_auth_failure() {
        let transport = Arc::new(FakeDriveTransport::default());
        *transport.fail_probe.lock().unwrap() = true;
        let adapter = DriveApiAdapter::new("folder-alpha", transport).unwrap();

        let error = adapter.health_check().unwrap_err();
        assert!(matches!(error, EventStoreError::Unreachable { .. }));
    }

    #[test]
    fn provider_adapter_advertises_durable_replay_only() {
        let transport = Arc::new(FakeDriveTransport::default());
        let adapter = DriveApiAdapter::new("folder-alpha", transport).unwrap();
        let capabilities = adapter.capabilities();
        assert_eq!(capabilities.len(), 1);
        assert!(capabilities.contains(&SubstrateCapability::DurableReplay));
    }
}
