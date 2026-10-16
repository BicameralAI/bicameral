"""Pinned Bot-owned Product onboarding semantic contract (cloud#136).

The ``bicameral-bot`` repository is the canonical source of the Hosted Product
onboarding semantic contract (bot#787). Cloud vendors the exact producer bytes
— ``manifest.json``, the versioned ``schemas/``, and the approved ``fixtures/``
— and pins the **contract version + canonical semantic fingerprint**, never a
UI/SPA bundle digest.

Verification is deterministic and fail-closed: the semantic fingerprint is
recomputed from the vendored canonical inputs using the producer's algorithm
and compared against the pinned value and the vendored manifest. Any absent,
stale, or mismatched metadata raises :class:`OnboardingContractError` with a
diagnosable message so a drifted pin can never pass silently.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, cast

_DIR = Path(__file__).parent
_MANIFEST_PATH = _DIR / "manifest.json"
_SOURCE_PATH = _DIR / "source.json"
_SCHEMA_DIR = _DIR / "schemas"
_FIXTURE_DIR = _DIR / "fixtures"

# --- Authoritative pins (fail-closed) ------------------------------------- #
# These are the exact bot#787 producer values Cloud consumes. They are the
# source of truth for the fail-closed check; the vendored manifest and the
# recomputed fingerprint must both agree with them.
EXPECTED_CONTRACT_ID = "domain-product-onboarding-contract"
EXPECTED_CONTRACT_VERSION = "1.0.0"
EXPECTED_SEMANTIC_FINGERPRINT = (
    "236243a322fcc945ccbff71cdb00271b7d5d4c263a6bcb800bb3c47ba690397f"
)
# Producer commit recorded inside the bot manifest (canonical source commit).
EXPECTED_SOURCE_COMMIT = "92ff48fad81cd5401d72a9c16ba66041fd60a119"
# Exact bot `dev` commit Cloud vendored the bytes from (bot#787 via PR #791).
EXPECTED_VENDOR_COMMIT = "ff0b015acfc4ce868b61b083f11f062e8c3c2df5"


class OnboardingContractError(RuntimeError):
    """Raised when the pinned onboarding contract is absent/stale/mismatched."""


def _sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _canonical(data: Any) -> str:
    # Byte-for-byte identical to the bot producer's canonicalization so the
    # recomputed fingerprint matches without importing bot code.
    return json.dumps(data, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def _digest_map(directory: Path, suffix: str) -> dict[str, str]:
    return {
        p.name: _sha256_hex(p.read_bytes())
        for p in sorted(directory.glob(f"*{suffix}"))
    }


def fingerprint_material() -> dict[str, Any]:
    """Canonical semantic inputs the fingerprint is derived from.

    Deterministic and source-commit independent: contract identity/version
    plus the content digests of every vendored schema and fixture.
    """
    return {
        "contract": EXPECTED_CONTRACT_ID,
        "contract_version": EXPECTED_CONTRACT_VERSION,
        "schemas": _digest_map(_SCHEMA_DIR, ".schema.json"),
        "fixtures": _digest_map(_FIXTURE_DIR, ".json"),
    }


def recompute_semantic_fingerprint() -> str:
    """Recompute the semantic fingerprint from the vendored canonical inputs."""
    return _sha256_hex(_canonical(fingerprint_material()).encode("utf-8"))


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise OnboardingContractError(message)


def verify_onboarding_contract() -> dict[str, Any]:
    """Deterministically verify the pinned contract; fail closed on any drift.

    Returns the validated manifest on success. Raises
    :class:`OnboardingContractError` with a diagnosable message otherwise.
    """
    _require(
        _MANIFEST_PATH.is_file(),
        "onboarding contract manifest is absent — the pinned bot#787 "
        f"manifest must exist at {_MANIFEST_PATH.name}",
    )
    try:
        manifest = cast(dict[str, Any], json.loads(_MANIFEST_PATH.read_text()))
    except json.JSONDecodeError as exc:
        raise OnboardingContractError(
            f"onboarding contract manifest is not valid JSON: {exc}"
        ) from exc

    _require(
        manifest.get("contract") == EXPECTED_CONTRACT_ID,
        "onboarding contract identity mismatch — expected "
        f"{EXPECTED_CONTRACT_ID!r}, got {manifest.get('contract')!r}",
    )
    _require(
        manifest.get("contract_version") == EXPECTED_CONTRACT_VERSION,
        "onboarding contract version mismatch — expected "
        f"{EXPECTED_CONTRACT_VERSION!r}, got {manifest.get('contract_version')!r}",
    )
    _require(
        manifest.get("source_commit") == EXPECTED_SOURCE_COMMIT,
        "onboarding contract source commit mismatch — expected "
        f"{EXPECTED_SOURCE_COMMIT!r}, got {manifest.get('source_commit')!r}",
    )

    pinned_fp = manifest.get("semantic_fingerprint")
    _require(
        bool(pinned_fp),
        "onboarding contract manifest is missing a semantic_fingerprint",
    )
    _require(
        pinned_fp == EXPECTED_SEMANTIC_FINGERPRINT,
        "onboarding contract semantic fingerprint mismatch — expected "
        f"{EXPECTED_SEMANTIC_FINGERPRINT!r}, got {pinned_fp!r}",
    )

    # Per-file digests: the vendored bytes must match the manifest exactly, and
    # the manifest must not record files that are absent (or vice versa).
    for kind, directory, suffix in (
        ("schemas", _SCHEMA_DIR, ".schema.json"),
        ("fixtures", _FIXTURE_DIR, ".json"),
    ):
        actual = _digest_map(directory, suffix)
        recorded = cast(dict[str, str], manifest.get(kind, {}))
        for name, digest in actual.items():
            _require(
                recorded.get(name) == digest,
                f"onboarding contract {kind[:-1]} drift for {name} — recorded "
                f"{recorded.get(name)!r}, actual {digest!r}",
            )
        for name in recorded:
            _require(
                name in actual,
                f"onboarding contract manifest records stale {kind[:-1]} "
                f"{name!r} that is no longer vendored",
            )

    recomputed = recompute_semantic_fingerprint()
    _require(
        recomputed == EXPECTED_SEMANTIC_FINGERPRINT,
        "onboarding contract semantic fingerprint recomputation mismatch — "
        f"expected {EXPECTED_SEMANTIC_FINGERPRINT!r}, recomputed {recomputed!r}",
    )
    return manifest


def load_onboarding_manifest() -> dict[str, Any]:
    """Return the verified onboarding contract manifest (fail-closed)."""
    return verify_onboarding_contract()


def load_source_pin() -> dict[str, Any]:
    """Return the vendored provenance pin (bot source commit/tag + version)."""
    return cast(dict[str, Any], json.loads(_SOURCE_PATH.read_text()))


def schema_path(name: str) -> Path:
    """Return the path to a vendored, verified schema file."""
    verify_onboarding_contract()
    path = _SCHEMA_DIR / name
    if not path.is_file():
        raise OnboardingContractError(f"unknown onboarding schema {name!r}")
    return path


def load_schema(name: str) -> dict[str, Any]:
    """Load a vendored schema after verifying the pinned contract."""
    return cast(dict[str, Any], json.loads(schema_path(name).read_text()))


def fixture_path(name: str) -> Path:
    """Return the path to a vendored, verified fixture file."""
    verify_onboarding_contract()
    path = _FIXTURE_DIR / name
    if not path.is_file():
        raise OnboardingContractError(f"unknown onboarding fixture {name!r}")
    return path


def load_fixture(name: str) -> Any:
    """Load a vendored fixture after verifying the pinned contract."""
    return json.loads(fixture_path(name).read_text())


def fixture_names() -> list[str]:
    """Sorted names of every vendored onboarding fixture."""
    verify_onboarding_contract()
    return sorted(p.name for p in _FIXTURE_DIR.glob("*.json"))


# Fail closed at import time: a drifted or absent pin must not load silently.
ONBOARDING_CONTRACT_MANIFEST = verify_onboarding_contract()
ONBOARDING_CONTRACT_VERSION = EXPECTED_CONTRACT_VERSION
ONBOARDING_SEMANTIC_FINGERPRINT = EXPECTED_SEMANTIC_FINGERPRINT
ONBOARDING_SOURCE_COMMIT = EXPECTED_SOURCE_COMMIT

__all__ = [
    "EXPECTED_CONTRACT_ID",
    "EXPECTED_CONTRACT_VERSION",
    "EXPECTED_SEMANTIC_FINGERPRINT",
    "EXPECTED_SOURCE_COMMIT",
    "EXPECTED_VENDOR_COMMIT",
    "ONBOARDING_CONTRACT_MANIFEST",
    "ONBOARDING_CONTRACT_VERSION",
    "ONBOARDING_SEMANTIC_FINGERPRINT",
    "ONBOARDING_SOURCE_COMMIT",
    "OnboardingContractError",
    "fingerprint_material",
    "fixture_names",
    "fixture_path",
    "load_fixture",
    "load_onboarding_manifest",
    "load_schema",
    "load_source_pin",
    "recompute_semantic_fingerprint",
    "schema_path",
    "verify_onboarding_contract",
]
