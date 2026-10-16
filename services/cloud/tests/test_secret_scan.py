"""Tests for the local secret scanner."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from types import ModuleType

SCRIPT = Path(__file__).parents[1] / "scripts" / "secret_scan.py"


def _load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("secret_scan", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_scan_detects_assigned_secret(tmp_path: Path) -> None:
    module = _load_script()
    module.ROOT = tmp_path
    candidate = tmp_path / "candidate.py"
    value = "production-" + "value-1234"
    candidate.write_text(f'api_key = "{value}"\n', encoding="utf-8")
    findings = module._scan_file(candidate)
    assert len(findings) == 1
    assert "possible assigned secret" in findings[0]


def test_scan_allows_placeholder_assignment(tmp_path: Path) -> None:
    module = _load_script()
    module.ROOT = tmp_path
    candidate = tmp_path / "candidate.py"
    candidate.write_text('api_key = "example-value"\n', encoding="utf-8")
    assert module._scan_file(candidate) == []
