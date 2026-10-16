//! Live `preflight.run` impact-recall integration for issue #772.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
use bicameral_runtime::replay::replay_from_adapter;
use std::path::Path;
use std::process::Command;

fn git(args: &[&str], dir: &Path) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    git(&["init"], path);
    git(&["config", "user.email", "test@test.com"], path);
    git(&["config", "user.name", "Test"], path);
    std::fs::create_dir_all(path.join("src/payments")).unwrap();
    std::fs::write(
        path.join("src/payments/idempotency.rs"),
        "pub fn apply_idempotency() {}\n",
    )
    .unwrap();
    git(&["add", "."], path);
    git(&["commit", "-m", "init"], path);
    dir
}

fn authority(actor_id: &str, surface: &str) -> AuthorityContext {
    AuthorityContext {
        actor_id: actor_id.into(),
        auth_method: AuthMethod::McpSession,
        session_id: Some("issue-772-live-recall".into()),
        workspace: Some("/workspace".into()),
        policy_scope: vec!["default".into(), "review".into()],
        audit_metadata: {
            let mut metadata = serde_json::Map::new();
            metadata.insert("surface".into(), surface.into());
            metadata
        },
    }
}

fn seed_accepted_decision(
    adapter: &MemoryAdapter,
    workspace: &Path,
    source_uri: &str,
    title: &str,
    description: &str,
) -> String {
    let ingest = ToolRequest::new(
        ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
            source_uri: source_uri.into(),
            source_type: "meeting_transcript".into(),
            label: Some(title.into()),
            title: title.into(),
            description: description.into(),
            suggested_level: Some("architecture".into()),
            snapshot_content: Some(description.into()),
            evidence: vec![IngestEvidenceItem {
                excerpt: description.into(),
                span_start: Some(0),
                span_end: Some(description.len()),
                confidence: Some(0.95),
            }],
            candidate_drafts: Vec::new(),
            binding_hints: Vec::new(),
            rationale: Some("Issue #772 live-path fixture".into()),
            metadata: None,
        }),
        authority("seed-agent", "mcp"),
    );
    let ingest_response = dispatch_with_workspace(&ingest, adapter, Some(workspace));
    assert_eq!(ingest_response.status, ToolResponseStatus::Ok);
    let candidate_id = ingest_response.result.unwrap()["candidate_ids"][0]
        .as_str()
        .unwrap()
        .to_string();

    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id,
            reason: Some("Accepted for live recall integration proof".into()),
        }),
        authority("reviewer", "dashboard"),
    );
    let accept_response = dispatch_with_workspace(&accept, adapter, Some(workspace));
    assert_eq!(accept_response.status, ToolResponseStatus::Ok);

    replay_from_adapter(adapter)
        .unwrap()
        .decisions
        .values()
        .find(|decision| decision.title == title)
        .unwrap()
        .id
        .to_string()
}

fn preflight_request(files: Vec<&str>, symbols: Vec<&str>) -> ToolRequest {
    ToolRequest::new(
        ToolCommand::PreflightRun(PreflightRunParams {
            files: files.into_iter().map(str::to_string).collect(),
            symbols: symbols.into_iter().map(str::to_string).collect(),
            diff_context: None,
            branch: None,
            checkpoint_hint: Some(CheckpointHint::PreWork),
            managed_planning: None,
        }),
        authority("preflight-agent", "mcp"),
    )
}

#[test]
fn live_preflight_recall_seed_readiness_retrieve_is_ranked_and_read_only() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    let relevant_id = seed_accepted_decision(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );
    let irrelevant_id = seed_accepted_decision(
        &adapter,
        workspace.path(),
        "meeting://profiles/architecture",
        "User avatars use immutable object keys",
        "Profile images must retain immutable object keys across updates.",
    );
    let before_events = adapter.replay().unwrap();
    let before_state = replay_from_adapter(&adapter).unwrap();

    let response = dispatch_with_workspace(
        &preflight_request(
            vec!["src/payments/idempotency.rs"],
            vec!["apply_idempotency"],
        ),
        &adapter,
        Some(workspace.path()),
    );
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let result = response.result.unwrap();
    let recall = &result["live_recall"];

    assert_eq!(recall["index_readiness"]["status"], "ready");
    assert_eq!(recall["index_readiness"]["record_count"], 2);
    assert_eq!(recall["index_readiness"]["ready_record_count"], 2);
    assert!(recall["index_readiness"]["corpus_version"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(recall["index_readiness"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["compilation_state"] == "accepted"
            && record["coverage_sufficient"] == true));
    assert_eq!(recall["outcome"], "results");
    assert_eq!(recall["scoring_eligible"], true);
    assert!(recall["readiness_limitations"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(recall["legacy_source_matching"], "characterization_only");
    assert_eq!(
        recall["implementation_provenance"]["surface"],
        "preflight.run"
    );
    assert_eq!(
        recall["implementation_provenance"]["compiler"],
        "decision_index_record_v1"
    );
    assert_eq!(
        recall["implementation_provenance"]["resolver"],
        "merge_and_rank_v1"
    );

    let ranked = recall["ranked_results"].as_array().unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0]["identity"]["id"], relevant_id);
    assert_ne!(ranked[0]["identity"]["id"], irrelevant_id);
    assert_eq!(ranked[0]["reason_codes"][0], "structured_corpus_match");
    assert_eq!(ranked[0]["tier"], 3);
    assert!(ranked[0]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source_ref| source_ref == "meeting://payments/architecture"));

    let packet = &recall["packet"];
    assert_eq!(packet["canonical_decisions"][0]["decision_id"], relevant_id);
    assert_eq!(packet["searched_scope"]["level"], "replayable");
    assert_eq!(packet["searched_scope"]["graph"]["status"], "searched");
    assert_eq!(
        packet["searched_scope"]["retrieval"]["version"]
            .as_str()
            .unwrap()
            .split(':')
            .next(),
        Some("decision-index-v1")
    );
    assert_eq!(
        packet["recall_trace"]["decision_index_version"],
        "decision-index-v1"
    );
    assert_eq!(packet["recall_trace"]["replay_available"], true);
    assert_eq!(
        packet["recall_trace"]["final_rank_tuples"][0]["result_id"],
        relevant_id
    );
    assert_eq!(
        packet["recall_trace"]["route_results"][0]["status"],
        "completed"
    );

    let repeated = dispatch_with_workspace(
        &preflight_request(
            vec!["src/payments/idempotency.rs"],
            vec!["apply_idempotency"],
        ),
        &adapter,
        Some(workspace.path()),
    );
    assert_eq!(
        repeated.result.unwrap()["live_recall"]["ranked_results"],
        recall["ranked_results"]
    );

    let after_events = adapter.replay().unwrap();
    let after_state = replay_from_adapter(&adapter).unwrap();
    assert_eq!(after_events.len(), before_events.len());
    assert_eq!(after_state.decisions.len(), before_state.decisions.len());
    assert_eq!(after_state.bindings.len(), before_state.bindings.len());
    assert_eq!(
        serde_json::to_value(after_state.decisions).unwrap(),
        serde_json::to_value(before_state.decisions).unwrap()
    );
    assert!(response.governance_result.is_none());
    let serialized = serde_json::to_string(&result).unwrap().to_lowercase();
    for forbidden_claim in [
        "compliance confirmed",
        "signoff approved",
        "no conflict exists",
        "safe to merge",
        "global scope complete",
    ] {
        assert!(!serialized.contains(forbidden_claim));
    }
}

#[test]
fn live_preflight_recall_complete_scope_distinguishes_no_match() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    seed_accepted_decision(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );

    let response = dispatch_with_workspace(
        &preflight_request(vec!["src/profiles/avatar.rs"], vec!["rotate_avatar_image"]),
        &adapter,
        Some(workspace.path()),
    );
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let recall = &response.result.unwrap()["live_recall"];
    assert_eq!(recall["outcome"], "no_match");
    assert_eq!(recall["scoring_eligible"], true);
    assert_eq!(
        recall["no_result_language"]["permitted_claim"],
        "no_relevant_decision_in_searched_scope"
    );
    assert!(recall["ranked_results"].as_array().unwrap().is_empty());
    assert_eq!(recall["packet"]["searched_scope"]["level"], "replayable");
    assert_eq!(
        recall["packet"]["recall_trace"]["route_results"][0]["status"],
        "empty"
    );
}

#[test]
fn live_preflight_recall_unavailable_scope_is_not_a_scored_miss() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    seed_accepted_decision(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );

    let before_events = adapter.replay().unwrap().len();
    let response = dispatch_with_workspace(
        &preflight_request(vec!["src/unknown.rs"], vec!["unknown_symbol"]),
        &adapter,
        None,
    );
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let result = response.result.unwrap();
    let recall = &result["live_recall"];
    assert_eq!(recall["outcome"], "readiness_limited");
    assert_eq!(recall["scoring_eligible"], false);
    assert!(recall["readiness_limitations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "graph_scope_notconfigured"));
    assert_eq!(
        recall["no_result_language"]["permitted_claim"],
        "enumerated_scope_only"
    );
    assert!(recall["ranked_results"].as_array().unwrap().is_empty());
    assert_eq!(recall["packet"]["searched_scope"]["level"], "enumerated");
    assert_eq!(adapter.replay().unwrap().len(), before_events);
}
