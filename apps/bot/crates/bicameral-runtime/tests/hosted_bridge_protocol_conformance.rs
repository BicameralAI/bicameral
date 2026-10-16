//! Hosted bridge protocol fixture conformance — issue #577.
//!
//! Proves that hosted bridge fixtures correctly represent:
//!   1. User-level pairing before project selection.
//!   2. Project list/readiness as post-pairing projection data, not authority.
//!   3. Workspace binding as unavailable until explicitly requested inside a project.
//!   4. Negative fixtures reject authority-smuggling fields.
//!   5. All fixtures round-trip through serde_json without data loss.
//!
//! Authority boundary: this file is verification-only. It does not implement
//! route handlers, store pairing sessions, or add browser UI. The bridge
//! contract is separate from `ToolRequest` and `ExternalIngestEnvelope`.
//!
//! Scope boundary: fixtures describe wire shapes, not execution behavior.

use bicameral_api::fixtures::{
    all_bridge_allowed_commands, all_bridge_forbidden_commands, all_compatibility_envelopes,
    all_hosted_bridge_fixtures, all_hosted_bridge_negative_fixtures,
    all_workspace_discovery_fixtures, hosted_bridge_capability_discovery,
    hosted_bridge_compat_current_version, hosted_bridge_compat_newer_daemon,
    hosted_bridge_compat_previous_minor, hosted_bridge_compat_too_old_daemon,
    hosted_bridge_operator_home_session, hosted_bridge_paired_response,
    hosted_bridge_pairing_request, hosted_bridge_revoked_response,
    hosted_bridge_selected_project_needs_binding, hosted_bridge_selected_project_ready,
    hosted_bridge_selected_project_ref, hosted_bridge_unpaired_response,
    hosted_bridge_unsupported_command, hosted_bridge_wrong_account, hosted_bridge_wrong_daemon,
    hosted_bridge_wrong_project,
};
use bicameral_api::hosted_bridge::{
    detect_smuggled_fields, CompatibilityOutcome, DaemonBindingReadiness, HostedBridgeErrorKind,
    HostedBridgePairingState, ProtocolCompatibilityEnvelope, TeamVisibleWorkspaceDiscovery,
    BRIDGE_ALLOWED_COMMANDS, BRIDGE_FORBIDDEN_COMMANDS, BRIDGE_FORBIDDEN_FIELDS,
    NEVER_EXPOSE_FIELDS,
};

// ─── 1. User-level pairing before project selection ──────────────────────────

#[test]
fn paired_response_state_is_user_level_paired() {
    let resp = hosted_bridge_paired_response();
    assert_eq!(resp.state, HostedBridgePairingState::Paired);
    // Pairing is user-level; no project selection fields.
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("\"project_id\""));
    assert!(!json.contains("\"selected_project\""));
}

#[test]
fn unpaired_response_indicates_no_active_pairing() {
    let resp = hosted_bridge_unpaired_response();
    assert_eq!(resp.error, HostedBridgeErrorKind::NotPaired);
    assert!(resp.retry_after_repair);
}

#[test]
fn revoked_response_is_terminal_state() {
    let resp = hosted_bridge_revoked_response();
    assert_eq!(resp.state, HostedBridgePairingState::Revoked);
}

#[test]
fn pairing_request_has_origin_and_csrf() {
    let req = hosted_bridge_pairing_request();
    assert!(!req.origin.is_empty());
    assert!(!req.csrf_token.is_empty());
    assert!(!req.cloud_account_id.is_empty());
}

// ─── 2. Project list as projection data, not authority ───────────────────────

#[test]
fn operator_home_projects_are_non_authoritative_projection() {
    let home = hosted_bridge_operator_home_session();
    assert!(!home.projects.is_empty());

    let json = serde_json::to_string(&home).unwrap();
    // Projection data must not contain authority fields.
    assert!(!json.contains("authority_context"));
    assert!(!json.contains("tool_command"));
    assert!(!json.contains("review_command"));
    assert!(!json.contains("signoff_approval"));
    assert!(!json.contains("decision_write"));
    assert!(!json.contains("event_store_write"));
}

#[test]
fn operator_home_pairing_state_is_paired() {
    let home = hosted_bridge_operator_home_session();
    assert_eq!(home.pairing_state, HostedBridgePairingState::Paired);
}

// ─── 2b. Selected Cloud project scopes readiness projection (#719) ───────────

#[test]
fn selected_project_ref_carries_only_safe_cloud_identity() {
    let sel = hosted_bridge_selected_project_ref();
    assert!(!sel.project_id.is_empty());
    // Safe cloud-coordination identity only — no local path/secret leakage.
    let json = serde_json::to_string(&sel).unwrap();
    for field in NEVER_EXPOSE_FIELDS {
        assert!(
            !json.contains(field),
            "selected project ref leaked: {field}"
        );
    }
}

#[test]
fn selected_project_ready_echoes_selection_and_is_ready() {
    let resp = hosted_bridge_selected_project_ready();
    assert_eq!(resp.pairing_state, HostedBridgePairingState::Paired);
    // The selected Cloud project id flows through to the scoped projection.
    assert_eq!(resp.selected_project.project_id, "my-service");
    assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Ready);
    assert!(resp.controls_available);
    // Scoped projection is present and matches the selection.
    assert_eq!(
        resp.project.as_ref().map(|p| p.name.as_str()),
        Some("my-service")
    );
}

#[test]
fn selected_project_needs_binding_is_unbound_without_membership_leak() {
    let resp = hosted_bridge_selected_project_needs_binding();
    // Missing local binding renders the "needs binding" readiness state.
    assert_eq!(resp.binding_readiness, DaemonBindingReadiness::Unbound);
    assert!(!resp.controls_available);
    assert!(resp.project.is_none());
    // The unscoped local list is not the membership source: no other project
    // name is surfaced for a selection the daemon does not have bound.
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("my-service"));
}

#[test]
fn selected_project_responses_never_expose_forbidden_fields() {
    for resp in [
        hosted_bridge_selected_project_ready(),
        hosted_bridge_selected_project_needs_binding(),
    ] {
        let json = serde_json::to_string(&resp).unwrap();
        for field in NEVER_EXPOSE_FIELDS {
            assert!(
                !json.contains(field),
                "selected-project response leaked forbidden field: {field}"
            );
        }
    }
}

// ─── 3. Workspace binding unavailable until explicit project request ─────────

#[test]
fn capability_discovery_workspace_binding_unavailable() {
    let cap = hosted_bridge_capability_discovery();
    assert!(
        !cap.workspace_binding_available,
        "workspace binding must be false at capability level"
    );
}

#[test]
fn operator_home_workspace_binding_unavailable() {
    let home = hosted_bridge_operator_home_session();
    assert!(
        !home.workspace_binding_available,
        "workspace binding must be unavailable at operator-home level"
    );
}

#[test]
fn paired_response_capabilities_workspace_unavailable() {
    let resp = hosted_bridge_paired_response();
    assert!(
        !resp.capabilities.workspace_binding_available,
        "paired response capabilities must not offer workspace binding"
    );
}

// ─── 4. Negative fixtures reject authority-smuggling fields ──────────────────

#[test]
fn negative_fixtures_detect_and_reject_smuggling() {
    for (name, payload) in all_hosted_bridge_negative_fixtures() {
        assert!(
            !payload.forbidden_fields.is_empty(),
            "bridge_negative/{name}: must detect at least one forbidden field"
        );
        assert_eq!(
            payload.rejection.error,
            HostedBridgeErrorKind::AuthoritySmugglingRejected,
            "bridge_negative/{name}: rejection error must be AuthoritySmugglingRejected"
        );
        assert!(
            !payload.rejection.retry_after_repair,
            "bridge_negative/{name}: smuggling rejections must not suggest retry"
        );
    }
}

#[test]
fn negative_fixtures_cover_all_required_smuggling_vectors() {
    let payloads = all_hosted_bridge_negative_fixtures();
    let all_forbidden: Vec<String> = payloads
        .iter()
        .flat_map(|(_, p)| p.forbidden_fields.clone())
        .collect();

    let required = [
        "external_ingest_envelope",
        "review_command",
        "signoff_approval",
        "source_binding",
        "workspace_path",
        "provider_token",
        "authority_context",
        "event_store_write",
    ];
    for field in required {
        assert!(
            all_forbidden.contains(&field.to_string()),
            "negative fixtures must cover forbidden field '{field}'"
        );
    }
}

#[test]
fn detect_smuggled_fields_rejects_known_fields() {
    for &field in BRIDGE_FORBIDDEN_FIELDS {
        let payload = serde_json::json!({ field: "value" });
        let found = detect_smuggled_fields(&payload);
        assert!(
            found.contains(&field.to_string()),
            "detect_smuggled_fields must catch '{field}'"
        );
    }
}

#[test]
fn detect_smuggled_fields_passes_clean_payload() {
    let clean = serde_json::json!({
        "action": "list_projects",
        "session_token": "bridge-sess-001",
        "page": 1
    });
    let found = detect_smuggled_fields(&clean);
    assert!(
        found.is_empty(),
        "clean payload must pass smuggling detection"
    );
}

// ─── 5. All fixtures round-trip through serde_json ───────────────────────────

#[test]
fn all_positive_fixtures_roundtrip() {
    for (name, val) in all_hosted_bridge_fixtures() {
        let json = serde_json::to_string_pretty(&val)
            .unwrap_or_else(|e| panic!("bridge/{name}: serialize failed: {e}"));
        let rt: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("bridge/{name}: deserialize failed: {e}"));
        assert_eq!(val, rt, "bridge/{name}: round-trip mismatch");
    }
}

#[test]
fn all_negative_fixtures_roundtrip() {
    for (name, payload) in all_hosted_bridge_negative_fixtures() {
        let json = serde_json::to_string_pretty(&payload)
            .unwrap_or_else(|e| panic!("bridge_negative/{name}: serialize failed: {e}"));
        let rt: bicameral_api::hosted_bridge::AuthoritySmugglingPayload =
            serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("bridge_negative/{name}: deserialize failed: {e}"));
        assert_eq!(rt.forbidden_fields, payload.forbidden_fields);
        assert_eq!(rt.rejection, payload.rejection);
    }
}

// ─── Error outcome fixtures ──────────────────────────────────────────────────

#[test]
fn wrong_daemon_error_kind() {
    let resp = hosted_bridge_wrong_daemon();
    assert_eq!(resp.error, HostedBridgeErrorKind::WrongDaemon);
    assert!(resp.retry_after_repair);
}

#[test]
fn wrong_project_error_kind() {
    let resp = hosted_bridge_wrong_project();
    assert_eq!(resp.error, HostedBridgeErrorKind::ProjectNotFound);
    assert!(!resp.retry_after_repair);
}

#[test]
fn wrong_account_error_kind() {
    let resp = hosted_bridge_wrong_account();
    assert_eq!(resp.error, HostedBridgeErrorKind::AccountMismatch);
    assert!(resp.retry_after_repair);
}

#[test]
fn unsupported_command_error_kind() {
    let resp = hosted_bridge_unsupported_command();
    assert_eq!(resp.error, HostedBridgeErrorKind::UnsupportedCommand);
    assert!(!resp.retry_after_repair);
}

// ─── 6. Compatibility envelope fixtures (#612) ──────────────────────────────

#[test]
fn compat_envelope_current_version_fully_compatible() {
    let env = hosted_bridge_compat_current_version();
    assert_eq!(env.outcome, CompatibilityOutcome::FullyCompatible);
    assert!(env.mutable_controls_available);
    assert!(env.read_only_projections_available);
}

#[test]
fn compat_envelope_previous_minor_degraded() {
    let env = hosted_bridge_compat_previous_minor();
    assert_eq!(env.outcome, CompatibilityOutcome::DegradedCompatible);
    assert!(env.mutable_controls_available);
    assert!(env.read_only_projections_available);
}

#[test]
fn compat_envelope_too_old_daemon_blocked() {
    let env = hosted_bridge_compat_too_old_daemon();
    assert_eq!(env.outcome, CompatibilityOutcome::Blocked);
    assert!(!env.mutable_controls_available);
    assert!(!env.read_only_projections_available);
}

#[test]
fn compat_envelope_newer_daemon_mutable_controls_blocked() {
    let env = hosted_bridge_compat_newer_daemon();
    assert_eq!(env.outcome, CompatibilityOutcome::NewerDaemonDegraded);
    assert!(
        !env.mutable_controls_available,
        "newer daemon must block mutable controls"
    );
    assert!(
        env.read_only_projections_available,
        "read-only projections remain available"
    );
}

#[test]
fn all_compat_envelopes_roundtrip() {
    for (name, env) in all_compatibility_envelopes() {
        let json = serde_json::to_string_pretty(&env)
            .unwrap_or_else(|e| panic!("compat/{name}: serialize failed: {e}"));
        let rt: ProtocolCompatibilityEnvelope = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("compat/{name}: deserialize failed: {e}"));
        assert_eq!(env, rt, "compat/{name}: round-trip mismatch");
    }
}

// ─── 7. Team-visible workspace discovery fixtures (#612) ─────────────────────

#[test]
fn workspace_discovery_fixtures_contain_no_authority_data() {
    for (name, ws) in all_workspace_discovery_fixtures() {
        let json = serde_json::to_string(&ws).unwrap();
        assert!(
            !json.contains("workspace_path"),
            "discovery/{name}: workspace_path leaked"
        );
        assert!(
            !json.contains("provider_token"),
            "discovery/{name}: provider_token leaked"
        );
        assert!(
            !json.contains("provider_config"),
            "discovery/{name}: provider_config leaked"
        );
        assert!(
            !json.contains("authority_context"),
            "discovery/{name}: authority_context leaked"
        );
    }
}

#[test]
fn workspace_discovery_fixtures_roundtrip() {
    for (name, ws) in all_workspace_discovery_fixtures() {
        let json = serde_json::to_string_pretty(&ws)
            .unwrap_or_else(|e| panic!("discovery/{name}: serialize failed: {e}"));
        let rt: TeamVisibleWorkspaceDiscovery = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("discovery/{name}: deserialize failed: {e}"));
        assert_eq!(ws, rt, "discovery/{name}: round-trip mismatch");
    }
}

#[test]
fn workspace_discovery_covers_setup_states() {
    let fixtures = all_workspace_discovery_fixtures();
    let states: Vec<String> = fixtures
        .iter()
        .map(|(_, ws)| ws.setup_state.clone())
        .collect();
    assert!(
        states.contains(&"normal".to_string()),
        "must cover 'normal' setup state"
    );
    assert!(
        states.contains(&"repair_required".to_string()),
        "must cover 'repair_required'"
    );
}

// ─── 8. Bridge command allowlist (#612) ──────────────────────────────────────

#[test]
fn forbidden_commands_are_all_denied() {
    for cmd in all_bridge_forbidden_commands() {
        assert!(
            !cmd.allowed,
            "bridge command '{}' must be forbidden",
            cmd.name
        );
    }
}

#[test]
fn allowed_commands_are_all_permitted() {
    for cmd in all_bridge_allowed_commands() {
        assert!(cmd.allowed, "bridge command '{}' must be allowed", cmd.name);
    }
}

#[test]
fn forbidden_command_fixtures_match_const_list() {
    let fixture_names: Vec<String> = all_bridge_forbidden_commands()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    for name in &fixture_names {
        assert!(
            BRIDGE_FORBIDDEN_COMMANDS.contains(&name.as_str()),
            "forbidden fixture '{}' not in BRIDGE_FORBIDDEN_COMMANDS const",
            name
        );
    }
}

#[test]
fn allowed_command_fixtures_match_const_list() {
    let fixture_names: Vec<String> = all_bridge_allowed_commands()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    for name in &fixture_names {
        assert!(
            BRIDGE_ALLOWED_COMMANDS.contains(&name.as_str()),
            "allowed fixture '{}' not in BRIDGE_ALLOWED_COMMANDS const",
            name
        );
    }
}

#[test]
fn allowed_and_forbidden_commands_do_not_overlap() {
    let allowed: std::collections::HashSet<String> = all_bridge_allowed_commands()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    let forbidden: std::collections::HashSet<String> = all_bridge_forbidden_commands()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    let overlap: Vec<_> = allowed.intersection(&forbidden).collect();
    assert!(
        overlap.is_empty(),
        "command sets must not overlap: {:?}",
        overlap
    );
}

#[test]
fn authority_bearing_command_names_are_forbidden() {
    let authority_commands = [
        "signoff",
        "decision_write",
        "source_bind",
        "compliance_resolve",
        "review_approve",
    ];
    let forbidden_names: Vec<String> = all_bridge_forbidden_commands()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    for cmd in authority_commands {
        assert!(
            forbidden_names.contains(&cmd.to_string()),
            "authority-bearing command '{}' must be present in forbidden fixtures",
            cmd
        );
    }
}

// ─── 8. Pending pairing / local-confirmation flow (#790) ─────────────────────
//
// The hosted bridge no longer issues a session token from `POST /pair`. These
// fixture-conformance tests pin the pending-pairing wire shapes: a pending
// outcome with an opaque handle/proof and no token, capability/version pinning,
// terminal denied/expired outcomes that never reveal a token, an approved
// handoff that does, and a typed update-required outcome for old consumers.

mod pending_pairing_flow {
    use bicameral_api::fixtures::{
        hosted_bridge_pairing_update_required, hosted_bridge_pending_local_view,
        hosted_bridge_pending_response, hosted_bridge_status_approved, hosted_bridge_status_denied,
        hosted_bridge_status_expired,
    };
    use bicameral_api::hosted_bridge::{
        HostedBridgeErrorKind, PendingPairingOutcome, HOSTED_BRIDGE_PROTOCOL_VERSION,
        PENDING_PAIRING_MIN_PROTOCOL_VERSION,
    };

    #[test]
    fn pending_response_is_pending_and_carries_no_token() {
        let resp = hosted_bridge_pending_response();
        assert_eq!(resp.status, PendingPairingOutcome::Pending);
        assert!(!resp.request_handle.is_empty());
        assert!(!resp.poll_proof.is_empty());
        assert!(!resp.expires_at.is_empty());
        let json = serde_json::to_string(&resp).unwrap();
        // The pending response never mentions a session token or paired state.
        assert!(!json.contains("session_token"));
        assert!(!json.contains("\"paired\""));
    }

    #[test]
    fn pending_response_pins_capability_version() {
        let resp = hosted_bridge_pending_response();
        assert_eq!(resp.protocol_version, HOSTED_BRIDGE_PROTOCOL_VERSION);
        assert_eq!(
            resp.capabilities.hosted_bridge_protocol_version,
            HOSTED_BRIDGE_PROTOCOL_VERSION
        );
    }

    #[test]
    fn pending_response_surfaces_local_confirmation_affordance() {
        let resp = hosted_bridge_pending_response();
        let c = &resp.confirmation;
        let url = c.local_confirmation_url.as_deref().unwrap();
        assert!(url.starts_with("/local/pairing?request="));
        assert!(url.ends_with(&resp.request_handle));
        assert_eq!(c.terminal_command, "bicameral pairing approve");
        assert!(!c.requesting_origin.is_empty());
        assert!(!c.cloud_account_id.is_empty());
        assert!(!c.capability_summary.allowed.is_empty());
        assert!(!c.capability_summary.forbidden.is_empty());
    }

    #[test]
    fn local_view_carries_nonce_and_capabilities() {
        let view = hosted_bridge_pending_local_view();
        assert_eq!(view.status, PendingPairingOutcome::Pending);
        assert!(!view.local_confirmation_nonce.is_empty());
        assert!(!view.capability_summary.forbidden.is_empty());
    }

    #[test]
    fn approved_status_releases_token_and_expiry() {
        let resp = hosted_bridge_status_approved();
        assert_eq!(resp.status, PendingPairingOutcome::Approved);
        assert!(resp.session_token.is_some());
        assert!(resp.expires_at.is_some());
        assert!(resp.capabilities.is_some());
    }

    #[test]
    fn denied_and_expired_status_never_reveal_token() {
        for resp in [
            hosted_bridge_status_denied(),
            hosted_bridge_status_expired(),
        ] {
            assert!(resp.status.is_terminal());
            assert!(resp.session_token.is_none());
            assert!(resp.expires_at.is_none());
            let json = serde_json::to_string(&resp).unwrap();
            assert!(!json.contains("session_token"));
        }
    }

    #[test]
    fn update_required_is_incompatible_typed_outcome() {
        let resp = hosted_bridge_pairing_update_required();
        assert_eq!(resp.status, PendingPairingOutcome::Incompatible);
        assert_eq!(resp.error, HostedBridgeErrorKind::UpdateRequired);
        assert_eq!(
            resp.required_protocol_version,
            PENDING_PAIRING_MIN_PROTOCOL_VERSION
        );
        assert!(resp.client_protocol_version < PENDING_PAIRING_MIN_PROTOCOL_VERSION);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("session_token"));
    }

    #[test]
    fn all_pending_flow_fixtures_roundtrip() {
        let pending = hosted_bridge_pending_response();
        let round: bicameral_api::hosted_bridge::HostedBridgePairingPendingResponse =
            serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
        assert_eq!(round, pending);

        let approved = hosted_bridge_status_approved();
        let round: bicameral_api::hosted_bridge::HostedBridgePairingStatusResponse =
            serde_json::from_str(&serde_json::to_string(&approved).unwrap()).unwrap();
        assert_eq!(round, approved);
    }
}
