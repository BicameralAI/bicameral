"""In-process fake canonical Product daemon for hosted-onboarding tests (cloud#127).

Cloud is a hosted coordination/projection tier. The paired *local daemon* stays
authoritative for canonical Product creation, membership, roles, canonical
events, replay truth, and local provider/event-store registration
(ADR-0005/0006). This module models that authoritative daemon deterministically
so a two-user hosted onboarding journey can prove the cross-repository authority
boundary without a real daemon, live Supabase, or deployed credentials.

The daemon:

- verifies the Cloud-signed :class:`SignedProductJoinTicket` envelope
  (Ed25519 signature, issuer/audience/product/domain/account/expiry) before it
  materializes any membership;
- grants the creator BOTH ``Policy Owner`` and ``Product Owner``;
- grants a joining peer ``Product Member`` ONLY (never an owner role);
- folds an append-only canonical event log to answer replay, so role truth is
  derived from events, not a mutable cache;
- is idempotent per join nonce and per actor: a replayed ticket is rejected and
  a resumed setup never appends a second membership event;
- retains provider secrets and local filesystem paths in a ``local-only`` area
  that is *never* returned in any Cloud/browser-facing projection, so redaction
  can be asserted end to end.

The same object is durable across a Cloud application restart in the tests: the
test process holds the daemon, so canonical state survives recreating the Cloud
app (which only owns non-canonical coordination projections).
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass, field
from datetime import UTC, datetime

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from bicameral_cloud.models.product_join_ticket import ProductJoinTicket

POLICY_OWNER = "Policy Owner"
PRODUCT_OWNER = "Product Owner"
PRODUCT_MEMBER = "Product Member"

# Deterministic join-failure codes, matching the bot ``ProductJoinFailureCode``
# contract (bot#767) so Cloud/browser handling stays reproducible.
JOIN_FAILURES = (
    "expired_ticket",
    "wrong_domain",
    "wrong_account",
    "wrong_product",
    "replayed_ticket",
    "bad_signature",
)


class FakeDaemonError(RuntimeError):
    """Raised for daemon misuse that is not a normal typed join failure."""


@dataclass
class _CanonicalEvent:
    """An append-only canonical event; the log is the source of role truth."""

    seq: int
    kind: str
    actor_ref: str
    detail: dict[str, object] = field(default_factory=dict)


@dataclass
class _ProductState:
    """Canonical local state for one Product, owned only by the daemon."""

    product_id: str
    display_name: str
    verified_domain: str
    creator_actor_ref: str
    registration_ref: str
    creation_receipt: str
    events: list[_CanonicalEvent] = field(default_factory=list)
    # local-only material that must never cross to Cloud/browser/logs
    provider_secret: str = ""
    event_store_path: str = ""

    def _append(
        self, kind: str, actor_ref: str, detail: dict[str, object] | None = None
    ) -> _CanonicalEvent:
        event = _CanonicalEvent(
            seq=len(self.events) + 1,
            kind=kind,
            actor_ref=actor_ref,
            detail=dict(detail or {}),
        )
        self.events.append(event)
        return event

    def membership_event_count(self, actor_ref: str) -> int:
        return sum(
            1
            for e in self.events
            if e.kind == "role.granted"
            and e.actor_ref == actor_ref
            and e.detail.get("role") == PRODUCT_MEMBER
        )

    def has_role(self, actor_ref: str, role: str) -> bool:
        return any(
            e.kind == "role.granted"
            and e.actor_ref == actor_ref
            and e.detail.get("role") == role
            for e in self.events
        )


@dataclass(frozen=True)
class ProductCreation:
    """Safe creation handoff returned to the owner for Cloud publication."""

    product_id: str
    display_name: str
    verified_domain: str
    registration_ref: str
    creation_receipt: str
    provider_hint: str | None
    substrate_hint: str | None


class FakeProductDaemon:
    """Deterministic authoritative local daemon over an in-memory event log."""

    def __init__(
        self,
        *,
        verification_keys: dict[str, bytes],
        issuer: str,
    ) -> None:
        self._public_keys = {
            key_id: Ed25519PublicKey.from_public_bytes(raw)
            for key_id, raw in verification_keys.items()
        }
        self._issuer = issuer
        self._products: dict[str, _ProductState] = {}
        self._nonces_seen: set[str] = set()

    # --- owner path -----------------------------------------------------------
    def create_product(
        self,
        *,
        creator_actor_ref: str,
        display_name: str,
        verified_domain: str,
        provider_hint: str | None = "github",
        substrate_hint: str | None = "drive_folder",
    ) -> ProductCreation:
        """Create a canonical Product; creator becomes Policy + Product Owner."""

        product_id = f"prd_{uuid.uuid4().hex[:16]}"
        registration_ref = f"reg_{uuid.uuid4().hex[:12]}"
        creation_receipt = f"rcpt_{uuid.uuid4().hex[:12]}"
        state = _ProductState(
            product_id=product_id,
            display_name=display_name,
            verified_domain=verified_domain.strip().lower(),
            creator_actor_ref=creator_actor_ref,
            registration_ref=registration_ref,
            creation_receipt=creation_receipt,
            # local-only, never projected to Cloud/browser:
            provider_secret=f"ghp_{uuid.uuid4().hex}",
            event_store_path=f"/home/creator/.bicameral/{product_id}/events.jsonl",
        )
        state._append(
            "product.created", creator_actor_ref, {"display_name": display_name}
        )
        state._append("role.granted", creator_actor_ref, {"role": POLICY_OWNER})
        state._append("role.granted", creator_actor_ref, {"role": PRODUCT_OWNER})
        state._append(
            "provider.access.granted",
            creator_actor_ref,
            {"provider": provider_hint or "github"},
        )
        state._append("project.registered", creator_actor_ref, {})
        self._products[product_id] = state
        return ProductCreation(
            product_id=product_id,
            display_name=display_name,
            verified_domain=state.verified_domain,
            registration_ref=registration_ref,
            creation_receipt=creation_receipt,
            provider_hint=provider_hint,
            substrate_hint=substrate_hint,
        )

    # --- peer path ------------------------------------------------------------
    def product_join(
        self,
        *,
        signed_ticket: dict[str, object],
        local_actor_ref: str,
        request_id: str | None = None,
        now: datetime | None = None,
    ) -> dict[str, object]:
        """Validate a Cloud-signed ticket and materialize Product Member only.

        Returns a bot-shaped ``ProductJoinResult`` mapping. Membership is created
        only on success and only once per actor (a resumed/interrupted setup does
        not append a second membership event). Replayed nonces fail closed.
        """

        request_id = request_id or f"req_{uuid.uuid4().hex}"
        moment = now or datetime.now(UTC)

        failure = self._validate_ticket(signed_ticket, local_actor_ref, moment)
        if failure is not None:
            return self._join_result(
                request_id=request_id,
                product_id=str(
                    (signed_ticket.get("ticket") or {}).get("product_id", "")  # type: ignore[union-attr]
                ),
                outcome="rejected",
                failure=failure,
                decided_at=moment,
            )

        ticket = ProductJoinTicket.model_validate(signed_ticket["ticket"])
        state = self._products[ticket.product_id]

        self._nonces_seen.add(ticket.nonce)
        # Idempotent per actor: only append a membership event the first time.
        if state.membership_event_count(local_actor_ref) == 0:
            state._append("role.granted", local_actor_ref, {"role": PRODUCT_MEMBER})
        membership_event = next(
            e
            for e in state.events
            if e.kind == "role.granted"
            and e.actor_ref == local_actor_ref
            and e.detail.get("role") == PRODUCT_MEMBER
        )
        return self._join_result(
            request_id=request_id,
            product_id=ticket.product_id,
            outcome="granted",
            failure=None,
            decided_at=moment,
            membership_event_ref=f"evt_{ticket.product_id}_{membership_event.seq}",
        )

    def continue_provider_access(
        self, *, product_id: str, local_actor_ref: str
    ) -> None:
        """Resume post-join provider access + ProjectRegistration.

        Interrupting and resuming this step must never mint a second membership
        event: it appends only provider/registration events for a member.
        """

        state = self._require(product_id)
        if state.membership_event_count(local_actor_ref) == 0:
            raise FakeDaemonError("cannot continue setup before joining")
        state._append("provider.access.granted", local_actor_ref, {})
        state._append("project.registered", local_actor_ref, {})

    # --- replay / projections -------------------------------------------------
    def replay_roles(self, product_id: str) -> dict[str, list[str]]:
        """Fold the canonical event log into per-actor roles (replay truth)."""

        state = self._require(product_id)
        roles: dict[str, set[str]] = {}
        for event in state.events:
            if event.kind == "role.granted":
                role = str(event.detail.get("role"))
                roles.setdefault(event.actor_ref, set()).add(role)
        return {actor: sorted(vals) for actor, vals in roles.items()}

    def membership_event_count(self, product_id: str, actor_ref: str) -> int:
        return self._require(product_id).membership_event_count(actor_ref)

    def safe_projection(self, product_id: str) -> dict[str, object]:
        """Redacted browser/Cloud-facing projection: no secrets, paths, or roles."""

        state = self._require(product_id)
        return {
            "product_id": state.product_id,
            "display_name": state.display_name,
            "verified_domain": state.verified_domain,
            "registration_ref": state.registration_ref,
        }

    def local_only_material(self, product_id: str) -> tuple[str, str]:
        """Return (provider_secret, event_store_path) for redaction assertions."""

        state = self._require(product_id)
        return state.provider_secret, state.event_store_path

    # --- internals ------------------------------------------------------------
    def _require(self, product_id: str) -> _ProductState:
        state = self._products.get(product_id)
        if state is None:
            raise FakeDaemonError(f"unknown product {product_id!r}")
        return state

    def _join_result(
        self,
        *,
        request_id: str,
        product_id: str,
        outcome: str,
        failure: str | None,
        decided_at: datetime,
        membership_event_ref: str | None = None,
    ) -> dict[str, object]:
        granted = outcome == "granted"
        return {
            "request_id": request_id,
            "product_id": product_id,
            "cloud_account_id": self._issuer,
            "outcome": outcome,
            "granted_role": PRODUCT_MEMBER if granted else None,
            "membership_event_ref": membership_event_ref,
            "receipt_id": f"rcpt_{uuid.uuid4().hex}" if granted else None,
            "failure": None if failure is None else {"code": failure},
            "decided_at": decided_at.isoformat().replace("+00:00", "Z"),
        }

    def _validate_ticket(
        self,
        signed_ticket: dict[str, object],
        local_actor_ref: str,
        moment: datetime,
    ) -> str | None:
        raw_ticket = signed_ticket.get("ticket")
        key_id = signed_ticket.get("key_id")
        signature_b64 = signed_ticket.get("signature")
        if not isinstance(raw_ticket, dict) or not isinstance(signature_b64, str):
            return "bad_signature"
        try:
            ticket = ProductJoinTicket.model_validate(raw_ticket)
        except ValueError:
            return "bad_signature"

        public_key = self._public_keys.get(str(key_id))
        if public_key is None:
            return "bad_signature"
        import base64

        try:
            public_key.verify(base64.b64decode(signature_b64), ticket.signing_bytes())
        except (InvalidSignature, ValueError):
            return "bad_signature"

        if ticket.issuer != self._issuer:
            return "bad_signature"
        if ticket.nonce in self._nonces_seen:
            return "replayed_ticket"
        if _as_aware(ticket.expiry) <= moment:
            return "expired_ticket"

        state = self._products.get(ticket.product_id)
        if state is None:
            return "wrong_product"
        if ticket.audience != f"daemon:{state.product_id}":
            return "wrong_product"
        if ticket.verified_domain.strip().lower() != state.verified_domain:
            return "wrong_domain"
        # The daemon only trusts a ticket whose Cloud account matches the
        # locally confirmed actor's Cloud identity.
        if ticket.cloud_account_id != local_actor_ref:
            return "wrong_account"
        return None


def _as_aware(value: datetime) -> datetime:
    return value if value.tzinfo is not None else value.replace(tzinfo=UTC)
