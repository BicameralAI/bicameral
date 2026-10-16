#!/usr/bin/env python3
"""Dependency-free scanner for common committed credential shapes."""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

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

PLACEHOLDERS = {
    "change-me",
    "changeme",
    "dummy",
    "example",
    "join-ticket",
    "local",
    "placeholder",
    "redacted",
    "test",
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


def _is_skipped(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    return bool(set(relative.parts) & SKIP_DIRS) or (
        path.suffix.lower() in SKIP_SUFFIXES
    )


def _is_placeholder(value: str) -> bool:
    normalized = re.sub(r"[^a-z0-9-]", "", value.lower())
    return any(marker in normalized for marker in PLACEHOLDERS)


def _scan_file(path: Path) -> list[str]:
    findings: list[str] = []
    try:
        content = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return findings
    for line_number, line in enumerate(content.splitlines(), start=1):
        for label, pattern in PATTERNS:
            match = pattern.search(line)
            if match is None:
                continue
            if label == "assigned secret" and _is_placeholder(match.group(2)):
                continue
            findings.append(
                f"{path.relative_to(ROOT)}:{line_number}: possible {label}"
            )
    return findings


def main() -> int:
    findings: list[str] = []
    for directory, directory_names, filenames in os.walk(ROOT):
        directory_names[:] = [
            name for name in directory_names if name not in SKIP_DIRS
        ]
        for filename in filenames:
            path = Path(directory) / filename
            if not _is_skipped(path):
                findings.extend(_scan_file(path))
    if findings:
        print("Secret scan failed:")
        for finding in findings:
            print(f"  {finding}")
        return 1
    print("Secret scan passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
