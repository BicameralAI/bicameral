#!/usr/bin/env python3
"""Publication preflight: content and workflow rules for a public repository.

    python3 tools/governance/check_publication.py                 # check the working tree
    python3 tools/governance/check_publication.py --history REF   # also check REF's history

Content rules catch the kinds of material removed before publication (personal
addresses, private workspace links, agent-session URLs, local tool logs, real Factory
attestations). Workflow rules keep root workflows safe for pull requests from forks:
no privileged triggers, no secrets, no write permissions except the CodeQL
security-events upload and the install-site Pages token, and no deploy or
publish actions except actions/deploy-pages in the install-site workflow.
Removed workflow paths must stay absent. --history checks that REF is a single
parentless commit, so no earlier object is reachable from it.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import check_structure

SKIPPED_SUFFIXES = (".lock", "package-lock.json")

CONTENT_RULES: tuple[tuple[str, re.Pattern[str]], ...] = (
    (
        "personal email address",
        re.compile(r"[A-Za-z0-9._%+-]+@(?:gmail|googlemail|outlook|hotmail|yahoo|icloud|proton)\.(?:com|me)\b"),
    ),
    ("private Slack direct-message link", re.compile(r"slack\.com/archives/D[A-Z0-9]{8,}")),
    ("Notion page link", re.compile(r"notion\.so/\S*[0-9a-f]{32}")),
    ("agent session URL", re.compile(r"(?:app\.devin\.ai/sessions/[0-9a-f]{32}|claude\.ai/code/session_)")),
)

FORBIDDEN_PATHS = re.compile(r"(?:^|/)(?:\.gstack/|\.env$|[^/]+\.(?:pem|key|p12|pfx)$)")
ATTESTATION_DIR = ".bicameral/factory-attestations/"
SYNTHETIC_ATTESTATION = "synthetic-test-placeholder"

WORKFLOW_DIR = ".github/workflows/"
INSTALLER_PAGES_WORKFLOW = ".github/workflows/installer-pages.yml"
# pages: write, id-token: write, and actions/deploy-pages are the GitHub Pages
# deployment for the install site. They are allowed on that workflow only.
INSTALLER_PAGES_WRITE = re.compile(r"^\s*(?:pages|id-token)\s*:\s*write\b", re.MULTILINE)
INSTALLER_PAGES_DEPLOY = re.compile(r"actions/deploy-pages\b")
PRIVILEGED_TRIGGERS = re.compile(r"^\s{0,4}['\"]?(pull_request_target|workflow_run)['\"]?\s*:", re.MULTILINE)
SECRETS_REFERENCE = re.compile(r"\$\{\{[^}]*\bsecrets\.")
# security-events: write is the CodeQL SARIF upload. Every other write is rejected.
WRITE_PERMISSION = re.compile(
    r"^\s*(?!security-events\b)[\w-]+\s*:\s*write\b|^\s*permissions\s*:\s*write-all\b",
    re.MULTILINE,
)
TOP_LEVEL_PERMISSIONS = re.compile(r"^permissions\s*:", re.MULTILINE)
DEPLOY_OR_PUBLISH = re.compile(
    r"google-github-actions/(?:auth|deploy-cloudrun)|actions/deploy-pages|"
    r"action-gh-release|gh-action-pypi-publish|\bcargo publish\b|\bnpm publish\b|\btwine upload\b"
)
REMOVED_WORKFLOWS = (
    "services/cloud/.github/workflows/deploy-cloud-run.yml",
    "services/cloud/.github/workflows/dispatch-cloud-deploy.yml",
    "services/cloud/.github/workflows/atlas-release-assignment-status.yml",
    "services/cloud/.github/workflows/authorize-internal-admission-migration.yml",
    "services/cloud/.github/workflows/vendor-dashboard-bundle.yml",
    "services/cloud/.github/workflows/factory-attestation.yml",
    "services/cloud/.github/workflows/factory-attestation-migration-gate.yml",
    "services/cloud/.github/workflows/terminal-hosted-product-journey.yml",
    "apps/bot/.github/workflows/release.yml",
    "apps/bot/.github/workflows/tag-release-on-main-version.yml",
    "apps/bot/.github/workflows/atlas-release-assignment-status.yml",
    "apps/bot/.github/workflows/dispatch-cloud-dashboard-vendor.yml",
    "apps/bot/.github/workflows/dispatch-integrations-external-ingest-contract.yml",
    "installer/.github/workflows/pages.yml",
    "packages/mcp/.github/workflows/publish.yml",
    "packages/mcp/.github/workflows/notify-slack-on-merge.yml",
    "packages/mcp/.github/workflows/atlas-release-assignment-status.yml",
    "packages/integrations/.github/workflows/contributor-check.yml",
)


def check_content(path: str, text: str) -> list[str]:
    errors = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        for name, pattern in CONTENT_RULES:
            if pattern.search(line):
                errors.append(f"{path}:{line_number}: {name}")
    return errors


def check_attestation(path: str, text: str) -> list[str]:
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return [f"{path}: Factory attestation is not JSON"]
    if not isinstance(value, dict) or value.get("fixture_kind") != SYNTHETIC_ATTESTATION:
        return [f"{path}: Factory attestation must be a synthetic test placeholder"]
    return []


def check_workflow(path: str, text: str) -> list[str]:
    errors = []
    if PRIVILEGED_TRIGGERS.search(text):
        errors.append(f"{path}: pull_request_target and workflow_run triggers are not allowed")
    if SECRETS_REFERENCE.search(text):
        errors.append(f"{path}: workflows must not reference secrets")
    if path == INSTALLER_PAGES_WORKFLOW:
        write_text = INSTALLER_PAGES_WRITE.sub("", text)
        deploy_text = INSTALLER_PAGES_DEPLOY.sub("", text)
    else:
        write_text = text
        deploy_text = text
    if WRITE_PERMISSION.search(write_text):
        errors.append(f"{path}: workflows must not request write permissions")
    if DEPLOY_OR_PUBLISH.search(deploy_text):
        errors.append(f"{path}: workflows must not deploy or publish")
    if not TOP_LEVEL_PERMISSIONS.search(text):
        errors.append(f"{path}: workflows must declare top-level permissions")
    return errors


def check_removed_workflows(root: Path) -> list[str]:
    return [
        f"{rel}: privileged workflow must not be in the public repository"
        for rel in REMOVED_WORKFLOWS
        if (root / rel).exists()
    ]


def check_file(path: str, text: str) -> list[str]:
    errors = check_content(path, text)
    if ATTESTATION_DIR in path and path.endswith(".json"):
        errors += check_attestation(path, text)
    if path.startswith(WORKFLOW_DIR) and path.endswith((".yml", ".yaml")):
        errors += check_workflow(path, text)
    return errors


def run(root: Path) -> list[str]:
    errors = check_removed_workflows(root)
    for rel in check_structure.candidate_files(root):
        if FORBIDDEN_PATHS.search(rel):
            errors.append(f"{rel}: file type must not be published")
            continue
        if rel.endswith(SKIPPED_SUFFIXES):
            continue
        source = root / rel
        if not source.is_file() or source.is_symlink():
            continue
        data = source.read_bytes()
        if b"\0" in data[:8192]:
            continue
        errors += check_file(rel, data.decode("utf-8", "replace"))
    return errors


def check_history(root: Path, ref: str) -> list[str]:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=root, check=True, capture_output=True, text=True
        ).stdout.strip()

    commits = git("rev-list", ref).split()
    if len(commits) != 1:
        return [f"{ref}: history must be a single commit, found {len(commits)}"]
    if git("rev-list", "--parents", "-n", "1", ref).split()[1:]:
        return [f"{ref}: the publication commit must have no parents"]
    return []


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Check the repository is safe to publish.")
    parser.add_argument("--history", metavar="REF", help="also require REF to be a single root commit")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[2]
    errors = run(root)
    if args.history:
        errors += check_history(root, args.history)
    for error in errors:
        print(f"publication: {error}", file=sys.stderr)
    if errors:
        return 1
    print("publication: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
