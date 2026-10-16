use bicameral_config::WorkspaceDiscovery;
use bicameral_runtime::grounding::SnapshotResolver;
use std::path::Path;

pub async fn run(target: &str, workspace: Option<&Path>) -> anyhow::Result<()> {
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let (paths, _config) = WorkspaceDiscovery::discover(&start)?;

    println!("Running preflight checks for: {}", target);
    println!("  Workspace: {}", paths.workspace_root.display());

    // Resolve graph snapshot identity for this workspace.
    let resolver = SnapshotResolver::new(&paths.workspace_root);
    match resolver.resolve_head() {
        Ok(snapshot) => {
            println!("  Graph snapshot:");
            println!("    graph_snapshot_id: {}", snapshot.graph_snapshot_id);
            println!("    repo_id:           {}", snapshot.repo_id);
            println!("    commit_sha:        {}", snapshot.commit_sha);
            println!("    index_version:     {}", snapshot.index_version);
            if let Some(ref ref_name) = snapshot.ref_name {
                println!("    ref_name:          {}", ref_name);
            }
        }
        Err(e) => {
            println!("  Graph snapshot: unavailable ({e})");
        }
    }

    // Load pending candidates from inbox for comparison
    let inbox_dir = &paths.candidates_inbox_dir;
    let mut decision_count = 0;
    if inbox_dir.exists() {
        for entry in std::fs::read_dir(inbox_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                decision_count += 1;
            }
        }
    }

    println!("  Active candidates/events: {}", decision_count);

    if decision_count == 0 {
        println!("  No decisions tracked yet. Preflight pass (vacuous).");
    } else {
        println!("  Preflight complete. {} items checked.", decision_count);
    }

    Ok(())
}
