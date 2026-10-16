//! Live `preflight.run` producer path for the canonical alpha-recall
//! DecisionCandidate + decision-attention product contract (issue #827).
//!
//! Proves the daemon projects replayed proposed candidates into the canonical
//! `alpha_recall.product.packet.decision_candidates`, authors the typed
//! decision-attention signal, excludes promoted candidates, keeps a bounded
//! no-match from claiming global absence, and appends no events.

use bicameral_api::event_store::EventStoreAdapter;
use bicameral_api::tool_request::*;
use bicameral_runtime::dispatch::dispatch_with_workspace;
use bicameral_runtime::event_store_adapters::MemoryAdapter;
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
        session_id: Some("issue-827-candidate-path".into()),
        workspace: Some("/workspace".into()),
        policy_scope: vec!["default".into(), "review".into()],
        audit_metadata: {
            let mut metadata = serde_json::Map::new();
            metadata.insert("surface".into(), surface.into());
            metadata
        },
    }
}

/// Ingest a candidate and leave it in the Proposed state (no acceptance), so it
/// is surfaced as a reviewable DecisionCandidate rather than a canonical
/// Decision.
fn seed_proposed_candidate(
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
            rationale: Some("Issue #827 candidate-path fixture".into()),
            metadata: None,
        }),
        authority("seed-agent", "mcp"),
    );
    let response = dispatch_with_workspace(&ingest, adapter, Some(workspace));
    assert_eq!(response.status, ToolResponseStatus::Ok);
    response.result.unwrap()["candidate_ids"][0]
        .as_str()
        .unwrap()
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
fn proposed_candidate_is_projected_into_canonical_packet_with_attention() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    let candidate_id = seed_proposed_candidate(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );

    let before_events = adapter.replay().unwrap().len();
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
    let packet = &result["alpha_recall"]["product"]["packet"];

    let candidates = packet["decision_candidates"].as_array().unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "the proposed candidate must be surfaced"
    );
    let surfaced = &candidates[0];
    assert_eq!(surfaced["candidate_id"], candidate_id);
    assert_eq!(surfaced["candidate_state"], "proposed");
    assert_eq!(surfaced["review_state"], "proposed");
    assert_eq!(surfaced["relevance_reasons"][0], "source_text_match");
    assert_eq!(surfaced["evidence_freshness"], "current");
    // Informed-review fields are populated.
    assert!(surfaced["title"].as_str().unwrap().contains("idempotency"));
    assert!(!surfaced["summary"].as_str().unwrap().is_empty());
    assert!(!surfaced["source_refs"].as_array().unwrap().is_empty());
    let outcomes = surfaced["allowed_outcomes"].as_array().unwrap();
    assert!(outcomes.iter().any(|o| o == "new_constraint"));
    assert!(outcomes.iter().any(|o| o == "needs_signoff_review"));

    // Daemon-authored, typed decision-attention: the consumer reads it, never
    // infers it from candidate presence. Attention is warranted; the reason is
    // one of the daemon's typed reviewable reasons.
    assert_eq!(packet["decision_attention"]["state"], "warranted");
    let reason = packet["decision_attention"]["reason"].as_str().unwrap();
    assert!(
        matches!(
            reason,
            "reviewable_candidates_present" | "candidate_ambiguity"
        ),
        "unexpected decision-attention reason: {reason}"
    );
    assert!(!packet["decision_attention"]["detail"]
        .as_str()
        .unwrap()
        .is_empty());

    // Read-only: the preflight lookup appended no events.
    assert_eq!(adapter.replay().unwrap().len(), before_events);
    assert!(response.governance_result.is_none());
}

#[test]
fn bounded_no_match_reports_explicit_not_warranted_attention() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    seed_proposed_candidate(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );

    // A query in a disjoint scope: the candidate must not be surfaced and the
    // attention signal must be an explicit not-warranted state, not an inferred
    // global absence.
    let response = dispatch_with_workspace(
        &preflight_request(vec!["src/profiles/avatar.rs"], vec!["rotate_avatar_image"]),
        &adapter,
        Some(workspace.path()),
    );
    assert_eq!(response.status, ToolResponseStatus::Ok);
    let result = response.result.unwrap();
    let packet = &result["alpha_recall"]["product"]["packet"];

    // The empty candidate list is omitted from the wire form; absence of
    // candidates is still an explicit, typed no-attention signal.
    assert!(packet
        .get("decision_candidates")
        .and_then(|v| v.as_array())
        .map(|c| c.is_empty())
        .unwrap_or(true));
    assert_eq!(packet["decision_attention"]["state"], "not_warranted");
    assert_eq!(
        packet["decision_attention"]["reason"],
        "no_reviewable_candidates"
    );

    // The bounded no-match must not be dressed up as a global compliance claim.
    let serialized = serde_json::to_string(&result).unwrap().to_lowercase();
    for forbidden in [
        "compliance confirmed",
        "no conflict exists",
        "safe to merge",
        "global scope complete",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn promoted_candidate_is_not_surfaced_as_a_reviewable_candidate() {
    let workspace = setup_workspace();
    let adapter = MemoryAdapter::new();
    let candidate_id = seed_proposed_candidate(
        &adapter,
        workspace.path(),
        "meeting://payments/architecture",
        "Payment retries require idempotency keys",
        "Every payment retry must call apply_idempotency before settlement.",
    );

    // Accept the candidate — it becomes a canonical Decision and must no longer
    // appear in the reviewable candidate list.
    let accept = ToolRequest::new(
        ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
            target_id: candidate_id.clone(),
            reason: Some("Accepted; must leave the candidate list".into()),
        }),
        authority("reviewer", "dashboard"),
    );
    assert_eq!(
        dispatch_with_workspace(&accept, &adapter, Some(workspace.path())).status,
        ToolResponseStatus::Ok
    );

    let response = dispatch_with_workspace(
        &preflight_request(
            vec!["src/payments/idempotency.rs"],
            vec!["apply_idempotency"],
        ),
        &adapter,
        Some(workspace.path()),
    );
    let result = response.result.unwrap();
    let packet = &result["alpha_recall"]["product"]["packet"];

    let empty = Vec::new();
    let surfaced_ids: Vec<&str> = packet["decision_candidates"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|c| c["candidate_id"].as_str().unwrap())
        .collect();
    assert!(
        !surfaced_ids.contains(&candidate_id.as_str()),
        "a promoted candidate must not be surfaced for review"
    );
}
