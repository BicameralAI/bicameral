//! Sensitive-data detection catalog for ingest content.
//!
//! Rust port of `bicameral-mcp/handlers/sensitive_patterns.py`.
//! Three classes with distinct detection semantics:
//!
//! - `secret`: pure regex (cloud-provider key prefixes, JWT shape, PEM blocks).
//! - `phi`: regex with required label-adjacency (e.g. `MRN:` + digits).
//! - `pan`: regex + Luhn checksum + context-label exclusion.

use once_cell::sync::Lazy;
use regex::Regex;

/// Catalog version — bump on pattern change.
pub const CATALOG_VERSION: &str = "v1";

const EXCERPT_MAX: usize = 64;
const PAN_CONTEXT_LOOKBACK: usize = 30;

/// A single match of a sensitive-data pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveHit {
    pub cls: &'static str,
    pub pattern_id: usize,
    pub match_excerpt: String,
}

// ── Secret patterns ──────────────────────────────────────────────

static SECRET_PATTERNS: Lazy<Vec<(&str, Regex)>> = Lazy::new(|| {
    vec![
        (
            "aws-access-key",
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        ),
        (
            "github-pat",
            Regex::new(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36}\b").unwrap(),
        ),
        (
            "azure-storage-key",
            Regex::new(
                r"DefaultEndpointsProtocol=https;AccountName=[a-z0-9]+;AccountKey=[A-Za-z0-9+/]{40,};"
            )
            .unwrap(),
        ),
        (
            "private-key-pem",
            Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
        ),
        (
            "jwt",
            Regex::new(r"\beyJ[A-Za-z0-9_\-]+\.eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\b").unwrap(),
        ),
    ]
});

// ── PHI patterns (require label-adjacency) ───────────────────────

static PHI_PATTERNS: Lazy<Vec<(&str, Regex)>> = Lazy::new(|| {
    vec![
        (
            "mrn-with-label",
            Regex::new(
                r"(?i)\b(?:mrn|medical\s+record(?:\s+number)?|patient\s+id)\s*[:=]\s*\d{5,12}\b",
            )
            .unwrap(),
        ),
        (
            "phi-field-label",
            Regex::new(
                r"(?i)\b(?:patient_(?:id|name|email)|date_of_birth|dob|ssn|social_security(?:_number)?)\s*[:=]",
            )
            .unwrap(),
        ),
    ]
});

// ── PAN patterns (Luhn + context exclusion) ──────────────────────

static PAN_CANDIDATE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{13,19}\b").unwrap());

static PAN_CONTEXT_LABEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:order_id|order|ref|ref_id|transaction_id|txn_id|id|user_id|account_id|invoice_id|receipt)\s*[:=]\s*$",
    )
    .unwrap()
});

/// Luhn checksum validation.
fn luhn_valid(digits: &str) -> bool {
    let parity = digits.len() % 2;
    let mut total: u32 = 0;
    for (i, ch) in digits.chars().enumerate() {
        let mut n = ch.to_digit(10).unwrap_or(0);
        if i % 2 == parity {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        total += n;
    }
    total % 10 == 0
}

/// Check if the digit sequence is preceded by an ID-class label.
fn is_id_preceded(content: &str, start: usize) -> bool {
    let lookback_start = start.saturating_sub(PAN_CONTEXT_LOOKBACK);
    let preceding = &content[lookback_start..start];
    PAN_CONTEXT_LABEL_RE.is_match(preceding)
}

/// Redact secret-class excerpts (keep prefix/suffix, mask body).
fn redact_excerpt(cls: &str, raw: &str) -> String {
    let truncated: String = raw.chars().take(EXCERPT_MAX).collect();
    if cls == "secret" && truncated.len() > 8 {
        let chars: Vec<char> = truncated.chars().collect();
        let prefix: String = chars[..4].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        let mid = "*".repeat(chars.len() - 8);
        format!("{prefix}{mid}{suffix}")
    } else {
        truncated
    }
}

/// Run all pattern classes against content; return all hits.
///
/// Empty vec = clean. This is the HARD gate — any hit blocks ingest.
pub fn detect_sensitive(content: &str) -> Vec<SensitiveHit> {
    let mut hits = Vec::new();

    for (idx, (_label, pattern)) in SECRET_PATTERNS.iter().enumerate() {
        for m in pattern.find_iter(content) {
            hits.push(SensitiveHit {
                cls: "secret",
                pattern_id: idx,
                match_excerpt: redact_excerpt("secret", m.as_str()),
            });
        }
    }

    for (idx, (_label, pattern)) in PHI_PATTERNS.iter().enumerate() {
        for m in pattern.find_iter(content) {
            hits.push(SensitiveHit {
                cls: "phi",
                pattern_id: idx,
                match_excerpt: redact_excerpt("phi", m.as_str()),
            });
        }
    }

    for m in PAN_CANDIDATE_RE.find_iter(content) {
        let digits = m.as_str();
        if is_id_preceded(content, m.start()) {
            continue;
        }
        if !luhn_valid(digits) {
            continue;
        }
        hits.push(SensitiveHit {
            cls: "pan",
            pattern_id: 0,
            match_excerpt: redact_excerpt("pan", digits),
        });
    }

    hits
}

/// Redact sensitive spans (secret / PHI / PAN) from free text before it crosses
/// a network origin.
///
/// Per #698 item 5 (T4), hosted-dashboard titles/summaries are rendered only
/// after passing through this redactor (ADR-0025 keeps the hosted tier a
/// renderer that never holds Confidential data). Non-sensitive text passes
/// through unchanged; each matched span is replaced with a typed placeholder
/// (`[redacted:<class>]`).
pub fn redact_sensitive(content: &str) -> String {
    // Collect (start, end) byte spans to redact, tagged with their class.
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();

    for (_label, pattern) in SECRET_PATTERNS.iter() {
        for m in pattern.find_iter(content) {
            spans.push((m.start(), m.end(), "secret"));
        }
    }
    for (_label, pattern) in PHI_PATTERNS.iter() {
        for m in pattern.find_iter(content) {
            spans.push((m.start(), m.end(), "phi"));
        }
    }
    for m in PAN_CANDIDATE_RE.find_iter(content) {
        if is_id_preceded(content, m.start()) {
            continue;
        }
        if !luhn_valid(m.as_str()) {
            continue;
        }
        spans.push((m.start(), m.end(), "pan"));
    }

    if spans.is_empty() {
        return content.to_string();
    }

    spans.sort_by_key(|(start, _, _)| *start);

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (start, end, cls) in spans {
        // Skip spans that overlap an already-redacted region.
        if start < cursor {
            continue;
        }
        out.push_str(&content[cursor..start]);
        out.push_str("[redacted:");
        out.push_str(cls);
        out.push(']');
        cursor = end;
    }
    out.push_str(&content[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_key() {
        let content = "key is AKIAIOSFODNN7EXAMPLE";
        let hits = detect_sensitive(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cls, "secret");
        assert!(hits[0].match_excerpt.contains("AKIA"));
    }

    #[test]
    fn detects_github_pat() {
        let content = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let hits = detect_sensitive(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cls, "secret");
    }

    #[test]
    fn detects_private_key_pem() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\ndata\n-----END RSA PRIVATE KEY-----";
        let hits = detect_sensitive(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cls, "secret");
    }

    #[test]
    fn detects_mrn_with_label() {
        let content = "Patient record MRN: 1234567890";
        let hits = detect_sensitive(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cls, "phi");
    }

    #[test]
    fn detects_pan_with_luhn() {
        // Valid Luhn: 4111111111111111 (Visa test number)
        let content = "card number 4111111111111111 here";
        let hits = detect_sensitive(content);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cls, "pan");
    }

    #[test]
    fn skips_pan_with_id_label() {
        let content = "order_id: 4111111111111111";
        let hits = detect_sensitive(content);
        assert!(hits.is_empty());
    }

    #[test]
    fn skips_invalid_luhn() {
        let content = "number 1234567890123 here";
        let hits = detect_sensitive(content);
        assert!(hits.is_empty());
    }

    #[test]
    fn clean_content_returns_empty() {
        let content = "This is normal governance content about architecture decisions.";
        let hits = detect_sensitive(content);
        assert!(hits.is_empty());
    }

    #[test]
    fn redact_sensitive_passes_clean_text_through() {
        let content = "Adopt the gateway authority boundary for review routes.";
        assert_eq!(redact_sensitive(content), content);
    }

    #[test]
    fn redact_sensitive_masks_secret_span() {
        let content = "title with AKIAIOSFODNN7EXAMPLE embedded";
        let out = redact_sensitive(content);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains("[redacted:secret]"));
        assert!(out.starts_with("title with "));
        assert!(out.ends_with(" embedded"));
    }

    #[test]
    fn redact_sensitive_masks_multiple_spans() {
        let content = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij and AKIAIOSFODNN7EXAMPLE";
        let out = redact_sensitive(content);
        assert!(!out.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij"));
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert_eq!(out.matches("[redacted:secret]").count(), 2);
    }
}
