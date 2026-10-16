use std::collections::{BTreeSet, HashMap};

use bicameral_api::candidate::CandidateState;
use bicameral_api::decision_index::{
    CompilationState, DecisionIndexRecord, IndexSourceRef, MetadataAcceptanceState, MetadataEntry,
    MetadataExtractionMethod, MetadataProvenance, MetadataSourceType, METADATA_SCHEMA_VERSION,
};
use bicameral_api::evidence::GraphReadiness;
use bicameral_api::recall::{
    AuthorityLabel, AuthorityLabelKind, CandidateRelevance, CanonicalDecisionEntry,
    EvidenceFreshness, EvidenceRef, EvidenceRefKind, NoResultLanguage, RankTuple, RecallPacket,
    RecallSignal, RecallTrace, RouteResult, RouteStatus, ScopeLevel, ScopeSection, SearchedScope,
    StopCondition, StopConditionKind, SurfacedDecisionCandidate, UnknownScopeEntry,
};
use bicameral_api::recall_route_merge::{
    merge_and_rank, CandidateIdentity, CandidateIdentityKind, RankedRecallCandidate, ReasonCode,
    RecallPreferenceProfile, RouteCandidate, RouteClass,
};
use bicameral_api::recall_taxonomy::TaxonomyVersion;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::preflight::{PreflightGraphContext, PreflightQuery, PreflightReadiness};
use crate::replay::ReplayedState;

const DECISION_INDEX_VERSION: &str = "decision-index-v1";
const COMPILER_VERSION: &str = "decision_index_record_v1";
const RESOLVER_VERSION: &str = "merge_and_rank_v1";
const CONFIG_VERSION: &str = "preflight-live-recall-alpha-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveRecallOutcome {
    Results,
    NoMatch,
    ReadinessLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionIndexReadinessStatus {
    Ready,
    Partial,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionIndexRecordProof {
    pub decision_id: Uuid,
    pub compilation_state: CompilationState,
    pub coverage_sufficient: bool,
    pub coverage_ratio: f64,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionIndexReadinessProof {
    pub status: DecisionIndexReadinessStatus,
    pub index_version: String,
    pub metadata_schema_version: u32,
    pub corpus_version: String,
    pub record_count: usize,
    pub ready_record_count: usize,
    pub records: Vec<DecisionIndexRecordProof>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallImplementationProvenance {
    pub surface: String,
    pub runtime_crate: String,
    pub runtime_version: String,
    pub runtime_commit: Option<String>,
    pub runtime_commit_unavailable_reason: Option<String>,
    pub compiler: String,
    pub resolver: String,
    pub config_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveRecallView {
    pub packet: RecallPacket,
    pub ranked_results: Vec<RankedRecallCandidate>,
    pub index_readiness: DecisionIndexReadinessProof,
    pub outcome: LiveRecallOutcome,
    pub scoring_eligible: bool,
    pub readiness_limitations: Vec<String>,
    pub no_result_language: Option<NoResultLanguage>,
    pub legacy_source_matching: String,
    pub implementation_provenance: RecallImplementationProvenance,
}

pub fn build_live_recall(
    state: &ReplayedState,
    query: &PreflightQuery,
    readiness: &PreflightReadiness,
    graph_ctx: Option<&PreflightGraphContext>,
) -> LiveRecallView {
    let acceptance_events = acceptance_events_by_decision(state);
    let mut decisions: Vec<_> = state.decisions.values().collect();
    decisions.sort_by_key(|decision| decision.id);

    let compiled: Vec<_> = decisions
        .iter()
        .map(|decision| {
            compile_decision_record(
                state,
                decision,
                acceptance_events.get(&decision.id).copied(),
            )
        })
        .collect();
    let corpus_version = corpus_version(&compiled);
    let index_readiness = index_readiness(&compiled, corpus_version.clone());
    let query_terms = query_terms(query);
    let graph_snapshot = graph_ctx.and_then(|ctx| ctx.resolution.snapshot());
    let graph_ready = readiness.state == GraphReadiness::Ready && graph_snapshot.is_some();
    let scoring_eligible = index_readiness.status == DecisionIndexReadinessStatus::Ready
        && graph_ready
        && !query_terms.is_empty();
    let mut readiness_limitations = Vec::new();
    if index_readiness.status != DecisionIndexReadinessStatus::Ready {
        readiness_limitations.push("decision_index_partial".to_string());
    }
    if !graph_ready {
        readiness_limitations.push(format!("graph_scope_{:?}", readiness.state).to_lowercase());
    }
    if query_terms.is_empty() {
        readiness_limitations.push("query_scope_empty".to_string());
    }

    let route_candidates = route_candidates(
        state,
        &compiled,
        &query_terms,
        graph_snapshot.map(|snapshot| snapshot.graph_snapshot_id.as_str()),
    );
    let merged = merge_and_rank(route_candidates, &RecallPreferenceProfile::default());
    let ranked_results = if scoring_eligible { merged } else { Vec::new() };
    let outcome = if !scoring_eligible {
        LiveRecallOutcome::ReadinessLimited
    } else if ranked_results.is_empty() {
        LiveRecallOutcome::NoMatch
    } else {
        LiveRecallOutcome::Results
    };

    let searched_scope = searched_scope(
        state,
        query,
        readiness,
        graph_snapshot.map(|snapshot| snapshot.graph_snapshot_id.clone()),
        corpus_version.clone(),
        scoring_eligible,
    );
    let trace = recall_trace(
        query,
        &query_terms,
        &ranked_results,
        graph_snapshot.map(|snapshot| snapshot.graph_snapshot_id.clone()),
        corpus_version.clone(),
        scoring_eligible,
    );
    let no_result_language = ranked_results
        .is_empty()
        .then(|| NoResultLanguage::from_scope(searched_scope.clone()));
    let packet = recall_packet(
        state,
        &ranked_results,
        &query_terms,
        searched_scope,
        trace,
        readiness,
    );

    LiveRecallView {
        packet,
        ranked_results,
        index_readiness,
        outcome,
        scoring_eligible,
        readiness_limitations,
        no_result_language,
        legacy_source_matching: "characterization_only".to_string(),
        implementation_provenance: RecallImplementationProvenance {
            surface: "preflight.run".to_string(),
            runtime_crate: env!("CARGO_PKG_NAME").to_string(),
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            runtime_commit: graph_snapshot.map(|snapshot| snapshot.commit_sha.clone()),
            runtime_commit_unavailable_reason: graph_snapshot.is_none().then(|| {
                "workspace graph snapshot unavailable; runtime commit could not be resolved"
                    .to_string()
            }),
            compiler: COMPILER_VERSION.to_string(),
            resolver: RESOLVER_VERSION.to_string(),
            config_version: CONFIG_VERSION.to_string(),
        },
    }
}

fn acceptance_events_by_decision(
    state: &ReplayedState,
) -> HashMap<Uuid, (&bicameral_api::event_store::EventStoreEntry, Uuid)> {
    state
        .events
        .iter()
        .filter_map(|event| {
            if event.event_type != bicameral_api::event_store::EventType::AcceptCandidate {
                return None;
            }
            let decision_id = event
                .payload
                .get("decision_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())?;
            Some((decision_id, (event, event.id)))
        })
        .collect()
}

fn compile_decision_record(
    state: &ReplayedState,
    decision: &bicameral_api::candidate::Decision,
    acceptance: Option<(&bicameral_api::event_store::EventStoreEntry, Uuid)>,
) -> DecisionIndexRecord {
    let source = decision
        .source_id
        .as_ref()
        .and_then(|source_id| state.sources.get(source_id));
    let source_ref = decision.source_id.clone();
    let source_domain = source
        .map(|item| item.source_type.clone())
        .unwrap_or_else(|| "canonical_decision".to_string());
    let source_component = source_ref
        .clone()
        .unwrap_or_else(|| decision.id.to_string());
    let taxonomy_version = TaxonomyVersion::alpha().to_string();
    let (accepted_by, accepted_at, acceptance_event_id) = acceptance
        .map(|(event, event_id)| {
            (
                Some(event.actor.clone()),
                Some(event.timestamp),
                Some(event_id),
            )
        })
        .unwrap_or_else(|| {
            (
                Some(decision.promoted_by.clone()),
                Some(decision.promoted_at),
                None,
            )
        });
    let provenance = MetadataProvenance {
        source_type: MetadataSourceType::HumanAuthored,
        source_ref,
        proposer: decision.promoted_by.clone(),
        extraction_method: MetadataExtractionMethod::CopyFromCanonical,
        accepted_by,
        accepted_at,
        acceptance_event_id,
    };
    let compiled_at = acceptance
        .map(|(event, _)| event.timestamp)
        .unwrap_or(decision.promoted_at);
    let values = [
        ("domain", serde_json::Value::String(source_domain)),
        ("component", serde_json::Value::String(source_component)),
        (
            "decision_level",
            serde_json::to_value(decision.level).expect("DecisionLevel is serializable"),
        ),
        (
            "summary",
            serde_json::Value::String(format!("{} {}", decision.title, decision.body)),
        ),
        (
            "keywords",
            serde_json::Value::Array(
                tokenize(&format!("{} {}", decision.title, decision.body))
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        ),
    ];

    let mut record =
        DecisionIndexRecord::for_decision(decision.id, decision.title.clone(), decision.level);
    record.source_ref = IndexSourceRef::Decision {
        decision_id: decision.id,
        title: decision.title.clone(),
        level: decision.level,
        signoff_state: decision.signoff_state,
    };
    for (key, value) in values {
        record.add_metadata(MetadataEntry {
            key: key.to_string(),
            value,
            provenance: provenance.clone(),
            acceptance_state: MetadataAcceptanceState::Accepted,
            quality_labels: Vec::new(),
            taxonomy_version: taxonomy_version.clone(),
            compiled_at,
        });
    }
    record.compiled_at = compiled_at;
    record
}

fn index_readiness(
    compiled: &[DecisionIndexRecord],
    corpus_version: String,
) -> DecisionIndexReadinessProof {
    let records: Vec<_> = compiled
        .iter()
        .map(|record| {
            let decision_id = match record.source_ref {
                IndexSourceRef::Decision { decision_id, .. } => decision_id,
                IndexSourceRef::Candidate { candidate_id, .. } => candidate_id,
            };
            let mut source_refs: Vec<_> = record
                .metadata
                .iter()
                .filter_map(|entry| entry.provenance.source_ref.clone())
                .collect();
            source_refs.sort();
            source_refs.dedup();
            DecisionIndexRecordProof {
                decision_id,
                compilation_state: record.compilation_state,
                coverage_sufficient: record.coverage.is_sufficient(),
                coverage_ratio: record.coverage.coverage_ratio,
                source_refs,
            }
        })
        .collect();
    let ready_record_count = records
        .iter()
        .filter(|record| {
            record.compilation_state == CompilationState::Accepted && record.coverage_sufficient
        })
        .count();
    let status = if ready_record_count == records.len() {
        DecisionIndexReadinessStatus::Ready
    } else {
        DecisionIndexReadinessStatus::Partial
    };

    DecisionIndexReadinessProof {
        status,
        index_version: DECISION_INDEX_VERSION.to_string(),
        metadata_schema_version: METADATA_SCHEMA_VERSION,
        corpus_version,
        record_count: records.len(),
        ready_record_count,
        records,
    }
}

fn route_candidates(
    state: &ReplayedState,
    compiled: &[DecisionIndexRecord],
    query_terms: &BTreeSet<String>,
    graph_snapshot_id: Option<&str>,
) -> Vec<RouteCandidate> {
    compiled
        .iter()
        .filter(|record| {
            record.compilation_state == CompilationState::Accepted
                && record.coverage.is_sufficient()
        })
        .filter_map(|record| {
            let (decision_id, title) = match &record.source_ref {
                IndexSourceRef::Decision {
                    decision_id, title, ..
                } => (*decision_id, title.clone()),
                IndexSourceRef::Candidate { .. } => return None,
            };
            let indexed_terms = metadata_terms(record);
            let matched_count = query_terms.intersection(&indexed_terms).count();
            if matched_count == 0 {
                return None;
            }
            let decision = state.decisions.get(&decision_id)?;
            let mut source_refs = Vec::new();
            if let Some(source_id) = &decision.source_id {
                source_refs.push(source_id.clone());
            }
            source_refs.extend(
                decision
                    .evidence_refs
                    .iter()
                    .map(|evidence_id| format!("evidence:{evidence_id}")),
            );
            source_refs.extend(
                decision
                    .evidence_reference_ids
                    .iter()
                    .map(|evidence_id| format!("evidence_reference:{evidence_id}")),
            );
            source_refs.sort();
            source_refs.dedup();

            Some(RouteCandidate {
                identity: CandidateIdentity {
                    id: decision_id,
                    title,
                    kind: CandidateIdentityKind::CanonicalDecision,
                },
                route: RouteClass::Structured,
                reason: ReasonCode::StructuredCorpusMatch,
                secondary_reasons: vec![ReasonCode::StructuredMetadataMatch],
                route_score: matched_count as f64 / query_terms.len() as f64,
                freshness: EvidenceFreshness::Current,
                has_scope_proof: true,
                source_refs,
                graph_snapshot_id: graph_snapshot_id.map(str::to_string),
                caveats: Vec::new(),
            })
        })
        .collect()
}

fn metadata_terms(record: &DecisionIndexRecord) -> BTreeSet<String> {
    record
        .metadata
        .iter()
        .filter(|entry| entry.is_full_participant())
        .flat_map(|entry| match &entry.value {
            serde_json::Value::String(value) => tokenize(value),
            serde_json::Value::Array(values) => values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .flat_map(tokenize)
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn query_terms(query: &PreflightQuery) -> BTreeSet<String> {
    query
        .files
        .iter()
        .chain(query.symbols.iter())
        .chain(query.diff_context.iter())
        .flat_map(|value| tokenize(value))
        .collect()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|token| token.len() >= 3)
        .filter(|token| {
            !matches!(
                token.as_str(),
                "and" | "for" | "from" | "into" | "must" | "the" | "this" | "with"
            )
        })
        .collect()
}

fn corpus_version(compiled: &[DecisionIndexRecord]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DECISION_INDEX_VERSION.as_bytes());
    for record in compiled {
        if let IndexSourceRef::Decision {
            decision_id,
            title,
            level,
            signoff_state,
        } = &record.source_ref
        {
            hasher.update(decision_id.as_bytes());
            hasher.update(title.as_bytes());
            hasher.update(format!("{level:?}:{signoff_state:?}").as_bytes());
        }
        for entry in &record.metadata {
            hasher.update(entry.key.as_bytes());
            hasher.update(entry.value.to_string().as_bytes());
        }
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn searched_scope(
    state: &ReplayedState,
    query: &PreflightQuery,
    readiness: &PreflightReadiness,
    graph_snapshot_id: Option<String>,
    corpus_version: String,
    scoring_eligible: bool,
) -> SearchedScope {
    let mut source_targets: Vec<_> = state.sources.keys().cloned().collect();
    source_targets.sort();
    let mut code_targets = query.files.clone();
    code_targets.extend(query.symbols.iter().cloned());
    code_targets.sort();
    code_targets.dedup();

    SearchedScope {
        level: if scoring_eligible {
            ScopeLevel::Replayable
        } else {
            ScopeLevel::Enumerated
        },
        source: ScopeSection::searched(source_targets, Some(corpus_version.clone())),
        code: if scoring_eligible {
            ScopeSection::searched(code_targets, graph_snapshot_id.clone())
        } else {
            ScopeSection::partial(
                code_targets,
                "Code scope was declared but graph readiness was unavailable.",
                graph_snapshot_id.clone(),
            )
        },
        graph: if scoring_eligible {
            ScopeSection::searched(
                graph_snapshot_id.iter().cloned().collect(),
                graph_snapshot_id,
            )
        } else {
            ScopeSection::unavailable(format!(
                "Graph readiness is {:?}; no scored miss is permitted.",
                readiness.state
            ))
        },
        retrieval: ScopeSection::searched(
            vec!["canonical_decision_metadata".to_string()],
            Some(format!("{DECISION_INDEX_VERSION}:{corpus_version}")),
        ),
        advisory: ScopeSection::not_applicable(),
        unknown: if scoring_eligible {
            ScopeSection::not_applicable()
        } else {
            ScopeSection::unavailable(
                "Unsearched graph and hosted/team scope remain explicitly unknown.",
            )
        },
    }
}

fn recall_trace(
    query: &PreflightQuery,
    query_terms: &BTreeSet<String>,
    ranked_results: &[RankedRecallCandidate],
    graph_snapshot_id: Option<String>,
    corpus_version: String,
    scoring_eligible: bool,
) -> RecallTrace {
    let request_hash = sha256_json(query);
    let normalized_input_hash = sha256_json(query_terms);
    let route_status = if !scoring_eligible {
        RouteStatus::Skipped
    } else if ranked_results.is_empty() {
        RouteStatus::Empty
    } else {
        RouteStatus::Completed
    };
    let route_results = vec![RouteResult {
        route_id: "structured_decision_index".to_string(),
        status: route_status,
        candidate_count: ranked_results.len() as u32,
        detail: (!scoring_eligible).then(|| {
            "Index candidates were not scored because requested scope was readiness-limited."
                .to_string()
        }),
    }];
    let admitted_signals = ranked_results
        .iter()
        .flat_map(|candidate| {
            candidate.reason_codes.iter().map(|reason| RecallSignal {
                kind: reason_code_string(*reason),
                source: candidate.identity.id.to_string(),
                reason: reason.explanation().to_string(),
            })
        })
        .collect();
    let final_rank_tuples = ranked_results
        .iter()
        .map(|candidate| RankTuple {
            result_id: candidate.identity.id.to_string(),
            tier: candidate.tier.0,
            score: candidate.sort_key.effective_score,
            contributing_signals: candidate
                .reason_codes
                .iter()
                .map(|reason| reason_code_string(*reason))
                .collect(),
        })
        .collect();
    let trace_id = format!(
        "recall-{}-{}",
        &request_hash[..16],
        &corpus_version["sha256:".len()..][..16]
    );

    RecallTrace {
        trace_id,
        request_hash,
        normalized_input_hash,
        decision_index_version: DECISION_INDEX_VERSION.to_string(),
        metadata_schema_version: METADATA_SCHEMA_VERSION.to_string(),
        taxonomy_version: TaxonomyVersion::alpha().to_string(),
        graph_snapshot_id,
        source_corpus_version: corpus_version,
        semantic_index_version: None,
        codegenome_signal_version: None,
        preference_profile_version: "default-v1".to_string(),
        scoring_function_version: RESOLVER_VERSION.to_string(),
        route_results,
        admitted_signals,
        downgraded_signals: Vec::new(),
        rejected_signals: Vec::new(),
        final_rank_tuples,
        stop_conditions: vec![StopCondition {
            kind: if scoring_eligible {
                StopConditionKind::AllRoutesCompleted
            } else {
                StopConditionKind::ScopeBoundary
            },
            detail: if scoring_eligible {
                "All configured live recall routes completed.".to_string()
            } else {
                "Scope readiness boundary prevented scoring.".to_string()
            },
        }],
        replay_available: true,
        replay_unavailable_reason: None,
    }
}

fn recall_packet(
    state: &ReplayedState,
    ranked_results: &[RankedRecallCandidate],
    query_terms: &BTreeSet<String>,
    searched_scope: SearchedScope,
    recall_trace: RecallTrace,
    readiness: &PreflightReadiness,
) -> RecallPacket {
    let canonical_decisions = ranked_results
        .iter()
        .filter_map(|ranked| state.decisions.get(&ranked.identity.id))
        .map(|decision| CanonicalDecisionEntry {
            decision_id: decision.id,
            title: decision.title.clone(),
            summary: decision.body.clone(),
            level: decision.level,
            signoff_state: decision.signoff_state,
            evidence_refs: decision
                .evidence_refs
                .iter()
                .map(|evidence_id| EvidenceRef {
                    id: *evidence_id,
                    label: format!("Source evidence {evidence_id}"),
                    kind: EvidenceRefKind::SourceSpan,
                })
                .chain(decision.evidence_reference_ids.iter().map(|evidence_id| {
                    EvidenceRef {
                        id: *evidence_id,
                        label: format!("Evidence reference {evidence_id}"),
                        kind: EvidenceRefKind::ExternalDocument,
                    }
                }))
                .collect(),
            authority_labels: vec![AuthorityLabel {
                kind: AuthorityLabelKind::Advisory,
                description: "Recall ranking is read-only and carries no compliance, signoff, no-conflict, or merge-safety authority.".to_string(),
            }],
        })
        .collect();
    let unknown_scope = readiness
        .unknown_scope
        .iter()
        .map(|unknown| UnknownScopeEntry {
            scope_id: unknown.scope.clone(),
            description: format!("{}: {}", unknown.state, unknown.detail),
            expansion_targets: Vec::new(),
        })
        .collect();
    let decision_candidates = surfaced_candidates(state, query_terms);
    let mut packet = RecallPacket::new_alpha();
    packet.canonical_decisions = canonical_decisions;
    packet.decision_candidates = decision_candidates;
    packet.unknown_scope = unknown_scope;
    packet.searched_scope = Some(searched_scope);
    packet.recall_trace = Some(recall_trace);
    // The daemon owns the decision-attention determination; derive it from the
    // reviewable candidates it just surfaced so the consumer reads a typed
    // signal rather than inferring one (issue #827).
    packet.with_derived_decision_attention()
}

/// Project proposed candidates from replayed state into reviewable recall
/// entries, scoped by the query terms (issue #827).
///
/// Read-only: reads replayed candidate state and returns a projection. It
/// appends no events and mutates no Decision, candidate, signoff, binding,
/// compliance, or lifecycle state. Promoted candidates are excluded — they are
/// canonical Decisions, not review-pending candidates. Bounded scope is honest:
/// with query terms only text-matching candidates are surfaced; without them
/// the full proposed ledger is surfaced as a `FullLedgerScan`.
fn surfaced_candidates(
    state: &ReplayedState,
    query_terms: &BTreeSet<String>,
) -> Vec<SurfacedDecisionCandidate> {
    let has_query_terms = !query_terms.is_empty();
    let mut surfaced: Vec<SurfacedDecisionCandidate> = state
        .candidates
        .values()
        .filter(|candidate| candidate.candidate_state != CandidateState::Promoted)
        .filter_map(|candidate| {
            let relevance = if has_query_terms {
                let text =
                    format!("{} {}", candidate.title, candidate.description).to_ascii_lowercase();
                if query_terms.iter().any(|term| text.contains(term.as_str())) {
                    CandidateRelevance::SourceTextMatch
                } else {
                    return None;
                }
            } else {
                CandidateRelevance::FullLedgerScan
            };
            Some(SurfacedDecisionCandidate::from_candidate(
                candidate,
                vec![relevance],
                Vec::new(),
            ))
        })
        .collect();
    surfaced.sort_by(|a, b| {
        a.title
            .cmp(&b.title)
            .then(a.candidate_id.cmp(&b.candidate_id))
    });
    surfaced
}

fn sha256_json<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("live recall trace inputs are serializable");
    format!("{:x}", Sha256::digest(bytes))
}

fn reason_code_string(reason: ReasonCode) -> String {
    serde_json::to_value(reason)
        .expect("ReasonCode is serializable")
        .as_str()
        .expect("ReasonCode serializes as a string")
        .to_string()
}
