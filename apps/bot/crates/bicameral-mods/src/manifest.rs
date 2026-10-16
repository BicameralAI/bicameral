//! Mod manifest types.

use serde::{Deserialize, Serialize};

/// A declarative mod manifest.
///
/// Manifests define trigger conditions and actions that the runtime evaluates.
/// They cannot execute arbitrary code — only emit typed governance objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    /// Unique mod identifier.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Schema version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Who authored this mod.
    pub author: Option<String>,
    /// Trigger conditions.
    pub triggers: Vec<ModTrigger>,
    /// Actions to take when triggered.
    pub actions: Vec<ModAction>,
    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A trigger condition for a mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModTrigger {
    /// What event type triggers this mod.
    pub event_type: TriggerEvent,
    /// Optional filter conditions (field path → expected value).
    #[serde(default)]
    pub filters: Vec<TriggerFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerFilter {
    pub field: String,
    pub operator: FilterOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Equals,
    Contains,
    Matches,
    GreaterThan,
    LessThan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEvent {
    CandidateIngested,
    EvidenceAttached,
    ReviewSubmitted,
    GovernanceEvaluated,
}

/// An action a mod may take when triggered.
///
/// Mods may only emit candidates, evidence, warnings, or route to reviewers.
/// They may never directly create canonical decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModAction {
    pub action_type: ModActionType,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModActionType {
    /// Emit a warning advisory.
    EmitWarning,
    /// Route the candidate to specific reviewers.
    RouteToReviewer,
    /// Tag the candidate with additional metadata.
    AddTag,
    /// Elevate the required governance level.
    ElevateGovernance,
    /// Emit additional source evidence.
    EmitEvidence,
}

fn default_version() -> u32 {
    1
}
