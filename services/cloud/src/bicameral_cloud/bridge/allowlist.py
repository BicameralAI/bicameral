"""Bridge routing allowlist and default-deny logic (ADR-0007, issue #20).

Cloud bridge routing is hard allowlist and default-deny. Alpha may route only
explicitly allowed dashboard-intent command families. Authority-bearing commands
must never be routed through cloud.
"""

from __future__ import annotations

# Explicitly allowed dashboard-intent command families (ADR-0007).
ALLOWED_COMMAND_FAMILIES: frozenset[str] = frozenset(
    {
        "projection.read",
        "readiness.refresh",
        "workspace.discover",
        "preview.request",
        "receipt.fetch",
        "session.close",
    }
)

# Authority-bearing command families that must NEVER be routed (ADR-0007).
# These are rejected before any daemon forwarding attempt.
FORBIDDEN_COMMAND_FAMILIES: frozenset[str] = frozenset(
    {
        "signoff.approve",
        "signoff.reject",
        "signoff.create",
        "ledger.write",
        "ledger.accept",
        "ledger.materialize",
        "governance.enforce",
        "compliance.resolve",
        "compliance.enforce",
        "source_binding.create",
        "source_binding.mutate",
        "source_binding.delete",
        "decision.create",
        "decision.accept",
        "decision.supersede",
        "workspace.bind",
        "authority.grant",
        "authority.revoke",
    }
)

MAX_PAYLOAD_BYTES = 64 * 1024  # 64 KiB alpha limit


def classify_command(command_family: str) -> str:
    """Classify a command family as allowed, forbidden, or unknown.

    Returns one of: 'allowed', 'forbidden', 'unknown'.
    """
    if command_family in ALLOWED_COMMAND_FAMILIES:
        return "allowed"
    if command_family in FORBIDDEN_COMMAND_FAMILIES:
        return "forbidden"
    return "unknown"
