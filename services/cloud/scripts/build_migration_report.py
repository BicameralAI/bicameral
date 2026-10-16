#!/usr/bin/env python3
"""Build the secret-free report for a migration execution receipt."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


def canonical_digest(value: object) -> str:
    return (
        "sha256:"
        + hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def load_receipt(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("migration receipt must be an object")
    claimed = value.get("receipt_id")
    unsigned = {key: item for key, item in value.items() if key != "receipt_id"}
    if claimed != canonical_digest(unsigned):
        raise ValueError("receipt_id does not bind the migration receipt")
    return value


def report_lines(receipt: dict[str, Any], *, markdown: bool) -> list[str]:
    succeeded = (
        receipt.get("state") == "verified"
        and receipt.get("deployment_prerequisite_satisfied") is True
    )
    icon = "✅" if succeeded else "❌"
    title = f"Database migration execution {icon}"
    if markdown:
        title = f"### {title}"
    preflight = receipt.get("preflight")
    apply = receipt.get("apply")
    workflow = receipt.get("workflow")
    if not isinstance(preflight, dict) or not isinstance(workflow, dict):
        raise ValueError("migration receipt omitted preflight or workflow evidence")
    applied = (
        apply.get("applied_versions", [])
        if isinstance(apply, dict) and succeeded
        else []
    )
    migration_set_id = str(receipt.get("migration_set_id", ""))
    environment = str(receipt.get("environment", "unknown"))
    state = str(receipt.get("state", "unknown"))
    workflow_url = str(workflow.get("url", ""))
    sha = str(workflow.get("sha", ""))
    if succeeded:
        status = f"{environment} · verified · {len(applied)} applied"
    else:
        status = f"{environment} · {state} · application deploy blocked"
    applied_text = ", ".join(str(version) for version in applied) or "none"
    set_text = migration_set_id[:15]
    if markdown:
        evidence = (
            f"[commit `{sha[:8]}`]"
            f"(https://github.com/{workflow.get('repository')}/commit/{sha}) · "
            f"[workflow run]({workflow_url})"
        )
    else:
        evidence = (
            f"<https://github.com/{workflow.get('repository')}/commit/{sha}"
            f"|commit {sha[:8]}> · <{workflow_url}|workflow run>"
        )
    lines = [
        title,
        status,
        f"Applied: {applied_text}",
        f"Migration set: {set_text}",
        evidence,
    ]
    failure = receipt.get("failure")
    if not succeeded:
        if not isinstance(failure, dict):
            raise ValueError("failed migration receipt omitted failure classification")
        lines.insert(
            2,
            f"Failure: {failure.get('phase')} / {failure.get('code')}",
        )
    return lines


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--summary-path", type=Path)
    args = parser.parse_args()
    receipt = load_receipt(args.receipt)
    text = "\n".join(report_lines(receipt, markdown=False))
    if args.summary_path:
        with args.summary_path.open("a", encoding="utf-8") as summary:
            summary.write("\n\n".join(report_lines(receipt, markdown=True)) + "\n")
    print(json.dumps({"text": text}, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
