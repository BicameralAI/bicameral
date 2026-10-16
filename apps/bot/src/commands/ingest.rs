use bicameral_api::candidate::{DecisionCandidate, DecisionLevel};
use bicameral_api::event_store::{
    EventStoreAdapter, EventStoreEntry, EventType, IngestPayload, IngestRejectionReason,
};
use bicameral_api::evidence::SourceEvidence;
use bicameral_api::source::Source;
use bicameral_audit::store::AuditStore;
use bicameral_audit::AuditAction;
use bicameral_config::WorkspaceDiscovery;
use bicameral_runtime::event_store_adapters::GitAdapter;
use sha2::{Digest, Sha256};
use std::path::Path;

pub async fn run(file: &Path, workspace: Option<&Path>) -> anyhow::Result<()> {
    let start = workspace
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let (paths, _config) = WorkspaceDiscovery::discover(&start)?;
    paths.ensure_dirs()?;

    let content = std::fs::read_to_string(file)?;
    let input: serde_json::Value = serde_json::from_str(&content)?;

    let title = input["title"].as_str().unwrap_or("Untitled").to_string();
    let description = input["description"].as_str().unwrap_or("").to_string();
    let level = match input["level"].as_str() {
        Some("product") => DecisionLevel::Product,
        Some("architecture") => DecisionLevel::Architecture,
        Some("implementation") => DecisionLevel::Implementation,
        _ => DecisionLevel::Implementation,
    };
    let source_uri = input["source"].as_str().unwrap_or("cli-ingest").to_string();
    let source_type = input["source_type"]
        .as_str()
        .unwrap_or("manual")
        .to_string();

    // Parse evidence items from input
    let evidence_items: Vec<serde_json::Value> =
        input["evidence"].as_array().cloned().unwrap_or_default();

    if evidence_items.is_empty() {
        anyhow::bail!(
            "Ingest rejected: {} — at least one evidence item is required",
            serde_json::to_string(&IngestRejectionReason::EmptyEvidence)?
        );
    }

    // Build Source
    let source_record = Source {
        uri: source_uri.clone(),
        source_type,
        label: input["label"].as_str().map(|s| s.to_string()),
        source_kind: None,
    };

    // Build SourceSnapshot if snapshot_content provided
    let snapshot = input["snapshot_content"].as_str().map(|snap_content| {
        let hash = sha256_hex(snap_content);
        let snap_addr = format!("sha256:{}", hash);
        bicameral_api::source::SourceSnapshot {
            snapshot_addr: snap_addr,
            snapshot_ref: source_uri.clone(),
            captured_at: chrono::Utc::now(),
            content: Some(snap_content.to_string()),
            content_hash: Some(hash),
            capture_method: None,
        }
    });
    let snapshot_addr = snapshot.as_ref().map(|s| s.snapshot_addr.clone());

    // Build SourceEvidence
    let mut evidence_records: Vec<SourceEvidence> = Vec::new();
    for ev_val in &evidence_items {
        let snap_id = snapshot_addr
            .clone()
            .unwrap_or_else(|| format!("inline:{}", source_uri));
        let ev = SourceEvidence::new(
            snap_id,
            ev_val["span_start"].as_u64().unwrap_or(0) as usize,
            ev_val["span_end"].as_u64().unwrap_or(0) as usize,
            ev_val["excerpt"].as_str().unwrap_or("").to_string(),
            ev_val["confidence"].as_f64().unwrap_or(0.5),
        );
        evidence_records.push(ev);
    }

    // Build DecisionCandidate
    let mut candidate =
        DecisionCandidate::new(title.clone(), description, level, source_uri.clone());
    candidate.evidence_refs = evidence_records.iter().map(|e| e.id).collect();

    let cand_id = candidate.id;

    // Build portable IngestPayload and append through EventStoreAdapter
    let ingest_payload = IngestPayload {
        source: source_record,
        snapshot,
        evidence: evidence_records,
        candidate: candidate.clone(),
        evidence_references: Vec::new(),
        provenance: None,
    };
    let payload_value = serde_json::to_value(&ingest_payload)?;
    let entry = EventStoreEntry::new(EventType::Ingest, payload_value, "cli".to_string());

    let adapter = GitAdapter::new(&paths.events_dir);
    adapter
        .append(entry)
        .map_err(|e| anyhow::anyhow!("Event store append failed: {e}"))?;

    // Audit
    let mut audit = AuditStore::open(&paths.audit_dir)?;
    audit.record(
        AuditAction::CandidateIngested,
        "cli".to_string(),
        Some(cand_id),
        format!("Ingested from file: {}", file.display()),
    )?;

    // Also write candidate to inbox for backward compatibility
    let candidate_file = paths.candidates_inbox_dir.join(format!("{}.json", cand_id));
    let candidate_json = serde_json::to_string_pretty(&candidate)?;
    std::fs::write(&candidate_file, candidate_json)?;

    println!("Ingested candidate: {} ({})", title, cand_id);
    println!("  Level: {:?}", level);
    println!("  Event appended to: {}", paths.events_dir.display());
    println!("  Candidate inbox: {}", candidate_file.display());

    Ok(())
}

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}
