//! Schema generation and drift-detection test.
//!
//! Generates JSON Schema from the Rust types and verifies committed schemas
//! under `protocol/schemas/` still match the implementation.
//!
//! To update committed schemas after a struct change:
//!   WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen

use schemars::schema_for;

use bicameral_api::provider_acquisition::{ProviderItemEnvelope, ProviderResourceDescriptor};
use bicameral_api::workspace_binding::{
    WorkspaceBindErrorResponse, WorkspaceBindRequest, WorkspaceBindResponse,
    WorkspaceBindingProposal, WorkspaceBindingReadinessView,
};
use bicameral_api::ExternalIngestEnvelope;
use bicameral_gateway::routes::{ExternalIngestResponse, IngestRequest, IngestResponse};

fn schema_dir_v1() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/schemas/v1")
}

fn schema_dir_v2() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/schemas/v2")
}

fn assert_or_write(dir: &std::path::Path, filename: &str, fresh: &str) {
    let path = dir.join(filename);
    if std::env::var("WRITE_SCHEMAS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", fresh.trim())).unwrap();
        eprintln!("wrote {}", path.display());
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} not found — run `WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen`",
            path.display()
        )
    });
    assert_eq!(
        committed.trim(),
        fresh.trim(),
        "{filename} is stale — run `WRITE_SCHEMAS=1 cargo test -p bicameral-gateway --test schema_gen`"
    );
}

#[test]
fn ingest_request_schema_matches_committed() {
    let schema = schema_for!(IngestRequest);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(&schema_dir_v1(), "ingest-request.schema.json", &json);
}

#[test]
fn ingest_response_schema_matches_committed() {
    let schema = schema_for!(IngestResponse);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(&schema_dir_v1(), "ingest-response.schema.json", &json);
}

#[test]
fn external_ingest_request_schema_matches_committed() {
    let schema = schema_for!(ExternalIngestEnvelope);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "external-ingest-request.schema.json",
        &json,
    );
}

#[test]
fn external_ingest_response_schema_matches_committed() {
    let schema = schema_for!(ExternalIngestResponse);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "external-ingest-response.schema.json",
        &json,
    );
}

#[test]
fn provider_resource_descriptor_schema_matches_committed() {
    let schema = schema_for!(ProviderResourceDescriptor);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "provider-resource-descriptor.schema.json",
        &json,
    );
}

#[test]
fn provider_item_envelope_schema_matches_committed() {
    let schema = schema_for!(ProviderItemEnvelope);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "provider-item-envelope.schema.json",
        &json,
    );
}

#[test]
fn workspace_binding_proposal_schema_matches_committed() {
    let schema = schema_for!(WorkspaceBindingProposal);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "workspace-binding-proposal.schema.json",
        &json,
    );
}

#[test]
fn workspace_bind_request_schema_matches_committed() {
    let schema = schema_for!(WorkspaceBindRequest);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "workspace-bind-request.schema.json",
        &json,
    );
}

#[test]
fn workspace_bind_response_schema_matches_committed() {
    let schema = schema_for!(WorkspaceBindResponse);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "workspace-bind-response.schema.json",
        &json,
    );
}

#[test]
fn workspace_bind_error_schema_matches_committed() {
    let schema = schema_for!(WorkspaceBindErrorResponse);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(&schema_dir_v2(), "workspace-bind-error.schema.json", &json);
}

#[test]
fn workspace_binding_readiness_view_schema_matches_committed() {
    let schema = schema_for!(WorkspaceBindingReadinessView);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert_or_write(
        &schema_dir_v2(),
        "workspace-binding-readiness-view.schema.json",
        &json,
    );
}
