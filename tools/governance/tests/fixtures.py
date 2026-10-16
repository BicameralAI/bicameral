"""Isolated fixture builders shared by the governance tests.

Every fixture lives in a fresh temporary directory initialised as its own Git
repository. Nothing here reads from or writes to the real repository except
``REPO_ROOT`` reads in tests that explicitly copy real content into a fixture.
"""

from __future__ import annotations

import copy
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
OWNERS = ["@jinhongkuan", "@Knapp-Kevin"]

BASE_MANIFEST = {
    "version": 1,
    "default_owners": OWNERS,
    "canonical": {"architecture_records": "docs/architecture/records"},
    "root_files": [
        {"path": "README.md", "purpose": "Overview.", "required": True},
        {"path": "LICENSE", "purpose": "License.", "required": True, "owners": OWNERS},
        {"path": ".gitignore", "purpose": "Ignore rules.", "required": True},
    ],
    "directories": [
        {"path": ".github", "purpose": "GitHub.", "children": "open", "readme": False},
        {"path": "docs", "purpose": "Docs.", "children": "declared"},
        {"path": "docs/architecture", "purpose": "Architecture.", "children": "declared",
         "owners": OWNERS},
        {"path": "docs/architecture/records", "purpose": "Records.", "children": "open"},
        {"path": "governance", "purpose": "Manifest.", "children": "declared"},
        {"path": "tools", "purpose": "Tools.", "children": "declared"},
        {"path": "tools/governance", "purpose": "Validators.", "children": "open"},
        {"path": "packages", "purpose": "Packages.", "children": "declared"},
        {"path": "packages/mcp", "purpose": "MCP.", "children": "open", "status": "reserved",
         "source": "BicameralAI/bicameral-mcp"},
    ],
}

BASE_FILES = {
    "README.md": "# Fixture\n",
    "LICENSE": "license text\n",
    ".gitignore": "build/\n__pycache__/\n",
    "docs/README.md": "docs\n",
    "docs/architecture/README.md": "architecture\n",
    "docs/architecture/records/README.md": "records\n",
    "governance/README.md": "governance\n",
    "tools/README.md": "tools\n",
    "tools/governance/README.md": "validators\n",
    "packages/README.md": "packages\n",
    "packages/mcp/README.md": "Reserved for BicameralAI/bicameral-mcp. No code imported.\n",
}


def git_available() -> bool:
    return shutil.which("git") is not None


def init_git(root: Path) -> None:
    env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1")
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)


class TempRepoTestCase(unittest.TestCase):
    """Base class providing a throwaway Git repository per test."""

    def setUp(self) -> None:
        if not git_available():
            self.skipTest("git is required")
        self._tmp = tempfile.TemporaryDirectory(prefix="governance-test-")
        self.root = Path(self._tmp.name).resolve() / "repo"
        self.root.mkdir()
        init_git(self.root)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def write(self, rel: str, content: str = "x\n") -> Path:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(content)
        return path

    def read(self, rel: str) -> str:
        return (self.root / rel).read_text(encoding="utf-8")

    def write_manifest(self, manifest: dict) -> None:
        self.write("governance/structure.json", json.dumps(manifest, indent=2) + "\n")

    def build_structure_fixture(self, manifest: dict | None = None) -> dict:
        manifest = copy.deepcopy(BASE_MANIFEST if manifest is None else manifest)
        for rel, content in BASE_FILES.items():
            self.write(rel, content)
        self.write_manifest(manifest)
        return manifest

    def snapshot(self) -> dict[str, bytes]:
        result = {}
        for dirpath, dirnames, filenames in os.walk(self.root):
            dirnames[:] = [d for d in dirnames if d != ".git"]
            for name in filenames:
                full = Path(dirpath) / name
                result[full.relative_to(self.root).as_posix()] = full.read_bytes()
        return result


def record_text(
    record_id: str,
    title: str = "A record",
    *,
    kind: str = "decision",
    status: str = "proposed",
    date: str = "2026-10-16",
    owners: list[str] | None = None,
    governs: list[str] | None = None,
    depends_on: list[str] | None = None,
    supersedes: list[str] | None = None,
    references: list[str] | None = None,
    body: str | None = None,
    extra_front_matter: str = "",
) -> str:
    def fmt(values: list[str] | None) -> str:
        return "[" + ", ".join(values or []) + "]"

    sections = {
        "decision": "## Context\n\nc\n\n## Decision\n\nd\n\n## Consequences\n\nq\n",
        "design": "## Context\n\nc\n\n## Design\n\nd\n",
        "contract": "## Scope\n\ns\n\n## Contract\n\nc\n",
        "reference": "Reference material.\n",
    }
    if body is None:
        body = sections.get(kind, sections["decision"])
    return (
        "---\n"
        f"id: {record_id}\n"
        f"title: {title}\n"
        f"kind: {kind}\n"
        f"status: {status}\n"
        f"date: {date}\n"
        f"owners: {fmt(owners if owners is not None else OWNERS)}\n"
        f"governs: {fmt(governs if governs is not None else ['topic-' + record_id[-4:]])}\n"
        f"depends_on: {fmt(depends_on)}\n"
        f"supersedes: {fmt(supersedes)}\n"
        f"references: {fmt(references)}\n"
        f"{extra_front_matter}"
        "---\n\n"
        f"# {record_id}: {title}\n\n"
        f"{body}"
    )


def record_path(record_id: str, slug: str = "record") -> str:
    return f"docs/architecture/records/{record_id}-{slug}.md"
