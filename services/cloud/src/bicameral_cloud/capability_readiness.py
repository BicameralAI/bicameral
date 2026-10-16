"""Declared configuration requirements per capability and their readiness.

Readiness is based on constructing the real capability from current settings,
not merely checking whether environment variables are non-empty. This keeps a
malformed key, invalid TTL, or unusable verification set from masquerading as
ready configuration.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

from bicameral_cloud.config import CloudSettings
from bicameral_cloud.product_creation_proof import (
    Ed25519ProductCreationProofVerifier,
    ProductCreationProofVerifierUnavailable,
)
from bicameral_cloud.product_join_ticket import (
    Ed25519ProductJoinTicketSigner,
    ProductJoinSignerUnavailable,
)


@dataclass(frozen=True)
class CapabilityRequirement:
    """Configuration contract and real constructor probe for one capability."""

    name: str
    env_vars: tuple[str, ...]
    probe: Callable[[CloudSettings], object]


REQUIRED_CAPABILITIES: tuple[CapabilityRequirement, ...] = (
    CapabilityRequirement(
        name="product_join",
        env_vars=(
            "PRODUCT_JOIN_TICKET_ISSUER",
            "PRODUCT_JOIN_SIGNING_KEY_ID",
            "PRODUCT_JOIN_SIGNING_PRIVATE_KEY_B64",
        ),
        probe=Ed25519ProductJoinTicketSigner.from_settings,
    ),
    CapabilityRequirement(
        name="product_creation_proof",
        env_vars=("PRODUCT_CREATION_PROOF_VERIFICATION_KEYS_JSON",),
        probe=Ed25519ProductCreationProofVerifier.from_settings,
    ),
)

_UNAVAILABLE = (
    ProductJoinSignerUnavailable,
    ProductCreationProofVerifierUnavailable,
)


def capability_status(settings: CloudSettings) -> dict[str, bool]:
    """Return boolean constructability per declared capability.

    Only typed configuration-unavailable failures map to ``False``. Unexpected
    exceptions propagate because hiding a defect behind a generic not-ready
    result would make the health surface less trustworthy, not more.
    """

    status: dict[str, bool] = {}
    for requirement in REQUIRED_CAPABILITIES:
        try:
            requirement.probe(settings)
        except _UNAVAILABLE:
            status[requirement.name] = False
        else:
            status[requirement.name] = True
    return status
