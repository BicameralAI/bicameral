//! Phase 0 conformance for final-answer claim verification (issue #56,
//! ARCH-0006 proposed).
//!
//! Correct retrieval does not make a correct answer. This suite pins a
//! synthetic gold corpus of adversarial cases under
//! `protocol/claim-verification/fixtures/` and scores predictors on four
//! separate axes: retrieval, authority, groundedness and answer accuracy.
//!
//! Test-only by design: the verdict vocabulary below is not a protocol type,
//! and nothing here changes `RecallPacket` or the recall answer path. The
//! verifier joins as a new predictor once ARCH-0006 is accepted.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const CORPUS_FILE: &str = "claim-verification-corpus.json";

/// Rows of the #56 Checks table that need a fixture, plus the control.
const REQUIRED_ROWS: &[&str] = &[
    "Control",
    "Negation",
    "Superseded",
    "Paraphrase",
    "Missing evidence",
    "Traceable",
];

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Supported,
    Contradicted,
    Stale,
    NotSupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Approval {
    Approved,
    Superseded,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    corpus_version: String,
    status: String,
    issue: String,
    governing_record: String,
    #[serde(rename = "_comment")]
    _comment: String,
    product_id: String,
    sources: Vec<Source>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    source_id: String,
    snapshot_id: String,
    approval: Approval,
    superseded_by: Option<String>,
    locator: String,
    excerpt: String,
    excerpt_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    case_id: String,
    row: String,
    query: String,
    relevant_sources: Vec<String>,
    authoritative_source: Option<String>,
    claims: Vec<Claim>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    claim_id: String,
    text: String,
    cites: Vec<String>,
    verdict: Verdict,
}

fn load_corpus() -> Corpus {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/claim-verification/fixtures")
        .join(CORPUS_FILE);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{CORPUS_FILE}: parse failed: {e}"))
}

fn sha256_label(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

// ---------------------------------------------------------------------------
// Predictions and scoring
// ---------------------------------------------------------------------------

/// What a system under test reports for one case.
#[derive(Debug, Clone)]
struct Prediction {
    case_id: String,
    retrieved: BTreeSet<String>,
    selected_authority: Option<String>,
    verdicts: HashMap<String, Verdict>,
    /// Claims the final answer presents as canonical. Empty means abstain.
    presented: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Score {
    correct: usize,
    total: usize,
}

impl Score {
    const fn of(correct: usize, total: usize) -> Self {
        Self { correct, total }
    }
}

impl std::fmt::Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.correct, self.total)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Report {
    retrieval: Score,
    authority: Score,
    groundedness: Score,
    answer_accuracy: Score,
}

/// Score predictions against gold. A case or claim with no prediction counts
/// as wrong, never as skipped.
fn score(corpus: &Corpus, predictions: &[Prediction]) -> Report {
    let by_case: HashMap<&str, &Prediction> = predictions
        .iter()
        .map(|p| (p.case_id.as_str(), p))
        .collect();
    let cases = corpus.cases.len();
    let claims = corpus.cases.iter().map(|c| c.claims.len()).sum();
    let mut report = Report {
        retrieval: Score::of(0, cases),
        authority: Score::of(0, cases),
        groundedness: Score::of(0, claims),
        answer_accuracy: Score::of(0, cases),
    };

    for case in &corpus.cases {
        let Some(pred) = by_case.get(case.case_id.as_str()) else {
            continue;
        };
        if case
            .relevant_sources
            .iter()
            .all(|s| pred.retrieved.contains(s))
        {
            report.retrieval.correct += 1;
        }
        if pred.selected_authority == case.authoritative_source {
            report.authority.correct += 1;
        }
        for claim in &case.claims {
            if pred.verdicts.get(&claim.claim_id) == Some(&claim.verdict) {
                report.groundedness.correct += 1;
            }
        }
        let gold_presented: BTreeSet<String> = case
            .claims
            .iter()
            .filter(|c| c.verdict == Verdict::Supported)
            .map(|c| c.claim_id.clone())
            .collect();
        if pred.presented == gold_presented {
            report.answer_accuracy.correct += 1;
        }
    }
    report
}

/// Predictor that returns the gold answers. Checks the scorer itself.
fn gold_predictor(corpus: &Corpus) -> Vec<Prediction> {
    corpus
        .cases
        .iter()
        .map(|case| Prediction {
            case_id: case.case_id.clone(),
            retrieved: case.relevant_sources.iter().cloned().collect(),
            selected_authority: case.authoritative_source.clone(),
            verdicts: case
                .claims
                .iter()
                .map(|c| (c.claim_id.clone(), c.verdict))
                .collect(),
            presented: case
                .claims
                .iter()
                .filter(|c| c.verdict == Verdict::Supported)
                .map(|c| c.claim_id.clone())
                .collect(),
        })
        .collect()
}

/// Naive baseline that ARCH-0006 rejects: retrieval is perfect (taken from
/// gold, to isolate the later stages), the cited source is trusted as the
/// authority, and any claim citing a retrieved source counts as supported.
fn citation_trust_baseline(corpus: &Corpus) -> Vec<Prediction> {
    corpus
        .cases
        .iter()
        .map(|case| {
            let retrieved: BTreeSet<String> = case.relevant_sources.iter().cloned().collect();
            let verdicts: HashMap<String, Verdict> = case
                .claims
                .iter()
                .map(|c| {
                    let cited = c.cites.iter().any(|s| retrieved.contains(s));
                    let verdict = if cited {
                        Verdict::Supported
                    } else {
                        Verdict::NotSupported
                    };
                    (c.claim_id.clone(), verdict)
                })
                .collect();
            let presented = verdicts
                .iter()
                .filter(|(_, v)| **v == Verdict::Supported)
                .map(|(id, _)| id.clone())
                .collect();
            Prediction {
                case_id: case.case_id.clone(),
                selected_authority: case
                    .claims
                    .iter()
                    .flat_map(|c| c.cites.first())
                    .next()
                    .cloned(),
                retrieved,
                verdicts,
                presented,
            }
        })
        .collect()
}

fn print_report(name: &str, report: &Report) {
    println!(
        "claim-verification {name}: retrieval={} authority={} groundedness={} answer_accuracy={}",
        report.retrieval, report.authority, report.groundedness, report.answer_accuracy
    );
}

// ---------------------------------------------------------------------------
// Corpus integrity
// ---------------------------------------------------------------------------

#[test]
fn corpus_is_synthetic_and_tracks_issue_56() {
    let corpus = load_corpus();
    assert_eq!(corpus.corpus_version, "claim-verification-v0");
    assert_eq!(corpus.status, "synthetic_phase0");
    assert_eq!(corpus.issue, "BicameralAI/bicameral#56");
    assert!(corpus.governing_record.starts_with("ARCH-0006"));
    assert!(corpus.product_id.starts_with("synthetic-"));
    for source in &corpus.sources {
        assert!(
            source.locator.starts_with("synthetic://"),
            "{}: locator must be synthetic",
            source.source_id
        );
        assert!(!source.snapshot_id.is_empty());
    }
}

#[test]
fn corpus_covers_every_issue_row() {
    let corpus = load_corpus();
    let rows: HashSet<&str> = corpus.cases.iter().map(|c| c.row.as_str()).collect();
    for row in REQUIRED_ROWS {
        assert!(rows.contains(row), "no fixture for #56 row `{row}`");
    }
}

#[test]
fn identifiers_are_unique_and_references_resolve() {
    let corpus = load_corpus();
    let mut source_ids = HashSet::new();
    for source in &corpus.sources {
        assert!(
            source_ids.insert(source.source_id.as_str()),
            "duplicate {}",
            source.source_id
        );
    }
    let mut case_ids = HashSet::new();
    let mut claim_ids = HashSet::new();
    for case in &corpus.cases {
        assert!(
            case_ids.insert(case.case_id.as_str()),
            "duplicate {}",
            case.case_id
        );
        assert!(!case.query.is_empty(), "{}: empty query", case.case_id);
        let refs = case
            .relevant_sources
            .iter()
            .chain(&case.authoritative_source)
            .chain(case.claims.iter().flat_map(|c| &c.cites));
        for id in refs {
            assert!(
                source_ids.contains(id.as_str()),
                "{}: unknown source {id}",
                case.case_id
            );
        }
        for claim in &case.claims {
            assert!(
                claim_ids.insert(claim.claim_id.as_str()),
                "duplicate {}",
                claim.claim_id
            );
            assert!(!claim.text.is_empty(), "{}: empty text", claim.claim_id);
        }
    }
    for source in &corpus.sources {
        if let Some(next) = &source.superseded_by {
            assert!(
                source_ids.contains(next.as_str()),
                "{}: unknown successor",
                source.source_id
            );
        }
    }
}

#[test]
fn every_excerpt_matches_its_digest() {
    let corpus = load_corpus();
    for source in &corpus.sources {
        assert_eq!(
            sha256_label(&source.excerpt),
            source.excerpt_sha256,
            "{}: excerpt digest mismatch",
            source.source_id
        );
    }
}

/// Gold must be self-consistent: a supported claim is traceable to a current
/// approved source, a stale claim cites a superseded one, and the authority
/// is never a superseded source.
#[test]
fn gold_verdicts_agree_with_source_state() {
    let corpus = load_corpus();
    let sources: HashMap<&str, &Source> = corpus
        .sources
        .iter()
        .map(|s| (s.source_id.as_str(), s))
        .collect();
    for source in &corpus.sources {
        assert_eq!(
            source.approval == Approval::Superseded,
            source.superseded_by.is_some(),
            "{}: approval and superseded_by disagree",
            source.source_id
        );
    }
    for case in &corpus.cases {
        if let Some(authority) = &case.authoritative_source {
            assert_eq!(
                sources[authority.as_str()].approval,
                Approval::Approved,
                "{}",
                case.case_id
            );
        }
        for claim in &case.claims {
            let cited: Vec<&Source> = claim.cites.iter().map(|id| sources[id.as_str()]).collect();
            match claim.verdict {
                Verdict::Supported => {
                    assert!(
                        !cited.is_empty(),
                        "{}: supported without a source",
                        claim.claim_id
                    );
                    for source in &cited {
                        assert_eq!(source.approval, Approval::Approved, "{}", claim.claim_id);
                        assert!(!source.locator.is_empty() && !source.excerpt.is_empty());
                    }
                }
                Verdict::Stale => assert!(
                    cited.iter().any(|s| s.approval == Approval::Superseded),
                    "{}: stale without a superseded source",
                    claim.claim_id
                ),
                Verdict::Contradicted | Verdict::NotSupported => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Metrics harness
// ---------------------------------------------------------------------------

#[test]
fn gold_predictor_scores_full_marks() {
    let corpus = load_corpus();
    let report = score(&corpus, &gold_predictor(&corpus));
    print_report("gold", &report);
    assert_eq!(
        report,
        Report {
            retrieval: Score::of(6, 6),
            authority: Score::of(6, 6),
            groundedness: Score::of(6, 6),
            answer_accuracy: Score::of(6, 6),
        }
    );
}

/// Expected values are worked by hand, case by case:
///
/// | Case        | Authority (cited vs gold) | Verdict (baseline vs gold)  | Answer |
/// |-------------|---------------------------|-----------------------------|--------|
/// | CONTROL     | ok                        | supported = supported       | ok     |
/// | NEGATION    | ok                        | supported != contradicted   | wrong  |
/// | SUPERSEDED  | retention.v1 != v2        | supported != stale          | wrong  |
/// | PARAPHRASE  | ok                        | supported != not_supported  | wrong  |
/// | MISSING     | none = none               | not_supported = gold        | ok     |
/// | TRACEABLE   | ok                        | supported = supported       | ok     |
#[test]
fn citation_trust_baseline_retrieves_everything_but_is_half_grounded() {
    let corpus = load_corpus();
    let report = score(&corpus, &citation_trust_baseline(&corpus));
    print_report("citation_trust_baseline", &report);
    assert_eq!(
        report,
        Report {
            retrieval: Score::of(6, 6),
            authority: Score::of(5, 6),
            groundedness: Score::of(3, 6),
            answer_accuracy: Score::of(3, 6),
        }
    );
}

#[test]
fn missing_prediction_counts_as_wrong_not_skipped() {
    let corpus = load_corpus();
    let mut predictions = gold_predictor(&corpus);
    predictions.retain(|p| p.case_id != "CV-NEGATION-01");
    let report = score(&corpus, &predictions);
    assert_eq!(report.retrieval, Score::of(5, 6));
    assert_eq!(report.authority, Score::of(5, 6));
    assert_eq!(report.groundedness, Score::of(5, 6));
    assert_eq!(report.answer_accuracy, Score::of(5, 6));
}

/// Presenting a contradicted claim fails answer accuracy even when its
/// verdict is right: the four scores stay independent.
#[test]
fn presenting_a_flagged_claim_fails_only_answer_accuracy() {
    let corpus = load_corpus();
    let mut predictions = gold_predictor(&corpus);
    let negation = predictions
        .iter_mut()
        .find(|p| p.case_id == "CV-NEGATION-01")
        .expect("negation case");
    negation.presented.insert("CV-NEGATION-01.c1".to_string());
    let report = score(&corpus, &predictions);
    assert_eq!(report.retrieval, Score::of(6, 6));
    assert_eq!(report.authority, Score::of(6, 6));
    assert_eq!(report.groundedness, Score::of(6, 6));
    assert_eq!(report.answer_accuracy, Score::of(5, 6));
}
