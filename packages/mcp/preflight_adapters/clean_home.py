"""Observed clean host-config provenance, separate from hook execution.

Like adapter consent, this is trusted local operational state, not a signature
against an operator who can rewrite their own configuration.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import replace
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from uuid import uuid4

from .state import MCP_STATE_DIRNAME, AdapterMetadata

WITNESS_FILE = "clean-home.json"


def _digest(value: Any) -> str:
    return (
        "sha256:"
        + hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def _home_identity(home: Path) -> str:
    if home.is_symlink() or not home.is_dir():
        raise ValueError("prepared host home must remain a real directory")
    stat = home.stat()
    return _digest([str(home.resolve()), stat.st_dev, stat.st_ino])


def _install_identity(metadata: AdapterMetadata) -> str:
    values = metadata.to_dict()
    # Operational updates may change these without changing the installed bytes.
    for key in ("updated_at", "enabled"):
        values.pop(key)
    return _digest(values)


def prepare_home(home: Path, host: str, artifact_generation: str) -> dict[str, Any]:
    """Create, never adopt or clear, a private new host configuration root."""
    if not artifact_generation:
        raise ValueError("prepare-host requires an exact artifact generation")
    home.mkdir(mode=0o700, parents=False, exist_ok=False)
    if any(home.iterdir()):
        raise ValueError("new host configuration root is not empty")
    witness = {
        "schema_version": 1,
        "method": "exclusive-empty-config-home",
        "preparation_id": uuid4().hex,
        "host": host,
        "artifact_generation": artifact_generation,
        "home_identity": _home_identity(home),
        "prepared_at": datetime.now(UTC).isoformat(),
    }
    directory = home / MCP_STATE_DIRNAME
    directory.mkdir(mode=0o700)
    with (directory / WITNESS_FILE).open("x", encoding="utf-8") as handle:
        json.dump(witness, handle, sort_keys=True)
    return witness


def clean_home_witness(
    home: Path,
    metadata: AdapterMetadata,
    *,
    bind: bool = False,
    previous: AdapterMetadata | None = None,
) -> dict[str, Any] | None:
    """Bind a consented install, or check the same home and exact install later."""
    path = home / MCP_STATE_DIRNAME / WITNESS_FILE
    try:
        witness = json.loads(path.read_text(encoding="utf-8"))
        if (
            not isinstance(witness, dict)
            or witness.get("schema_version") != 1
            or witness.get("method") != "exclusive-empty-config-home"
            or witness.get("host") != metadata.host
            or witness.get("artifact_generation") != metadata.artifact_generation
            or witness.get("home_identity") != _home_identity(home)
            or not metadata.installed
            or not metadata.consent
            or not metadata.consent.granted
            or datetime.fromisoformat(witness["prepared_at"])
            > datetime.fromisoformat(metadata.consent.granted_at)
        ):
            return None
        identity = _install_identity(metadata)
        renew_consent = False
        if bind and previous is not None and previous.consent is not None:
            same_consent_time = replace(
                metadata,
                consent=replace(metadata.consent, granted_at=previous.consent.granted_at),
            )
            renew_consent = (
                _install_identity(same_consent_time) == _install_identity(previous)
                and clean_home_witness(home, previous) is not None
            )
        # Same-binding consent renewal retains the home observation, not old receipts.
        if bind and ("install_identity" not in witness or renew_consent):
            witness["install_identity"] = identity
            temporary = path.with_suffix(".tmp")
            temporary.write_text(json.dumps(witness, sort_keys=True), encoding="utf-8")
            temporary.replace(path)
        return witness if witness.get("install_identity") == identity else None
    except (OSError, ValueError, KeyError, TypeError):
        return None
