"""Pure-Python bridge client mirroring the dashboard pending-pairing contract.

The browser flow is pending-first (bot#790): Cloud initiates a pairing request
(``POST /pair`` -> ``202`` pending), the user approves on the local confirmation
surface, and Cloud polls ``GET /pair/status`` until a transient token is released
on approval. Cloud never approves and never mints or persists a token.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import httpx

# The protocol version the browser declares. A missing/older value is answered
# with update-required, never an immediate token.
CLIENT_PROTOCOL_VERSION = 4

DEFAULT_DAEMON_REPAIR_HINT = (
    "Check that the daemon is running and hosted bridge is enabled."
)


@dataclass(frozen=True)
class PairingOutcome:
    """Normalized result of the full initiate -> poll pending-pairing flow.

    ``state`` is one of: ``approved`` (transient token released),
    ``pending`` (never approved within the poll budget), ``denied``,
    ``expired``, ``revoked``, ``update_required``, ``unavailable``, ``repair``.
    """

    state: str
    session_token: str | None = None
    expires_at: str | None = None
    confirmation: dict[str, object] | None = None
    pairing_code: str | None = None
    repair_message: str | None = None
    repair_hint: str | None = None
    capabilities: dict[str, object] | None = field(default=None)
    # Legacy immediate-projection body, used only by the discovery/lifecycle
    # conformance suites that pair as a fixture (see ``attempt_pairing``).
    projection: dict[str, object] | None = None


def _status_to_state(status: str) -> str:
    mapping = {
        "approved": "approved",
        "denied": "denied",
        "expired": "expired",
        "revoked": "revoked",
        "incompatible": "update_required",
        "update_required": "update_required",
        "account_mismatch": "account_mismatch",
    }
    return mapping.get(status, "unavailable")


def initiate_pairing(
    daemon_base_url: str,
    *,
    origin: str,
    cloud_account_id: str,
    client_protocol_version: int = CLIENT_PROTOCOL_VERSION,
    timeout: float = 2.0,
) -> PairingOutcome:
    """POST the pending pairing request and normalize the 202 envelope."""

    url = daemon_base_url.rstrip("/") + "/api/v1/hosted-bridge/pair"
    body: dict[str, object] = {
        "origin": origin,
        "cloud_account_id": cloud_account_id,
        "csrf_token": "conformance-csrf",
        "team_id": "team-conformance",
        "cloud_access_token": "cloud-token-conformance",
        "client_protocol_version": client_protocol_version,
    }
    try:
        response = httpx.post(url, json=body, timeout=timeout)
    except httpx.HTTPError:
        return PairingOutcome(
            state="unavailable",
            repair_message="Could not reach the local daemon.",
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    try:
        payload = response.json()
    except ValueError:
        payload = {}
    payload = payload if isinstance(payload, dict) else {}

    if response.status_code == 426:
        return PairingOutcome(state="update_required")

    if response.status_code == 202 or payload.get("status") == "pending":
        return PairingOutcome(
            state="pending",
            pairing_code=str(payload["pairing_code"])
            if payload.get("pairing_code")
            else None,
            confirmation=payload.get("confirmation")
            if isinstance(payload.get("confirmation"), dict)
            else None,
        )

    if not response.is_success:
        kind = str(payload.get("error_type") or payload.get("detail") or "")
        if any(k in kind for k in ("protocol", "incompatible", "unsupported")):
            return PairingOutcome(state="update_required")
        return PairingOutcome(
            state="repair",
            repair_message=(
                f"Daemon rejected the pairing request ({response.status_code})."
            ),
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    # A 2xx that is not a pending envelope must never immediately pair.
    return PairingOutcome(state="update_required")


def poll_pairing_status(
    daemon_base_url: str,
    *,
    request_handle: str = "handle-abc",
    poll_proof: str = "proof-xyz",
    cloud_account_id: str,
    origin: str,
    timeout: float = 2.0,
) -> PairingOutcome:
    """Poll the status endpoint once and normalize the outcome."""

    url = daemon_base_url.rstrip("/") + "/api/v1/hosted-bridge/pair/status"
    params = {
        "request_handle": request_handle,
        "poll_proof": poll_proof,
        "cloud_account_id": cloud_account_id,
    }
    try:
        response = httpx.get(
            url, params=params, headers={"Origin": origin}, timeout=timeout
        )
    except httpx.HTTPError:
        return PairingOutcome(
            state="unavailable",
            repair_message="Could not reach the local daemon.",
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    try:
        payload = response.json()
    except ValueError:
        payload = {}
    payload = payload if isinstance(payload, dict) else {}
    status = str(payload.get("status", "")).lower()

    if not response.is_success:
        if response.status_code == 426:
            return PairingOutcome(state="update_required")
        # A rejected proof / origin means Cloud cannot safely keep polling.
        return PairingOutcome(
            state="unavailable",
            repair_message="The daemon rejected the status poll.",
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    if status == "approved" and payload.get("session_token"):
        return PairingOutcome(
            state="approved",
            session_token=str(payload["session_token"]),
            expires_at=payload.get("expires_at"),
            capabilities=payload.get("capabilities")
            if isinstance(payload.get("capabilities"), dict)
            else None,
        )
    if status == "pending" or status == "":
        return PairingOutcome(state="pending")
    return PairingOutcome(state=_status_to_state(status))


def run_pairing(
    daemon_base_url: str,
    *,
    origin: str,
    cloud_account_id: str,
    client_protocol_version: int = CLIENT_PROTOCOL_VERSION,
) -> PairingOutcome:
    """Initiate a pairing request, then poll once for the terminal outcome.

    Mirrors the browser: request -> (local approval) -> poll for the transient
    handoff. The fake daemon reports the post-approval status on the first poll.
    """

    initiated = initiate_pairing(
        daemon_base_url,
        origin=origin,
        cloud_account_id=cloud_account_id,
        client_protocol_version=client_protocol_version,
    )
    if initiated.state != "pending":
        return initiated
    return poll_pairing_status(
        daemon_base_url,
        cloud_account_id=cloud_account_id,
        origin=origin,
    )


# ── Legacy immediate-projection client ──────────────────────────────────────
# Retained ONLY for the hosted-discovery and project-lifecycle conformance
# suites, which pair as a fixture and then assert on the daemon's redacted
# discovery projection. The /connect pairing flow is pending-first (above) and
# must not use this path.

DAEMON_REPAIR_HINTS: dict[str, str] = {
    "origin_rejected": (
        "The daemon rejected the browser origin. Verify that "
        "https://app.bicameral-ai.com is in the daemon origin allowlist."
    ),
    "not_paired": (
        "No active pairing exists. Start the daemon and enable hosted bridge."
    ),
    "pairing_revoked": "Previous pairing was revoked. Re-pair from the daemon.",
    "pairing_expired": "Pairing session has expired. Re-pair from the daemon.",
    "protocol_unsupported": (
        "Protocol mismatch. Update the daemon to a compatible version."
    ),
    "csrf_rejected": "CSRF validation failed. Reload the page and try again.",
    "wrong_daemon": (
        "This daemon is not the expected target. Check your daemon configuration."
    ),
}


def _repair_hint(error_type: object) -> str:
    if isinstance(error_type, str):
        return DAEMON_REPAIR_HINTS.get(error_type, DEFAULT_DAEMON_REPAIR_HINT)
    return DEFAULT_DAEMON_REPAIR_HINT


def attempt_pairing(
    daemon_base_url: str,
    *,
    origin: str,
    cloud_account_id: str,
    selected_project_id: str | None = None,
    timeout: float = 2.0,
) -> PairingOutcome:
    """Legacy immediate-projection pairing (discovery/lifecycle fixtures only)."""

    url = daemon_base_url.rstrip("/") + "/api/v1/hosted-bridge/pair"
    body: dict[str, object] = {
        "origin": origin,
        "cloud_account_id": cloud_account_id,
    }
    if selected_project_id is not None:
        body["selected_project_id"] = selected_project_id
    try:
        response = httpx.post(url, json=body, timeout=timeout)
    except httpx.HTTPError:
        return PairingOutcome(
            state="unavailable",
            repair_message="Could not reach the local daemon.",
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    if not response.is_success:
        try:
            payload = response.json()
        except ValueError:
            payload = {}
        error_type = payload.get("error_type") if isinstance(payload, dict) else None
        if not error_type and isinstance(payload, dict):
            error_type = payload.get("detail")
        return PairingOutcome(
            state="repair",
            repair_message=(
                f"Daemon rejected the pairing request ({response.status_code})."
            ),
            repair_hint=_repair_hint(error_type),
        )

    try:
        projection = response.json()
    except ValueError:
        projection = None
    if not isinstance(projection, dict):
        return PairingOutcome(
            state="unavailable",
            repair_message="Could not reach the local daemon.",
            repair_hint=DEFAULT_DAEMON_REPAIR_HINT,
        )

    return PairingOutcome(state="paired", projection=projection)
