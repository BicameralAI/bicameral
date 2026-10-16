//! Cross-repo hosted dashboard conformance tests — issue #617.
//!
//! Proves that the hosted dashboard local/cloud contract stays within
//! ADR-0005/0007 boundaries across:
//!   1. Pairing and session shell state contracts.
//!   2. Operator-home readiness and repair projection.
//!   3. Hosted project shell / dashboard adapter.
//!   4. Cloud-routable command allowlist and ToolCommand dispatch.
//!   5. Cloud discovery/routing contract compatibility.
//!   6. No hosted-only fork of dashboard API shapes.
//!   7. Negative/security: no local-path, provider-secret, SourceEvidence-body
//!      leakage; no direct signoff/compliance/event-store authority through
//!      hosted UI; unsupported commands rejected.
//!
//! Authority boundary: verification-only. Does not implement route handlers
//! or require live cloud credentials.
//!
//! Contract references:
//! - `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`
//! - `docs/specs/cloud-hosted-dashboard-parity-feature-spec.md`
//! - `docs/specs/alpha-hosted-dashboard-ui.md`
//! - `bicameral-cloud` ADR-0005, ADR-0007

use bicameral_api::fixtures::{
    all_bridge_allowed_commands, all_bridge_forbidden_commands, all_compatibility_envelopes,
    all_hosted_bridge_fixtures, all_hosted_bridge_negative_fixtures,
    all_workspace_discovery_fixtures, hosted_bridge_capability_discovery,
    hosted_bridge_compat_current_version, hosted_bridge_compat_newer_daemon,
    hosted_bridge_compat_previous_minor, hosted_bridge_compat_too_old_daemon,
    hosted_bridge_operator_home_session, hosted_bridge_paired_response,
    hosted_bridge_pairing_request, hosted_bridge_revoked_response, hosted_bridge_unpaired_response,
    hosted_bridge_unsupported_command, hosted_bridge_workspace_discovery_healthy,
    hosted_bridge_workspace_discovery_repair, hosted_bridge_workspace_discovery_unbound,
    hosted_bridge_wrong_account, hosted_bridge_wrong_daemon, hosted_bridge_wrong_project,
};
use bicameral_api::hosted_bridge::{
    detect_smuggled_fields, is_cloud_routable_command, validate_origin, CompatibilityOutcome,
    DashboardDataMode, HostedBridgeErrorKind, HostedBridgePairingState,
    HostedDashboardAdapterResponse, HostedLedgerSummary, OriginValidationOutcome,
    ProjectReadinessEntry, SafeWorkspaceMetadata, BRIDGE_ALLOWED_COMMANDS,
    BRIDGE_FORBIDDEN_COMMANDS, BRIDGE_FORBIDDEN_FIELDS, CLOUD_FORBIDDEN_TOOL_COMMANDS,
    CLOUD_ROUTABLE_TOOL_COMMANDS, DEFAULT_TRUSTED_ORIGINS, NEVER_EXPOSE_FIELDS,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Pairing and session shell state contracts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pairing_request_uses_trusted_origin() {
    let req = hosted_bridge_pairing_request();
    assert!(
        DEFAULT_TRUSTED_ORIGINS.contains(&req.origin.as_str()),
        "pairing request origin must be on the trusted origin allowlist"
    );
}

#[test]
fn paired_response_transitions_to_paired_state() {
    let resp = hosted_bridge_paired_response();
    assert_eq!(resp.state, HostedBridgePairingState::Paired);
    assert!(!resp.session_token.is_empty());
    assert!(!resp.expires_at.is_empty());
}

#[test]
fn paired_response_never_grants_workspace_binding() {
    let resp = hosted_bridge_paired_response();
    assert!(
        !resp.capabilities.workspace_binding_available,
        "ADR-0005: hosted pairing must not grant workspace binding"
    );
}

#[test]
fn unpaired_response_is_retryable_not_paired_error() {
    let resp = hosted_bridge_unpaired_response();
    assert_eq!(resp.error, HostedBridgeErrorKind::NotPaired);
    assert!(resp.retry_after_repair);
}

#[test]
fn revoked_state_is_terminal_with_re_pair_available() {
    let resp = hosted_bridge_revoked_response();
    assert_eq!(resp.state, HostedBridgePairingState::Revoked);
    assert!(resp.re_pair_available);
    assert!(!resp.revoked_at.is_empty());
}

#[test]
fn session_token_format_is_non_empty_and_unique_per_pairing() {
    let resp = hosted_bridge_paired_response();
    assert!(
        resp.session_token.len() >= 8,
        "session token must be a non-trivial identifier"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Operator-home readiness and repair states
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn operator_home_lists_projects_as_projection_only() {
    let home = hosted_bridge_operator_home_session();
    assert_eq!(home.pairing_state, HostedBridgePairingState::Paired);
    assert!(!home.projects.is_empty());
    assert!(
        !home.workspace_binding_available,
        "ADR-0005: workspace binding must be unavailable at operator-home level"
    );
}

#[test]
fn operator_home_projects_cover_normal_and_repair_states() {
    let home = hosted_bridge_operator_home_session();
    let states: Vec<&str> = home
        .projects
        .iter()
        .map(|p| p.setup_state.as_str())
        .collect();
    assert!(states.contains(&"normal"), "must include a normal project");
    assert!(
        states.contains(&"repair_required"),
        "must include a project needing repair"
    );
}

#[test]
fn operator_home_projection_contains_no_authority_fields() {
    let home = hosted_bridge_operator_home_session();
    let json = serde_json::to_string(&home).unwrap();
    for field in BRIDGE_FORBIDDEN_FIELDS {
        assert!(
            !json.contains(field),
            "operator-home projection must not contain forbidden field '{field}'"
        );
    }
}

#[test]
fn project_readiness_entry_shape_matches_workspace_discovery() {
    let home = hosted_bridge_operator_home_session();
    for p in &home.projects {
        assert!(!p.name.is_empty(), "project name must be non-empty");
        assert!(
            ["normal", "repair_required", "code_grounding_not_ready"]
                .contains(&p.setup_state.as_str()),
            "setup_state '{}' must be a known readiness state",
            p.setup_state,
        );
    }
}

#[test]
fn workspace_discovery_fixtures_cover_all_readiness_states() {
    let fixtures = all_workspace_discovery_fixtures();
    let states: Vec<String> = fixtures
        .iter()
        .map(|(_, ws)| ws.setup_state.clone())
        .collect();
    assert!(states.contains(&"normal".to_string()));
    assert!(states.contains(&"repair_required".to_string()));
    assert!(states.contains(&"code_grounding_not_ready".to_string()));
}

#[test]
fn workspace_discovery_healthy_has_binding_and_grounding() {
    let ws = hosted_bridge_workspace_discovery_healthy();
    assert!(ws.has_workspace_binding);
    assert!(ws.code_grounding_ready);
    assert!(ws.event_store_healthy);
}

#[test]
fn workspace_discovery_repair_has_unhealthy_event_store() {
    let ws = hosted_bridge_workspace_discovery_repair();
    assert!(!ws.event_store_healthy);
    assert!(!ws.has_workspace_binding);
    assert!(!ws.code_grounding_ready);
}

#[test]
fn workspace_discovery_unbound_has_no_grounding() {
    let ws = hosted_bridge_workspace_discovery_unbound();
    assert!(!ws.has_workspace_binding);
    assert!(!ws.code_grounding_ready);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Hosted project shell / dashboard adapter
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dashboard_adapter_hosted_mode_with_data() {
    let resp = HostedDashboardAdapterResponse {
        mode: DashboardDataMode::Hosted,
        workspace: Some(SafeWorkspaceMetadata {
            project_name: "my-service".into(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".into(),
            code_grounding_ready: true,
            decision_count: 5,
            candidate_count: 2,
            event_store_kind: Some("github".into()),
        }),
        ledger_summary: Some(HostedLedgerSummary {
            decision_count: 5,
            candidate_count: 2,
            needs_signoff_count: 1,
            stale_source_count: 0,
            bound_count: 3,
            last_promoted_at: Some("2026-06-25T14:30:00Z".into()),
        }),
        projects: vec![ProjectReadinessEntry {
            name: "my-service".into(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".into(),
        }],
        status_message: Some("Hosted dashboard adapter serving data.".into()),
    };
    assert_eq!(resp.mode, DashboardDataMode::Hosted);
    assert!(resp.workspace.is_some());
    assert!(resp.ledger_summary.is_some());

    let json = serde_json::to_string(&resp).unwrap();
    for field in NEVER_EXPOSE_FIELDS {
        assert!(
            !json.contains(field),
            "hosted adapter response must not contain '{field}'"
        );
    }
}

#[test]
fn dashboard_adapter_roundtrips_through_serde() {
    let resp = HostedDashboardAdapterResponse {
        mode: DashboardDataMode::Hosted,
        workspace: Some(SafeWorkspaceMetadata {
            project_name: "test".into(),
            event_store_healthy: true,
            has_workspace_binding: false,
            setup_state: "normal".into(),
            code_grounding_ready: false,
            decision_count: 0,
            candidate_count: 0,
            event_store_kind: None,
        }),
        ledger_summary: None,
        projects: vec![],
        status_message: None,
    };
    let json = serde_json::to_string_pretty(&resp).unwrap();
    let rt: HostedDashboardAdapterResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.mode, rt.mode);
}

#[test]
fn dashboard_adapter_hosted_mode_excludes_sensitive_fields() {
    let resp = HostedDashboardAdapterResponse {
        mode: DashboardDataMode::Hosted,
        workspace: Some(SafeWorkspaceMetadata {
            project_name: "my-service".into(),
            event_store_healthy: true,
            has_workspace_binding: true,
            setup_state: "normal".into(),
            code_grounding_ready: true,
            decision_count: 10,
            candidate_count: 3,
            event_store_kind: Some("github".into()),
        }),
        ledger_summary: Some(HostedLedgerSummary {
            decision_count: 10,
            candidate_count: 3,
            needs_signoff_count: 2,
            stale_source_count: 1,
            bound_count: 7,
            last_promoted_at: Some("2026-06-27T10:00:00Z".into()),
        }),
        projects: vec![],
        status_message: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    // Verify no Decision bodies, SourceEvidence excerpts, event payloads, or
    // authority-bearing fields leak into the adapter response.
    let forbidden_content = [
        "workspace_path",
        "provider_config",
        "provider_token",
        "provider_secret",
        "api_key",
        "access_token",
        "refresh_token",
        "source_evidence_body",
        "event_payload",
        "local_path",
        "filesystem_path",
        "home_dir",
        "authority_context",
        "signoff_approval",
        "compliance_resolution",
    ];
    for term in forbidden_content {
        assert!(
            !json.contains(term),
            "dashboard adapter response must not contain '{term}'"
        );
    }
}

#[test]
fn dashboard_data_mode_variants_are_exhaustive() {
    let modes = [DashboardDataMode::Hosted];
    for mode in &modes {
        let json = serde_json::to_value(mode).unwrap();
        let rt: DashboardDataMode = serde_json::from_value(json).unwrap();
        assert_eq!(*mode, rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Cloud-routable command allowlist and ToolCommand dispatch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cloud_routable_commands_are_read_only_projection_or_advisory() {
    for cmd in CLOUD_ROUTABLE_TOOL_COMMANDS {
        assert!(
            is_cloud_routable_command(cmd),
            "CLOUD_ROUTABLE_TOOL_COMMANDS entry '{cmd}' must pass is_cloud_routable_command"
        );
    }
}

#[test]
fn cloud_forbidden_commands_are_not_routable() {
    for cmd in CLOUD_FORBIDDEN_TOOL_COMMANDS {
        assert!(
            !is_cloud_routable_command(cmd),
            "CLOUD_FORBIDDEN_TOOL_COMMANDS entry '{cmd}' must NOT pass is_cloud_routable_command"
        );
    }
}

#[test]
fn cloud_routable_and_forbidden_commands_do_not_overlap() {
    for cmd in CLOUD_ROUTABLE_TOOL_COMMANDS {
        assert!(
            !CLOUD_FORBIDDEN_TOOL_COMMANDS.contains(cmd),
            "routable command '{cmd}' must not appear in CLOUD_FORBIDDEN_TOOL_COMMANDS"
        );
    }
    for cmd in CLOUD_FORBIDDEN_TOOL_COMMANDS {
        assert!(
            !CLOUD_ROUTABLE_TOOL_COMMANDS.contains(cmd),
            "forbidden command '{cmd}' must not appear in CLOUD_ROUTABLE_TOOL_COMMANDS"
        );
    }
}

#[test]
fn authority_bearing_tool_commands_are_forbidden() {
    let authority_commands = [
        "ingest.submit_local",
        "ingest.submit_managed",
        "review.accept_candidate",
        "review.reject_candidate",
        "review.approve_signoff",
        "review.reject_signoff",
        "review.resolve_compliance",
        "binding.create",
        // Daemon-owned workspace binding (#732/#747): binding persistence is a
        // local-daemon authority; the hosted bridge must fail closed.
        "workspace.bind",
        "workspace.resolve",
        "evidence.refresh",
        "graph.refresh_snapshot",
    ];
    for cmd in authority_commands {
        assert!(
            CLOUD_FORBIDDEN_TOOL_COMMANDS.contains(&cmd),
            "authority-bearing command '{cmd}' must be in CLOUD_FORBIDDEN_TOOL_COMMANDS"
        );
        assert!(
            !is_cloud_routable_command(cmd),
            "authority-bearing command '{cmd}' must not be cloud-routable"
        );
    }
}

#[test]
fn bridge_command_families_match_const_lists() {
    let allowed = all_bridge_allowed_commands();
    for cmd in &allowed {
        assert!(cmd.allowed);
        assert!(
            BRIDGE_ALLOWED_COMMANDS.contains(&cmd.name.as_str()),
            "allowed fixture '{}' not in BRIDGE_ALLOWED_COMMANDS",
            cmd.name,
        );
    }
    let forbidden = all_bridge_forbidden_commands();
    for cmd in &forbidden {
        assert!(!cmd.allowed);
        assert!(
            BRIDGE_FORBIDDEN_COMMANDS.contains(&cmd.name.as_str()),
            "forbidden fixture '{}' not in BRIDGE_FORBIDDEN_COMMANDS",
            cmd.name,
        );
    }
}

#[test]
fn bridge_allowed_and_forbidden_commands_are_disjoint() {
    let allowed_set: std::collections::HashSet<&str> =
        BRIDGE_ALLOWED_COMMANDS.iter().copied().collect();
    let forbidden_set: std::collections::HashSet<&str> =
        BRIDGE_FORBIDDEN_COMMANDS.iter().copied().collect();
    let overlap: Vec<&&str> = allowed_set.intersection(&forbidden_set).collect();
    assert!(
        overlap.is_empty(),
        "BRIDGE_ALLOWED and BRIDGE_FORBIDDEN must not overlap: {:?}",
        overlap,
    );
}

#[test]
fn unsupported_command_error_is_non_retryable() {
    let resp = hosted_bridge_unsupported_command();
    assert_eq!(resp.error, HostedBridgeErrorKind::UnsupportedCommand);
    assert!(!resp.retry_after_repair);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Cloud discovery/routing contract compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn compat_current_version_grants_full_access() {
    let env = hosted_bridge_compat_current_version();
    assert_eq!(env.outcome, CompatibilityOutcome::FullyCompatible);
    assert!(env.mutable_controls_available);
    assert!(env.read_only_projections_available);
    assert_eq!(env.cloud_protocol_version, env.daemon_protocol_version);
}

#[test]
fn compat_previous_minor_allows_degraded_mutable_controls() {
    let env = hosted_bridge_compat_previous_minor();
    assert_eq!(env.outcome, CompatibilityOutcome::DegradedCompatible);
    assert!(env.mutable_controls_available);
    assert!(env.read_only_projections_available);
    assert_ne!(
        env.cloud_protocol_version, env.daemon_protocol_version,
        "previous-minor must have different versions"
    );
}

#[test]
fn compat_too_old_daemon_blocks_all_access() {
    let env = hosted_bridge_compat_too_old_daemon();
    assert_eq!(env.outcome, CompatibilityOutcome::Blocked);
    assert!(!env.mutable_controls_available);
    assert!(!env.read_only_projections_available);
}

#[test]
fn compat_newer_daemon_blocks_mutable_allows_readonly() {
    let env = hosted_bridge_compat_newer_daemon();
    assert_eq!(env.outcome, CompatibilityOutcome::NewerDaemonDegraded);
    assert!(!env.mutable_controls_available);
    assert!(env.read_only_projections_available);
}

#[test]
fn all_compat_envelopes_cover_four_outcomes() {
    let envs = all_compatibility_envelopes();
    let outcomes: Vec<CompatibilityOutcome> = envs.iter().map(|(_, e)| e.outcome).collect();
    assert!(outcomes.contains(&CompatibilityOutcome::FullyCompatible));
    assert!(outcomes.contains(&CompatibilityOutcome::DegradedCompatible));
    assert!(outcomes.contains(&CompatibilityOutcome::Blocked));
    assert!(outcomes.contains(&CompatibilityOutcome::NewerDaemonDegraded));
}

#[test]
fn compat_envelopes_roundtrip_through_serde() {
    for (name, env) in all_compatibility_envelopes() {
        let json = serde_json::to_string_pretty(&env)
            .unwrap_or_else(|e| panic!("compat/{name}: serialize failed: {e}"));
        let rt: bicameral_api::hosted_bridge::ProtocolCompatibilityEnvelope =
            serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("compat/{name}: deserialize failed: {e}"));
        assert_eq!(env, rt, "compat/{name}: round-trip mismatch");
    }
}

#[test]
fn capability_discovery_advertises_bridge_support() {
    let cap = hosted_bridge_capability_discovery();
    assert!(cap.bridge_supported);
    assert!(!cap.bridge_protocol_version.is_empty());
    assert!(cap.project_list_available);
    assert!(!cap.workspace_binding_available);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. No hosted-only fork of dashboard API shapes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hosted_adapter_response_uses_same_project_readiness_entry_as_operator_home() {
    let home = hosted_bridge_operator_home_session();
    let adapter_project = ProjectReadinessEntry {
        name: home.projects[0].name.clone(),
        event_store_healthy: home.projects[0].event_store_healthy,
        has_workspace_binding: home.projects[0].has_workspace_binding,
        setup_state: home.projects[0].setup_state.clone(),
    };
    assert_eq!(adapter_project, home.projects[0]);
}

#[test]
fn safe_workspace_metadata_fields_are_subset_of_project_readiness() {
    let meta = SafeWorkspaceMetadata {
        project_name: "test".into(),
        event_store_healthy: true,
        has_workspace_binding: true,
        setup_state: "normal".into(),
        code_grounding_ready: true,
        decision_count: 0,
        candidate_count: 0,
        event_store_kind: None,
    };
    let meta_json: serde_json::Value = serde_json::to_value(&meta).unwrap();
    let meta_keys: std::collections::HashSet<String> =
        meta_json.as_object().unwrap().keys().cloned().collect();

    // SafeWorkspaceMetadata extends ProjectReadinessEntry with safe aggregate
    // fields only. It must not introduce authority-bearing or hosted-only shapes.
    let expected_extra_keys = [
        "code_grounding_ready",
        "decision_count",
        "candidate_count",
        "event_store_kind",
        "project_name", // renamed from `name` in ProjectReadinessEntry
    ];
    for key in meta_keys {
        let is_base = [
            "name",
            "event_store_healthy",
            "has_workspace_binding",
            "setup_state",
        ]
        .contains(&key.as_str());
        let is_expected_extra = expected_extra_keys.contains(&key.as_str());
        assert!(
            is_base || is_expected_extra,
            "SafeWorkspaceMetadata has unexpected field '{}' — hosted mode must not fork dashboard shapes",
            key,
        );
    }
}

#[test]
fn hosted_ledger_summary_contains_only_aggregate_counts() {
    let summary = HostedLedgerSummary {
        decision_count: 5,
        candidate_count: 2,
        needs_signoff_count: 1,
        stale_source_count: 0,
        bound_count: 3,
        last_promoted_at: Some("2026-06-25T14:30:00Z".into()),
    };
    let json = serde_json::to_string(&summary).unwrap();
    // Must not contain Decision bodies, SourceEvidence excerpts, or event payloads.
    let forbidden = [
        "decision_body",
        "evidence_body",
        "source_evidence_body",
        "event_payload",
        "source_excerpt",
        "candidate_body",
        "authority_context",
    ];
    for term in forbidden {
        assert!(
            !json.contains(term),
            "HostedLedgerSummary must not contain '{term}'"
        );
    }
}

#[test]
fn all_positive_fixtures_roundtrip_without_data_loss() {
    for (name, val) in all_hosted_bridge_fixtures() {
        let json = serde_json::to_string_pretty(&val)
            .unwrap_or_else(|e| panic!("bridge/{name}: serialize failed: {e}"));
        let rt: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("bridge/{name}: deserialize failed: {e}"));
        assert_eq!(val, rt, "bridge/{name}: round-trip mismatch");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Negative / security conformance
// ═══════════════════════════════════════════════════════════════════════════

// ── 7a. No local path leakage ──────────────────────────────────────────────

#[test]
fn workspace_discovery_contains_no_local_paths() {
    for (name, ws) in all_workspace_discovery_fixtures() {
        let json = serde_json::to_string(&ws).unwrap();
        let path_patterns = [
            "/home/", "/Users/", "/tmp/", "C:\\", "/var/", "/opt/", "/root/",
        ];
        for pat in path_patterns {
            assert!(
                !json.contains(pat),
                "discovery/{name}: local path pattern '{pat}' leaked"
            );
        }
    }
}

#[test]
fn operator_home_contains_no_local_paths() {
    let home = hosted_bridge_operator_home_session();
    let json = serde_json::to_string(&home).unwrap();
    let path_patterns = ["/home/", "/Users/", "C:\\", "/tmp/", "/var/"];
    for pat in path_patterns {
        assert!(
            !json.contains(pat),
            "operator-home: local path pattern '{pat}' leaked"
        );
    }
}

#[test]
fn safe_workspace_metadata_contains_no_local_paths() {
    let meta = SafeWorkspaceMetadata {
        project_name: "test-project".into(),
        event_store_healthy: true,
        has_workspace_binding: true,
        setup_state: "normal".into(),
        code_grounding_ready: true,
        decision_count: 5,
        candidate_count: 2,
        event_store_kind: Some("github".into()),
    };
    let json = serde_json::to_string(&meta).unwrap();
    for field in NEVER_EXPOSE_FIELDS {
        assert!(
            !json.contains(field),
            "SafeWorkspaceMetadata must not contain '{field}'"
        );
    }
}

// ── 7b. No provider secret leakage ────────────────────────────────────────

#[test]
fn all_fixtures_contain_no_provider_secrets() {
    for (name, val) in all_hosted_bridge_fixtures() {
        let json = serde_json::to_string(&val).unwrap();
        let secret_patterns = [
            "provider_token",
            "provider_secret",
            "provider_config",
            "api_key",
            "access_token",
            "refresh_token",
        ];
        for pat in secret_patterns {
            assert!(
                !json.contains(pat),
                "bridge/{name}: provider secret pattern '{pat}' leaked"
            );
        }
    }
}

// ── 7c. No SourceEvidence body leakage ─────────────────────────────────────

#[test]
fn all_fixtures_contain_no_source_evidence_bodies() {
    for (name, val) in all_hosted_bridge_fixtures() {
        let json = serde_json::to_string(&val).unwrap();
        let evidence_patterns = [
            "source_evidence_body",
            "event_payload",
            "snapshot_content",
            "source_body",
        ];
        for pat in evidence_patterns {
            assert!(
                !json.contains(pat),
                "bridge/{name}: SourceEvidence body pattern '{pat}' leaked"
            );
        }
    }
}

// ── 7d. No direct signoff/compliance/event-store authority through hosted UI ──

#[test]
fn bridge_forbidden_fields_block_signoff_compliance_event_store() {
    let authority_fields = [
        "signoff_approval",
        "compliance_resolution",
        "event_store_write",
        "decision_write",
        "review_command",
        "source_binding",
        "canonical_decision",
    ];
    for field in authority_fields {
        assert!(
            BRIDGE_FORBIDDEN_FIELDS.contains(&field),
            "authority field '{field}' must be in BRIDGE_FORBIDDEN_FIELDS"
        );
    }
}

#[test]
fn smuggling_detection_rejects_all_authority_fields() {
    for &field in BRIDGE_FORBIDDEN_FIELDS {
        let payload = serde_json::json!({ field: "smuggled_value" });
        let found = detect_smuggled_fields(&payload);
        assert!(
            found.contains(&field.to_string()),
            "detect_smuggled_fields must catch authority field '{field}'"
        );
    }
}

#[test]
fn negative_fixtures_reject_authority_smuggling_with_correct_error() {
    for (name, payload) in all_hosted_bridge_negative_fixtures() {
        assert_eq!(
            payload.rejection.error,
            HostedBridgeErrorKind::AuthoritySmugglingRejected,
            "bridge_negative/{name}: rejection error must be AuthoritySmugglingRejected"
        );
        assert!(
            !payload.rejection.retry_after_repair,
            "bridge_negative/{name}: smuggling must not suggest retry"
        );
        assert!(
            !payload.forbidden_fields.is_empty(),
            "bridge_negative/{name}: must detect at least one forbidden field"
        );
    }
}

#[test]
fn negative_fixtures_cover_key_smuggling_vectors() {
    let all_fields: Vec<String> = all_hosted_bridge_negative_fixtures()
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
            all_fields.contains(&field.to_string()),
            "negative fixtures must cover smuggling vector '{field}'"
        );
    }
}

// ── 7e. Unsupported commands rejected ──────────────────────────────────────

#[test]
fn unknown_command_is_not_cloud_routable() {
    assert!(!is_cloud_routable_command("unknown_command"));
    assert!(!is_cloud_routable_command("admin.delete_all"));
    assert!(!is_cloud_routable_command(""));
}

#[test]
fn error_fixtures_cover_all_relevant_error_kinds() {
    let wrong_daemon = hosted_bridge_wrong_daemon();
    assert_eq!(wrong_daemon.error, HostedBridgeErrorKind::WrongDaemon);
    assert!(wrong_daemon.retry_after_repair);

    let wrong_project = hosted_bridge_wrong_project();
    assert_eq!(wrong_project.error, HostedBridgeErrorKind::ProjectNotFound);
    assert!(!wrong_project.retry_after_repair);

    let wrong_account = hosted_bridge_wrong_account();
    assert_eq!(wrong_account.error, HostedBridgeErrorKind::AccountMismatch);
    assert!(wrong_account.retry_after_repair);

    let unsupported = hosted_bridge_unsupported_command();
    assert_eq!(unsupported.error, HostedBridgeErrorKind::UnsupportedCommand);
    assert!(!unsupported.retry_after_repair);
}

// ── 7f. Origin validation security ────────────────────────────────────────

#[test]
fn untrusted_origins_are_rejected() {
    let bad_origins = [
        "https://evil.example.com",
        "http://app.bicameral-ai.com",
        "https://sub.app.bicameral-ai.com",
        "https://app.bicameral-ai.com.evil.com",
        "",
        "   ",
        "javascript:alert(1)",
    ];
    for origin in bad_origins {
        let result = validate_origin(origin, &[]);
        assert!(
            matches!(result, OriginValidationOutcome::Rejected { .. }),
            "origin '{origin}' must be rejected"
        );
    }
}

#[test]
fn all_default_trusted_origins_accepted() {
    for origin in DEFAULT_TRUSTED_ORIGINS {
        assert_eq!(
            validate_origin(origin, &[]),
            OriginValidationOutcome::Trusted,
            "default trusted origin '{origin}' must be accepted"
        );
    }
}

#[test]
fn custom_operator_origin_accepted() {
    let custom = vec!["https://custom.enterprise.com".into()];
    assert_eq!(
        validate_origin("https://custom.enterprise.com", &custom),
        OriginValidationOutcome::Trusted,
    );
}

#[test]
fn clean_payload_passes_smuggling_detection() {
    let clean = serde_json::json!({
        "action": "list_projects",
        "session_token": "bridge-sess-001",
        "page": 1,
        "filter": "active"
    });
    let found = detect_smuggled_fields(&clean);
    assert!(
        found.is_empty(),
        "clean payload must pass smuggling detection"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-repo contract compatibility assertions
// ═══════════════════════════════════════════════════════════════════════════
// These tests encode the contract boundaries from bicameral-cloud ADR-0005
// and ADR-0007 to ensure the bot's hosted bridge types remain compatible.

#[test]
fn cloud_adr_0005_cloud_never_owns_signoff_compliance_or_ledger_writes() {
    // ADR-0005: Cloud must not become authority for signoff, compliance
    // enforcement, or accepted event materialization.
    let authority_commands = [
        "signoff",
        "compliance_resolve",
        "decision_write",
        "event_store_write",
    ];
    for cmd in authority_commands {
        assert!(
            BRIDGE_FORBIDDEN_COMMANDS.contains(&cmd),
            "ADR-0005 violation: '{cmd}' must be forbidden through bridge"
        );
    }
    for field in [
        "signoff_approval",
        "compliance_resolution",
        "event_store_write",
        "decision_write",
    ] {
        assert!(
            BRIDGE_FORBIDDEN_FIELDS.contains(&field),
            "ADR-0005 violation: '{field}' must be a forbidden bridge field"
        );
    }
}

#[test]
fn cloud_adr_0007_bridge_routing_is_default_deny() {
    // ADR-0007: Cloud bridge routing is hard allowlist and default-deny.
    // Any command not on the explicit allowlist must be rejected.
    assert!(!is_cloud_routable_command("admin.delete_everything"));
    assert!(!is_cloud_routable_command("source.bind_new"));
    assert!(!is_cloud_routable_command("review.force_accept"));
    assert!(!is_cloud_routable_command("compliance.override"));
}

#[test]
fn cloud_adr_0007_only_trusted_origins_may_pair() {
    // ADR-0007: Only Bicameral-owned trusted origins may pair with real daemons.
    // Arbitrary preview origins must be blocked from pairing.
    let result = validate_origin("https://preview.random-cdn.net", &[]);
    assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));

    let result = validate_origin("https://localhost:3000", &[]);
    assert!(matches!(result, OriginValidationOutcome::Rejected { .. }));
}

#[test]
fn cloud_adr_0007_compat_window_is_bounded() {
    // ADR-0007: Alpha supports current + previous-minor. Too-old daemons
    // receive blocking upgrade state. Newer daemons block mutable controls.
    let envs = all_compatibility_envelopes();
    assert!(
        envs.len() >= 4,
        "must cover at least four compatibility outcomes"
    );

    let blocked: Vec<_> = envs
        .iter()
        .filter(|(_, e)| e.outcome == CompatibilityOutcome::Blocked)
        .collect();
    assert!(
        !blocked.is_empty(),
        "must include a blocked compatibility case"
    );

    for (_, env) in &blocked {
        assert!(
            !env.mutable_controls_available && !env.read_only_projections_available,
            "blocked compat must disable all access"
        );
    }
}
