//! Remote action callback gateway — bot-owned consumer for authenticated
//! remote review action callbacks.
//!
//! This module validates incoming `AuthenticatedRemoteActionEnvelope` payloads,
//! checks replay/idempotency, resolves actor bindings, evaluates authorization
//! policy, and dispatches permitted actions through the canonical `ToolRequest`
//! path.
//!
//! # Authority boundary
//!
//! Remote callbacks **cannot** directly:
//! - Mint `AuthorityContext` or actor identity
//! - Approve signoff (owner-only action)
//! - Resolve compliance
//! - Write to the event store
//! - Access local workspace
//! - Perform source ingest
//!
//! The gateway derives authority context from the validated callback and
//! routes it through `ActionAuthorizationPolicy`. Only candidate-review
//! class actions are supported in alpha.
//!
//! # Spec references
//!
//! - `docs/specs/action-authorization-policy-runtime-architecture.md` §3
//! - `docs/specs/action-authorization-policy.md` — Authenticated remote
//!   review action surface category
//! - `docs/domain/runtime-protocol.md` — "Authenticated remote review action"

use std::collections::HashSet;
use std::sync::Mutex;

use bicameral_api::action_authorization::ActionAuthorizationPolicy;
use bicameral_api::remote_action::{
    detect_remote_action_smuggled_fields, AuthenticatedRemoteActionEnvelope, CallbackSource,
    RemoteActionKind, RemoteActionOutcome, RemoteActionReceipt, RemoteActionRejection,
    RemoteActionRejectionReason, RemoteActionValidationResult, ValidatedRemoteAction,
};
use bicameral_api::tool_request::{AuthMethod, AuthorityContext};
use chrono::{Duration, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Actor binding registry (alpha: in-memory lookup)
// ---------------------------------------------------------------------------

/// Actor binding entry — maps an external identity to a Bicameral actor.
#[derive(Debug, Clone)]
pub struct ActorBinding {
    /// External actor ID from the provider (e.g. Slack user ID).
    pub external_actor_id: String,
    /// Callback source this binding applies to.
    pub callback_source: CallbackSource,
    /// Resolved Bicameral actor ID.
    pub bicameral_actor_id: String,
}

/// Actor binding registry trait — resolves external actors to Bicameral actors.
pub trait ActorBindingRegistry {
    /// Look up a Bicameral actor ID for an external actor + callback source.
    fn resolve_actor(
        &self,
        external_actor_id: &str,
        callback_source: &CallbackSource,
    ) -> Option<String>;
}

/// In-memory actor binding registry for alpha/testing.
#[derive(Debug, Default)]
pub struct MemoryActorBindingRegistry {
    bindings: Vec<ActorBinding>,
}

impl MemoryActorBindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_binding(&mut self, binding: ActorBinding) {
        self.bindings.push(binding);
    }
}

impl ActorBindingRegistry for MemoryActorBindingRegistry {
    fn resolve_actor(
        &self,
        external_actor_id: &str,
        callback_source: &CallbackSource,
    ) -> Option<String> {
        self.bindings
            .iter()
            .find(|b| {
                b.external_actor_id == external_actor_id && &b.callback_source == callback_source
            })
            .map(|b| b.bicameral_actor_id.clone())
    }
}

// ---------------------------------------------------------------------------
// Signature verifier (alpha: pluggable trait)
// ---------------------------------------------------------------------------

/// Signature verification trait for remote action callbacks.
pub trait SignatureVerifier {
    /// Verify the provider signature for a callback envelope.
    /// Returns `true` if the signature is valid.
    fn verify(&self, envelope: &AuthenticatedRemoteActionEnvelope) -> bool;
}

/// Accept-all verifier for testing. Never use in production.
#[derive(Debug)]
pub struct AcceptAllVerifier;

impl SignatureVerifier for AcceptAllVerifier {
    fn verify(&self, _envelope: &AuthenticatedRemoteActionEnvelope) -> bool {
        true
    }
}

/// Reject-all verifier for negative testing.
#[derive(Debug)]
pub struct RejectAllVerifier;

impl SignatureVerifier for RejectAllVerifier {
    fn verify(&self, _envelope: &AuthenticatedRemoteActionEnvelope) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Nonce store (replay protection)
// ---------------------------------------------------------------------------

/// Nonce store trait for replay detection.
pub trait NonceStore {
    /// Check if a nonce has been consumed. Returns `true` if it was already
    /// used (replay).
    fn check_and_consume(&self, nonce: &str) -> bool;
}

/// In-memory nonce store for alpha/testing.
#[derive(Debug, Default)]
pub struct MemoryNonceStore {
    consumed: Mutex<HashSet<String>>,
}

impl MemoryNonceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NonceStore for MemoryNonceStore {
    fn check_and_consume(&self, nonce: &str) -> bool {
        let mut consumed = self.consumed.lock().unwrap();
        !consumed.insert(nonce.to_string())
    }
}

// ---------------------------------------------------------------------------
// Remote action gateway
// ---------------------------------------------------------------------------

/// Configuration for the remote action gateway.
pub struct RemoteActionGatewayConfig {
    /// Maximum age of a callback before it's considered expired (seconds).
    pub max_callback_age_seconds: i64,
}

impl Default for RemoteActionGatewayConfig {
    fn default() -> Self {
        Self {
            max_callback_age_seconds: 300, // 5 minutes
        }
    }
}

/// Remote action callback gateway.
///
/// Validates, authenticates, and routes remote action callbacks through
/// the bot-owned authority path.
pub struct RemoteActionGateway<S: SignatureVerifier, N: NonceStore, A: ActorBindingRegistry> {
    verifier: S,
    nonce_store: N,
    actor_registry: A,
    config: RemoteActionGatewayConfig,
}

impl<S: SignatureVerifier, N: NonceStore, A: ActorBindingRegistry> RemoteActionGateway<S, N, A> {
    pub fn new(verifier: S, nonce_store: N, actor_registry: A) -> Self {
        Self {
            verifier,
            nonce_store,
            actor_registry,
            config: RemoteActionGatewayConfig::default(),
        }
    }

    pub fn with_config(
        verifier: S,
        nonce_store: N,
        actor_registry: A,
        config: RemoteActionGatewayConfig,
    ) -> Self {
        Self {
            verifier,
            nonce_store,
            actor_registry,
            config,
        }
    }

    /// Process a raw JSON payload as a remote action callback.
    ///
    /// This is the primary entry point. It performs:
    /// 1. Authority-smuggling field detection on the raw payload
    /// 2. Deserialization into `AuthenticatedRemoteActionEnvelope`
    /// 3. Signature verification
    /// 4. Timestamp/expiry check
    /// 5. Replay/nonce check
    /// 6. Actor binding resolution
    /// 7. Action kind validation
    ///
    /// Returns a `ValidatedRemoteAction` on success or a
    /// `RemoteActionRejection` on failure.
    pub fn validate_raw(&self, raw_payload: &serde_json::Value) -> RemoteActionValidationResult {
        // Step 1: detect authority-smuggling fields
        let smuggled = detect_remote_action_smuggled_fields(raw_payload);
        if !smuggled.is_empty() {
            let action_id = raw_payload
                .get("action_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or(Uuid::nil());
            return Err(RemoteActionRejection::new(
                RemoteActionRejectionReason::AuthoritySmugglingDetected,
                format!(
                    "Forbidden authority fields detected in remote action payload: {:?}",
                    smuggled
                ),
                action_id,
            ));
        }

        // Step 2: deserialize
        let envelope: AuthenticatedRemoteActionEnvelope =
            serde_json::from_value(raw_payload.clone()).map_err(|e| {
                RemoteActionRejection::new(
                    RemoteActionRejectionReason::UnsupportedAction,
                    format!("Failed to deserialize remote action envelope: {}", e),
                    Uuid::nil(),
                )
            })?;

        self.validate_envelope(&envelope)
    }

    /// Validate a deserialized envelope.
    pub fn validate_envelope(
        &self,
        envelope: &AuthenticatedRemoteActionEnvelope,
    ) -> RemoteActionValidationResult {
        // Step 3: signature verification
        if !self.verifier.verify(envelope) {
            return Err(RemoteActionRejection::new(
                RemoteActionRejectionReason::InvalidSignature,
                "Provider signature verification failed",
                envelope.action_id,
            ));
        }

        // Step 4: timestamp/expiry check
        let now = Utc::now();
        let max_age = Duration::seconds(self.config.max_callback_age_seconds);
        if now.signed_duration_since(envelope.issued_at) > max_age {
            return Err(RemoteActionRejection::new(
                RemoteActionRejectionReason::TimestampExpired,
                format!(
                    "Callback issued_at is older than {} seconds",
                    self.config.max_callback_age_seconds
                ),
                envelope.action_id,
            ));
        }
        // Reject future timestamps (clock skew tolerance: 60s)
        if envelope.issued_at > now + Duration::seconds(60) {
            return Err(RemoteActionRejection::new(
                RemoteActionRejectionReason::TimestampExpired,
                "Callback issued_at is in the future beyond clock skew tolerance",
                envelope.action_id,
            ));
        }

        // Step 5: replay/nonce check
        if self.nonce_store.check_and_consume(&envelope.nonce) {
            return Err(RemoteActionRejection::new(
                RemoteActionRejectionReason::ReplayDetected,
                format!("Nonce '{}' has already been consumed", envelope.nonce),
                envelope.action_id,
            ));
        }

        // Step 6: actor binding resolution
        let resolved_actor_id = self
            .actor_registry
            .resolve_actor(&envelope.external_actor_id, &envelope.callback_source)
            .ok_or_else(|| {
                RemoteActionRejection::new(
                    RemoteActionRejectionReason::ActorBindingFailed,
                    format!(
                        "Cannot bind external actor '{}' from {:?} to a Bicameral actor",
                        envelope.external_actor_id, envelope.callback_source
                    ),
                    envelope.action_id,
                )
            })?;

        // Step 7: action kind validation (only review-class in alpha)
        // All current RemoteActionKind variants are review-class, so this
        // is a no-op now but will matter when signoff actions are added.

        Ok(ValidatedRemoteAction {
            action_id: envelope.action_id,
            action_kind: envelope.action_kind,
            target_id: envelope.target_id.clone(),
            resolved_actor_id,
            callback_source: envelope.callback_source.clone(),
            reason: envelope.reason.clone(),
        })
    }

    /// Evaluate authorization policy for a validated remote action.
    ///
    /// Derives an `AuthorityContext` from the validated action and checks
    /// it against `ActionAuthorizationPolicy`.
    pub fn evaluate_authorization(
        &self,
        validated: &ValidatedRemoteAction,
    ) -> Result<(), bicameral_api::action_authorization::ActionDenial> {
        let authority = self.derive_authority_context(validated);
        let action_class = validated.action_kind.to_governed_action_class();
        ActionAuthorizationPolicy::evaluate_action_class(&authority, action_class)?;
        Ok(())
    }

    /// Derive an `AuthorityContext` from a validated remote action.
    ///
    /// The authority context is bot-derived, never caller-supplied.
    /// Auth method is `HostedBridge` for remote callbacks (they pass through
    /// the same hosted/remote gateway authority path).
    fn derive_authority_context(&self, validated: &ValidatedRemoteAction) -> AuthorityContext {
        let mut audit_metadata = serde_json::Map::new();
        audit_metadata.insert(
            "surface".into(),
            serde_json::Value::String("remote_action_callback".into()),
        );
        audit_metadata.insert(
            "callback_source".into(),
            serde_json::to_value(&validated.callback_source).unwrap_or_default(),
        );
        audit_metadata.insert(
            "action_id".into(),
            serde_json::Value::String(validated.action_id.to_string()),
        );

        AuthorityContext {
            actor_id: validated.resolved_actor_id.clone(),
            auth_method: AuthMethod::HostedBridge,
            session_id: None,
            workspace: None,
            policy_scope: vec!["remote_action_callback".into()],
            audit_metadata,
        }
    }

    /// Full processing pipeline: validate + authorize + produce receipt.
    ///
    /// This is the recommended single-call entry point for consuming a
    /// remote action callback end-to-end.
    pub fn process_callback(
        &self,
        raw_payload: &serde_json::Value,
    ) -> (RemoteActionReceipt, Option<ValidatedRemoteAction>) {
        let validation_result = self.validate_raw(raw_payload);

        match validation_result {
            Err(rejection) => {
                let receipt = RemoteActionReceipt {
                    receipt_id: Uuid::new_v4(),
                    action_id: rejection.action_id,
                    callback_source: CallbackSource::Other("unknown".into()),
                    external_actor_id: raw_payload
                        .get("external_actor_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    resolved_actor_id: None,
                    action_kind: RemoteActionKind::AcceptCandidate,
                    target_id: raw_payload
                        .get("target_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    outcome: RemoteActionOutcome::ValidationFailed,
                    processed_at: Utc::now(),
                    rejection: Some(rejection),
                };
                (receipt, None)
            }
            Ok(validated) => {
                let auth_result = self.evaluate_authorization(&validated);
                match auth_result {
                    Err(denial) => {
                        let rejection = RemoteActionRejection::new(
                            RemoteActionRejectionReason::UnsupportedAction,
                            format!("Authorization denied: {}", denial),
                            validated.action_id,
                        );
                        let receipt = RemoteActionReceipt {
                            receipt_id: Uuid::new_v4(),
                            action_id: validated.action_id,
                            callback_source: validated.callback_source.clone(),
                            external_actor_id: raw_payload
                                .get("external_actor_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            resolved_actor_id: Some(validated.resolved_actor_id.clone()),
                            action_kind: validated.action_kind,
                            target_id: validated.target_id.clone(),
                            outcome: RemoteActionOutcome::AuthorizationDenied,
                            processed_at: Utc::now(),
                            rejection: Some(rejection),
                        };
                        (receipt, None)
                    }
                    Ok(()) => {
                        let receipt = RemoteActionReceipt {
                            receipt_id: Uuid::new_v4(),
                            action_id: validated.action_id,
                            callback_source: validated.callback_source.clone(),
                            external_actor_id: raw_payload
                                .get("external_actor_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            resolved_actor_id: Some(validated.resolved_actor_id.clone()),
                            action_kind: validated.action_kind,
                            target_id: validated.target_id.clone(),
                            outcome: RemoteActionOutcome::Accepted,
                            processed_at: Utc::now(),
                            rejection: None,
                        };
                        (receipt, Some(validated))
                    }
                }
            }
        }
    }
}
