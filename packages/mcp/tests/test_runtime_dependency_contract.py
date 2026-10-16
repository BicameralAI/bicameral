"""Published runtime dependency bounds must preserve the supported MCP SDK API."""

from __future__ import annotations

import tomllib
from pathlib import Path

from packaging.requirements import Requirement
from packaging.version import Version

ROOT = Path(__file__).resolve().parents[1]


def test_published_package_accepts_supported_mcp_v1_and_rejects_breaking_v2() -> None:
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]
    requirement = next(
        Requirement(value) for value in project["dependencies"] if Requirement(value).name == "mcp"
    )

    assert Version("1.28.1") in requirement.specifier
    assert Version("2.0.0") not in requirement.specifier


def test_ci_input_uses_the_same_mcp_major_boundary_as_the_published_package() -> None:
    requirement_lines = (
        line.split(" #", 1)[0].strip()
        for line in (ROOT / "requirements-ci.in").read_text(encoding="utf-8").splitlines()
    )
    ci_requirements = [
        Requirement(line) for line in requirement_lines if line and not line.startswith("#")
    ]
    requirement = next(value for value in ci_requirements if value.name == "mcp")

    assert Version("1.28.1") in requirement.specifier
    assert Version("2.0.0") not in requirement.specifier
