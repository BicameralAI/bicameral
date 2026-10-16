#!/usr/bin/env python3
"""Validate factory attestation signal-context freshness for the current PR."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ATTESTATION_RUN_ID_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z._-]*$")

REQUIRED_CONTEXT = [
    "playbooks/devin-bicameral-factory-run.md",
    "blueprints/factory-run-signals/artifact-boundary.md",
    "blueprints/factory-run-signals/domain-entity-candidates.md",
    "blueprints/factory-run-signals/factory-learning-signals.md",
    "blueprints/factory-run-signals/minimum-product-validation.md",
]

# Documentation / governance / meta paths that do not exercise the Bicameral
# product surface and therefore do not require a product factory attestation.
# This is fail-closed: a PR is exempt only if EVERY changed path is exempt, so
# any product change (protocol/, crates/, src/, Cargo.*, dashboard/, …) still
# requires the attestation.
EXEMPT_PREFIXES = ("docs/", ".github/", "scripts/")
EXEMPT_EXACT = {
    ".gitignore",
    ".pre-commit-config.yaml",
    "LICENSE",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "NOTICE",
}


def is_exempt_path(path: str) -> bool:
    if path.endswith(".md"):
        return True
    if path in EXEMPT_EXACT:
        return True
    return any(path.startswith(prefix) for prefix in EXEMPT_PREFIXES)


def changed_files() -> list[str]:
    base_ref = os.environ.get("GITHUB_BASE_REF", "main")
    result = subprocess.run(
        ["git", "diff", "--name-only", "--diff-filter=ACMRT", f"origin/{base_ref}...HEAD"],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        result = subprocess.run(
            ["git", "diff", "--name-only", "--diff-filter=ACMRT", "HEAD~1...HEAD"],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    if result.returncode != 0:
        raise RuntimeError("Unable to determine changed files for factory attestation.")
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def main() -> int:
    try:
        changed = changed_files()
    except RuntimeError as error:
        print_failure(str(error))
        return 1

    if not changed:
        print(
            "Factory attestation not required: this ancestry-only PR changes "
            "no files."
        )
        return 0

    if all(is_exempt_path(path) for path in changed):
        print(
            "Factory attestation not required: this PR changes only "
            "documentation/governance/meta paths (no product surface)."
        )
        for path in changed:
            print(f"  exempt: {path}")
        return 0

    changed_attestations = changed_attestation_paths()
    if not changed_attestations:
        print_failure(
            "No changed factory attestation was found in this PR. "
            "Regenerate the commit-grounded attestation for this factory run."
        )
        return 1

    naming_errors = collision_safe_naming_errors(changed_attestations)
    if naming_errors:
        print_naming_failure(naming_errors)
        return 1

    failed = False
    for attestation_path in changed_attestations:
        command = [
            sys.executable,
            "scripts/validate-factory-attestation.py",
            str(attestation_path),
        ]
        for context_path in REQUIRED_CONTEXT:
            command.extend(["--require-context", context_path])

        result = subprocess.run(command, check=False)
        if result.returncode != 0:
            failed = True

    if not failed:
        return 0

    print_failure(
        "At least one changed factory attestation does not declare the current "
        "required factory signal contracts."
    )
    return 1


def changed_attestation_paths() -> list[Path]:
    # Kept as an escape hatch for local runs or future workflow wrappers.
    env_path = os.environ.get("FACTORY_ATTESTATION_PATH")
    if env_path:
        return [Path(env_path)]

    base_ref = os.environ.get("GITHUB_BASE_REF", "main")
    result = subprocess.run(
        ["git", "diff", "--name-only", "--diff-filter=ACMRT", f"origin/{base_ref}...HEAD"],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        result = subprocess.run(
            ["git", "diff", "--name-only", "--diff-filter=ACMRT", "HEAD~1...HEAD"],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )

    paths = []
    for line in result.stdout.splitlines():
        path = Path(line.strip())
        if (
            len(path.parts) == 3
            and path.parts[0] == ".bicameral"
            and path.parts[1] == "factory-attestations"
            and path.suffix == ".json"
            and path.is_file()
        ):
            paths.append(path)
    return paths


def added_attestation_paths() -> set[Path]:
    """Attestation files newly **added** by this PR (git status A).

    Only added files must adopt the per-run filename: modifying a pre-existing
    legacy ``<factory_commit>.json`` is still permitted so historical records
    stay valid.
    """
    base_ref = os.environ.get("GITHUB_BASE_REF", "main")
    result = subprocess.run(
        ["git", "diff", "--name-only", "--diff-filter=A", f"origin/{base_ref}...HEAD"],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        result = subprocess.run(
            ["git", "diff", "--name-only", "--diff-filter=A", "HEAD~1...HEAD"],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    added: set[Path] = set()
    for line in result.stdout.splitlines():
        path = Path(line.strip())
        if (
            len(path.parts) == 3
            and path.parts[0] == ".bicameral"
            and path.parts[1] == "factory-attestations"
            and path.suffix == ".json"
        ):
            added.add(path)
    return added


def collision_safe_naming_errors(attestations: list[Path]) -> list[str]:
    """Require newly-added attestations to use the per-run filename form
    ``<factory_commit>.<run-id>.json``.

    The legacy ``<factory_commit>.json`` form forces every PR sharing a factory
    version onto the same path, which guarantees merge collisions. Requiring a
    per-run discriminator (e.g. the head branch slug) on new files makes each
    PR's attestation a distinct, conflict-free file.
    """
    added = added_attestation_paths()
    errors: list[str] = []
    for path in attestations:
        if path not in added:
            continue
        try:
            factory_commit = json.loads(path.read_text()).get("factory_commit")
        except (OSError, json.JSONDecodeError):
            continue
        if not isinstance(factory_commit, str):
            continue
        stem = path.name[: -len(".json")] if path.name.endswith(".json") else path.name
        run_id = stem[len(factory_commit) + 1 :] if stem.startswith(f"{factory_commit}.") else ""
        if not (run_id and ATTESTATION_RUN_ID_RE.match(run_id)):
            errors.append(
                f"{path}: new factory attestations must be named "
                f"{factory_commit}.<run-id>.json (e.g. the head branch slug), not "
                f"the legacy {factory_commit}.json which collides across PRs."
            )
    return errors


def print_naming_failure(errors: list[str]) -> None:
    print(
        "\nFactory attestation naming check failed.\n\n"
        + "".join(f"- {error}\n" for error in errors)
        + "\n"
        "Per-run attestation filenames keep concurrent PRs that share a\n"
        "factory_commit from colliding on the same file. Rename your added\n"
        "attestation to .bicameral/factory-attestations/<factory_commit>.<run-id>.json\n"
        "where <run-id> is a stable per-PR token such as the head branch slug,\n"
        "for example: <factory_commit>.devin-1781821202-my-feature.json\n",
        file=sys.stderr,
    )


def print_failure(reason: str) -> None:
    print(
        f"\nFactory signal context check failed.\n"
        "\n"
        f"{reason}\n"
        "\n"
        "This usually means the PR was created with stale factory context or the "
        "attestation was not regenerated after the factory signal rules changed.\n"
        "\n"
        "To fix:\n"
        "1. Update your local bicameral-factory checkout:\n"
        "   git -C <path-to-bicameral-factory> switch main\n"
        "   git -C <path-to-bicameral-factory> pull --ff-only\n"
        "\n"
        "2. Reinstall copied factory skills:\n"
        "   npx skills@latest add git@github.com:BicameralAI/bicameral-factory.git --copy\n"
        "\n"
        "3. Re-run the factory playbook or pre-PR attestation step.\n"
        "\n"
        "4. Regenerate .bicameral/factory-attestations/<factory_commit>.<run-id>.json "
        "(use a per-PR <run-id> such as the head branch slug) so "
        "loaded_context includes the current signal contracts and "
        "state_refinement answers the checkpoint. If lifecycle behavior changed, "
        "record release state, added/deferred states, invariants, and "
        "minimization review. If not, state that no lifecycle behavior changed "
        "and leave states added/deferred empty.\n"
        "\n"
        "Required loaded_context entries:\n"
        + "".join(f"   - {context_path}\n" for context_path in REQUIRED_CONTEXT),
        file=sys.stderr,
    )


if __name__ == "__main__":
    raise SystemExit(main())
