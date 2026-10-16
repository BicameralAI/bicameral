//! Local semantic code chunk metadata index.
//!
//! This is derived runtime/cache state for semantic recall. It records stable
//! chunk identity, source snapshot identity, hashes, and explicit skipped/stale
//! caveats. It does not create event-store authority, relevance, compliance,
//! binding, no-conflict, or merge-safety facts.

use bicameral_api::evidence::{CodeSourceCurrentness, GraphEvidenceState, GraphSnapshot};
use bicameral_api::recall::{
    RecallProviderProvenance, RecallProviderStatus, RecallRankingMetadata,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::symbol_resolver::RustSymbolResolver;

pub const SEMANTIC_CHUNKER_VERSION: &str = "semantic-chunker-v1";
pub const SEMANTIC_CHUNK_INDEX_VERSION: &str = "semantic-code-index-v1";

/// Request for local semantic chunk metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticCodeChunkIndexRequest {
    pub workspace_id: Option<String>,
    pub requested_commit_sha: Option<String>,
    pub requested_graph_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default = "default_fixed_chunk_line_count")]
    pub fixed_chunk_line_count: usize,
}

fn default_fixed_chunk_line_count() -> usize {
    80
}

/// Local semantic chunk/index metadata read-model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCodeChunkIndex {
    pub status: RecallProviderStatus,
    pub currentness: CodeSourceCurrentness,
    pub provenance: RecallProviderProvenance,
    pub workspace_id: Option<String>,
    pub repo_id: String,
    pub commit_sha: String,
    pub graph_snapshot_id: String,
    pub chunker_version: String,
    pub index_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<SemanticCodeChunkMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_files: Vec<SemanticCodeSkippedFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranking_metadata: Vec<RecallRankingMetadata>,
}

impl SemanticCodeChunkIndex {
    pub fn is_authoritative(&self) -> bool {
        false
    }
}

/// Stable metadata for one code chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCodeChunkMetadata {
    pub chunk_id: String,
    pub workspace_id: Option<String>,
    pub repo_id: String,
    pub commit_sha: String,
    pub graph_snapshot_id: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    pub language: String,
    pub content_hash: String,
    pub chunker_version: String,
    pub index_version: String,
    pub chunk_kind: SemanticCodeChunkKind,
}

/// Chunk source/kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCodeChunkKind {
    RustSymbol,
    FixedLines,
    File,
}

/// Explicit skipped-file metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCodeSkippedFile {
    pub file_path: String,
    pub reason: SemanticCodeSkippedReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Why a file did not produce chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCodeSkippedReason {
    BinaryOrUnreadable,
    Vendor,
    Generated,
    Directory,
    Missing,
    Empty,
}

/// Local deterministic semantic chunk metadata indexer.
pub struct LocalSemanticCodeChunkIndexer {
    workspace_root: PathBuf,
    snapshot: GraphSnapshot,
}

impl LocalSemanticCodeChunkIndexer {
    pub fn new(workspace_root: &Path, snapshot: GraphSnapshot) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            snapshot,
        }
    }

    pub fn build_index(&self, request: SemanticCodeChunkIndexRequest) -> SemanticCodeChunkIndex {
        let mut status = RecallProviderStatus::Ready;
        let mut currentness = CodeSourceCurrentness::Current;
        let mut caveats = Vec::new();
        let mut chunks = Vec::new();
        let mut skipped_files = Vec::new();

        if request
            .requested_commit_sha
            .as_deref()
            .is_some_and(|sha| sha != self.snapshot.commit_sha)
        {
            status = RecallProviderStatus::Stale;
            currentness = CodeSourceCurrentness::ContentChanged;
            caveats.push("requested commit does not match indexed snapshot commit".to_string());
        }

        if request
            .requested_graph_snapshot_id
            .as_deref()
            .is_some_and(|id| id != self.snapshot.graph_snapshot_id)
        {
            status = RecallProviderStatus::Stale;
            currentness = CodeSourceCurrentness::ContentChanged;
            caveats.push(
                "requested graph snapshot does not match semantic chunk index snapshot".to_string(),
            );
        }

        let files = if request.files.is_empty() {
            self.collect_tracked_files()
        } else {
            request.files.clone()
        };
        let fixed_chunk_line_count = request.fixed_chunk_line_count.max(1);
        let rust_resolver = RustSymbolResolver::new(&self.workspace_root, self.snapshot.clone());

        for file_path in files {
            match self.index_file(
                &file_path,
                request.workspace_id.as_deref(),
                fixed_chunk_line_count,
                &rust_resolver,
            ) {
                FileIndexOutcome::Chunks(mut file_chunks) => chunks.append(&mut file_chunks),
                FileIndexOutcome::Skipped(skipped) => skipped_files.push(skipped),
                FileIndexOutcome::Stale(mut file_chunks, caveat) => {
                    status = RecallProviderStatus::Stale;
                    currentness = CodeSourceCurrentness::ContentChanged;
                    push_unique(&mut caveats, caveat);
                    chunks.append(&mut file_chunks);
                }
            }
        }

        SemanticCodeChunkIndex {
            status,
            currentness,
            provenance: semantic_chunk_provenance(),
            workspace_id: request.workspace_id,
            repo_id: self.snapshot.repo_id.clone(),
            commit_sha: self.snapshot.commit_sha.clone(),
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            chunker_version: SEMANTIC_CHUNKER_VERSION.to_string(),
            index_version: SEMANTIC_CHUNK_INDEX_VERSION.to_string(),
            chunks,
            skipped_files,
            caveats,
            ranking_metadata: Vec::new(),
        }
    }

    fn index_file(
        &self,
        file_path: &str,
        workspace_id: Option<&str>,
        fixed_chunk_line_count: usize,
        rust_resolver: &RustSymbolResolver,
    ) -> FileIndexOutcome {
        let full_path = self.workspace_root.join(file_path);

        if is_vendor_path(file_path) {
            return FileIndexOutcome::Skipped(skipped(
                file_path,
                SemanticCodeSkippedReason::Vendor,
                Some("vendor/dependency path skipped by default"),
                language_for_path(file_path),
            ));
        }

        if is_generated_path(file_path) {
            return FileIndexOutcome::Skipped(skipped(
                file_path,
                SemanticCodeSkippedReason::Generated,
                Some("generated/minified path skipped by default"),
                language_for_path(file_path),
            ));
        }

        if !full_path.exists() {
            return FileIndexOutcome::Skipped(skipped(
                file_path,
                SemanticCodeSkippedReason::Missing,
                Some("file not found in workspace"),
                language_for_path(file_path),
            ));
        }

        if !full_path.is_file() {
            return FileIndexOutcome::Skipped(skipped(
                file_path,
                SemanticCodeSkippedReason::Directory,
                Some("path is not a regular file"),
                language_for_path(file_path),
            ));
        }

        let content = match std::fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(_) => {
                return FileIndexOutcome::Skipped(skipped(
                    file_path,
                    SemanticCodeSkippedReason::BinaryOrUnreadable,
                    Some("binary or unreadable file"),
                    language_for_path(file_path),
                ));
            }
        };

        if content.trim().is_empty() {
            return FileIndexOutcome::Skipped(skipped(
                file_path,
                SemanticCodeSkippedReason::Empty,
                Some("empty file"),
                language_for_path(file_path),
            ));
        }

        let dirty = self.is_dirty(file_path);
        let chunks = if file_path.ends_with(".rs") && !dirty {
            let symbol_result = rust_resolver.resolve_file(file_path);
            if symbol_result.evidence_state == GraphEvidenceState::Verified
                && !symbol_result.occurrences.is_empty()
            {
                symbol_result
                    .occurrences
                    .into_iter()
                    .map(|occurrence| {
                        self.chunk(ChunkDraft {
                            workspace_id,
                            file_path,
                            line_start: occurrence.line_start,
                            line_end: occurrence.line_end,
                            symbol_name: Some(occurrence.symbol_name),
                            language: "rust",
                            content_hash: occurrence.content_hash,
                            chunk_kind: SemanticCodeChunkKind::RustSymbol,
                        })
                    })
                    .collect()
            } else {
                self.fixed_line_chunks(
                    workspace_id,
                    file_path,
                    &content,
                    fixed_chunk_line_count,
                    language_for_path(file_path).as_deref().unwrap_or("text"),
                )
            }
        } else {
            self.fixed_line_chunks(
                workspace_id,
                file_path,
                &content,
                fixed_chunk_line_count,
                language_for_path(file_path).as_deref().unwrap_or("text"),
            )
        };

        if dirty {
            FileIndexOutcome::Stale(
                chunks,
                format!("working tree content differs from indexed snapshot for {file_path}"),
            )
        } else {
            FileIndexOutcome::Chunks(chunks)
        }
    }

    fn fixed_line_chunks(
        &self,
        workspace_id: Option<&str>,
        file_path: &str,
        content: &str,
        fixed_chunk_line_count: usize,
        language: &str,
    ) -> Vec<SemanticCodeChunkMetadata> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            let content_hash = sha256_hex(content);
            return vec![self.chunk(ChunkDraft {
                workspace_id,
                file_path,
                line_start: 1,
                line_end: 1,
                symbol_name: None,
                language,
                content_hash,
                chunk_kind: SemanticCodeChunkKind::File,
            })];
        }

        let kind = if lines.len() <= fixed_chunk_line_count {
            SemanticCodeChunkKind::File
        } else {
            SemanticCodeChunkKind::FixedLines
        };
        lines
            .chunks(fixed_chunk_line_count)
            .enumerate()
            .map(|(index, chunk_lines)| {
                let line_start = (index * fixed_chunk_line_count + 1) as u32;
                let line_end = line_start + chunk_lines.len() as u32 - 1;
                let content_hash = sha256_hex(&chunk_lines.join("\n"));
                self.chunk(ChunkDraft {
                    workspace_id,
                    file_path,
                    line_start,
                    line_end,
                    symbol_name: None,
                    language,
                    content_hash,
                    chunk_kind: kind,
                })
            })
            .collect()
    }

    fn chunk(&self, draft: ChunkDraft<'_>) -> SemanticCodeChunkMetadata {
        let chunk_id = semantic_chunk_id(ChunkIdentity {
            repo_id: &self.snapshot.repo_id,
            commit_sha: &self.snapshot.commit_sha,
            graph_snapshot_id: &self.snapshot.graph_snapshot_id,
            file_path: draft.file_path,
            line_start: draft.line_start,
            line_end: draft.line_end,
            symbol_name: draft.symbol_name.as_deref(),
            content_hash: &draft.content_hash,
        });

        SemanticCodeChunkMetadata {
            chunk_id,
            workspace_id: draft.workspace_id.map(ToString::to_string),
            repo_id: self.snapshot.repo_id.clone(),
            commit_sha: self.snapshot.commit_sha.clone(),
            graph_snapshot_id: self.snapshot.graph_snapshot_id.clone(),
            file_path: draft.file_path.to_string(),
            line_start: draft.line_start,
            line_end: draft.line_end,
            symbol_name: draft.symbol_name,
            language: draft.language.to_string(),
            content_hash: draft.content_hash,
            chunker_version: SEMANTIC_CHUNKER_VERSION.to_string(),
            index_version: SEMANTIC_CHUNK_INDEX_VERSION.to_string(),
            chunk_kind: draft.chunk_kind,
        }
    }

    fn collect_tracked_files(&self) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["ls-files"])
            .current_dir(&self.workspace_root)
            .output();

        match output {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn is_dirty(&self, path: &str) -> bool {
        let status = std::process::Command::new("git")
            .args(["diff", "--quiet", "HEAD", "--", path])
            .current_dir(&self.workspace_root)
            .status();

        match status {
            Ok(status) => !status.success(),
            Err(_) => true,
        }
    }
}

struct ChunkDraft<'a> {
    workspace_id: Option<&'a str>,
    file_path: &'a str,
    line_start: u32,
    line_end: u32,
    symbol_name: Option<String>,
    language: &'a str,
    content_hash: String,
    chunk_kind: SemanticCodeChunkKind,
}

struct ChunkIdentity<'a> {
    repo_id: &'a str,
    commit_sha: &'a str,
    graph_snapshot_id: &'a str,
    file_path: &'a str,
    line_start: u32,
    line_end: u32,
    symbol_name: Option<&'a str>,
    content_hash: &'a str,
}

enum FileIndexOutcome {
    Chunks(Vec<SemanticCodeChunkMetadata>),
    Skipped(SemanticCodeSkippedFile),
    Stale(Vec<SemanticCodeChunkMetadata>, String),
}

fn semantic_chunk_provenance() -> RecallProviderProvenance {
    RecallProviderProvenance {
        provider_id: "local_semantic_chunk_index".to_string(),
        provider_version: env!("CARGO_PKG_VERSION").to_string(),
        index_version: Some(SEMANTIC_CHUNK_INDEX_VERSION.to_string()),
        overlay_id: None,
        model_id: None,
        model_version: None,
    }
}

fn semantic_chunk_id(identity: ChunkIdentity<'_>) -> String {
    let symbol = identity.symbol_name.unwrap_or("");
    let input = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        identity.repo_id,
        identity.commit_sha,
        identity.graph_snapshot_id,
        identity.file_path,
        identity.line_start,
        identity.line_end,
        symbol,
        identity.content_hash,
        SEMANTIC_CHUNKER_VERSION,
        SEMANTIC_CHUNK_INDEX_VERSION
    );
    format!("scc-{}", &sha256_hex(&input)[..24])
}

fn skipped(
    file_path: &str,
    reason: SemanticCodeSkippedReason,
    detail: Option<&str>,
    language: Option<String>,
) -> SemanticCodeSkippedFile {
    SemanticCodeSkippedFile {
        file_path: file_path.to_string(),
        reason,
        detail: detail.map(ToString::to_string),
        language,
    }
}

fn is_vendor_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains("/vendor/")
        || normalized.starts_with("vendor/")
        || normalized.contains("/node_modules/")
        || normalized.starts_with("node_modules/")
        || normalized.contains("/target/")
        || normalized.starts_with("target/")
        || normalized.contains("/.git/")
}

fn is_generated_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/generated/")
        || normalized.starts_with("generated/")
        || normalized.contains(".generated.")
        || normalized.ends_with(".min.js")
        || normalized.ends_with(".min.css")
}

fn language_for_path(path: &str) -> Option<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some("rust".to_string()),
        "ts" | "tsx" => Some("typescript".to_string()),
        "js" | "jsx" => Some("javascript".to_string()),
        "py" => Some("python".to_string()),
        "md" => Some("markdown".to_string()),
        "json" => Some("json".to_string()),
        "toml" => Some("toml".to_string()),
        "yaml" | "yml" => Some("yaml".to_string()),
        "svelte" => Some("svelte".to_string()),
        "" => None,
        other => Some(other.to_string()),
    }
}

fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::SnapshotResolver;
    use std::process::Command;

    fn setup_chunk_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        Command::new("git")
            .args(["init", "-b", "master"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(p)
            .output()
            .unwrap();

        std::fs::write(
            p.join("lib.rs"),
            "pub fn alpha() -> u32 {\n    1\n}\n\npub fn beta() -> u32 {\n    2\n}\n",
        )
        .unwrap();
        std::fs::write(p.join("notes.md"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        std::fs::create_dir_all(p.join("vendor")).unwrap();
        std::fs::write(p.join("vendor/lib.rs"), "pub fn vendored() {}\n").unwrap();
        std::fs::create_dir_all(p.join("src/generated")).unwrap();
        std::fs::write(p.join("src/generated/client.rs"), "pub fn generated() {}\n").unwrap();
        std::fs::write(p.join("blob.bin"), [0xff, 0xfe, 0xfd]).unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init semantic chunks"])
            .current_dir(p)
            .output()
            .unwrap();

        dir
    }

    fn snapshot(dir: &tempfile::TempDir) -> GraphSnapshot {
        SnapshotResolver::new(dir.path()).resolve_head().unwrap()
    }

    #[test]
    fn stable_chunk_ids_for_identical_snapshot_and_content() {
        let dir = setup_chunk_repo();
        let snap = snapshot(&dir);
        let indexer = LocalSemanticCodeChunkIndexer::new(dir.path(), snap);
        let request = SemanticCodeChunkIndexRequest {
            workspace_id: Some("workspace-a".to_string()),
            files: vec!["lib.rs".to_string()],
            ..Default::default()
        };

        let first = indexer.build_index(request.clone());
        let second = indexer.build_index(request);

        let first_ids: Vec<&str> = first
            .chunks
            .iter()
            .map(|chunk| chunk.chunk_id.as_str())
            .collect();
        let second_ids: Vec<&str> = second
            .chunks
            .iter()
            .map(|chunk| chunk.chunk_id.as_str())
            .collect();
        assert_eq!(first_ids, second_ids);
        assert!(first
            .chunks
            .iter()
            .any(|chunk| chunk.symbol_name.as_deref() == Some("alpha")));
        assert!(first
            .chunks
            .iter()
            .all(|chunk| chunk.chunk_kind == SemanticCodeChunkKind::RustSymbol));
        assert!(!first.is_authoritative());
    }

    #[test]
    fn changed_content_changes_chunk_hash_and_marks_index_stale() {
        let dir = setup_chunk_repo();
        let snap = snapshot(&dir);
        let indexer = LocalSemanticCodeChunkIndexer::new(dir.path(), snap);
        let request = SemanticCodeChunkIndexRequest {
            files: vec!["notes.md".to_string()],
            fixed_chunk_line_count: 3,
            ..Default::default()
        };

        let before = indexer.build_index(request.clone());
        std::fs::write(
            dir.path().join("notes.md"),
            "one\ntwo\nchanged\nfour\nfive\n",
        )
        .unwrap();
        let after = indexer.build_index(request);

        assert_ne!(before.chunks[0].content_hash, after.chunks[0].content_hash);
        assert_ne!(before.chunks[0].chunk_id, after.chunks[0].chunk_id);
        assert_eq!(after.status, RecallProviderStatus::Stale);
        assert_eq!(after.currentness, CodeSourceCurrentness::ContentChanged);
        assert!(after
            .caveats
            .iter()
            .any(|caveat| caveat.contains("working tree content differs")));
    }

    #[test]
    fn skipped_files_are_reported_explicitly() {
        let dir = setup_chunk_repo();
        let snap = snapshot(&dir);
        let indexer = LocalSemanticCodeChunkIndexer::new(dir.path(), snap);
        let result = indexer.build_index(SemanticCodeChunkIndexRequest {
            files: vec![
                "vendor/lib.rs".to_string(),
                "src/generated/client.rs".to_string(),
                "blob.bin".to_string(),
                "missing.rs".to_string(),
            ],
            ..Default::default()
        });

        assert_eq!(result.chunks.len(), 0);
        assert_eq!(result.skipped_files.len(), 4);
        assert!(result.skipped_files.iter().any(|file| {
            file.file_path == "vendor/lib.rs" && file.reason == SemanticCodeSkippedReason::Vendor
        }));
        assert!(result.skipped_files.iter().any(|file| {
            file.file_path == "src/generated/client.rs"
                && file.reason == SemanticCodeSkippedReason::Generated
        }));
        assert!(result.skipped_files.iter().any(|file| {
            file.file_path == "blob.bin"
                && file.reason == SemanticCodeSkippedReason::BinaryOrUnreadable
        }));
        assert!(result.skipped_files.iter().any(|file| {
            file.file_path == "missing.rs" && file.reason == SemanticCodeSkippedReason::Missing
        }));
    }

    #[test]
    fn requested_snapshot_mismatch_returns_stale_caveat_data() {
        let dir = setup_chunk_repo();
        let snap = snapshot(&dir);
        let wrong_commit = "0000000000000000000000000000000000000000".to_string();
        let wrong_graph = "gs-wrong".to_string();
        let indexer = LocalSemanticCodeChunkIndexer::new(dir.path(), snap);

        let result = indexer.build_index(SemanticCodeChunkIndexRequest {
            requested_commit_sha: Some(wrong_commit),
            requested_graph_snapshot_id: Some(wrong_graph),
            files: vec!["lib.rs".to_string()],
            ..Default::default()
        });

        assert_eq!(result.status, RecallProviderStatus::Stale);
        assert_eq!(result.currentness, CodeSourceCurrentness::ContentChanged);
        assert!(result
            .caveats
            .contains(&"requested commit does not match indexed snapshot commit".to_string()));
        assert!(result.caveats.contains(
            &"requested graph snapshot does not match semantic chunk index snapshot".to_string()
        ));
        assert!(!result.chunks.is_empty());
    }

    #[test]
    fn fallback_chunks_non_rust_text_by_fixed_lines() {
        let dir = setup_chunk_repo();
        let snap = snapshot(&dir);
        let indexer = LocalSemanticCodeChunkIndexer::new(dir.path(), snap);

        let result = indexer.build_index(SemanticCodeChunkIndexRequest {
            files: vec!["notes.md".to_string()],
            fixed_chunk_line_count: 2,
            ..Default::default()
        });

        assert_eq!(result.status, RecallProviderStatus::Ready);
        assert_eq!(result.chunks.len(), 3);
        assert_eq!(result.chunks[0].line_start, 1);
        assert_eq!(result.chunks[0].line_end, 2);
        assert_eq!(result.chunks[0].language, "markdown");
        assert_eq!(
            result.chunks[0].chunk_kind,
            SemanticCodeChunkKind::FixedLines
        );
    }
}
