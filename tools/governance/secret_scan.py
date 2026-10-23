#!/usr/bin/env python3
"""Repository-wide secret scan for the public monorepo.

High-confidence credential shapes only. The assigned-secret heuristic stays in
the component scanners, which already allowlist their own fixtures. Known
example strings are ignored by exact value. A private-key header counts only
when key material follows it.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

SKIP_DIRS = {
    ".agent",
    ".agents",
    ".claude",
    ".cursor",
    ".failsafe",
    ".git",
    ".idea",
    ".pytest_cache",
    ".qor",
    ".tmp",
    ".venv",
    ".vscode",
    ".windsurf",
    "__pycache__",
    "dist",
    "node_modules",
    "target",
    "test-artifacts",
}

SKIP_SUFFIXES = {
    ".gif",
    ".ico",
    ".jpeg",
    ".jpg",
    ".lock",
    ".pdf",
    ".png",
    ".webp",
}

# Files that intentionally contain example credentials.
ALLOWLIST_FILES = {
    "apps/bot/protocol/schemas/v2/fixtures/invalid-external-ingest-sensitive-data.json",
    "apps/bot/protocol/schemas/v2/fixtures/invalid-external-ingest-private-key.json",
    "apps/bot/protocol/schemas/v2/fixtures/invalid-provider-descriptor-secret-fields.json",
    "apps/bot/protocol/schemas/v2/fixtures/invalid-provider-item-secret-fields.json",
    "apps/bot/crates/bicameral-gateway/src/sensitive_patterns.rs",
    "apps/bot/crates/bicameral-gateway/tests/hosted_bridge_e2e_conformance.rs",
}

# Exact non-credentials already in the tree. A different value on the same line still fails.
PLACEHOLDER_SECRETS = frozenset(
    {
        "AKIAIOSFODNN7EXAMPLE",  # AWS documentation example
        "AKIAABCDEFGHIJKLMNOP",  # sequential Integrations fixture
        "ghp_0123456789abcdefghijklmnopqrstuvwxyz",  # sequential Integrations fixture
    }
)

TOKEN_PATTERNS = [
    ("aws access key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("github token", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{36,}\b")),
    ("slack token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b")),
]
# A header alone is not a key. Require key material after it, including a literal "\n"
# escape so a one-line source string cannot hide a body.
PRIVATE_KEY_BODY = re.compile(
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----(?:\\n|\s)*[A-Za-z0-9+/=]{40,}"
)


def is_skipped(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    if relative.as_posix() in ALLOWLIST_FILES:
        return True
    return bool(set(relative.parts) & SKIP_DIRS) or path.suffix.lower() in SKIP_SUFFIXES


def findings_in_text(text: str) -> list[tuple[int, str]]:
    found: list[tuple[int, str]] = []
    for line_no, line in enumerate(text.splitlines(), start=1):
        for label, pattern in TOKEN_PATTERNS:
            for match in pattern.finditer(line):
                if match.group(0) in PLACEHOLDER_SECRETS:
                    continue
                found.append((line_no, label))
    for match in PRIVATE_KEY_BODY.finditer(text):
        line_no = text.count("\n", 0, match.start()) + 1
        found.append((line_no, "private key"))
    return found


def scan_file(path: Path) -> list[str]:
    try:
        content = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return []
    relative = path.relative_to(ROOT)
    return [
        f"{relative}:{line_no}: possible {label}"
        for line_no, label in findings_in_text(content)
    ]


def main() -> int:
    findings: list[str] = []
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [name for name in dirnames if name not in SKIP_DIRS]
        for filename in filenames:
            path = Path(dirpath) / filename
            if not is_skipped(path):
                findings.extend(scan_file(path))
    if findings:
        print("Secret scan failed:")
        for finding in findings:
            print(f"  {finding}")
        return 1
    print("Secret scan passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
