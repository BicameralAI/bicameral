//! `product.continue_setup` — local setup-continuation contract (issue #769).
//!
//! Continues an already-successful canonical Product Member auto-join (bot#767)
//! through the *local* setup lifecycle: provider access, event-store descriptor
//! validation, a single durable local [`ProjectRegistration`], adapter opening,
//! and canonical Product replay. It implements the authority/state model in
//! `docs/specs/explicit-project-registration-hosted-local-discovery.md` and the
//! onboarding contract `docs/specs/domain-product-onboarding-contract.md`.
//!
//! Authority boundary (fail-closed):
//!
//! - This slice never creates, changes, or revokes canonical Product Member
//!   membership — that is owned by `product.join` (bot#767). Membership is an
//!   input here, never an output. A failed setup leaves membership untouched
//!   and never asks the owner to reapprove a join.
//! - Project identity comes from explicit registration, not ambient filesystem
//!   discovery. The daemon writes at most one [`ProjectRegistration`] per
//!   `(product_id, cloud_account_id)`; a repeated completion updates it
//!   idempotently rather than duplicating it.
//! - Provider authorization, descriptor validation, adapter opening, and replay
//!   are daemon-local. The provider-access and descriptor-validation facts are
//!   daemon-derived (published under the metadata keys below), never
//!   SPA-asserted.
//! - Neither the SPA nor Cloud ever receives a provider secret, raw credential,
//!   unredacted event-store descriptor, or local filesystem path. Every type in
//!   this module carries only non-secret metadata and opaque references.
//! - `LocalWorkspaceBinding` is orthogonal and optional; it is never a
//!   setup/join-completion condition and is not referenced here.
//!
//! This module is intentionally IO-free: provider access, descriptor
//! validation, and replay are injected as typed facts so the pure decision
//! function [`derive_setup_decision`] is fully unit-testable. The concrete
//! adapter opening/replay lives in the runtime.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Daemon-set metadata keys (non-secret, daemon-derived facts)
// ---------------------------------------------------------------------------

/// Audit-metadata key under which the daemon publishes the resolved provider
/// access state for a continuation request. Set by the daemon from its local
/// provider-connection resolution, never by the SPA. Value is one of the
/// [`ProviderAccessState`] wire strings.
pub const PROVIDER_ACCESS_METADATA_KEY: &str = "setup_provider_access";

/// Audit-metadata key under which the daemon publishes the resolved event-store
/// descriptor validation state for a continuation request. Set by the daemon
/// from its local descriptor validation, never by the SPA. Value is one of the
/// [`DescriptorValidation`] wire strings.
pub const DESCRIPTOR_VALIDATION_METADATA_KEY: &str = "setup_descriptor_validation";

/// Daemon-derived audit-metadata key carrying the exact Google Drive folder id
/// that the daemon itself validated through the shipped resource-selection
/// flow (#1264). Operator-scoped `product.create` accepts a Google
/// `event_store_descriptor` only when its `folder_id` equals this value, so the
/// browser can transport a daemon-issued descriptor but never mint one.
pub const VALIDATED_EVENT_STORE_FOLDER_ID_METADATA_KEY: &str =
    "setup_validated_event_store_folder_id";

// ---------------------------------------------------------------------------
// Daemon-derived setup facts
// ---------------------------------------------------------------------------

/// Local provider-access state for the Product's required connection.
///
/// Daemon-derived and non-secret: it reports *whether* usable provider access
/// exists, never the credential itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccessState {
    /// No usable provider connection yet — the operator must connect one.
    Absent,
    /// A usable, authorized provider connection exists.
    Authorized,
    /// Provider access was attempted and denied (missing scope, revoked, etc.).
    Denied,
}

impl ProviderAccessState {
    /// Stable wire string (matches the serde `snake_case` rename).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Authorized => "authorized",
            Self::Denied => "denied",
        }
    }

    /// Parse a daemon-published metadata value; unknown/absent fails safe to
    /// [`ProviderAccessState::Absent`] (which yields `Continue setup`, never a
    /// false `Open`).
    pub fn from_metadata(value: Option<&str>) -> Self {
        match value {
            Some("authorized") => Self::Authorized,
            Some("denied") => Self::Denied,
            _ => Self::Absent,
        }
    }
}

/// Local event-store descriptor validation state.
///
/// Daemon-derived and non-secret: the raw descriptor is never surfaced; only
/// the typed validation outcome crosses any boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptorValidation {
    /// Descriptor validation has not been attempted yet.
    NotAttempted,
    /// A safe, usable event-store descriptor + connection metadata was resolved.
    Valid,
    /// The descriptor is structurally/semantically invalid.
    Invalid,
    /// The descriptor is well-formed but the substrate is unreachable.
    SubstrateUnreachable,
}

impl DescriptorValidation {
    /// Stable wire string (matches the serde `snake_case` rename).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAttempted => "not_attempted",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::SubstrateUnreachable => "unreachable_substrate",
        }
    }

    /// Parse a daemon-published metadata value; unknown/absent fails safe to
    /// [`DescriptorValidation::NotAttempted`].
    pub fn from_metadata(value: Option<&str>) -> Self {
        match value {
            Some("valid") => Self::Valid,
            Some("invalid") => Self::Invalid,
            Some("unreachable_substrate") => Self::SubstrateUnreachable,
            _ => Self::NotAttempted,
        }
    }
}

/// Outcome of opening the registered adapter and replaying canonical Product
/// state. Injected as a fact so the decision stays IO-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayOutcome {
    /// The adapter opened and canonical replay reconstructed Product state.
    Succeeded,
    /// The adapter could not open or replay failed.
    Failed,
}

// ---------------------------------------------------------------------------
// Non-secret connection + registration shapes
// ---------------------------------------------------------------------------

/// Provider a registration routes to. `None` is a valid substrate-only routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisteredProvider {
    Github,
    Google,
    None,
}

/// Event-store substrate a registration routes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisteredSubstrate {
    Git,
    DriveFolder,
}

/// Validated, non-secret metadata describing a usable provider/substrate
/// connection. Contains only display-safe labels and opaque references — never
/// a token, raw credential, unredacted descriptor, or local filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    pub provider: RegisteredProvider,
    pub substrate: RegisteredSubstrate,
    /// Safe, display-only account label (e.g. an org/handle), never a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    /// Opaque reference to the daemon-local event-store descriptor. Never the
    /// raw descriptor or a filesystem path.
    pub descriptor_ref: String,
}

/// Explicit, durable, operator-local mapping from a joined Product to its
/// validated provider/substrate routing.
///
/// This is the sole durable local fact this slice adds. It is orthogonal to
/// canonical membership (which lives in `ProductMemberAdded`) and to
/// `LocalWorkspaceBinding` (optional, independently addable). It excludes
/// provider tokens, raw descriptors, accepted Decision Ledger events, and local
/// filesystem paths. Matches the registration shape in the explicit-registration
/// spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRegistration {
    pub product_id: String,
    pub cloud_account_id: String,
    /// Opaque, Cloud-minted routing reference carried on the join ticket.
    pub registration_ref: String,
    pub connection: ConnectionMetadata,
    /// Active canonical Governance Policy Version at registration time.
    pub governance_policy_version: String,
    pub registered_at: DateTime<Utc>,
}

/// Legacy event payload retained for migration compatibility.
///
/// Current daemons persist [`ProjectRegistration`] in the operator registry,
/// never in a Product event ledger. Older ledgers may contain this payload;
/// startup migration imports it without writing a replacement event. It never
/// carries a provider token, raw descriptor, or filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRegistrationWrittenPayload {
    pub product_id: String,
    pub cloud_account_id: String,
    pub registration_ref: String,
    pub connection: ConnectionMetadata,
    pub governance_policy_version: String,
    pub written_at: DateTime<Utc>,
}

impl ProjectRegistrationWrittenPayload {
    /// Materialize the durable [`ProjectRegistration`] this payload represents.
    pub fn to_registration(&self) -> ProjectRegistration {
        ProjectRegistration {
            product_id: self.product_id.clone(),
            cloud_account_id: self.cloud_account_id.clone(),
            registration_ref: self.registration_ref.clone(),
            connection: self.connection.clone(),
            governance_policy_version: self.governance_policy_version.clone(),
            registered_at: self.written_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Readiness — the Open / Continue setup / Repair projection axis
// ---------------------------------------------------------------------------

/// The next user-facing setup step when a Product is `Continue setup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinueSetupStep {
    /// Provider access is not yet available; connect the required provider.
    ConnectProvider,
    /// Provider is connected; validate the event-store descriptor.
    ValidateDescriptor,
}

impl ContinueSetupStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConnectProvider => "connect_provider",
            Self::ValidateDescriptor => "validate_descriptor",
        }
    }

    /// Safe, human-readable label (no secrets/paths).
    pub fn label(self) -> &'static str {
        match self {
            Self::ConnectProvider => "Connect provider",
            Self::ValidateDescriptor => "Validate event store",
        }
    }
}

/// Typed, recoverable reason a Product is in `Repair`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupRepairReason {
    /// Provider access was denied (missing scope, revoked, admin approval, …).
    ProviderDenied,
    /// The event-store descriptor is invalid.
    InvalidDescriptor,
    /// The descriptor is well-formed but the substrate is unreachable.
    UnreachableSubstrate,
    /// A registration exists but opening/replaying the Product failed.
    ReplayFailed,
}

impl SetupRepairReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderDenied => "provider_denied",
            Self::InvalidDescriptor => "invalid_descriptor",
            Self::UnreachableSubstrate => "unreachable_substrate",
            Self::ReplayFailed => "replay_failed",
        }
    }

    /// Safe, non-secret human-readable message.
    pub fn message(self) -> &'static str {
        match self {
            Self::ProviderDenied => "provider access was denied for the required connection",
            Self::InvalidDescriptor => "the event-store descriptor is invalid",
            Self::UnreachableSubstrate => "the event-store substrate is unreachable",
            Self::ReplayFailed => "opening or replaying the registered product failed",
        }
    }
}

/// The daemon-authoritative readiness of a joined Product's local setup.
///
/// Exactly the three states the issue requires. Membership is retained in every
/// state; none of these revoke a Product Member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SetupReadiness {
    /// Registered, adapter opened, and canonical replay succeeded.
    Open,
    /// Canonical membership exists but provider access / registration setup is
    /// incomplete.
    ContinueSetup { step: ContinueSetupStep },
    /// Setup or replay failed in a typed, recoverable way.
    Repair { reason: SetupRepairReason },
}

impl SetupReadiness {
    /// Stable label used by the Operator Home / dashboard projection.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ContinueSetup { .. } => "continue_setup",
            Self::Repair { .. } => "repair",
        }
    }
}

// ---------------------------------------------------------------------------
// Pure decision
// ---------------------------------------------------------------------------

/// All inputs to the pure setup-continuation decision. Everything is
/// daemon-derived or canonical; the SPA supplies none of it.
pub struct SetupEvaluationInput<'a> {
    /// Whether a canonical Product Member exists for this Product/account.
    /// Continuation is a post-join lifecycle, so this is normally `true`.
    pub membership_present: bool,
    /// Existing durable local registration, if any (from replay).
    pub existing_registration: Option<&'a ProjectRegistration>,
    /// Daemon-resolved provider access.
    pub provider_access: ProviderAccessState,
    /// Daemon-resolved descriptor validation.
    pub descriptor: DescriptorValidation,
    /// Adapter open + replay outcome. Only consulted when a registration
    /// exists (the daemon opens+replays the registered adapter before deciding).
    pub replay_probe: Option<ReplayOutcome>,
}

/// The pure decision produced by [`derive_setup_decision`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupDecision {
    /// A registration already exists and the Product opened + replayed cleanly.
    /// Terminal readiness `Open`; append no event.
    AlreadyOpen,
    /// Provider access + descriptor are valid and no registration exists yet:
    /// write exactly one registration, then open + replay to reach `Open`. The
    /// daemon materializes the non-secret connection metadata from the
    /// validated provider/descriptor facts.
    WriteRegistration,
    /// Setup is incomplete; surface the next step. Append no event.
    Continue(ContinueSetupStep),
    /// A typed, recoverable failure. Append no event; never revoke membership.
    Repair(SetupRepairReason),
    /// Fail-closed safety: continuation was requested without canonical
    /// membership. Append no event and take no action.
    MembershipRequired,
}

impl SetupDecision {
    /// Terminal readiness for decisions that need no further daemon action.
    /// Returns `None` for [`SetupDecision::WriteRegistration`] (the daemon must
    /// write then replay) and [`SetupDecision::MembershipRequired`] (rejection).
    pub fn terminal_readiness(&self) -> Option<SetupReadiness> {
        match self {
            Self::AlreadyOpen => Some(SetupReadiness::Open),
            Self::Continue(step) => Some(SetupReadiness::ContinueSetup { step: *step }),
            Self::Repair(reason) => Some(SetupReadiness::Repair { reason: *reason }),
            Self::WriteRegistration | Self::MembershipRequired => None,
        }
    }
}

/// Derive the local setup-continuation decision. Pure and fail-closed.
///
/// Order:
/// 1. Require canonical membership (safety; continuation is post-join).
/// 2. If a registration already exists, the readiness is decided entirely by
///    the adapter open+replay probe (idempotent — never write a second
///    registration): success ⇒ `Open`, otherwise `Repair(ReplayFailed)`, and
///    the registration is retained.
/// 3. Otherwise gate on provider access, then descriptor validation, mapping
///    each failure to its typed `Repair` reason and each incomplete step to
///    `Continue`. When both are satisfied, write exactly one registration.
pub fn derive_setup_decision(input: &SetupEvaluationInput<'_>) -> SetupDecision {
    if !input.membership_present {
        return SetupDecision::MembershipRequired;
    }

    if let Some(reg) = input.existing_registration {
        return match input.replay_probe {
            Some(ReplayOutcome::Succeeded) => SetupDecision::AlreadyOpen,
            // Registered but the substrate would not open/replay. Registration
            // is retained (we append nothing); the Product is recoverable.
            _ => {
                let _ = reg;
                SetupDecision::Repair(SetupRepairReason::ReplayFailed)
            }
        };
    }

    match input.provider_access {
        ProviderAccessState::Absent => {
            return SetupDecision::Continue(ContinueSetupStep::ConnectProvider)
        }
        ProviderAccessState::Denied => {
            return SetupDecision::Repair(SetupRepairReason::ProviderDenied)
        }
        ProviderAccessState::Authorized => {}
    }

    match input.descriptor {
        DescriptorValidation::NotAttempted => {
            return SetupDecision::Continue(ContinueSetupStep::ValidateDescriptor)
        }
        DescriptorValidation::Invalid => {
            return SetupDecision::Repair(SetupRepairReason::InvalidDescriptor)
        }
        DescriptorValidation::SubstrateUnreachable => {
            return SetupDecision::Repair(SetupRepairReason::UnreachableSubstrate)
        }
        DescriptorValidation::Valid => {}
    }

    SetupDecision::WriteRegistration
}

// ---------------------------------------------------------------------------
// Command params + daemon-authoritative result
// ---------------------------------------------------------------------------

/// Parameters for the `product.continue_setup` tool command.
///
/// The SPA supplies only which Product to continue plus optional non-secret
/// routing hints; provider access, descriptor validation, the confirmed local
/// actor, and the paired Cloud account are all daemon-derived. Unknown fields
/// are rejected so a caller cannot smuggle a secret or an authority claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductContinueSetupParams {
    /// Correlates the request/result pair.
    pub request_id: String,
    /// The joined Product to continue setup for.
    pub product_id: String,
    /// Opaque Cloud-minted routing reference from the join ticket.
    pub registration_ref: String,
    /// Non-secret provider hint (e.g. `github`, `google`). Display/routing only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_hint: Option<RegisteredProvider>,
    /// Non-secret substrate hint. Display/routing only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substrate_hint: Option<RegisteredSubstrate>,
    /// Safe, display-only account label. Never a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    /// When the surface created the request.
    pub requested_at: DateTime<Utc>,
}

/// Outcome discriminant for a [`ProductContinueSetupResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinueSetupOutcome {
    /// Setup advanced (or was already `Open`): a typed readiness is returned.
    Advanced,
    /// Fail-closed rejection (e.g. no canonical membership). No readiness.
    Rejected,
}

/// Typed, non-secret rejection code for a fail-closed continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinueSetupRejectionCode {
    /// Continuation was requested for a Product with no canonical membership.
    MembershipRequired,
}

impl ContinueSetupRejectionCode {
    /// Stable wire string (matches the serde `snake_case` rename).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MembershipRequired => "membership_required",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::MembershipRequired => {
                "no canonical product membership exists for this product/account"
            }
        }
    }
}

/// Typed failure detail attached to a rejected [`ProductContinueSetupResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinueSetupFailure {
    pub code: ContinueSetupRejectionCode,
    pub message: String,
}

/// Daemon-authoritative outcome of a `product.continue_setup`.
///
/// On success it reports the typed [`SetupReadiness`] and, when a registration
/// exists, the opaque `registration_ref`. `membership_retained` is always
/// `true` on an advanced result — this slice never revokes a Product Member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductContinueSetupResult {
    pub request_id: String,
    pub product_id: String,
    pub cloud_account_id: String,
    pub outcome: ContinueSetupOutcome,
    /// Whether canonical membership is retained. Always `true` on an advanced
    /// result; a failed setup never revokes membership.
    pub membership_retained: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<SetupReadiness>,
    /// Opaque routing reference; present once a registration exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ContinueSetupFailure>,
    pub decided_at: DateTime<Utc>,
}

impl ProductContinueSetupResult {
    /// Build an advanced result carrying the typed readiness.
    pub fn advanced(
        request_id: String,
        product_id: String,
        cloud_account_id: String,
        readiness: SetupReadiness,
        registration_ref: Option<String>,
        decided_at: DateTime<Utc>,
    ) -> Self {
        Self {
            request_id,
            product_id,
            cloud_account_id,
            outcome: ContinueSetupOutcome::Advanced,
            membership_retained: true,
            readiness: Some(readiness),
            registration_ref,
            failure: None,
            decided_at,
        }
    }

    /// Build a fail-closed rejection. Membership (if any) is still retained.
    pub fn rejected(
        request_id: String,
        product_id: String,
        cloud_account_id: String,
        code: ContinueSetupRejectionCode,
        membership_retained: bool,
        decided_at: DateTime<Utc>,
    ) -> Self {
        Self {
            request_id,
            product_id,
            cloud_account_id,
            outcome: ContinueSetupOutcome::Rejected,
            membership_retained,
            readiness: None,
            registration_ref: None,
            failure: Some(ContinueSetupFailure {
                code,
                message: code.message().to_string(),
            }),
            decided_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Redacted hosted projection (Operator Home / dashboard)
// ---------------------------------------------------------------------------

/// Safe, redacted setup projection for the hosted SPA / Operator Home.
///
/// Carries only the readiness label, the safe next-step / repair-reason codes,
/// and whether a registration exists. It never carries a registration_ref,
/// descriptor, account label, provider secret, or local path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedSetupProjection {
    pub product_id: String,
    /// `open` | `continue_setup` | `repair`.
    pub readiness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_reason: Option<String>,
    pub has_registration: bool,
    pub membership_retained: bool,
}

impl HostedSetupProjection {
    /// Project a daemon-authoritative readiness into the safe hosted shape.
    pub fn project(
        product_id: impl Into<String>,
        readiness: &SetupReadiness,
        has_registration: bool,
    ) -> Self {
        let (next_step, repair_reason) = match readiness {
            SetupReadiness::Open => (None, None),
            SetupReadiness::ContinueSetup { step } => (Some(step.as_str().to_string()), None),
            SetupReadiness::Repair { reason } => (None, Some(reason.as_str().to_string())),
        };
        Self {
            product_id: product_id.into(),
            readiness: readiness.label().to_string(),
            next_step,
            repair_reason,
            has_registration,
            membership_retained: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn registration() -> ProjectRegistration {
        ProjectRegistration {
            product_id: "prj_1".into(),
            cloud_account_id: "acct_1".into(),
            registration_ref: "reg_ref_opaque".into(),
            connection: ConnectionMetadata {
                provider: RegisteredProvider::Github,
                substrate: RegisteredSubstrate::Git,
                account_label: Some("acme-org".into()),
                descriptor_ref: "descriptor.local.opaque".into(),
            },
            governance_policy_version: "gpv-1".into(),
            registered_at: ts(),
        }
    }

    fn input<'a>(
        membership: bool,
        reg: Option<&'a ProjectRegistration>,
        provider: ProviderAccessState,
        descriptor: DescriptorValidation,
        replay: Option<ReplayOutcome>,
    ) -> SetupEvaluationInput<'a> {
        SetupEvaluationInput {
            membership_present: membership,
            existing_registration: reg,
            provider_access: provider,
            descriptor,
            replay_probe: replay,
        }
    }

    #[test]
    fn joined_without_provider_is_continue_connect_provider() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Absent,
            DescriptorValidation::NotAttempted,
            None,
        ));
        assert_eq!(
            d,
            SetupDecision::Continue(ContinueSetupStep::ConnectProvider)
        );
        assert_eq!(
            d.terminal_readiness(),
            Some(SetupReadiness::ContinueSetup {
                step: ContinueSetupStep::ConnectProvider
            })
        );
    }

    #[test]
    fn provider_authorized_descriptor_not_attempted_is_continue_validate() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Authorized,
            DescriptorValidation::NotAttempted,
            None,
        ));
        assert_eq!(
            d,
            SetupDecision::Continue(ContinueSetupStep::ValidateDescriptor)
        );
    }

    #[test]
    fn provider_authorized_and_descriptor_valid_writes_registration() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Authorized,
            DescriptorValidation::Valid,
            None,
        ));
        assert_eq!(d, SetupDecision::WriteRegistration);
        // Not terminal: the daemon must write then open+replay.
        assert_eq!(d.terminal_readiness(), None);
    }

    #[test]
    fn provider_denied_is_repair() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Denied,
            DescriptorValidation::NotAttempted,
            None,
        ));
        assert_eq!(d, SetupDecision::Repair(SetupRepairReason::ProviderDenied));
    }

    #[test]
    fn invalid_descriptor_is_repair() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Authorized,
            DescriptorValidation::Invalid,
            None,
        ));
        assert_eq!(
            d,
            SetupDecision::Repair(SetupRepairReason::InvalidDescriptor)
        );
    }

    #[test]
    fn unreachable_substrate_is_repair() {
        let d = derive_setup_decision(&input(
            true,
            None,
            ProviderAccessState::Authorized,
            DescriptorValidation::SubstrateUnreachable,
            None,
        ));
        assert_eq!(
            d,
            SetupDecision::Repair(SetupRepairReason::UnreachableSubstrate)
        );
    }

    #[test]
    fn existing_registration_with_successful_replay_is_open() {
        let reg = registration();
        let d = derive_setup_decision(&input(
            true,
            Some(&reg),
            ProviderAccessState::Authorized,
            DescriptorValidation::Valid,
            Some(ReplayOutcome::Succeeded),
        ));
        assert_eq!(d, SetupDecision::AlreadyOpen);
        assert_eq!(d.terminal_readiness(), Some(SetupReadiness::Open));
    }

    #[test]
    fn existing_registration_with_failed_replay_is_repair_and_keeps_registration() {
        let reg = registration();
        let d = derive_setup_decision(&input(
            true,
            Some(&reg),
            ProviderAccessState::Authorized,
            DescriptorValidation::Valid,
            Some(ReplayOutcome::Failed),
        ));
        assert_eq!(d, SetupDecision::Repair(SetupRepairReason::ReplayFailed));
    }

    #[test]
    fn existing_registration_reused_even_when_provider_facts_absent() {
        // Idempotency: once registered, provider/descriptor facts no longer
        // gate readiness — the registration is reused, never rewritten.
        let reg = registration();
        let d = derive_setup_decision(&input(
            true,
            Some(&reg),
            ProviderAccessState::Absent,
            DescriptorValidation::NotAttempted,
            Some(ReplayOutcome::Succeeded),
        ));
        assert_eq!(d, SetupDecision::AlreadyOpen);
    }

    #[test]
    fn missing_membership_is_fail_closed() {
        let d = derive_setup_decision(&input(
            false,
            None,
            ProviderAccessState::Authorized,
            DescriptorValidation::Valid,
            None,
        ));
        assert_eq!(d, SetupDecision::MembershipRequired);
        assert_eq!(d.terminal_readiness(), None);
    }

    #[test]
    fn metadata_parsing_fails_safe() {
        assert_eq!(
            ProviderAccessState::from_metadata(None),
            ProviderAccessState::Absent
        );
        assert_eq!(
            ProviderAccessState::from_metadata(Some("bogus")),
            ProviderAccessState::Absent
        );
        assert_eq!(
            ProviderAccessState::from_metadata(Some("authorized")),
            ProviderAccessState::Authorized
        );
        assert_eq!(
            DescriptorValidation::from_metadata(None),
            DescriptorValidation::NotAttempted
        );
        assert_eq!(
            DescriptorValidation::from_metadata(Some("unreachable_substrate")),
            DescriptorValidation::SubstrateUnreachable
        );
    }

    #[test]
    fn hosted_projection_is_redacted() {
        let reg = registration();
        let projection = HostedSetupProjection::project(
            reg.product_id.clone(),
            &SetupReadiness::Repair {
                reason: SetupRepairReason::InvalidDescriptor,
            },
            true,
        );
        let json = serde_json::to_string(&projection).unwrap();
        // Never leak descriptor_ref, registration_ref, account label, or paths.
        assert!(!json.contains("descriptor.local.opaque"));
        assert!(!json.contains("reg_ref_opaque"));
        assert!(!json.contains("acme-org"));
        assert!(json.contains("repair"));
        assert!(json.contains("invalid_descriptor"));
    }

    #[test]
    fn advanced_result_retains_membership_and_serializes_readiness() {
        let result = ProductContinueSetupResult::advanced(
            "req_1".into(),
            "prj_1".into(),
            "acct_1".into(),
            SetupReadiness::ContinueSetup {
                step: ContinueSetupStep::ConnectProvider,
            },
            None,
            ts(),
        );
        assert!(result.membership_retained);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"state\":\"continue_setup\""));
        assert!(json.contains("connect_provider"));
    }

    #[test]
    fn rejected_result_keeps_membership_flag() {
        let result = ProductContinueSetupResult::rejected(
            "req_1".into(),
            "prj_1".into(),
            "acct_1".into(),
            ContinueSetupRejectionCode::MembershipRequired,
            false,
            ts(),
        );
        assert_eq!(result.outcome, ContinueSetupOutcome::Rejected);
        assert!(result.readiness.is_none());
        assert!(result.failure.is_some());
    }

    #[test]
    fn payload_round_trips_to_registration() {
        let payload = ProjectRegistrationWrittenPayload {
            product_id: "prj_1".into(),
            cloud_account_id: "acct_1".into(),
            registration_ref: "reg_ref_opaque".into(),
            connection: ConnectionMetadata {
                provider: RegisteredProvider::Github,
                substrate: RegisteredSubstrate::Git,
                account_label: Some("acme-org".into()),
                descriptor_ref: "descriptor.local.opaque".into(),
            },
            governance_policy_version: "gpv-1".into(),
            written_at: ts(),
        };
        let reg = payload.to_registration();
        assert_eq!(reg.product_id, "prj_1");
        assert_eq!(reg.registration_ref, "reg_ref_opaque");
        assert_eq!(reg.registered_at, ts());
    }
}
