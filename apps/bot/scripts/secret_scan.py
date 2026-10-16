#!/usr/bin/env python3
"""Small dependency-free secret scanner for CI and pre-commit.

This is intentionally conservative: it catches common committed credential
shapes without requiring network access or a third-party scanner.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

SKIP_DIRS = {
    ".git",
    ".qor",
    ".agent",
    ".agents",
    ".claude",
    ".failsafe",
    ".idea",
    ".vscode",
    ".cursor",
    ".windsurf",
    "target",
    "node_modules",
    "dist",
}

SKIP_SUFFIXES = {
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".webp",
    ".ico",
    ".pdf",
    ".lock",
}

# Files that legitimately contain secret-shaped strings and must not be flagged.
# Kept as an explicit, auditable allowlist (not a broad directory skip) so a real
# secret committed elsewhere is still caught.
ALLOWLIST_FILES = {
    # Test fixtures that deliberately embed invalid/sample secrets to exercise the
    # gateway sensitive-data HARD reject gate (filenames are prefixed "invalid-").
    "protocol/schemas/v2/fixtures/invalid-external-ingest-sensitive-data.json",
    "protocol/schemas/v2/fixtures/invalid-external-ingest-private-key.json",
    # Provider acquisition invalid fixtures that document forbidden secret-bearing
    # fields (prefixed "invalid-provider-*-secret-fields").
    "protocol/schemas/v2/fixtures/invalid-provider-descriptor-secret-fields.json",
    "protocol/schemas/v2/fixtures/invalid-provider-item-secret-fields.json",
    # The sensitive-pattern detector's own source + unit tests, which necessarily
    # contain example credentials (AWS docs example key, sample PAT, PEM markers).
    "crates/bicameral-gateway/src/sensitive_patterns.rs",
    # Hosted-bridge renderable ledger leakage test (#705): deliberately seeds a
    # decision/title/body with a sample AWS docs key and PAT to prove they are
    # redacted or dropped before crossing the network origin (never leaked).
    "crates/bicameral-gateway/tests/hosted_bridge_e2e_conformance.rs",
}

PLACEHOLDERS = {
    "changeme",
    "change-me",
    "example",
    "placeholder",
    "redacted",
    "dummy",
    "test",
    "local",
}

PATTERNS = [
    ("private key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("aws access key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("github token", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{36,}\b")),
    ("slack token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b")),
    (
        "assigned secret",
        re.compile(
            r"(?i)\b(password|passwd|secret|token|api[_-]?key|client[_-]?secret)\b"
            r"\s*[:=]\s*['\"]([^'\"]{8,})['\"]"
        ),
    ),
]


def is_skipped(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    if relative.as_posix() in ALLOWLIST_FILES:
        return True
    parts = set(relative.parts)
    return bool(parts & SKIP_DIRS) or path.suffix.lower() in SKIP_SUFFIXES


def is_placeholder(value: str) -> bool:
    normalized = re.sub(r"[^a-z0-9-]", "", value.lower())
    return any(marker in normalized for marker in PLACEHOLDERS)


def scan_file(path: Path) -> list[str]:
    findings: list[str] = []
    try:
        content = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return findings

    for line_no, line in enumerate(content.splitlines(), start=1):
        for label, pattern in PATTERNS:
            match = pattern.search(line)
            if not match:
                continue
            if label == "assigned secret" and is_placeholder(match.group(2)):
                continue
            findings.append(f"{path.relative_to(ROOT)}:{line_no}: possible {label}")
    return findings


def main() -> int:
    findings: list[str] = []
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
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
