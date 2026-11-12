//! Frozen spec-registry payloads (ARCH-0004, monorepo #40).
//!
//! Decoding a payload grants no actor authority and performs no state transition.
//! These are separate from the legacy Decision-based event-store payloads.

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSource {
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub person_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UntrackSource {
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub person_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectSnapshot {
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub person_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersedeSnapshot {
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub successor_snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub person_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveConflict {
    #[serde(deserialize_with = "non_empty_string")]
    pub person_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
    #[serde(deserialize_with = "resolution_members")]
    pub members: Vec<ResolutionMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionMember {
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    pub state: ResolutionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionState {
    Approved,
    Rejected,
    Superseded,
}

fn resolution_members<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ResolutionMember>, D::Error> {
    let members = Vec::<ResolutionMember>::deserialize(deserializer)?;
    let approved = members
        .iter()
        .filter(|member| member.state == ResolutionState::Approved)
        .count();
    let superseded = members
        .iter()
        .any(|member| member.state == ResolutionState::Superseded);
    if members.len() < 2 || approved > 1 || (superseded && approved != 1) {
        return Err(serde::de::Error::custom(
            "resolution needs at least two members, at most one approved, and an approved successor when superseding",
        ));
    }
    Ok(members)
}

fn non_empty_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom("expected a non-empty string"));
    }
    Ok(value)
}
