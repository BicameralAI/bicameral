//! Runtime recall provider seam.
//!
//! Providers are read-only hint sources. The no-op implementations are the
//! default posture: no configured provider means no recall behavior changes and
//! no provider output can become verified graph evidence.

use bicameral_api::evidence::CodeSourceCurrentness;
use bicameral_api::recall::{
    NoResultLanguage, RecallProviderProvenance, RecallProviderStatus, RecallRankingMetadata,
    RecallScopeKind, RecallSearchedScope, RecallSignalRequest, RecallSignalResult, ScopeLevel,
    ScopeSection, SearchedScope, SemanticCodeCandidate, SemanticCodeSearchRequest,
    SemanticCodeSearchResult, SemanticCodeTargetType, SemanticScoreKind,
};
use std::collections::BTreeSet;

use crate::semantic_chunk_index::{
    SemanticCodeChunkIndex, SemanticCodeChunkKind, SemanticCodeChunkMetadata,
    SemanticCodeSkippedReason,
};

/// Read-only provider interface for recall signal retrieval.
pub trait RecallSignalProvider: Send + Sync {
    fn search(&self, request: &RecallSignalRequest) -> RecallSignalResult;
}

/// Read-only provider interface for semantic code retrieval.
pub trait SemanticCodeSearchProvider: Send + Sync {
    fn search(&self, request: &SemanticCodeSearchRequest) -> SemanticCodeSearchResult;
}

/// Default recall provider used when no provider is configured.
#[derive(Debug, Clone)]
pub struct NoopRecallSignalProvider {
    provenance: RecallProviderProvenance,
}

impl Default for NoopRecallSignalProvider {
    fn default() -> Self {
        Self {
            provenance: noop_provenance("noop_recall"),
        }
    }
}

impl NoopRecallSignalProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RecallSignalProvider for NoopRecallSignalProvider {
    fn search(&self, request: &RecallSignalRequest) -> RecallSignalResult {
        RecallSignalResult {
            status: RecallProviderStatus::NotConfigured,
            provenance: self.provenance.clone(),
            graph_snapshot_id: request.graph_snapshot_id.clone(),
            searched_scope: request.searched_scope(),
            unknown_scope: Vec::new(),
            currentness: CodeSourceCurrentness::GraphNotConfigured,
            signals: Vec::new(),
        }
    }
}

/// Default semantic provider used when no semantic index/provider is configured.
#[derive(Debug, Clone)]
pub struct NoopSemanticCodeSearchProvider {
    provenance: RecallProviderProvenance,
}

impl Default for NoopSemanticCodeSearchProvider {
    fn default() -> Self {
        Self {
            provenance: noop_provenance("noop_semantic_code"),
        }
    }
}

impl NoopSemanticCodeSearchProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SemanticCodeSearchProvider for NoopSemanticCodeSearchProvider {
    fn search(&self, request: &SemanticCodeSearchRequest) -> SemanticCodeSearchResult {
        SemanticCodeSearchResult {
            status: RecallProviderStatus::NotConfigured,
            provenance: self.provenance.clone(),
            graph_snapshot_id: request.graph_snapshot_id.clone(),
            searched_scope: request.searched_scope(),
            unknown_scope: Vec::new(),
            currentness: CodeSourceCurrentness::GraphNotConfigured,
            candidates: Vec::new(),
        }
    }
}

/// Configured local provider backed by the semantic chunk metadata index.
///
/// This is the first real provider posture: it consumes a derived chunk index
/// and produces replayable, provenance-rich candidates, but it deliberately
/// searches metadata rather than embeddings. Its output stays approximate until
/// `CodeLocator` verifies a candidate against graph evidence.
#[derive(Debug, Clone, Default)]
pub struct LocalSemanticCodeSearchProvider {
    index: Option<SemanticCodeChunkIndex>,
}

impl LocalSemanticCodeSearchProvider {
    pub fn not_configured() -> Self {
        Self { index: None }
    }

    pub fn from_index(index: SemanticCodeChunkIndex) -> Self {
        Self { index: Some(index) }
    }
}

impl SemanticCodeSearchProvider for LocalSemanticCodeSearchProvider {
    fn search(&self, request: &SemanticCodeSearchRequest) -> SemanticCodeSearchResult {
        let Some(index) = &self.index else {
            return NoopSemanticCodeSearchProvider::new().search(request);
        };

        let (status, currentness) = provider_status_for_request(index, request);
        let provenance = local_semantic_provenance(index);
        let searched_scope = searched_scope_from_index(index, request);
        let unknown_scope = unknown_scope_from_index(index);
        let mut candidates = if matches!(
            status,
            RecallProviderStatus::Ready | RecallProviderStatus::Stale
        ) {
            search_index_candidates(index, request, &provenance, currentness)
        } else {
            Vec::new()
        };

        candidates.truncate(request.limit.max(1));

        SemanticCodeSearchResult {
            status,
            provenance,
            graph_snapshot_id: Some(index.graph_snapshot_id.clone()),
            searched_scope,
            unknown_scope,
            currentness,
            candidates,
        }
    }
}

/// Fixture-backed semantic provider for deterministic tests.
///
/// The fake provider is intentionally read-only and advisory: its candidates
/// are semantic hints only, even when a fixture supplies stable files, ranges,
/// or content hashes. Later deterministic runtime verification must still prove
/// any authoritative code target.
#[derive(Debug, Clone)]
pub struct FakeSemanticCodeSearchProvider {
    fixture: FakeSemanticCodeSearchFixture,
}

impl FakeSemanticCodeSearchProvider {
    pub fn new(fixture: FakeSemanticCodeSearchFixture) -> Self {
        Self { fixture }
    }

    pub fn ready(candidates: Vec<SemanticCodeCandidate>) -> Self {
        Self::new(FakeSemanticCodeSearchFixture::ready(candidates))
    }

    pub fn with_status(status: RecallProviderStatus) -> Self {
        Self::new(FakeSemanticCodeSearchFixture::with_status(status))
    }

    pub fn not_configured() -> Self {
        Self::with_status(RecallProviderStatus::NotConfigured)
    }

    pub fn indexing() -> Self {
        Self::with_status(RecallProviderStatus::Indexing)
    }

    pub fn stale(candidates: Vec<SemanticCodeCandidate>) -> Self {
        let mut fixture = FakeSemanticCodeSearchFixture::ready(candidates);
        fixture.status = RecallProviderStatus::Stale;
        fixture.currentness = CodeSourceCurrentness::ContentChanged;
        Self::new(fixture)
    }

    pub fn failed() -> Self {
        Self::with_status(RecallProviderStatus::Failed)
    }

    pub fn unsupported() -> Self {
        Self::with_status(RecallProviderStatus::Unsupported)
    }

    pub fn no_results(searched_scope: RecallSearchedScope) -> Self {
        let mut fixture = FakeSemanticCodeSearchFixture::ready(Vec::new());
        fixture.searched_scope = Some(searched_scope);
        Self::new(fixture)
    }
}

impl SemanticCodeSearchProvider for FakeSemanticCodeSearchProvider {
    fn search(&self, request: &SemanticCodeSearchRequest) -> SemanticCodeSearchResult {
        self.fixture.result_for(request)
    }
}

/// Deterministic fake semantic provider fixture.
#[derive(Debug, Clone)]
pub struct FakeSemanticCodeSearchFixture {
    pub status: RecallProviderStatus,
    pub provenance: RecallProviderProvenance,
    pub graph_snapshot_id: Option<String>,
    pub searched_scope: Option<RecallSearchedScope>,
    pub unknown_scope: Vec<bicameral_api::recall::RecallUnknownProviderScope>,
    pub currentness: CodeSourceCurrentness,
    pub candidates: Vec<SemanticCodeCandidate>,
}

impl FakeSemanticCodeSearchFixture {
    pub fn ready(candidates: Vec<SemanticCodeCandidate>) -> Self {
        Self {
            status: RecallProviderStatus::Ready,
            provenance: fake_semantic_provenance(),
            graph_snapshot_id: None,
            searched_scope: None,
            unknown_scope: Vec::new(),
            currentness: CodeSourceCurrentness::Current,
            candidates,
        }
    }

    pub fn with_status(status: RecallProviderStatus) -> Self {
        let currentness = currentness_for_status(status);
        Self {
            status,
            provenance: fake_semantic_provenance(),
            graph_snapshot_id: None,
            searched_scope: None,
            unknown_scope: Vec::new(),
            currentness,
            candidates: Vec::new(),
        }
    }

    pub fn result_for(&self, request: &SemanticCodeSearchRequest) -> SemanticCodeSearchResult {
        let graph_snapshot_id = self
            .graph_snapshot_id
            .clone()
            .or_else(|| request.graph_snapshot_id.clone());
        let mut result = SemanticCodeSearchResult {
            status: self.status,
            provenance: self.provenance.clone(),
            graph_snapshot_id: graph_snapshot_id.clone(),
            searched_scope: self
                .searched_scope
                .clone()
                .unwrap_or_else(|| request.searched_scope()),
            unknown_scope: self.unknown_scope.clone(),
            currentness: self.currentness,
            candidates: self.candidates.clone(),
        };

        for candidate in &mut result.candidates {
            candidate.provenance = result.provenance.clone();
            if result.status == RecallProviderStatus::Stale
                && candidate.currentness == CodeSourceCurrentness::Current
            {
                candidate.currentness = CodeSourceCurrentness::ContentChanged;
            }
        }

        result
    }
}

/// Build an approximate fake semantic code candidate.
pub fn fake_semantic_candidate(file: impl Into<String>, score: f64) -> SemanticCodeCandidate {
    SemanticCodeCandidate {
        file: file.into(),
        range: None,
        symbol: None,
        chunk_id: None,
        content_hash: None,
        target_type: SemanticCodeTargetType::CodeRegion,
        score,
        score_kind: SemanticScoreKind::SemanticSimilarity,
        reasons: vec!["fixture semantic similarity".to_string()],
        matched_terms: Vec::new(),
        matched_excerpt: None,
        currentness: CodeSourceCurrentness::Current,
        provenance: fake_semantic_provenance(),
    }
}

/// Advisory ranking metadata helper for fake semantic fixtures.
pub fn fake_semantic_ranking_metadata(
    key: impl Into<String>,
    value: impl Into<String>,
) -> RecallRankingMetadata {
    RecallRankingMetadata {
        key: key.into(),
        value: value.into(),
    }
}

/// Convert provider-level no-result output into searched-scope-limited language.
pub fn fake_semantic_no_result_language(result: &SemanticCodeSearchResult) -> NoResultLanguage {
    let scope = searched_scope_from_provider_result(result);
    NoResultLanguage::from_scope(scope)
}

/// Bundled provider configuration for code paths that want explicit defaults.
pub struct RecallProviders {
    pub recall: Box<dyn RecallSignalProvider>,
    pub semantic_code: Box<dyn SemanticCodeSearchProvider>,
}

impl Default for RecallProviders {
    fn default() -> Self {
        Self {
            recall: Box::new(NoopRecallSignalProvider::default()),
            semantic_code: Box::new(NoopSemanticCodeSearchProvider::default()),
        }
    }
}

fn noop_provenance(provider_id: &str) -> RecallProviderProvenance {
    RecallProviderProvenance {
        provider_id: provider_id.to_string(),
        provider_version: env!("CARGO_PKG_VERSION").to_string(),
        index_version: None,
        overlay_id: None,
        model_id: None,
        model_version: None,
    }
}

fn fake_semantic_provenance() -> RecallProviderProvenance {
    RecallProviderProvenance {
        provider_id: "fake_semantic_code".to_string(),
        provider_version: env!("CARGO_PKG_VERSION").to_string(),
        index_version: Some("fixture-index-v1".to_string()),
        overlay_id: Some("fixture-overlay".to_string()),
        model_id: Some("deterministic-fixture".to_string()),
        model_version: Some("fixture-v1".to_string()),
    }
}

fn local_semantic_provenance(index: &SemanticCodeChunkIndex) -> RecallProviderProvenance {
    RecallProviderProvenance {
        provider_id: "local_semantic_code_metadata".to_string(),
        provider_version: env!("CARGO_PKG_VERSION").to_string(),
        index_version: Some(index.index_version.clone()),
        overlay_id: None,
        model_id: Some("metadata-lexical".to_string()),
        model_version: Some(index.chunker_version.clone()),
    }
}

fn provider_status_for_request(
    index: &SemanticCodeChunkIndex,
    request: &SemanticCodeSearchRequest,
) -> (RecallProviderStatus, CodeSourceCurrentness) {
    let mut status = index.status;
    let mut currentness = index.currentness;

    if request
        .commit_sha
        .as_deref()
        .is_some_and(|sha| sha != index.commit_sha)
        || request
            .graph_snapshot_id
            .as_deref()
            .is_some_and(|id| id != index.graph_snapshot_id)
    {
        status = RecallProviderStatus::Stale;
        currentness = CodeSourceCurrentness::ContentChanged;
    }

    (status, currentness)
}

fn searched_scope_from_index(
    index: &SemanticCodeChunkIndex,
    request: &SemanticCodeSearchRequest,
) -> RecallSearchedScope {
    let mut scope = request.searched_scope();
    if scope.files.is_empty() {
        scope.files = index
            .chunks
            .iter()
            .map(|chunk| chunk.file_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    if scope.symbols.is_empty() {
        scope.symbols = index
            .chunks
            .iter()
            .filter_map(|chunk| chunk.symbol_name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    scope.corpus_id = request
        .workspace_id
        .clone()
        .or_else(|| index.workspace_id.clone())
        .or_else(|| Some(index.repo_id.clone()));
    scope.corpus_version = Some(index.graph_snapshot_id.clone());
    scope.scope_fingerprint = Some(format!(
        "{}:{}:{}:{}",
        index.repo_id, index.commit_sha, index.graph_snapshot_id, index.index_version
    ));
    scope
}

fn unknown_scope_from_index(
    index: &SemanticCodeChunkIndex,
) -> Vec<bicameral_api::recall::RecallUnknownProviderScope> {
    index
        .skipped_files
        .iter()
        .map(
            |skipped| bicameral_api::recall::RecallUnknownProviderScope {
                scope: skipped.file_path.clone(),
                reason: match skipped.reason {
                    SemanticCodeSkippedReason::BinaryOrUnreadable => {
                        bicameral_api::recall::RecallUnknownProviderScopeReason::UnsupportedLanguage
                    }
                    SemanticCodeSkippedReason::Vendor
                    | SemanticCodeSkippedReason::Generated
                    | SemanticCodeSkippedReason::Directory
                    | SemanticCodeSkippedReason::Missing
                    | SemanticCodeSkippedReason::Empty => {
                        bicameral_api::recall::RecallUnknownProviderScopeReason::NotIndexed
                    }
                },
                detail: skipped.detail.clone(),
            },
        )
        .collect()
}

fn search_index_candidates(
    index: &SemanticCodeChunkIndex,
    request: &SemanticCodeSearchRequest,
    provenance: &RecallProviderProvenance,
    currentness: CodeSourceCurrentness,
) -> Vec<SemanticCodeCandidate> {
    let tokens = request_tokens(request);
    let requested_files: BTreeSet<_> = request.files.iter().map(|f| normalize(f)).collect();
    let requested_symbols: BTreeSet<_> = request.symbols.iter().map(|s| normalize(s)).collect();

    let mut scored = Vec::new();
    for chunk in &index.chunks {
        if !requested_files.is_empty() && !requested_files.contains(&normalize(&chunk.file_path)) {
            continue;
        }
        if !requested_symbols.is_empty()
            && !chunk
                .symbol_name
                .as_ref()
                .is_some_and(|symbol| requested_symbols.contains(&normalize(symbol)))
        {
            continue;
        }

        let matched_terms = matched_terms_for_chunk(chunk, &tokens);
        if tokens.is_empty()
            || !matched_terms.is_empty()
            || requested_files.contains(&normalize(&chunk.file_path))
            || chunk
                .symbol_name
                .as_ref()
                .is_some_and(|symbol| requested_symbols.contains(&normalize(symbol)))
        {
            let score = candidate_score(chunk, &matched_terms, tokens.len());
            scored.push((
                score,
                SemanticCodeCandidate {
                    file: chunk.file_path.clone(),
                    range: Some(format!("{}-{}", chunk.line_start, chunk.line_end)),
                    symbol: chunk.symbol_name.clone(),
                    chunk_id: Some(chunk.chunk_id.clone()),
                    content_hash: Some(chunk.content_hash.clone()),
                    target_type: target_type_for_chunk(chunk),
                    score,
                    score_kind: SemanticScoreKind::HybridRank,
                    reasons: vec![
                        "local semantic chunk metadata matched request terms".to_string(),
                        "metadata provider output is advisory until graph verification".to_string(),
                    ],
                    matched_terms,
                    matched_excerpt: chunk.symbol_name.clone(),
                    currentness,
                    provenance: provenance.clone(),
                },
            ));
        }
    }

    scored.sort_by(|(a, a_candidate), (b, b_candidate)| {
        b.partial_cmp(a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a_candidate.file.cmp(&b_candidate.file))
            .then_with(|| a_candidate.range.cmp(&b_candidate.range))
    });
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

fn request_tokens(request: &SemanticCodeSearchRequest) -> BTreeSet<String> {
    request
        .query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .chain(request.symbols.iter().map(String::as_str))
        .chain(request.changed_files.iter().map(String::as_str))
        .filter_map(|token| {
            let normalized = normalize(token);
            (normalized.len() >= 2).then_some(normalized)
        })
        .collect()
}

fn matched_terms_for_chunk(
    chunk: &SemanticCodeChunkMetadata,
    tokens: &BTreeSet<String>,
) -> Vec<String> {
    let haystack = normalize(&format!(
        "{} {} {} {} {:?}",
        chunk.file_path,
        chunk.symbol_name.as_deref().unwrap_or(""),
        chunk.language,
        chunk.chunk_id,
        chunk.chunk_kind
    ));
    tokens
        .iter()
        .filter(|token| haystack.contains(token.as_str()))
        .cloned()
        .collect()
}

fn candidate_score(
    chunk: &SemanticCodeChunkMetadata,
    matched_terms: &[String],
    token_count: usize,
) -> f64 {
    let denominator = token_count.max(1) as f64;
    let mut score = (matched_terms.len() as f64 / denominator).min(1.0);
    if chunk.symbol_name.is_some() {
        score += 0.10;
    }
    if chunk.chunk_kind == SemanticCodeChunkKind::RustSymbol {
        score += 0.05;
    }
    score.min(1.0)
}

fn target_type_for_chunk(chunk: &SemanticCodeChunkMetadata) -> SemanticCodeTargetType {
    if chunk.symbol_name.is_some() {
        SemanticCodeTargetType::Symbol
    } else if chunk.chunk_kind == SemanticCodeChunkKind::File {
        SemanticCodeTargetType::FilePath
    } else {
        SemanticCodeTargetType::CodeRegion
    }
}

fn normalize(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace(['-', '/', '.', ':'], "_")
}

fn currentness_for_status(status: RecallProviderStatus) -> CodeSourceCurrentness {
    match status {
        RecallProviderStatus::Ready => CodeSourceCurrentness::Current,
        RecallProviderStatus::Stale => CodeSourceCurrentness::ContentChanged,
        RecallProviderStatus::NotConfigured => CodeSourceCurrentness::GraphNotConfigured,
        RecallProviderStatus::Indexing
        | RecallProviderStatus::Failed
        | RecallProviderStatus::Unsupported => CodeSourceCurrentness::GraphUnavailable,
    }
}

fn searched_scope_from_provider_result(result: &SemanticCodeSearchResult) -> SearchedScope {
    let code_targets = result.searched_scope.files.clone();
    let graph_version = result
        .graph_snapshot_id
        .clone()
        .or_else(|| result.searched_scope.corpus_version.clone());
    let retrieval_targets = match result.searched_scope.scope_kind {
        RecallScopeKind::Query => vec!["query".to_string()],
        RecallScopeKind::Binding => vec!["binding".to_string()],
        RecallScopeKind::Diff => vec!["diff".to_string()],
        RecallScopeKind::FileSet => result.searched_scope.files.clone(),
        RecallScopeKind::SymbolSet => result.searched_scope.symbols.clone(),
        RecallScopeKind::BindingRevalidation => vec!["binding_revalidation".to_string()],
    };

    let has_scope = result.searched_scope.scope_fingerprint.is_some()
        || !result.searched_scope.files.is_empty()
        || !result.searched_scope.symbols.is_empty()
        || result.searched_scope.corpus_id.is_some();

    SearchedScope {
        level: if has_scope {
            ScopeLevel::Replayable
        } else {
            ScopeLevel::Unscoped
        },
        source: ScopeSection::not_applicable(),
        code: if code_targets.is_empty() {
            ScopeSection::not_applicable()
        } else {
            ScopeSection::searched(code_targets, graph_version.clone())
        },
        graph: match result.currentness {
            CodeSourceCurrentness::Current => ScopeSection::searched(Vec::new(), graph_version),
            CodeSourceCurrentness::ContentChanged | CodeSourceCurrentness::TargetMissing => {
                ScopeSection::stale(
                    "fake semantic provider reported stale code currentness",
                    graph_version,
                )
            }
            CodeSourceCurrentness::GraphUnavailable | CodeSourceCurrentness::GraphNotConfigured => {
                ScopeSection::unavailable("fake semantic provider has no current graph proof")
            }
        },
        retrieval: if retrieval_targets.is_empty() {
            ScopeSection::not_applicable()
        } else {
            ScopeSection::searched(retrieval_targets, result.provenance.index_version.clone())
        },
        advisory: ScopeSection::searched(
            vec!["semantic_code_fixture".to_string()],
            result.provenance.model_version.clone(),
        ),
        unknown: if result.unknown_scope.is_empty() {
            ScopeSection::not_applicable()
        } else {
            ScopeSection::partial(
                result
                    .unknown_scope
                    .iter()
                    .map(|scope| scope.scope.clone())
                    .collect(),
                "fake semantic provider reported unknown scope",
                None,
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall_admissibility::{
        RecallAdmissibilityGate, RecallAdmissibilityOutcome, RecallAdmissibilityReasonCode,
        RecallAdmissibleUse,
    };
    use crate::semantic_chunk_index::{
        SemanticCodeChunkIndex, SemanticCodeChunkKind, SemanticCodeChunkMetadata,
        SemanticCodeSkippedFile, SemanticCodeSkippedReason, SEMANTIC_CHUNKER_VERSION,
        SEMANTIC_CHUNK_INDEX_VERSION,
    };
    use bicameral_api::recall::{
        NoResultClaim, RecallUnknownProviderScope, RecallUnknownProviderScopeReason,
    };

    #[test]
    fn noop_recall_provider_returns_not_configured_without_signals() {
        let provider = NoopRecallSignalProvider::new();
        let request = RecallSignalRequest {
            query: Some("checkout".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            graph_snapshot_id: Some("graph-1".to_string()),
            commit_sha: None,
            ref_name: None,
            files: vec!["src/checkout.rs".to_string()],
            symbols: Vec::new(),
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 5,
            scope_kind: Some(RecallScopeKind::FileSet),
        };

        let result = provider.search(&request);
        assert_eq!(result.status, RecallProviderStatus::NotConfigured);
        assert_eq!(result.provenance.provider_id, "noop_recall");
        assert_eq!(result.graph_snapshot_id.as_deref(), Some("graph-1"));
        assert_eq!(result.searched_scope.scope_kind, RecallScopeKind::FileSet);
        assert_eq!(
            result.currentness,
            CodeSourceCurrentness::GraphNotConfigured
        );
        assert!(result.signals.is_empty());
    }

    #[test]
    fn noop_semantic_provider_returns_not_configured_without_candidates() {
        let provider = NoopSemanticCodeSearchProvider::new();
        let request = SemanticCodeSearchRequest {
            query: "checkout".to_string(),
            workspace_id: Some("workspace-a".to_string()),
            graph_snapshot_id: Some("graph-1".to_string()),
            commit_sha: None,
            ref_name: None,
            files: Vec::new(),
            symbols: vec!["Checkout".to_string()],
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 5,
            scope_kind: Some(RecallScopeKind::SymbolSet),
        };

        let result = provider.search(&request);
        assert_eq!(result.status, RecallProviderStatus::NotConfigured);
        assert_eq!(result.provenance.provider_id, "noop_semantic_code");
        assert_eq!(result.graph_snapshot_id.as_deref(), Some("graph-1"));
        assert_eq!(result.searched_scope.scope_kind, RecallScopeKind::SymbolSet);
        assert_eq!(
            result.currentness,
            CodeSourceCurrentness::GraphNotConfigured
        );
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn default_provider_bundle_is_quiet_and_not_configured() {
        let providers = RecallProviders::default();
        let recall_request = RecallSignalRequest {
            query: Some("nothing configured".to_string()),
            workspace_id: None,
            graph_snapshot_id: None,
            commit_sha: None,
            ref_name: None,
            files: Vec::new(),
            symbols: Vec::new(),
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 20,
            scope_kind: None,
        };
        let semantic_request = SemanticCodeSearchRequest {
            query: "nothing configured".to_string(),
            workspace_id: None,
            graph_snapshot_id: None,
            commit_sha: None,
            ref_name: None,
            files: Vec::new(),
            symbols: Vec::new(),
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 20,
            scope_kind: None,
        };

        assert_eq!(
            providers.recall.search(&recall_request).status,
            RecallProviderStatus::NotConfigured
        );
        assert_eq!(
            providers.semantic_code.search(&semantic_request).status,
            RecallProviderStatus::NotConfigured
        );
    }

    #[test]
    fn fake_semantic_provider_returns_ready_fixture_candidates_with_request_scope() {
        let provider = FakeSemanticCodeSearchProvider::ready(vec![fake_semantic_candidate(
            "src/recall.rs",
            0.92,
        )]);
        let request = SemanticCodeSearchRequest {
            query: "recall admissibility".to_string(),
            workspace_id: Some("workspace-a".to_string()),
            graph_snapshot_id: Some("graph-1".to_string()),
            commit_sha: None,
            ref_name: None,
            files: vec!["src/recall.rs".to_string()],
            symbols: Vec::new(),
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 3,
            scope_kind: Some(RecallScopeKind::FileSet),
        };

        let result = provider.search(&request);

        assert_eq!(result.status, RecallProviderStatus::Ready);
        assert_eq!(result.provenance.provider_id, "fake_semantic_code");
        assert_eq!(result.graph_snapshot_id.as_deref(), Some("graph-1"));
        assert_eq!(result.searched_scope.scope_kind, RecallScopeKind::FileSet);
        assert_eq!(result.searched_scope.files, vec!["src/recall.rs"]);
        assert_eq!(result.currentness, CodeSourceCurrentness::Current);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].score_kind,
            SemanticScoreKind::SemanticSimilarity
        );
    }

    #[test]
    fn fake_semantic_provider_supports_all_fixture_statuses() {
        let cases = [
            (
                FakeSemanticCodeSearchProvider::not_configured(),
                RecallProviderStatus::NotConfigured,
                CodeSourceCurrentness::GraphNotConfigured,
            ),
            (
                FakeSemanticCodeSearchProvider::indexing(),
                RecallProviderStatus::Indexing,
                CodeSourceCurrentness::GraphUnavailable,
            ),
            (
                FakeSemanticCodeSearchProvider::stale(vec![fake_semantic_candidate(
                    "src/stale.rs",
                    0.71,
                )]),
                RecallProviderStatus::Stale,
                CodeSourceCurrentness::ContentChanged,
            ),
            (
                FakeSemanticCodeSearchProvider::failed(),
                RecallProviderStatus::Failed,
                CodeSourceCurrentness::GraphUnavailable,
            ),
            (
                FakeSemanticCodeSearchProvider::unsupported(),
                RecallProviderStatus::Unsupported,
                CodeSourceCurrentness::GraphUnavailable,
            ),
        ];
        let request = semantic_request();

        for (provider, expected_status, expected_currentness) in cases {
            let result = provider.search(&request);
            assert_eq!(result.status, expected_status);
            assert_eq!(result.currentness, expected_currentness);
        }
    }

    #[test]
    fn fake_semantic_no_result_language_is_limited_to_searched_scope() {
        let provider = FakeSemanticCodeSearchProvider::no_results(RecallSearchedScope {
            scope_kind: RecallScopeKind::FileSet,
            scope_fingerprint: Some("scope-123".to_string()),
            files: vec!["src/no_result.rs".to_string()],
            symbols: Vec::new(),
            corpus_id: Some("workspace-a".to_string()),
            corpus_version: Some("graph-1".to_string()),
        });

        let result = provider.search(&semantic_request());
        let language = fake_semantic_no_result_language(&result);

        assert!(result.candidates.is_empty());
        assert_eq!(
            language.permitted_claim,
            NoResultClaim::NoRelevantDecisionInSearchedScope
        );
        assert!(language.scope.allows_no_result_claim());
        assert_eq!(
            language.scope.code.searched_targets,
            vec!["src/no_result.rs"]
        );
    }

    #[test]
    fn fake_semantic_unscoped_no_result_refuses_global_absence_claim() {
        let provider = FakeSemanticCodeSearchProvider::no_results(RecallSearchedScope::query());

        let result = provider.search(&semantic_request());
        let language = fake_semantic_no_result_language(&result);

        assert_eq!(language.permitted_claim, NoResultClaim::Refused);
        assert!(!language.scope.allows_no_result_claim());
    }

    #[test]
    fn fake_semantic_candidates_remain_rank_only_and_non_authoritative() {
        let mut candidate = fake_semantic_candidate("src/semantic.rs", 0.88);
        candidate
            .reasons
            .push("fixture exact-looking reason".to_string());
        candidate.matched_terms.push("admissibility".to_string());
        candidate.matched_excerpt = Some("evaluate_semantic_candidate".to_string());
        let provider = FakeSemanticCodeSearchProvider::ready(vec![candidate]);

        let result = provider.search(&semantic_request());
        let decision = RecallAdmissibilityGate::new()
            .evaluate_semantic_candidate(&result, &result.candidates[0]);

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::SemanticSignalRankOnly));
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::ApproximateTargetIdentity));
        assert!(decision
            .allowed_uses
            .contains(&RecallAdmissibleUse::RankOnly));
        assert!(!decision
            .allowed_uses
            .contains(&RecallAdmissibleUse::SliceHint));
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn fake_semantic_stale_output_maps_to_caveats_not_authority() {
        let provider = FakeSemanticCodeSearchProvider::stale(vec![fake_semantic_candidate(
            "src/stale.rs",
            0.64,
        )]);

        let result = provider.search(&semantic_request());
        let decision = RecallAdmissibilityGate::new()
            .evaluate_semantic_candidate(&result, &result.candidates[0]);

        assert_eq!(result.status, RecallProviderStatus::Stale);
        assert_eq!(
            result.candidates[0].currentness,
            CodeSourceCurrentness::ContentChanged
        );
        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::ProviderStale));
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::StaleCurrentness));
        assert!(!decision.authority_effects.any());
    }

    #[test]
    fn fake_semantic_fixture_can_report_unknown_scope_without_default_behavior_changes() {
        let mut fixture = FakeSemanticCodeSearchFixture::ready(Vec::new());
        fixture.unknown_scope.push(RecallUnknownProviderScope {
            scope: "src/generated".to_string(),
            reason: RecallUnknownProviderScopeReason::NotIndexed,
            detail: Some("generated files excluded from fixture".to_string()),
        });
        let provider = FakeSemanticCodeSearchProvider::new(fixture);

        let result = provider.search(&semantic_request());

        assert_eq!(result.unknown_scope.len(), 1);
        assert_eq!(
            NoopSemanticCodeSearchProvider::new()
                .search(&semantic_request())
                .status,
            RecallProviderStatus::NotConfigured
        );
    }

    #[test]
    fn local_semantic_provider_is_not_configured_without_index() {
        let provider = LocalSemanticCodeSearchProvider::not_configured();
        let result = provider.search(&semantic_request());

        assert_eq!(result.status, RecallProviderStatus::NotConfigured);
        assert_eq!(result.provenance.provider_id, "noop_semantic_code");
        assert!(result.candidates.is_empty());
        assert!(result.unknown_scope.is_empty());
    }

    #[test]
    fn local_semantic_provider_returns_metadata_candidates_with_scope_and_provenance() {
        let provider = LocalSemanticCodeSearchProvider::from_index(local_semantic_index(
            RecallProviderStatus::Ready,
            CodeSourceCurrentness::Current,
        ));
        let mut request = semantic_request();
        request.query = "checkout rate limiter".to_string();
        request.files = Vec::new();
        request.scope_kind = Some(RecallScopeKind::Query);

        let result = provider.search(&request);

        assert_eq!(result.status, RecallProviderStatus::Ready);
        assert_eq!(
            result.provenance.provider_id,
            "local_semantic_code_metadata"
        );
        assert_eq!(
            result.provenance.index_version.as_deref(),
            Some(SEMANTIC_CHUNK_INDEX_VERSION)
        );
        assert_eq!(result.graph_snapshot_id.as_deref(), Some("graph-1"));
        assert_eq!(
            result.searched_scope.corpus_version.as_deref(),
            Some("graph-1")
        );
        assert_eq!(result.unknown_scope.len(), 1);
        assert_eq!(
            result.unknown_scope[0].reason,
            RecallUnknownProviderScopeReason::NotIndexed
        );
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].file, "src/checkout.rs");
        assert_eq!(result.candidates[0].symbol.as_deref(), Some("RateLimiter"));
        assert_eq!(result.candidates[0].chunk_id.as_deref(), Some("chunk-rate"));
        assert_eq!(
            result.candidates[0].target_type,
            SemanticCodeTargetType::Symbol
        );
        assert_eq!(
            result.candidates[0].score_kind,
            SemanticScoreKind::HybridRank
        );
        assert_eq!(
            result.candidates[0].provenance.provider_id,
            "local_semantic_code_metadata"
        );
        assert!(result.candidates[0]
            .matched_terms
            .contains(&"checkout".to_string()));
        assert!(result.candidates[0]
            .matched_terms
            .contains(&"rate".to_string()));
    }

    #[test]
    fn local_semantic_provider_maps_snapshot_mismatch_to_stale_status() {
        let provider = LocalSemanticCodeSearchProvider::from_index(local_semantic_index(
            RecallProviderStatus::Ready,
            CodeSourceCurrentness::Current,
        ));
        let mut request = semantic_request();
        request.query = "checkout".to_string();
        request.files = Vec::new();
        request.scope_kind = Some(RecallScopeKind::Query);
        request.graph_snapshot_id = Some("graph-newer".to_string());

        let result = provider.search(&request);

        assert_eq!(result.status, RecallProviderStatus::Stale);
        assert_eq!(result.currentness, CodeSourceCurrentness::ContentChanged);
        assert_eq!(
            result.candidates[0].currentness,
            CodeSourceCurrentness::ContentChanged
        );
    }

    #[test]
    fn local_semantic_provider_candidates_remain_advisory_until_graph_verification() {
        let provider = LocalSemanticCodeSearchProvider::from_index(local_semantic_index(
            RecallProviderStatus::Ready,
            CodeSourceCurrentness::Current,
        ));
        let mut request = semantic_request();
        request.query = "checkout rate limiter".to_string();
        request.files = Vec::new();
        request.scope_kind = Some(RecallScopeKind::Query);
        let result = provider.search(&request);

        let decision = RecallAdmissibilityGate::new()
            .evaluate_semantic_candidate(&result, &result.candidates[0]);

        assert_eq!(decision.outcome, RecallAdmissibilityOutcome::Downgrade);
        assert!(decision
            .reason_codes
            .contains(&RecallAdmissibilityReasonCode::SemanticSignalRankOnly));
        assert!(!decision.authority_effects.any());
    }

    fn semantic_request() -> SemanticCodeSearchRequest {
        SemanticCodeSearchRequest {
            query: "fixture query".to_string(),
            workspace_id: Some("workspace-a".to_string()),
            graph_snapshot_id: Some("graph-1".to_string()),
            commit_sha: None,
            ref_name: None,
            files: vec!["src/lib.rs".to_string()],
            symbols: Vec::new(),
            changed_files: Vec::new(),
            source_evidence_ids: Vec::new(),
            decision_id: None,
            candidate_id: None,
            limit: 5,
            scope_kind: Some(RecallScopeKind::FileSet),
        }
    }

    fn local_semantic_index(
        status: RecallProviderStatus,
        currentness: CodeSourceCurrentness,
    ) -> SemanticCodeChunkIndex {
        SemanticCodeChunkIndex {
            status,
            currentness,
            provenance: RecallProviderProvenance {
                provider_id: "local_semantic_chunk_index".to_string(),
                provider_version: env!("CARGO_PKG_VERSION").to_string(),
                index_version: Some(SEMANTIC_CHUNK_INDEX_VERSION.to_string()),
                overlay_id: None,
                model_id: None,
                model_version: None,
            },
            workspace_id: Some("workspace-a".to_string()),
            repo_id: "repo-a".to_string(),
            commit_sha: "commit-1".to_string(),
            graph_snapshot_id: "graph-1".to_string(),
            chunker_version: SEMANTIC_CHUNKER_VERSION.to_string(),
            index_version: SEMANTIC_CHUNK_INDEX_VERSION.to_string(),
            chunks: vec![
                SemanticCodeChunkMetadata {
                    chunk_id: "chunk-rate".to_string(),
                    workspace_id: Some("workspace-a".to_string()),
                    repo_id: "repo-a".to_string(),
                    commit_sha: "commit-1".to_string(),
                    graph_snapshot_id: "graph-1".to_string(),
                    file_path: "src/checkout.rs".to_string(),
                    line_start: 10,
                    line_end: 24,
                    symbol_name: Some("RateLimiter".to_string()),
                    language: "rust".to_string(),
                    content_hash: "hash-rate".to_string(),
                    chunker_version: SEMANTIC_CHUNKER_VERSION.to_string(),
                    index_version: SEMANTIC_CHUNK_INDEX_VERSION.to_string(),
                    chunk_kind: SemanticCodeChunkKind::RustSymbol,
                },
                SemanticCodeChunkMetadata {
                    chunk_id: "chunk-ledger".to_string(),
                    workspace_id: Some("workspace-a".to_string()),
                    repo_id: "repo-a".to_string(),
                    commit_sha: "commit-1".to_string(),
                    graph_snapshot_id: "graph-1".to_string(),
                    file_path: "src/ledger.rs".to_string(),
                    line_start: 1,
                    line_end: 12,
                    symbol_name: None,
                    language: "rust".to_string(),
                    content_hash: "hash-ledger".to_string(),
                    chunker_version: SEMANTIC_CHUNKER_VERSION.to_string(),
                    index_version: SEMANTIC_CHUNK_INDEX_VERSION.to_string(),
                    chunk_kind: SemanticCodeChunkKind::File,
                },
            ],
            skipped_files: vec![SemanticCodeSkippedFile {
                file_path: "vendor/generated.rs".to_string(),
                reason: SemanticCodeSkippedReason::Vendor,
                detail: Some("vendor/dependency path skipped by default".to_string()),
                language: Some("rust".to_string()),
            }],
            caveats: Vec::new(),
            ranking_metadata: Vec::new(),
        }
    }
}
