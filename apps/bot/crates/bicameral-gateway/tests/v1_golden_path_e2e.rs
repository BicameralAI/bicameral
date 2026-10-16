//! V1 local golden-path end-to-end verification.
//!
//! Exercises the complete V1 lifecycle on a fresh in-memory gateway:
//!
//!   ingest source evidence
//!   → accept candidate → Decision(proposed)
//!   → approve signoff → Decision(approved)
//!   → attach verified code evidence (fixture)
//!   → rebuild reviewable-context projection from event-store replay
//!   → read Ledger View from projection
//!   → confirm no compliance / enforcement / readiness-to-ship claims
//!
//! Per issue #236, ADR-0022, and the `v1-reviewable-context-projection` spec.
//!
//! Dependencies exercised:
//!   - #232 synthetic code-grounding fixture (`tests/fixtures/code-grounding`)
//!   - #233 V1 reviewable-context projection read model
//!   - #234 Ledger View projection-backed reads
//!
//! Non-goals:
//!   - No hosted/team graph scope.
//!   - No Enforcement Memory compliance workflows.
//!   - No connector-specific refresh.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bicameral_api::dashboard::SignoffState;
use bicameral_audit::AuditStore;
use bicameral_gateway::routes::{
    bind_routes, dashboard_api_routes, ingest_routes, projection_api_routes,
};
use bicameral_gateway::state::AppState;
use tower::ServiceExt;
use uuid::Uuid;

fn test_audit() -> AuditStore {
    let dir = tempfile::tempdir().unwrap();
    AuditStore::open(dir.path()).unwrap()
}

/// Helper: POST JSON to a route and return (status, body bytes).
async fn post_json(app: Router, uri: &str, payload: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

/// Helper: GET a route and return (status, body bytes).
async fn get_json(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body.to_vec())
}

fn build_app(state: AppState) -> Router {
    Router::new()
        .merge(ingest_routes())
        .merge(dashboard_api_routes())
        .merge(projection_api_routes())
        .merge(bind_routes())
        .with_state(state)
}

// ── Full golden-path E2E ─────────────────────────────────────────

/// Runs the complete V1 local golden path:
///
///   ingest → accept → signoff → bind code evidence → rebuild projection
///   → read Ledger View → verify no compliance/enforcement claims
///
/// This test is the primary acceptance evidence for issue #236.
#[tokio::test]
async fn v1_golden_path_full_lifecycle() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());

    // ── Step 1: Ingest source evidence ───────────────────────────
    //
    // Uses the synthetic fixture scenario: a Jira decision about using
    // the code-grounding fixture workspace's `process_order` function.
    let ingest_body = serde_json::json!({
        "title": "All order processing must use process_order",
        "description": "The team decided that all order processing logic must \
            route through the process_order function in the code-grounding \
            fixture workspace for consistency and auditability.",
        "level": "product",
        "source": "jira://PROJ-236",
        "source_type": "jira",
        "label": "V1 golden-path verification",
        "snapshot_content": "Meeting notes 2026-06-08: All order processing logic must \
            route through process_order for consistency and auditability. Agreed by \
            engineering lead and product owner.",
        "evidence": [{
            "span_start": 0,
            "span_end": 120,
            "excerpt": "All order processing logic must route through process_order \
                for consistency and auditability.",
            "confidence": 0.95
        }]
    });

    let (status, _body) = post_json(app.clone(), "/api/v1/ingest", ingest_body).await;
    assert_eq!(status, StatusCode::CREATED, "Step 1: ingest must succeed");

    // Capture candidate ID.
    let candidate_id = {
        let candidates = state.candidates.read().await;
        assert_eq!(candidates.len(), 1, "exactly one candidate after ingest");
        candidates.keys().next().unwrap().to_string()
    };

    // Verify source evidence was captured.
    {
        let evidence = state.evidence.read().await;
        assert!(
            !evidence.is_empty(),
            "Step 1: source evidence must be captured"
        );
        let ev = evidence.values().next().unwrap();
        assert!(
            ev.excerpt.contains("process_order"),
            "Step 1: evidence excerpt must contain the cited content"
        );
    }

    println!("  ✓ Step 1: Source evidence ingested (candidate={candidate_id})");

    // ── Step 2: Accept candidate → Decision(proposed) ────────────

    let accept_cmd = serde_json::json!({
        "target_id": candidate_id,
        "command": "accept_candidate",
        "reason": "governance"
    });

    let (status, _) = post_json(app.clone(), "/api/v1/dashboard/command", accept_cmd).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "Step 2: accept_candidate must succeed"
    );

    let decision_id = {
        let decisions = state.decisions.read().await;
        assert_eq!(decisions.len(), 1, "exactly one Decision after acceptance");
        let d = decisions.values().next().unwrap();
        assert_eq!(
            d.signoff_state,
            SignoffState::Proposed,
            "Step 2: Decision signoff must be Proposed"
        );
        assert_eq!(d.title, "All order processing must use process_order");
        d.id.to_string()
    };

    println!("  ✓ Step 2: Candidate accepted → Decision(proposed) (decision={decision_id})");

    // ── Step 3: Approve signoff → Decision(approved) ─────────────

    let signoff_cmd = serde_json::json!({
        "target_id": decision_id,
        "command": "approve_signoff",
        "reason": "tech-lead"
    });

    let (status, _) = post_json(app.clone(), "/api/v1/dashboard/command", signoff_cmd).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "Step 3: approve_signoff must succeed"
    );

    {
        let decisions = state.decisions.read().await;
        let d = decisions
            .get(&Uuid::parse_str(&decision_id).unwrap())
            .expect("decision must exist");
        assert_eq!(
            d.signoff_state,
            SignoffState::Approved,
            "Step 3: Decision signoff must be Approved"
        );
    }

    println!("  ✓ Step 3: Signoff approved → Decision(approved)");

    // ── Step 4: Attach verified code evidence from fixture ───────
    //
    // Binds the Decision to `src/lib.rs` (process_order function) in
    // the committed code-grounding fixture (tests/fixtures/code-grounding).
    // Uses a synthetic snapshot address and verified evidence state.

    let bind_body = serde_json::json!({
        "decision_id": decision_id,
        "proposal": {
            "candidate_id": candidate_id,
            "binding_type": "file_path",
            "target": "tests/fixtures/code-grounding/src/lib.rs:1:15",
            "confidence": "high",
            "explanation": "process_order function in fixture workspace",
            "graph_snapshot_id": "sha256:fixture-snapshot-236",
            "validated_sha": "abc123def456",
            "content_hash": "sha256:lib-rs-content-hash"
        },
        "evidence_state": "verified",
        "line_start": 1,
        "line_end": 15
    });

    let (status, body) = post_json(app.clone(), "/api/v1/bind", bind_body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Step 4: bind_to_code must succeed"
    );

    let bind_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        bind_resp["status"].as_str().unwrap(),
        "bound",
        "Step 4: binding must be accepted"
    );
    let binding_id = bind_resp["binding_id"].as_str().unwrap();

    // Verify binding was stored.
    {
        let bindings = state.bindings.read().await;
        let decision_uuid = Uuid::parse_str(&decision_id).unwrap();
        let bs = bindings.get(&decision_uuid).expect("bindings for decision");
        assert_eq!(bs.len(), 1, "Step 4: exactly one binding after bind");
        assert!(
            bs[0].target.contains("lib.rs"),
            "Step 4: binding target must reference fixture file"
        );
    }

    println!("  ✓ Step 4: Code evidence bound (binding={binding_id})");

    // ── Step 5: Rebuild projection from event-store replay ───────

    let (status, proj_body) = post_json(
        app.clone(),
        "/api/v1/projection/rebuild",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Step 5: projection rebuild must succeed"
    );

    let proj: serde_json::Value = serde_json::from_slice(&proj_body).unwrap();

    // Verify projection contains the Decision.
    assert!(
        !proj["decisions"].as_object().unwrap().is_empty(),
        "Step 5: projection must contain the Decision"
    );

    // Verify projection contains source evidence.
    assert!(
        !proj["source_evidence"].as_object().unwrap().is_empty(),
        "Step 5: projection must contain source evidence"
    );

    // Verify projection contains code evidence.
    assert!(
        !proj["code_evidence"].as_object().unwrap().is_empty(),
        "Step 5: projection must contain code evidence"
    );

    // Verify projection contains graph readiness.
    assert!(
        !proj["graph_readiness"].as_object().unwrap().is_empty(),
        "Step 5: projection must contain graph readiness"
    );

    // Verify projection contains ledger events.
    assert!(
        !proj["ledger_events"].as_array().unwrap().is_empty(),
        "Step 5: projection must contain ledger event history"
    );

    println!("  ✓ Step 5: Projection rebuilt from event-store replay");

    // ── Step 6: Read Ledger View from projection ─────────────────

    let (status, ledger_body) = get_json(app.clone(), "/api/v1/projection/ledger").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Step 6: projection ledger read must succeed"
    );

    let ledger: serde_json::Value = serde_json::from_slice(&ledger_body).unwrap();

    // Verify Decisions are present.
    assert!(
        !ledger["decisions"].as_object().unwrap().is_empty(),
        "Step 6: Ledger View must contain Decisions"
    );

    // Verify source evidence from cited snapshot.
    assert!(
        !ledger["source_evidence"].as_object().unwrap().is_empty(),
        "Step 6: Ledger View must contain source evidence"
    );

    // Verify code evidence from fixture binding.
    assert!(
        !ledger["code_evidence"].as_object().unwrap().is_empty(),
        "Step 6: Ledger View must contain code evidence"
    );

    // Verify graph snapshots.
    assert!(
        !ledger["graph_snapshots"].as_object().unwrap().is_empty(),
        "Step 6: Ledger View must contain graph snapshots"
    );

    // Verify graph readiness.
    assert!(
        !ledger["graph_readiness"].as_object().unwrap().is_empty(),
        "Step 6: Ledger View must contain graph readiness"
    );

    println!("  ✓ Step 6: Ledger View read from projection");

    // ── Step 7: Dashboard Ledger View reads from projection ──────
    //
    // Wire the projection into the AppState and verify the
    // dashboard ledger endpoint reads from it — exercising the
    // #234 projection-backed read path.

    {
        use bicameral_api::projection::ProjectionState;

        // Parse the projection.
        let proj_state: ProjectionState = serde_json::from_slice(&proj_body).unwrap();

        // Store the projection in AppState.
        let mut projection_guard = state.projection.write().await;
        *projection_guard = Some(proj_state);
    }

    let (status, dashboard_body) = get_json(app.clone(), "/api/v1/dashboard/ledger").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Step 7: dashboard ledger read must succeed"
    );

    let items: Vec<serde_json::Value> = serde_json::from_slice(&dashboard_body).unwrap();
    assert!(
        !items.is_empty(),
        "Step 7: dashboard Ledger View must contain items"
    );

    // Verify Decision properties.
    let decision_item = items
        .iter()
        .find(|i| i["kind"].as_str() == Some("decision"))
        .expect("Step 7: Ledger View must contain a decision item");
    assert_eq!(
        decision_item["summary"].as_str().unwrap(),
        "All order processing must use process_order"
    );
    assert_eq!(decision_item["signoff"].as_str().unwrap(), "approved");

    // Verify source evidence is present.
    let sources = decision_item["sources"].as_array();
    assert!(
        sources.is_some() && !sources.unwrap().is_empty(),
        "Step 7: Decision must have source evidence"
    );

    // Verify code evidence section is present.
    let code_evidence = &decision_item["code_evidence"];
    assert!(
        !code_evidence.is_null(),
        "Step 7: Decision must have code evidence section"
    );

    // Verify code regions from fixture binding.
    let regions = decision_item["regions"].as_array();
    assert!(
        regions.is_some() && !regions.unwrap().is_empty(),
        "Step 7: Decision must have code regions from binding"
    );
    let region = &regions.unwrap()[0];
    assert!(
        region["file"].as_str().unwrap().contains("lib.rs"),
        "Step 7: code region must reference fixture file"
    );

    // Verify graph readiness section is present.
    let graph_readiness = &decision_item["graph_readiness"];
    assert!(
        !graph_readiness.is_null(),
        "Step 7: Decision must have graph readiness"
    );

    println!("  ✓ Step 7: Dashboard Ledger View reads from projection with source/code evidence and graph readiness");

    // ── Step 8: No compliance/enforcement/readiness-to-ship ──────
    //
    // The V1 path must not expose compliance verdicts, enforcement,
    // implementation correctness, feature readiness, or merge safety
    // (ADR-0022).

    let all_bodies = [
        String::from_utf8_lossy(&proj_body).to_string(),
        String::from_utf8_lossy(&ledger_body).to_string(),
        String::from_utf8_lossy(&dashboard_body).to_string(),
    ];

    let forbidden_terms = [
        "compliance_state",
        "compliance_verdict",
        "enforcement",
        "readiness_to_ship",
        "ready_to_ship",
        "merge_safety",
        "blocking_enforcement",
        "implementation_correctness",
        "feature_readiness",
    ];

    for body_str in &all_bodies {
        for term in &forbidden_terms {
            assert!(
                !body_str.contains(term),
                "V1 path must not contain '{term}' — ADR-0022 boundary violated"
            );
        }
    }

    println!("  ✓ Step 8: No compliance/enforcement/readiness-to-ship claims on V1 path");

    // ── Step 9: Verify event-store actor chain ───────────────────
    //
    // The ledger event history must preserve the full actor chain:
    // extractor → governance (accept) → dashboard (signoff) → gateway (bind).

    let events = proj["ledger_events"].as_array().unwrap();
    assert!(
        events.len() >= 4,
        "Step 9: event history must contain at least 4 events (ingest, accept, signoff, bind)"
    );

    // Verify event types in order.
    let event_summaries: Vec<&str> = events
        .iter()
        .map(|e| e["summary"].as_str().unwrap())
        .collect();
    assert!(
        event_summaries[0].contains("ingested"),
        "Step 9: first event must be ingest"
    );
    assert!(
        event_summaries[1].contains("accepted"),
        "Step 9: second event must be accept"
    );
    assert!(
        event_summaries[2].contains("approved"),
        "Step 9: third event must be signoff"
    );
    assert!(
        event_summaries[3].contains("Bound"),
        "Step 9: fourth event must be bind"
    );

    println!("  ✓ Step 9: Event-store actor chain preserved");

    // ── Summary ──────────────────────────────────────────────────

    println!();
    println!("V1 Golden Path — PASSED");
    println!("  Decision: {decision_id}");
    println!("  Candidate: {candidate_id}");
    println!("  Binding: {binding_id}");
    println!(
        "  Projection records: {} decisions, {} source_evidence, {} code_evidence, {} events",
        proj["decisions"].as_object().unwrap().len(),
        proj["source_evidence"].as_object().unwrap().len(),
        proj["code_evidence"].as_object().unwrap().len(),
        events.len(),
    );
    println!(
        "  Dashboard Ledger View: {} items from projection-backed read path",
        items.len()
    );
    println!("  Compliance/enforcement claims: NONE (ADR-0022 boundary verified)");
}

// ── Tracked vs untracked freshness (soft dependency #235) ────────

/// Note: Issue #235 (PR #241) has not merged to `dev` yet.
/// When #235 merges, this test should be expanded to verify:
///   - Tracked Decisions surface freshness warnings and refresh actions.
///   - Untracked Decisions do not surface staleness as a governance failure.
///
/// For now, this test verifies that the `tracking_state` field is present
/// in the projection and the Ledger View, establishing the structural
/// readiness for #235 acceptance.
///
/// Follow-up: #189 final acceptance once #235 merges.
#[tokio::test]
async fn v1_tracking_state_structural_readiness() {
    let state = AppState::new(test_audit());
    let app = build_app(state.clone());

    // Ingest + accept to get a Decision with tracking_state.
    let ingest_body = serde_json::json!({
        "title": "Tracking state check",
        "description": "Verifies tracking_state is present in projection",
        "level": "product",
        "source": "jira://TRACK-1",
        "evidence": [{
            "span_start": 0,
            "span_end": 10,
            "excerpt": "Tracking state test"
        }]
    });

    let (status, _) = post_json(app.clone(), "/api/v1/ingest", ingest_body).await;
    assert_eq!(status, StatusCode::CREATED);

    let candidate_id = state
        .candidates
        .read()
        .await
        .keys()
        .next()
        .unwrap()
        .to_string();

    let (status, _) = post_json(
        app.clone(),
        "/api/v1/dashboard/command",
        serde_json::json!({
            "target_id": candidate_id,
            "command": "accept_candidate"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // Read from projection endpoint.
    let (status, body) = get_json(app.clone(), "/api/v1/projection/ledger").await;
    assert_eq!(status, StatusCode::OK);

    let proj: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let decision = proj["decisions"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .expect("must have a decision");

    // tracking_state is present.
    assert!(
        decision.get("tracking_state").is_some(),
        "tracking_state must be present in projected Decision"
    );

    // Default tracking_state for promoted Decisions is Tracked.
    assert_eq!(
        decision["tracking_state"].as_str().unwrap(),
        "tracked",
        "promoted Decision should default to tracked"
    );

    println!("  ✓ tracking_state structural readiness verified (full #235 follow-up: #189)");
}
