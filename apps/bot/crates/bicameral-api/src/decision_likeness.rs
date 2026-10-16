//! Deterministic, reviewable decision-likeness evaluation for candidate projection.
//!
//! Integration hints and noise advisories are features only. They never create a
//! candidate by themselves and never delete accepted evidence. The evaluator
//! requires decision language plus corroboration from the screened source body
//! or evidence excerpts before a proposal may be materialized.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DECISION_LIKENESS_THRESHOLD: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionLikenessReasonCode {
    EmptyProposal,
    StrongDecisionLanguage,
    WeakDecisionLanguage,
    SourceDecisionLanguage,
    ExactSourceCorroboration,
    TokenCorroboration,
    EvidenceCorroboration,
    AdvisoryNoisePenalty,
    SpeculativeLanguagePenalty,
    BelowThreshold,
    Qualified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionLikenessEvaluation {
    pub qualified: bool,
    pub score: i32,
    pub threshold: i32,
    pub reason_codes: Vec<DecisionLikenessReasonCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionLikenessInputKind {
    SourceContent,
    TitleDescription,
    CandidateHint,
    CandidateDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionLikenessRecord {
    pub input_kind: DecisionLikenessInputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_index: Option<usize>,
    pub evaluation: DecisionLikenessEvaluation,
}

pub fn evaluate_candidate(
    title: &str,
    body: &str,
    source_texts: &[&str],
    evidence_excerpts: &[String],
    advisory_labels: &[String],
) -> DecisionLikenessEvaluation {
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() || body.is_empty() {
        return DecisionLikenessEvaluation {
            qualified: false,
            score: 0,
            threshold: DECISION_LIKENESS_THRESHOLD,
            reason_codes: vec![
                DecisionLikenessReasonCode::EmptyProposal,
                DecisionLikenessReasonCode::BelowThreshold,
            ],
        };
    }

    let proposal = format!("{title} {body}").to_ascii_lowercase();
    let source = source_texts
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let evidence = evidence_excerpts
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    let mut score = 0;
    let mut reasons = BTreeSet::new();

    let explicitly_non_decision = contains_any(&proposal, EXPLICIT_NON_DECISION_MARKERS);
    let proposal_has_decision_language = if explicitly_non_decision {
        false
    } else if contains_any(&proposal, STRONG_DECISION_MARKERS) {
        score += 3;
        reasons.insert(DecisionLikenessReasonCode::StrongDecisionLanguage);
        true
    } else if contains_any(&proposal, WEAK_DECISION_MARKERS) {
        score += 1;
        reasons.insert(DecisionLikenessReasonCode::WeakDecisionLanguage);
        true
    } else {
        false
    };

    if contains_any(&source, STRONG_DECISION_MARKERS) {
        score += 2;
        reasons.insert(DecisionLikenessReasonCode::SourceDecisionLanguage);
    } else if contains_any(&source, WEAK_DECISION_MARKERS) {
        score += 1;
        reasons.insert(DecisionLikenessReasonCode::SourceDecisionLanguage);
    }

    let normalized_body = normalize_whitespace(body);
    if normalized_body.len() >= 12 && normalize_whitespace(&source).contains(&normalized_body) {
        score += 2;
        reasons.insert(DecisionLikenessReasonCode::ExactSourceCorroboration);
    }

    let proposal_tokens = significant_tokens(&proposal);
    let source_overlap = token_overlap(&proposal_tokens, &significant_tokens(&source));
    if source_overlap >= 3 {
        score += 2;
        reasons.insert(DecisionLikenessReasonCode::TokenCorroboration);
    } else if source_overlap > 0 {
        score += 1;
        reasons.insert(DecisionLikenessReasonCode::TokenCorroboration);
    }

    let evidence_overlap = token_overlap(&proposal_tokens, &significant_tokens(&evidence));
    if evidence_overlap >= 2 {
        score += 1;
        reasons.insert(DecisionLikenessReasonCode::EvidenceCorroboration);
    }

    let noise_count = advisory_labels
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .filter(|label| ADVISORY_NOISE_LABELS.contains(&label.as_str()))
        .count();
    if noise_count > 0 {
        score -= i32::try_from(noise_count.min(2)).unwrap_or(2);
        reasons.insert(DecisionLikenessReasonCode::AdvisoryNoisePenalty);
    }

    if contains_any(&proposal, SPECULATIVE_MARKERS) {
        score -= 3;
        reasons.insert(DecisionLikenessReasonCode::SpeculativeLanguagePenalty);
    }

    let qualified = proposal_has_decision_language && score >= DECISION_LIKENESS_THRESHOLD;
    reasons.insert(if qualified {
        DecisionLikenessReasonCode::Qualified
    } else {
        DecisionLikenessReasonCode::BelowThreshold
    });

    DecisionLikenessEvaluation {
        qualified,
        score,
        threshold: DECISION_LIKENESS_THRESHOLD,
        reason_codes: reasons.into_iter().collect(),
    }
}

pub fn evaluate_source(
    source_texts: &[&str],
    evidence_excerpts: &[String],
) -> DecisionLikenessEvaluation {
    let combined = source_texts
        .iter()
        .copied()
        .chain(evidence_excerpts.iter().map(String::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    evaluate_candidate(&combined, &combined, source_texts, evidence_excerpts, &[])
}

pub fn first_decision_like_segment(
    source_texts: &[&str],
    evidence_excerpts: &[String],
) -> Option<String> {
    source_texts
        .iter()
        .copied()
        .chain(evidence_excerpts.iter().map(String::as_str))
        .flat_map(|text| text.split(['\n', '.', '!', '?']))
        .map(str::trim)
        .filter(|segment| segment.len() >= 12)
        .find(|segment| {
            evaluate_candidate(segment, segment, source_texts, evidence_excerpts, &[]).qualified
        })
        .map(str::to_string)
}

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

fn normalize_whitespace(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn significant_tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() >= 4)
        .filter(|token| !STOP_WORDS.contains(token))
        .map(str::to_string)
        .collect()
}

fn token_overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> usize {
    left.intersection(right).count()
}

const STRONG_DECISION_MARKERS: &[&str] = &[
    "architecture decision",
    "decision:",
    "decision is",
    "we decided",
    "we will use",
    "we will adopt",
    "must ",
    "must use",
    "must persist",
    "must remain",
    "must not",
    "is required",
    "required to",
    "shall ",
    "selected ",
    "chosen ",
    "canonical ",
    "we prefer",
    "preference is",
    "approved ",
];

const WEAK_DECISION_MARKERS: &[&str] = &[
    "should ",
    "recommend",
    "propose",
    "plan to",
    "intend to",
    "prefer ",
    "would rather",
    "needs to",
];

const SPECULATIVE_MARKERS: &[&str] = &[
    "should we",
    "could we",
    "might ",
    "maybe ",
    "consider ",
    "considering ",
    "for discussion",
    "open question",
];

const EXPLICIT_NON_DECISION_MARKERS: &[&str] = &[
    "no proposed decision",
    "no decision was made",
    "no decision made",
    "not a decision",
    "decision not made",
    "no recommendation",
    "no proposed change",
];

const ADVISORY_NOISE_LABELS: &[&str] = &[
    "dependency_automation",
    "status_only",
    "administrative_only",
    "generated_sync_event",
    "template_dominant",
    "low_signal_density",
];

const STOP_WORDS: &[&str] = &[
    "about", "after", "again", "also", "been", "before", "being", "between", "could", "from",
    "have", "into", "just", "more", "must", "only", "other", "should", "some", "than", "that",
    "their", "there", "these", "they", "this", "those", "through", "using", "very", "were", "what",
    "when", "where", "which", "while", "with", "would",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_hint_without_source_corroboration_does_not_qualify() {
        let evaluation = evaluate_candidate(
            "Adopt PostgreSQL",
            "We decided to adopt PostgreSQL for canonical events",
            &["Dependabot updated serde from 1.0.1 to 1.0.2"],
            &[],
            &["dependency_automation".into(), "status_only".into()],
        );
        assert!(!evaluation.qualified);
        assert!(evaluation
            .reason_codes
            .contains(&DecisionLikenessReasonCode::AdvisoryNoisePenalty));
    }

    #[test]
    fn bot_authored_decision_with_source_corroboration_qualifies() {
        let source =
            "Automation summary: we decided to use SQLite WAL for daemon-local working state.";
        let evaluation = evaluate_candidate(
            "Use SQLite WAL",
            "We decided to use SQLite WAL for daemon-local working state",
            &[source],
            &[source.to_string()],
            &["bot_authored".into()],
        );
        assert!(evaluation.qualified);
        assert!(evaluation
            .reason_codes
            .contains(&DecisionLikenessReasonCode::Qualified));
    }

    #[test]
    fn corroborated_must_requirement_qualifies() {
        let source = "The ingestion adapter must preserve screened evidence before projection.";
        let evaluation = evaluate_candidate(
            "Persist evidence before projection",
            "The daemon must accept screened evidence before candidate projection runs",
            &[source],
            &["preserve screened evidence before projection".into()],
            &["ingest".into(), "durability".into()],
        );
        assert!(evaluation.qualified, "evaluation: {evaluation:?}");
    }

    #[test]
    fn status_only_source_does_not_qualify() {
        let source = "Build succeeded. All checks passed. Branch is up to date.";
        let evaluation = evaluate_candidate(
            "Build status",
            "All checks passed",
            &[source],
            &[],
            &["status_only".into()],
        );
        assert!(!evaluation.qualified);
    }

    #[test]
    fn explicit_non_decision_language_does_not_qualify() {
        let source = "Routine implementation status update with no proposed decision.";
        let evidence = vec!["Routine implementation status update".to_string()];
        let evaluation = evaluate_source(&[source], &evidence);
        assert!(!evaluation.qualified, "evaluation: {evaluation:?}");
    }

    #[test]
    fn content_segment_can_be_derived_without_a_hint() {
        let source = "Routine preface. Architecture decision: the daemon must persist evidence before projection. Follow-up notes.";
        let segment = first_decision_like_segment(&[source], &[]).unwrap();
        assert!(segment.contains("daemon must persist evidence"));
    }

    #[test]
    fn evidence_only_decision_segment_can_be_derived() {
        let evidence = vec![
            "Product decision: we will require explicit confirmation before promotion.".to_string(),
        ];
        let segment = first_decision_like_segment(&["Routine meeting record."], &evidence).unwrap();
        assert!(segment.contains("explicit confirmation before promotion"));
        assert!(evaluate_source(&["Routine meeting record."], &evidence).qualified);
    }

    #[test]
    fn speculative_discussion_is_penalized_below_threshold() {
        let source = "Open question: should we consider migrating to Postgres?";
        let evaluation = evaluate_candidate(
            "Consider Postgres",
            "Should we consider migrating to Postgres for discussion?",
            &[source],
            &[source.to_string()],
            &[],
        );
        assert!(!evaluation.qualified, "evaluation: {evaluation:?}");
        assert!(evaluation
            .reason_codes
            .contains(&DecisionLikenessReasonCode::SpeculativeLanguagePenalty));
    }
}
