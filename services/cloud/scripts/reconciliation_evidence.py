#!/usr/bin/env python3
"""Thin launcher for the reconciliation evidence tooling (cloud#141).

Prepares the owner-only live evidence export required by the Cloud #106 gate. Runs
and parses without live credentials, emitting typed ``environment_inconclusive``
outcomes when a CLI, credential, or piece of infrastructure is absent.

Usage:
    python3 scripts/reconciliation_evidence.py plan
    python3 scripts/reconciliation_evidence.py export --environment staging \\
        --output bundle.json
    python3 scripts/reconciliation_evidence.py compare bundle.json
    python3 scripts/reconciliation_evidence.py validate bundle.json
"""

from __future__ import annotations

import sys
from pathlib import Path

# Allow running as a bare script without installing the package.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bicameral_cloud.evidence.cli import main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(main())
