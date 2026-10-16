#!/usr/bin/env python3
"""Validate Bicameral Cloud's customer-distribution boundary.

This validator is deliberately stdlib-only. It proves three distinct facts:

1. the Docker build context is deny-by-default and contains only runtime build inputs;
2. the Dockerfile does not copy internal development/governance roots and removes
   build-only repository metadata before runtime; and
3. runtime/browser source and an exported final image contain no known Factory-only
   paths or references.

It does not treat CI or Factory evidence as product acceptance. The control is only a
release-artifact leakage boundary.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess  # nosec B404 - git is invoked with fixed arguments only
import sys
import tarfile
from dataclasses import asdict, dataclass
from pathlib import Path, PurePosixPath
from typing import Iterable

ROOT = Path(__file__).resolve().parents[1]
DOCKERIGNORE = ROOT / ".dockerignore"
DOCKERFILE = ROOT / "Dockerfile"
RUNTIME_SOURCE = ROOT / "src" / "bicameral_cloud"

REQUIRED_DOCKER_CONTEXT_RULES = {
    "*",
    "!Dockerfile",
    "!pyproject.toml",
    "!README.md",
    "!requirements.lock",
    "!src/",
    "!src/**",
}

ALLOWED_DOCKER_COPY_SOURCES = {
    "pyproject.toml",
    "README.md",
    "requirements.lock",
    "src",
}

BUILD_ONLY_RUNTIME_PATHS = {
    "app/README.md",
    "app/pyproject.toml",
    "app/requirements.lock",
    "app/src",
}

# Exact or highly specific internal references. Avoid generic words such as
# "factory" or "SOC 2", which may legitimately occur in dependencies or public
# trust metadata and would create meaningless false positives.
FORBIDDEN_TEXT_MARKERS = (
    "github.com/BicameralAI/bicameral-factory",
    "BicameralAI/bicameral-factory",
    ".bicameral/factory-context.local.json",
    ".bicameral/factory-attestations",
    ".bicameral/factory-skills",
    "../bicameral-factory",
    "test-artifacts/real-process-hosted-topology",
    "/home/runner/work/bicameral-cloud",
)

FORBIDDEN_RUNTIME_PATH_MARKERS = (
    ".bicameral/",
    ".github/",
    "docs/governance/",
    "factory-attestations/",
    "factory-context.local.json",
    "factory-run-manifest",
    "factory-worker-receipt",
    "test-artifacts/",
)

TEXT_SCAN_LIMIT = 3 * 1024 * 1024


@dataclass(frozen=True)
class Violation:
    surface: str
    subject: str
    rule: str


def _git_head() -> str | None:
    value = os.environ.get("GITHUB_SHA", "").strip()
    if re.fullmatch(r"[0-9a-f]{40}", value):
        return value
    result = subprocess.run(  # nosec B603 B607
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    candidate = result.stdout.strip()
    return candidate if re.fullmatch(r"[0-9a-f]{40}", candidate) else None


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _significant_lines(path: Path) -> list[str]:
    lines: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            lines.append(line)
    return lines


def validate_dockerignore() -> list[Violation]:
    violations: list[Violation] = []
    if not DOCKERIGNORE.exists():
        return [Violation("docker-context", ".dockerignore", "missing")]

    lines = _significant_lines(DOCKERIGNORE)
    if not lines or lines[0] != "*":
        violations.append(
            Violation(
                "docker-context",
                ".dockerignore",
                "build context must be deny-by-default with '*' as the first rule",
            )
        )

    missing = sorted(REQUIRED_DOCKER_CONTEXT_RULES - set(lines))
    for rule in missing:
        violations.append(
            Violation(
                "docker-context",
                ".dockerignore",
                f"required allowlist rule missing: {rule}",
            )
        )

    # No governance/evidence root may be explicitly re-included.
    for line in lines:
        if not line.startswith("!"):
            continue
        included = line[1:].lstrip("./")
        for marker in (
            ".bicameral",
            ".github",
            "docs",
            "tests",
            "scripts",
            "test-artifacts",
        ):
            if included == marker or included.startswith(marker + "/"):
                violations.append(
                    Violation(
                        "docker-context",
                        line,
                        "internal development/governance root must not be re-included",
                    )
                )
    return violations


def _docker_copy_sources(text: str) -> list[str]:
    sources: list[str] = []
    logical = text.replace("\\\n", " ")
    for raw in logical.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = re.match(r"^(?:COPY|ADD)\s+(.+)$", line, flags=re.IGNORECASE)
        if not match:
            continue
        rest = match.group(1).strip()
        if rest.startswith("["):
            try:
                values = json.loads(rest)
            except json.JSONDecodeError:
                sources.append("<invalid-json-copy>")
                continue
            if isinstance(values, list) and len(values) >= 2:
                sources.extend(str(value) for value in values[:-1])
            continue
        tokens = rest.split()
        while tokens and tokens[0].startswith("--"):
            tokens.pop(0)
        if len(tokens) >= 2:
            sources.extend(tokens[:-1])
    return sources


def validate_dockerfile() -> list[Violation]:
    violations: list[Violation] = []
    if not DOCKERFILE.exists():
        return [Violation("container-build", "Dockerfile", "missing")]

    text = DOCKERFILE.read_text(encoding="utf-8")
    for source in _docker_copy_sources(text):
        normalized = source.rstrip("/").lstrip("./") or "."
        if normalized not in ALLOWED_DOCKER_COPY_SOURCES:
            violations.append(
                Violation(
                    "container-build",
                    source,
                    "Docker COPY/ADD source is outside the customer build-input allowlist",
                )
            )

    cleanup_targets = (
        "/app/README.md",
        "/app/pyproject.toml",
        "/app/requirements.lock",
        "/app/src",
    )
    for target in cleanup_targets:
        if target not in text:
            violations.append(
                Violation(
                    "container-build",
                    target,
                    "build-only repository input is not explicitly removed from runtime image",
                )
            )
    return violations


def _decode_text(data: bytes) -> str | None:
    if len(data) > TEXT_SCAN_LIMIT or b"\x00" in data:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def _text_violations(surface: str, subject: str, data: bytes) -> list[Violation]:
    text = _decode_text(data)
    if text is None:
        return []
    violations: list[Violation] = []
    for marker in FORBIDDEN_TEXT_MARKERS:
        if marker in text:
            violations.append(Violation(surface, subject, f"forbidden reference: {marker}"))
    return violations


def validate_runtime_source(root: Path = RUNTIME_SOURCE) -> list[Violation]:
    violations: list[Violation] = []
    if not root.exists():
        return [Violation("runtime-source", str(root), "missing runtime source")]
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        try:
            relative = path.relative_to(ROOT).as_posix()
        except ValueError:
            relative = str(path)
        violations.extend(_text_violations("runtime-source", relative, path.read_bytes()))
    return violations


def _normalized_tar_member(name: str) -> str:
    value = name.replace("\\", "/").lstrip("./")
    parts = PurePosixPath(value).parts
    if any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"unsafe container member path: {name!r}")
    return "/".join(parts)


def _customer_runtime_member(member: str) -> bool:
    return member.startswith("app/") or "/site-packages/bicameral_cloud/" in f"/{member}"


def validate_container_tar(path: Path) -> list[Violation]:
    violations: list[Violation] = []
    if not path.exists():
        return [Violation("container-image", str(path), "container export missing")]

    try:
        archive = tarfile.open(path, mode="r:*")
    except (tarfile.TarError, OSError) as exc:
        return [Violation("container-image", str(path), f"cannot open container export: {exc}")]

    with archive:
        for info in archive.getmembers():
            try:
                member = _normalized_tar_member(info.name)
            except ValueError as exc:
                violations.append(Violation("container-image", info.name, str(exc)))
                continue

            if member in BUILD_ONLY_RUNTIME_PATHS or any(
                member.startswith(prefix + "/") for prefix in BUILD_ONLY_RUNTIME_PATHS
            ):
                violations.append(
                    Violation(
                        "container-image",
                        member,
                        "build-only repository material remains in runtime filesystem",
                    )
                )

            if _customer_runtime_member(member):
                lowered = member.lower()
                for marker in FORBIDDEN_RUNTIME_PATH_MARKERS:
                    if marker.lower() in lowered:
                        violations.append(
                            Violation(
                                "container-image",
                                member,
                                f"forbidden runtime path marker: {marker}",
                            )
                        )

            if not info.isfile() or not _customer_runtime_member(member):
                continue
            handle = archive.extractfile(info)
            if handle is None:
                continue
            data = handle.read(TEXT_SCAN_LIMIT + 1)
            if len(data) > TEXT_SCAN_LIMIT:
                continue
            violations.extend(_text_violations("container-image", member, data))

    return violations


def validate_paths(paths: Iterable[Path]) -> list[Violation]:
    """Scan retained browser/evidence artifacts when a caller supplies them."""
    violations: list[Violation] = []
    for root in paths:
        if not root.exists():
            violations.append(Violation("retained-artifact", str(root), "path does not exist"))
            continue
        items = [root] if root.is_file() else sorted(root.rglob("*"))
        for item in items:
            if not item.is_file():
                continue
            subject = item.as_posix()
            lowered = subject.lower()
            for marker in FORBIDDEN_RUNTIME_PATH_MARKERS:
                if marker.lower() in lowered:
                    violations.append(
                        Violation(
                            "retained-artifact",
                            subject,
                            f"forbidden retained path marker: {marker}",
                        )
                    )
            violations.extend(_text_violations("retained-artifact", subject, item.read_bytes()))
    return violations


def _receipt(violations: list[Violation], container_tar: Path | None) -> dict[str, object]:
    receipt: dict[str, object] = {
        "schema_version": 1,
        "status": "failed" if violations else "passed",
        "cloud_commit": _git_head(),
        "dockerfile_sha256": _sha256(DOCKERFILE) if DOCKERFILE.exists() else None,
        "dockerignore_sha256": _sha256(DOCKERIGNORE) if DOCKERIGNORE.exists() else None,
        "factory_runtime_dependency": "prohibited",
        "checks": [
            "deny-default-docker-context",
            "docker-copy-allowlist",
            "runtime-source-reference-scan",
            *( ["final-container-filesystem-scan"] if container_tar is not None else [] ),
        ],
        "violations": [asdict(item) for item in violations],
    }
    if container_tar is not None and container_tar.exists():
        receipt["container_export_sha256"] = _sha256(container_tar)
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--container-tar",
        type=Path,
        default=None,
        help="Optional docker export tar for final runtime filesystem validation.",
    )
    parser.add_argument(
        "--scan-path",
        type=Path,
        action="append",
        default=[],
        help="Optional retained browser/evidence artifact path; repeatable.",
    )
    parser.add_argument("--receipt", type=Path, default=None, help="Write sanitized JSON receipt.")
    args = parser.parse_args()

    violations: list[Violation] = []
    violations.extend(validate_dockerignore())
    violations.extend(validate_dockerfile())
    violations.extend(validate_runtime_source())
    if args.container_tar is not None:
        violations.extend(validate_container_tar(args.container_tar))
    if args.scan_path:
        violations.extend(validate_paths(args.scan_path))

    receipt = _receipt(violations, args.container_tar)
    if args.receipt is not None:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if violations:
        print("Customer distribution boundary check failed:", file=sys.stderr)
        for item in violations:
            print(f"- {item.surface}: {item.subject}: {item.rule}", file=sys.stderr)
        return 1

    print("Customer distribution boundary check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
