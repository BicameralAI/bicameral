//! Deterministic verdict mapping for the advisory SMR feedback experiment.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Conclusive result of one bounded satisfiability check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundedCheckOutcome {
    Possible,
    Impossible,
    Unavailable,
    Unjustified,
}

impl BoundedCheckOutcome {
    const fn is_conclusive(self) -> bool {
        matches!(self, Self::Possible | Self::Impossible)
    }
}

/// Advisory verdict for one independently checkable assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackVerdict {
    Entailed,
    Refuted,
    Underdetermined,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("a semantic verdict requires conclusive checks for both the assertion and its negation")]
pub struct InconclusiveVerdictChecks {
    pub assertion: BoundedCheckOutcome,
    pub negated_assertion: BoundedCheckOutcome,
}

/// Map the bounded checks for `T + p` and `T + not-p` to one formal verdict.
pub fn map_feedback_verdict(
    assertion: BoundedCheckOutcome,
    negated_assertion: BoundedCheckOutcome,
) -> Result<FeedbackVerdict, InconclusiveVerdictChecks> {
    if !assertion.is_conclusive() || !negated_assertion.is_conclusive() {
        return Err(InconclusiveVerdictChecks {
            assertion,
            negated_assertion,
        });
    }

    match (assertion, negated_assertion) {
        (BoundedCheckOutcome::Possible, BoundedCheckOutcome::Impossible) => {
            Ok(FeedbackVerdict::Entailed)
        }
        (BoundedCheckOutcome::Impossible, BoundedCheckOutcome::Possible) => {
            Ok(FeedbackVerdict::Refuted)
        }
        (BoundedCheckOutcome::Possible, BoundedCheckOutcome::Possible) => {
            Ok(FeedbackVerdict::Underdetermined)
        }
        (BoundedCheckOutcome::Impossible, BoundedCheckOutcome::Impossible) => {
            Ok(FeedbackVerdict::Conflict)
        }
        _ => unreachable!("inconclusive checks returned before verdict mapping"),
    }
}
