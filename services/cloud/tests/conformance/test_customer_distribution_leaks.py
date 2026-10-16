from __future__ import annotations

from collections.abc import Callable

from fastapi.testclient import TestClient

from tests.conformance.test_cloud_dashboard_daemon_pairing import (
    _contains_forbidden_token,
    cloud_auth_client,
)

# These are internal development-governance/evidence classes, not customer data.
# Public trust language such as a generic "SOC 2" statement is intentionally not
# forbidden; raw control/evidence references are.
FORBIDDEN_INTERNAL_OUTPUT_TOKENS = {
    "BicameralAI/bicameral-factory",
    "github.com/BicameralAI/bicameral-factory",
    ".bicameral/factory-attestations",
    "factory-context.local.json",
    "factory-run-manifest",
    "factory-worker-receipt",
    "roadmap reconciliation",
    "risk register",
    "review queue",
    "soc2-application-controls.md",
}


def test_factory_and_soc2_internal_evidence_never_leaks_to_browser_surfaces(
    cloud_auth_client: tuple[TestClient, Callable[[str], str]],
) -> None:
    client, mint_jwt = cloud_auth_client
    token = mint_jwt("operator@bicameral-ai.com")
    auth_headers = {"Authorization": f"Bearer {token}"}

    surfaces: list[object] = [
        client.get("/connect").text,
        client.get("/api/v1/auth/config").json(),
        client.get("/api/v1/auth/session", headers=auth_headers).json(),
    ]

    assert not any(
        _contains_forbidden_token(surface, FORBIDDEN_INTERNAL_OUTPUT_TOKENS)
        for surface in surfaces
    )
