"""Evidence bundle assembly, schema validation, and round-trip tests."""

from __future__ import annotations

import json
from pathlib import Path

from bicameral_cloud.evidence.bundle import EvidenceBundle, validate_bundle
from bicameral_cloud.evidence.outcomes import CollectionStatus
from tests.evidence.helpers import scenario_bundle

_SCHEMA_PATH = (
    Path(__file__).resolve().parents[2]
    / "docs"
    / "evidence"
    / "evidence-bundle.schema.json"
)


def test_hardened_bundle_validates() -> None:
    bundle = scenario_bundle()
    assert validate_bundle(bundle.to_json()) == []


def test_collected_items_carry_matching_digest() -> None:
    bundle = scenario_bundle()
    collected = [i for i in bundle.items if i.status == CollectionStatus.COLLECTED]
    assert collected, "expected some collected items"
    for item in collected:
        assert item.payload is not None
        assert item.digest is not None and item.digest.startswith("sha256:")


def test_tampered_digest_is_rejected() -> None:
    document = scenario_bundle().to_json()
    items = document["items"]
    assert isinstance(items, list)
    first = items[0]
    assert isinstance(first, dict)
    first["digest"] = "sha256:" + "0" * 64
    errors = validate_bundle(document)
    assert any("digest does not match" in error for error in errors)


def test_round_trip_from_json() -> None:
    bundle = scenario_bundle()
    restored = EvidenceBundle.from_json(bundle.to_json())
    assert restored.to_json() == bundle.to_json()


def test_schema_file_is_valid_json() -> None:
    schema = json.loads(_SCHEMA_PATH.read_text(encoding="utf-8"))
    assert schema["properties"]["schema_version"]["const"] == 1
