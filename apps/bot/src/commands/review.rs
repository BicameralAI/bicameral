use bicameral_api::candidate::DecisionCandidate;
use bicameral_audit::store::AuditStore;
use bicameral_audit::AuditAction;
use bicameral_config::WorkspaceDiscovery;
use bicameral_runtime::governance::GovernanceEngine;
use std::path::Path;

pub async fn list(workspace: Option<&Path>) -> anyhow::Result<()> {
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let (paths, _config) = WorkspaceDiscovery::discover(&start)?;

    println!("Pending reviews in: {}", paths.workspace_root.display());

    let inbox_dir = &paths.candidates_inbox_dir;
    if !inbox_dir.exists() {
        println!("  (none)");
        return Ok(());
    }

    let mut count = 0;
    for entry in std::fs::read_dir(inbox_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(candidate) = serde_json::from_str::<DecisionCandidate>(&content) {
                    println!(
                        "  {} — {} [{:?}]",
                        candidate.id, candidate.title, candidate.level
                    );
                    count += 1;
                }
            }
        }
    }

    if count == 0 {
        println!("  (none)");
    }

    Ok(())
}

pub async fn submit(id: &str, workspace: Option<&Path>) -> anyhow::Result<()> {
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let (paths, config) = WorkspaceDiscovery::discover(&start)?;

    // Load the candidate from the inbox
    let candidate_file = paths.candidates_inbox_dir.join(format!("{}.json", id));
    if !candidate_file.exists() {
        anyhow::bail!(
            "Candidate {} not found in inbox ({})",
            id,
            paths.candidates_inbox_dir.display()
        );
    }

    let content = std::fs::read_to_string(&candidate_file)?;
    let candidate: DecisionCandidate = serde_json::from_str(&content)?;

    // Evaluate governance policy
    let engine = GovernanceEngine::new(config.governance.clone());
    let result = engine.evaluate(&candidate);

    // Record audit receipt for the review submission
    let mut audit = AuditStore::open(&paths.audit_dir)?;
    audit.record(
        AuditAction::ReviewCommandIssued,
        "cli".to_string(),
        Some(candidate.id),
        format!(
            "Review submitted for candidate {}; verdict: {:?}",
            id, result.verdict
        ),
    )?;

    // Persist the governance result alongside the candidate
    let result_file = paths
        .candidates_inbox_dir
        .join(format!("{}.governance.json", id));
    let result_json = serde_json::to_string_pretty(&result)?;
    std::fs::write(&result_file, result_json)?;

    // Output honest state
    println!(
        "Review submitted for candidate: {} ({})",
        candidate.title, id
    );
    println!("  Governance verdict: {:?}", result.verdict);
    println!("  Reason: {}", result.reason);
    if GovernanceEngine::may_materialize(&result) {
        println!("  Status: accepted — eligible for event-store materialization");
    } else {
        println!("  Status: not accepted — candidate remains in inbox pending further review");
    }

    Ok(())
}
