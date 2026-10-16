//! Provider resource-selection contract (#1085).
//!
//! Daemon-owned Google Drive folder selection and validation for Product
//! creation/setup. Resource selection is a bounded creation-attempt
//! operation that reuses the provider Connection credential boundary; it
//! is deliberately separate from operator-global `ConnectionRegistry`
//! state and from canonical Product creation.
//!
//! Every type here is a redacted daemon-authored projection. No access
//! token, refresh token, authorization code, client secret, account PII,
//! raw Drive content, or arbitrary child listing may appear in these
//! shapes. Browser-local folder IDs or fixture rows can never satisfy
//! `ready`.

use serde::{Deserialize, Serialize};

/// Daemon/provider-authored resource readiness state.
///
/// Mirrors the established provider-setup vocabulary. `Ready` is only
/// reachable through daemon-side provider validation of an explicitly
/// selected resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderResourceState {
    Ready,
    PendingSelection,
    NotReady,
    ActionNeeded,
    Unsupported,
}

/// Redacted safe facts about a validated provider resource.
///
/// Suitable for mapping directly to
/// `ProductEventStoreDescriptor::Google { folder_id }` after validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderResourceDescriptor {
    /// Provider id, e.g. `google`.
    pub provider: String,
    /// Resource kind, e.g. `drive_folder`.
    pub resource_kind: String,
    /// Provider resource id (Drive folder id).
    pub folder_id: String,
    /// Display name of the validated folder.
    pub folder_display_name: String,
    /// Shared-drive id when the folder lives in a shared drive. Safe
    /// derived metadata; `None` for My Drive folders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    /// Validated MIME type (always the Drive folder MIME type when
    /// readiness is `ready`).
    pub mime_type: String,
    /// Whether the current credential can create children in the folder.
    pub can_add_children: bool,
    /// Whether the current credential can list children, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_list_children: Option<bool>,
}

/// Typed outcome for starting Google resource selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ResourceSelectionStartOutcome {
    /// System-browser Picker/OAuth URL for the user to open.
    StartUrl {
        provider: String,
        url: String,
        /// Opaque daemon-authored selection-session id used for callback
        /// binding and polling. Not connection auth-session state.
        selection_session_id: String,
    },
    /// Selection cannot start until the operator acts (e.g. provider
    /// connection/config missing).
    ActionNeeded { provider: String, message: String },
    /// Provider or resource kind not supported for selection.
    Unsupported { provider: String, message: String },
}

/// Callback body for the Picker/OAuth redirect return.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceSelectionCallbackRequest {
    /// The `selection_session_id` returned by start.
    pub selection_session_id: String,
    /// Authorization code from the redirect (consumed server-side, never
    /// stored or echoed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// OAuth `state` parameter for session/CSRF binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Picker-selected file ids from the redirect. Exactly one folder id
    /// is required for readiness.
    #[serde(default)]
    pub picked_file_ids: Vec<String>,
    /// Provider error code from the redirect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Daemon-authored projection of one resource-selection attempt.
///
/// Returned by the callback and by polling. Carries only safe facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceSelectionProjection {
    pub provider: String,
    pub state: ProviderResourceState,
    /// Operator-safe message describing the state.
    pub message: String,
    /// Present only when `state == ready`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ProviderResourceDescriptor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_state_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ProviderResourceState::PendingSelection).unwrap(),
            "\"pending_selection\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderResourceState::ActionNeeded).unwrap(),
            "\"action_needed\""
        );
    }

    #[test]
    fn projection_round_trips_and_hides_absent_fields() {
        let projection = ResourceSelectionProjection {
            provider: "google".into(),
            state: ProviderResourceState::Ready,
            message: "Folder validated.".into(),
            resource: Some(ProviderResourceDescriptor {
                provider: "google".into(),
                resource_kind: "drive_folder".into(),
                folder_id: "folder-123".into(),
                folder_display_name: "Bicameral Events".into(),
                drive_id: None,
                mime_type: "application/vnd.google-apps.folder".into(),
                can_add_children: true,
                can_list_children: Some(true),
            }),
        };
        let json = serde_json::to_string(&projection).unwrap();
        assert!(!json.contains("drive_id"), "absent optionals stay hidden");
        let back: ResourceSelectionProjection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, projection);
    }

    #[test]
    fn callback_request_defaults_picked_ids_empty() {
        let req: ResourceSelectionCallbackRequest =
            serde_json::from_str(r#"{"selection_session_id":"rsel-1"}"#).unwrap();
        assert!(req.picked_file_ids.is_empty());
        assert!(req.code.is_none());
    }
}
