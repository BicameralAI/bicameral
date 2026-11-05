#!/usr/bin/env python3
"""Validate authority-resolution fixture payloads against the spec-registry schemas.

Reuses `errors_for` from validate_spec_registry_schemas.py rather than carrying a
second schema implementation. Each fixture (bicameral#55) wraps registry payloads:
`source_snapshot`, `governed_domain`, `signoff` and `supersede_snapshot` per
snapshot, plus `check_records`. Absent `signoff` / `supersede_snapshot` are null.

Cross-object rules (digest recomputation, signoff binding, supersede source) are
checked by the Rust harness, not here.

Usage: python3 -I scripts/validate_authority_resolution_fixtures.py [FIXTURE_DIR]
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from validate_spec_registry_schemas import (  # noqa: E402
    ID_PREFIX,
    ROOT,
    errors_for,
    load_json,
    schema_store,
)

DEFAULT_DIR = ROOT / "protocol" / "authority-resolution" / "fixtures"

PER_SNAPSHOT = (
    ("source_snapshot", "source-snapshot"),
    ("governed_domain", "governed-domain"),
    ("signoff", "signoff"),
    ("supersede_snapshot", "supersede-snapshot"),
)


def fixture_errors(name: str, doc: Any, store: dict[str, dict]) -> list[str]:
    errors: list[str] = []

    def check(label: str, value: Any, schema_name: str) -> None:
        schema = store[f"{ID_PREFIX}{schema_name}.schema.json"]
        for found in errors_for(value, schema, store):
            errors.append(f"{name}: {label}: {found}")

    snapshots = doc.get("snapshots") if isinstance(doc, dict) else None
    if not isinstance(snapshots, list) or not snapshots:
        return [f"{name}: `snapshots` must be a non-empty list"]
    for snap in snapshots:
        source = snap.get("source_snapshot") if isinstance(snap, dict) else None
        addr = source.get("snapshot_addr", "?") if isinstance(source, dict) else "?"
        for key, schema_name in PER_SNAPSHOT:
            value = snap.get(key) if isinstance(snap, dict) else None
            if value is None:
                if key in ("source_snapshot", "governed_domain"):
                    errors.append(f"{name}: {addr}: `{key}` is required")
                continue
            check(f"{addr}.{key}", value, schema_name)
    for index, record in enumerate(doc.get("check_records", [])):
        check(f"check_records[{index}]", record, "check-record")
    return errors


def main(argv: list[str]) -> int:
    directory = Path(argv[1]) if len(argv) > 1 else DEFAULT_DIR
    files = sorted(directory.glob("*.json"))
    if not files:
        print(f"authority-resolution fixture check failed: no fixtures in {directory}")
        return 1
    store = schema_store()
    errors: list[str] = []
    for path in files:
        errors.extend(fixture_errors(path.stem, load_json(path), store))
    if errors:
        print(f"authority-resolution fixture check failed ({len(errors)})")
        for error in errors:
            print(f"- {error}")
        return 1
    print(f"authority-resolution fixture check passed ({len(files)} fixtures)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
