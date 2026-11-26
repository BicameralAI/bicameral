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

// GovernedDomain, CheckRecord, Signoff and ApprovePolicy (ARCH-0003, ARCH-0004).
// Payload decoding only, as above: no lease, model call, check or approval runs here.

/// Encodes a registry payload for writing and proves the result decodes.
///
/// Cross-field rules also run whenever a checked payload is serialized, but
/// field rules (non-empty strings, RFC 3339 times, permission cells) run only at
/// decode. A writer uses this so that it cannot store a payload a reader refuses.
pub fn encode_checked<T>(payload: &T) -> Result<serde_json::Value, serde_json::Error>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let value = serde_json::to_value(payload)?;
    T::deserialize(&value)?;
    Ok(value)
}

/// Decodes through the derived field rules, then applies the cross-field rules in `check`.
macro_rules! checked_payload {
    ($name:ident) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = $name::deserialize(deserializer)?;
                value.check().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.check().map_err(serde::ser::Error::custom)?;
                $name::serialize(self, serializer)
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractRoute {
    SystemOne,
    OpenaiCompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainClaim {
    #[serde(deserialize_with = "non_empty_string")]
    pub claim_id: String,
    pub revision: u64,
    pub context_statement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainAtom {
    #[serde(deserialize_with = "non_empty_string")]
    pub atom_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub atom_kind: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub semantic_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainClaimAssertion {
    #[serde(deserialize_with = "non_empty_string")]
    pub assertion_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub claim_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub atom_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub polarity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRelation {
    #[serde(deserialize_with = "non_empty_string")]
    pub relation_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub relation_kind: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub from_atom_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub to_atom_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_strings"
    )]
    pub supporting_claim_ids: Option<Vec<String>>,
}

/// A spec `Source`: a file or a tracker item, looked up by `uri`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecSource {
    #[serde(deserialize_with = "non_empty_string")]
    pub uri: String,
    pub source_type: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub label: Option<String>,
    pub source_kind: SpecSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSourceKind {
    File,
    Tracker,
}

/// Registry form of one immutable capture. `source_uri` is the `Source` uri, a
/// back-pointer, not the address of the captured bytes. Decoding does not derive or
/// verify `snapshot_addr` or `content_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    pub snapshot_ref: String,
    #[serde(deserialize_with = "rfc3339_string")]
    pub captured_at: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub content: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub content_hash: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub capture_method: Option<crate::source::CaptureMethod>,
    #[serde(deserialize_with = "non_empty_string")]
    pub source_uri: String,
}

/// Finished failed extract. An operational receipt, not an authorization state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractError {
    #[serde(deserialize_with = "non_empty_string")]
    pub reason: String,
    #[serde(deserialize_with = "rfc3339_string")]
    pub failed_at: String,
}

/// One claim set for one `SourceSnapshot`. Empty at capture. A completed extract carries
/// `source_digest`, `route`, `model_id`, `prompt_id` and `prompt_digest` together, also
/// for an empty claim set. A failed extract carries only `extract_error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct GovernedDomain {
    #[serde(deserialize_with = "non_empty_string")]
    pub product_id: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_string"
    )]
    pub source_digest: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub route: Option<ExtractRoute>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_string"
    )]
    pub model_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_string"
    )]
    pub prompt_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_string"
    )]
    pub prompt_digest: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub extract_error: Option<ExtractError>,
    pub claims: Vec<DomainClaim>,
    pub atoms: Vec<DomainAtom>,
    pub claim_assertions: Vec<DomainClaimAssertion>,
    pub relations: Vec<DomainRelation>,
}
checked_payload!(GovernedDomain);

impl GovernedDomain {
    /// The domain a capture creates: no claim set, no completion, no extract error.
    pub fn captured(product_id: String, snapshot_addr: String) -> Self {
        Self {
            product_id,
            snapshot_addr,
            source_digest: None,
            route: None,
            model_id: None,
            prompt_id: None,
            prompt_digest: None,
            extract_error: None,
            claims: Vec::new(),
            atoms: Vec::new(),
            claim_assertions: Vec::new(),
            relations: Vec::new(),
        }
    }

    fn check(&self) -> Result<(), &'static str> {
        let completion = [
            self.source_digest.is_some(),
            self.route.is_some(),
            self.model_id.is_some(),
            self.prompt_id.is_some(),
            self.prompt_digest.is_some(),
        ];
        let has_claim_set = !(self.claims.is_empty()
            && self.atoms.is_empty()
            && self.claim_assertions.is_empty()
            && self.relations.is_empty());
        let any_completion = completion.contains(&true);
        if (any_completion || has_claim_set) && completion.contains(&false) {
            return Err("a completed extract writes source_digest, route, model_id, prompt_id and prompt_digest together");
        }
        if self.extract_error.is_some() && (any_completion || has_claim_set) {
            return Err("extract_error excludes a completed extract and its claim set");
        }
        Ok(())
    }

    /// ARCH-0003 digest of this claim set: SHA-256 hex of the compact JSON object holding
    /// exactly `atoms`, `claim_assertions`, `claims` and `relations`, keys sorted, arrays in
    /// stored order. Computed for any domain; only a completed extract stores it.
    ///
    /// A captured or failed domain returns the empty-set digest, the same value as a
    /// completed zero-claim extract. Code that decides whether a domain may be checked or
    /// signed off must read the stored `source_digest` (`None` until extract), not this.
    pub fn claim_set_digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let claim_set = serde_json::json!({
            "atoms": self.atoms,
            "claim_assertions": self.claim_assertions,
            "claims": self.claims,
            "relations": self.relations,
        });
        let mut canonical = String::new();
        write_sorted_json(&claim_set, &mut canonical);
        Sha256::digest(canonical.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

/// Compact JSON with object keys sorted, independent of serde_json's map ordering feature.
fn write_sorted_json(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::from(key.as_str()).to_string());
                out.push(':');
                write_sorted_json(item, out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_sorted_json(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// One pairwise check for one unordered pair of `source_digest` values, stored in
/// lexicographic order. A missing or failed check is not a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct CheckRecord {
    #[serde(deserialize_with = "non_empty_string")]
    pub domain_digest: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub other_domain_digest: String,
    pub conflict: bool,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
}
checked_payload!(CheckRecord);

impl CheckRecord {
    fn check(&self) -> Result<(), &'static str> {
        if self.domain_digest >= self.other_domain_digest {
            return Err(
                "domain_digest must be the lexicographically lesser of two different digests",
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignoffActor {
    Manual,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductRole {
    Owner,
    Member,
}

/// The approved state of one `GovernedDomain`. Decoding does not establish that the
/// submitted check records are the ones the approval requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Signoff {
    #[serde(deserialize_with = "non_empty_string")]
    pub snapshot_addr: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub domain_digest: String,
    #[serde(deserialize_with = "non_empty_string")]
    pub daemon_id: String,
    pub actor: SignoffActor,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub role: Option<ProductRole>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_non_empty_string"
    )]
    pub person_id: Option<String>,
    pub check_records: Vec<CheckRecord>,
}
checked_payload!(Signoff);

impl Signoff {
    fn check(&self) -> Result<(), &'static str> {
        let identified = (self.role.is_some(), self.person_id.is_some());
        match (self.actor, identified) {
            (SignoffActor::Manual, (true, true)) | (SignoffActor::Policy, (false, false)) => Ok(()),
            (SignoffActor::Manual, _) => Err("a manual signoff records role and person_id"),
            (SignoffActor::Policy, _) => Err("a policy signoff records neither role nor person_id"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovePolicy {
    pub auto_approve: bool,
    #[serde(deserialize_with = "approve_permission_cells")]
    pub approve_permission: Vec<ApprovePermissionCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovePermissionCell {
    pub role: ProductRole,
    pub in_conflict: bool,
    pub allow: bool,
}

fn approve_permission_cells<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ApprovePermissionCell>, D::Error> {
    let cells = Vec::<ApprovePermissionCell>::deserialize(deserializer)?;
    let covered = |role, in_conflict| {
        cells
            .iter()
            .filter(|cell| cell.role == role && cell.in_conflict == in_conflict)
            .count()
            == 1
    };
    let complete = cells.len() == 4
        && [ProductRole::Owner, ProductRole::Member]
            .into_iter()
            .all(|role| covered(role, false) && covered(role, true));
    if !complete || cells.iter().any(|cell| cell.in_conflict && cell.allow) {
        return Err(serde::de::Error::custom(
            "approve_permission needs exactly one cell per (role, in_conflict), and no cell may allow approval in conflict",
        ));
    }
    Ok(cells)
}

/// An optional field that is absent or a value; an explicit null is not a value.
fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn optional_non_empty_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    non_empty_string(deserializer).map(Some)
}

fn optional_non_empty_strings<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error> {
    let values = Vec::<String>::deserialize(deserializer)?;
    if values.iter().any(String::is_empty) {
        return Err(serde::de::Error::custom("expected non-empty strings"));
    }
    Ok(Some(values))
}

fn rfc3339_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    chrono::DateTime::parse_from_rfc3339(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}
