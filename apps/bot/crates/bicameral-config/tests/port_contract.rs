//! Cross-artifact contract test for the canonical daemon port.
//!
//! The daemon default port is the single source of truth for the
//! browser/MCP/dashboard <-> daemon contract. It is declared in three places
//! that MUST agree:
//!
//!   1. Code:  `bicameral_config::DEFAULT_GATEWAY_PORT` (schema.rs default_port)
//!   2. Docs:  `protocol/README.md` "Port contract" section
//!   3. Cloud: bicameral-cloud hosted-bridge default (guarded by a test there)
//!
//! When (1) and (2) drift, browser -> daemon pairing silently breaks — exactly
//! what happened when the cloud expected 19520 while the daemon bound 37373.
//! This test fails loudly on that drift so docs and code can never diverge.

use bicameral_config::DEFAULT_GATEWAY_PORT;

/// Path to the repo-root `protocol/README.md` relative to this crate.
fn protocol_readme() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR"); // crates/bicameral-config
    let path = format!("{manifest_dir}/../../protocol/README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read protocol/README.md at {path}: {e}"))
}

#[test]
fn protocol_doc_default_bind_matches_code_constant() {
    let readme = protocol_readme();
    let expected = format!("127.0.0.1:{DEFAULT_GATEWAY_PORT}");
    assert!(
        readme.contains(&expected),
        "protocol/README.md must document the canonical default bind address `{expected}` \
         (from bicameral_config::DEFAULT_GATEWAY_PORT). Update the doc and code together."
    );
}

#[test]
fn protocol_doc_names_canonical_port_as_source_of_truth() {
    let readme = protocol_readme();
    let expected = format!("canonical default port `{DEFAULT_GATEWAY_PORT}`");
    assert!(
        readme.contains(&expected),
        "protocol/README.md must declare `{expected}` as the single source of truth; \
         it drifted from bicameral_config::DEFAULT_GATEWAY_PORT ({DEFAULT_GATEWAY_PORT})."
    );
}

#[test]
fn protocol_doc_has_no_stale_ports() {
    // Guard against a specific historical divergence: the cloud once hardcoded
    // 19520 while the daemon bound 37373. Ensure the stale value never creeps
    // back into the protocol doc.
    let readme = protocol_readme();
    assert!(
        !readme.contains("19520"),
        "protocol/README.md contains the stale daemon port 19520; the canonical port is {DEFAULT_GATEWAY_PORT}."
    );
}
