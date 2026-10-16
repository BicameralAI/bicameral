use std::sync::Arc;

use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_gateway::live_projection_work::{LiveProjectionWorkStore, ProjectionResumeInput};
use bicameral_gateway::projection_work_repository::{
    ProjectionWorkRepository, ProjectionWorkRequest, SqliteProjectionWorkRepository,
    WorkingCandidate,
};
use chrono::{Duration as ChronoDuration, Utc};

struct SeededStore {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    store: LiveProjectionWorkStore,
    candidate: DecisionCandidate,
    fingerprint: String,
    acceptance_key: String,
    source_uri: String,
    content_digest: String,
}

fn seeded_store() -> SeededStore {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("candidate-disposition.sqlite3");
    let repository = Arc::new(SqliteProjectionWorkRepository::open(&path).unwrap());
    let store = LiveProjectionWorkStore::open(repository.clone()).unwrap();
    let now = Utc::now();
    let acceptance_key = "acceptance:github:issue-42:v1".to_string();
    let source_uri = "https://github.example/repo/issues/42".to_string();
    let content_digest = "sha256:screened-v1".to_string();
    let fingerprint = "sha256:decision-fingerprint".to_string();
    let request =
        ProjectionWorkRequest::pending(acceptance_key.clone(), "github", source_uri.clone(), now);
    repository.enqueue(&request).unwrap();
    store
        .store_resume_input(
            request.request_id,
            &ProjectionResumeInput {
                content_locator: "sha256/aa/screened-v1".to_string(),
                content_digest: content_digest.clone(),
                evidence_excerpts: vec!["Use SQLite for local working state.".to_string()],
                candidate_hints: Vec::new(),
            },
            now,
        )
        .unwrap();
    let working = WorkingCandidate::active(
        request.request_id,
        acceptance_key.clone(),
        fingerprint.clone(),
        "Use SQLite",
        "Use daemon-local SQLite for projection working state.",
        "architecture",
        now,
    );
    let converged = repository.converge_candidate(&working).unwrap();
    let mut candidate = DecisionCandidate::new(
        "Use SQLite".to_string(),
        "Use daemon-local SQLite for projection working state.".to_string(),
        DecisionLevel::Architecture,
        source_uri.clone(),
    );
    candidate.id = converged.candidate_id;
    store
        .store_candidate_payload(request.request_id, &acceptance_key, &candidate, now)
        .unwrap();

    SeededStore {
        _dir: dir,
        path,
        store,
        candidate,
        fingerprint,
        acceptance_key,
        source_uri,
        content_digest,
    }
}

#[test]
fn hide_is_session_scoped_and_lost_on_restart() {
    let seeded = seeded_store();
    let candidate_id = seeded.candidate.id;

    assert!(seeded
        .store
        .hide_candidate("session:one", candidate_id)
        .unwrap());
    assert!(seeded.store.is_hidden("session:one", candidate_id));
    assert!(!seeded.store.is_hidden("session:two", candidate_id));
    assert!(seeded
        .store
        .visible_candidate_payloads("session:one")
        .unwrap()
        .is_empty());
    assert_eq!(
        seeded
            .store
            .visible_candidate_payloads("session:two")
            .unwrap()
            .len(),
        1
    );

    let reopened_repository = Arc::new(SqliteProjectionWorkRepository::open(&seeded.path).unwrap());
    let reopened = LiveProjectionWorkStore::open(reopened_repository).unwrap();
    assert!(!reopened.is_hidden("session:one", candidate_id));
    assert_eq!(
        reopened
            .visible_candidate_payloads("session:one")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn suppression_survives_restart_and_same_evidence_remains_suppressed() {
    let seeded = seeded_store();
    let now = Utc::now();
    let candidate_id = seeded.candidate.id;
    let suppression = seeded
        .store
        .suppress_candidate(
            candidate_id,
            "Duplicate architecture proposal",
            Some(now + ChronoDuration::hours(1)),
            now,
        )
        .unwrap()
        .unwrap();
    assert_eq!(suppression.candidate_id, candidate_id);
    assert!(seeded
        .store
        .active_candidate_payloads(now)
        .unwrap()
        .is_empty());
    assert!(seeded
        .store
        .suppression_applies(
            &seeded.fingerprint,
            &seeded.acceptance_key,
            &seeded.source_uri,
            &seeded.content_digest,
            now,
        )
        .unwrap());

    let reopened_repository = Arc::new(SqliteProjectionWorkRepository::open(&seeded.path).unwrap());
    let reopened = LiveProjectionWorkStore::open(reopened_repository).unwrap();
    assert!(reopened.active_candidate_payloads(now).unwrap().is_empty());
    assert!(reopened
        .suppression_applies(
            &seeded.fingerprint,
            &seeded.acceptance_key,
            &seeded.source_uri,
            &seeded.content_digest,
            now,
        )
        .unwrap());
}

#[test]
fn changed_evidence_invalidates_suppression_and_reactivates_candidate() {
    let seeded = seeded_store();
    let now = Utc::now();
    seeded
        .store
        .suppress_candidate(seeded.candidate.id, "Wait for changed evidence", None, now)
        .unwrap()
        .unwrap();

    assert!(!seeded
        .store
        .suppression_applies(
            &seeded.fingerprint,
            &seeded.acceptance_key,
            &seeded.source_uri,
            "sha256:screened-v2",
            now + ChronoDuration::seconds(1),
        )
        .unwrap());
    assert_eq!(
        seeded
            .store
            .active_candidate_payloads(now + ChronoDuration::seconds(1))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn expiry_reactivates_candidate_without_canonical_history() {
    let seeded = seeded_store();
    let now = Utc::now();
    seeded
        .store
        .suppress_candidate(
            seeded.candidate.id,
            "Hide until tomorrow",
            Some(now + ChronoDuration::minutes(5)),
            now,
        )
        .unwrap()
        .unwrap();

    assert!(seeded
        .store
        .active_candidate_payloads(now + ChronoDuration::minutes(1))
        .unwrap()
        .is_empty());
    assert_eq!(
        seeded
            .store
            .active_candidate_payloads(now + ChronoDuration::minutes(6))
            .unwrap()
            .len(),
        1
    );
}
