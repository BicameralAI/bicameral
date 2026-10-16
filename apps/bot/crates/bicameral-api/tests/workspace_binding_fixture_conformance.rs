//! Deterministic conformance for the workspace binding proposal fixtures
//! (bicameral-bot#731).
//!
//! Each protocol JSON fixture under `protocol/schemas/v2/fixtures/` for
//! `workspace-binding-*` must:
//!
//! - deserialize into the `bicameral_api::workspace_binding` wire types;
//! - reproduce its declared outcome through the pure `classify_bind_request`
//!   classifier (so downstream bot / hosted-dashboard / MCP workers derive the
//!   same result before daemon persistence lands);
//! - keep every local absolute path out of the hosted-safe view.

use std::path::PathBuf;

use bicameral_api::workspace_binding::{
    classify_bind_request, DaemonBindingFacts, WorkspaceBindErrorResponse, WorkspaceBindRequest,
    WorkspaceBindResponse, WorkspaceBindingReadinessView, WorkspaceBindingState,
};

const SCENARIOS: &[&str] = &[
    "workspace-binding-unbound",
    "workspace-binding-bound",
    "workspace-binding-repair-required",
    "workspace-binding-rejected-unsafe-path",
    "workspace-binding-confirmation-missing",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/schemas/v2/fixtures")
        .canonicalize()
        .expect("fixtures dir exists")
}

fn read_fixture(name: &str) -> serde_json::Value {
    let path = fixtures_dir().join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{name}: read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"))
}

fn contains_absolute_path(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => s.starts_with('/'),
        serde_json::Value::Array(items) => items.iter().any(contains_absolute_path),
        serde_json::Value::Object(map) => map.values().any(contains_absolute_path),
        _ => false,
    }
}

fn contains_key_deep(value: &serde_json::Value, key: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key(key) || map.values().any(|v| contains_key_deep(v, key))
        }
        serde_json::Value::Array(items) => items.iter().any(|v| contains_key_deep(v, key)),
        _ => false,
    }
}

/// Every fixture deserializes into the wire types without loss.
#[test]
fn fixtures_deserialize_into_wire_types() {
    for name in SCENARIOS {
        let doc = read_fixture(name);
        let local = doc
            .get("local")
            .unwrap_or_else(|| panic!("{name}: missing `local`"));

        let request: WorkspaceBindRequest =
            serde_json::from_value(local.get("request").expect("request").clone())
                .unwrap_or_else(|e| panic!("{name}: request does not deserialize: {e}"));
        assert_eq!(
            request.proposal.project_id, "proj_ada",
            "{name}: project id"
        );

        if let Some(response) = local.get("response") {
            let _: WorkspaceBindResponse = serde_json::from_value(response.clone())
                .unwrap_or_else(|e| panic!("{name}: response does not deserialize: {e}"));
        }
        if let Some(error) = local.get("error") {
            let _: WorkspaceBindErrorResponse = serde_json::from_value(error.clone())
                .unwrap_or_else(|e| panic!("{name}: error does not deserialize: {e}"));
        }

        let view: WorkspaceBindingReadinessView =
            serde_json::from_value(doc.get("hosted_safe_view").expect("view").clone())
                .unwrap_or_else(|e| panic!("{name}: hosted_safe_view does not deserialize: {e}"));
        assert_eq!(view.project_id, "proj_ada", "{name}: view project id");
    }
}

/// No hosted-safe view carries a local absolute path or the local-only
/// `candidate_path` field.
#[test]
fn hosted_safe_views_have_no_local_path() {
    for name in SCENARIOS {
        let doc = read_fixture(name);
        let view = doc
            .get("hosted_safe_view")
            .unwrap_or_else(|| panic!("{name}: missing hosted_safe_view"));
        assert!(
            !contains_key_deep(view, "candidate_path"),
            "{name}: hosted_safe_view must not contain candidate_path"
        );
        assert!(
            !contains_absolute_path(view),
            "{name}: hosted_safe_view must not contain a local absolute path"
        );
    }
}

/// The declared success response reproduces from the classifier.
#[test]
fn bound_scenario_reproduces_from_classifier() {
    let doc = read_fixture("workspace-binding-bound");
    let request: WorkspaceBindRequest =
        serde_json::from_value(doc["local"]["request"].clone()).unwrap();
    let expected: WorkspaceBindResponse =
        serde_json::from_value(doc["local"]["response"].clone()).unwrap();

    let facts = DaemonBindingFacts {
        project_registered: true,
        session_project_id: Some("proj_ada".into()),
        current_state: WorkspaceBindingState::LocalWorkspaceUnbound,
        daemon_capability: 1,
        candidate_path_safe: true,
    };
    let got = classify_bind_request(&request, &facts).expect("bound scenario succeeds");
    assert_eq!(got, expected);
}

/// Each rejection fixture's typed error reproduces from the classifier under the
/// facts that scenario describes.
#[test]
fn rejection_scenarios_reproduce_from_classifier() {
    let cases = [
        (
            "workspace-binding-confirmation-missing",
            DaemonBindingFacts {
                project_registered: true,
                session_project_id: Some("proj_ada".into()),
                current_state: WorkspaceBindingState::LocalWorkspaceUnbound,
                daemon_capability: 1,
                candidate_path_safe: true,
            },
        ),
        (
            "workspace-binding-rejected-unsafe-path",
            DaemonBindingFacts {
                project_registered: true,
                session_project_id: Some("proj_ada".into()),
                current_state: WorkspaceBindingState::LocalWorkspaceUnbound,
                daemon_capability: 1,
                candidate_path_safe: false,
            },
        ),
        (
            "workspace-binding-repair-required",
            DaemonBindingFacts {
                project_registered: true,
                session_project_id: Some("proj_ada".into()),
                current_state: WorkspaceBindingState::LocalWorkspaceRepairRequired,
                daemon_capability: 1,
                candidate_path_safe: true,
            },
        ),
    ];

    for (name, facts) in cases {
        let doc = read_fixture(name);
        let request: WorkspaceBindRequest =
            serde_json::from_value(doc["local"]["request"].clone()).unwrap();
        let expected: WorkspaceBindErrorResponse =
            serde_json::from_value(doc["local"]["error"].clone()).unwrap();

        let got = classify_bind_request(&request, &facts)
            .expect_err(&format!("{name}: expected typed rejection"));
        assert_eq!(
            got, expected,
            "{name}: classifier reproduces the fixture error"
        );
    }
}
