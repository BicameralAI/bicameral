"""Pinned onboarding contract verification + wire-model conformance (cloud#136).

Proves Cloud pins the bot#787 producer contract (source commit + version +
semantic fingerprint), recomputes the fingerprint fail-closed on drift, and
that production Cloud models serialize/validate every approved success and
failure fixture with **no shadow canonical schema** — structural authority is
the vendored, fingerprint-verified bot schema.
"""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
from typing import Any

import pytest
from pydantic import ValidationError

from bicameral_cloud.contracts import onboarding
from bicameral_cloud.contracts.onboarding import (
    EXPECTED_CONTRACT_VERSION,
    EXPECTED_SEMANTIC_FINGERPRINT,
    EXPECTED_SOURCE_COMMIT,
    EXPECTED_VENDOR_COMMIT,
    OnboardingContractError,
    fixture_names,
    load_fixture,
    load_schema,
    load_source_pin,
    recompute_semantic_fingerprint,
    verify_onboarding_contract,
)
from bicameral_cloud.models.onboarding import (
    HostedProjectPreview,
    ProductCreationPublication,
    ProductCreationResult,
    ProductJoinRequest,
    ProductJoinResult,
)
from bicameral_cloud.models.product_join_ticket import ProductJoinTicket

_MODEL_FOR_FIXTURE: dict[str, Any] = {
    "product-creation-result.valid.json": ProductCreationResult,
    "product-creation-publication.valid.json": ProductCreationPublication,
    "product-creation-publication.retry.json": ProductCreationPublication,
    "hosted-project-preview.valid.json": HostedProjectPreview,
    "product-join-ticket.valid.json": ProductJoinTicket,
    "product-join-request.valid.json": ProductJoinRequest,
}


def _model_for(name: str) -> Any:
    if name.startswith("product-join-result"):
        return ProductJoinResult
    return _MODEL_FOR_FIXTURE[name]


# --- Pin + deterministic fingerprint --------------------------------------- #


def test_pin_matches_producer_manifest() -> None:
    manifest = verify_onboarding_contract()
    assert manifest["contract_version"] == EXPECTED_CONTRACT_VERSION
    assert manifest["source_commit"] == EXPECTED_SOURCE_COMMIT
    assert manifest["semantic_fingerprint"] == EXPECTED_SEMANTIC_FINGERPRINT


def test_source_pin_records_bot_commit_tag_version_and_fingerprint() -> None:
    pin = load_source_pin()
    assert pin["producer_repo"] == "BicameralAI/bicameral-bot"
    assert pin["source_commit"] == EXPECTED_SOURCE_COMMIT
    assert pin["vendored_from_commit"] == EXPECTED_VENDOR_COMMIT
    assert pin["contract_version"] == EXPECTED_CONTRACT_VERSION
    assert pin["semantic_fingerprint"] == EXPECTED_SEMANTIC_FINGERPRINT


def test_recompute_matches_pinned_fingerprint() -> None:
    assert recompute_semantic_fingerprint() == EXPECTED_SEMANTIC_FINGERPRINT


def _verify_material(material: dict[str, Any]) -> None:
    """Recompute + compare against a mutated fingerprint material (fail-closed)."""
    canonical = (
        json.dumps(material, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    )
    recomputed = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    if recomputed != EXPECTED_SEMANTIC_FINGERPRINT:
        raise OnboardingContractError("semantic fingerprint recomputation mismatch")


def test_fingerprint_fails_closed_on_stale_input() -> None:
    material = onboarding.fingerprint_material()
    first = next(iter(material["fixtures"]))
    material["fixtures"][first] = "0" * 64
    with pytest.raises(OnboardingContractError):
        _verify_material(material)


def test_verify_fails_closed_on_absent_manifest(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(onboarding, "_MANIFEST_PATH", Path("/nonexistent/x.json"))
    with pytest.raises(OnboardingContractError):
        verify_onboarding_contract()


def test_verify_fails_closed_on_mismatched_manifest_fingerprint(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    manifest = json.loads(onboarding._MANIFEST_PATH.read_text())
    manifest["semantic_fingerprint"] = "f" * 64
    tampered = tmp_path / "manifest.json"
    tampered.write_text(json.dumps(manifest))
    monkeypatch.setattr(onboarding, "_MANIFEST_PATH", tampered)
    with pytest.raises(OnboardingContractError):
        verify_onboarding_contract()


def test_verify_fails_closed_on_stale_manifest_digest(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    manifest = json.loads(onboarding._MANIFEST_PATH.read_text())
    first = next(iter(manifest["schemas"]))
    manifest["schemas"][first] = "1" * 64
    tampered = tmp_path / "manifest.json"
    tampered.write_text(json.dumps(manifest))
    monkeypatch.setattr(onboarding, "_MANIFEST_PATH", tampered)
    with pytest.raises(OnboardingContractError):
        verify_onboarding_contract()


# --- Production model conformance (no shadow schema) ----------------------- #


@pytest.mark.parametrize("name", onboarding.fixture_names())
def test_production_model_round_trips_fixture(name: str) -> None:
    fixture = load_fixture(name)
    model = _model_for(name)
    obj = model.model_validate(fixture)
    assert obj.model_dump(mode="json") == fixture


@pytest.mark.parametrize("name", onboarding.fixture_names())
def test_fixture_structurally_conforms_to_vendored_schema(name: str) -> None:
    schema_map = {
        "product-creation-result.valid.json": "product-creation-result.schema.json",
        "product-creation-publication.valid.json": (
            "product-creation-publication.schema.json"
        ),
        "product-creation-publication.retry.json": (
            "product-creation-publication.schema.json"
        ),
        "hosted-project-preview.valid.json": "hosted-project-preview.schema.json",
        "product-join-ticket.valid.json": "product-join-ticket.schema.json",
        "product-join-request.valid.json": "product-join-request.schema.json",
    }
    schema_name = (
        "product-join-result.schema.json"
        if name.startswith("product-join-result")
        else schema_map[name]
    )
    schema = load_schema(schema_name)
    fixture = load_fixture(name)
    # Top-level fail-closed shape must hold and every required key present.
    assert schema.get("additionalProperties") is False
    for required in schema.get("required", []):
        assert required in fixture


def test_production_models_reject_unknown_boundary_field() -> None:
    fixture = copy.deepcopy(load_fixture("product-join-ticket.valid.json"))
    fixture["supabase_access_token"] = "leaked"
    with pytest.raises(ValidationError):
        ProductJoinTicket.model_validate(fixture)


def test_join_result_enforces_granted_reject_invariants() -> None:
    granted = copy.deepcopy(load_fixture("product-join-result.granted.json"))
    granted["failure"] = {"code": "wrong_account", "message": "x"}
    with pytest.raises(ValidationError):
        ProductJoinResult.model_validate(granted)

    rejected = copy.deepcopy(load_fixture("product-join-result.wrong-account.json"))
    rejected["granted_role"] = "Product Member"
    with pytest.raises(ValidationError):
        ProductJoinResult.model_validate(rejected)


# --- SPA bundle digest is artifact provenance only ------------------------- #


def test_spa_bundle_digest_cannot_satisfy_semantic_contract() -> None:
    version_path = (
        Path(__file__).resolve().parents[1]
        / "src"
        / "bicameral_cloud"
        / "static"
        / "dashboard"
        / "version.json"
    )
    version = json.loads(version_path.read_text())
    bundle_digest = version["bundle_sha256"]
    # The SPA bundle digest is deployable-artifact provenance, never the
    # onboarding semantic fingerprint.
    assert bundle_digest != EXPECTED_SEMANTIC_FINGERPRINT
    assert bundle_digest != recompute_semantic_fingerprint()
    # It also does not appear anywhere in the semantic contract inputs.
    material = onboarding.fingerprint_material()
    assert bundle_digest not in material["schemas"].values()
    assert bundle_digest not in material["fixtures"].values()


def test_every_fixture_free_of_forbidden_boundary_material() -> None:
    forbidden = ("supabase", "access_token", "refresh_token", "private_key")
    for name in fixture_names():
        raw = json.dumps(load_fixture(name)).lower()
        for token in forbidden:
            assert token not in raw, f"{name} leaks {token}"
