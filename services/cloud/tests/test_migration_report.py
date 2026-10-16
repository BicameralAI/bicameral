"""Public CLI behavior for the concise migration execution report."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORTER = ROOT / "scripts" / "build_migration_report.py"


def canonical_digest(value: object) -> str:
    return (
        "sha256:"
        + hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
    )


def receipt(*, state: str = "verified") -> dict[str, object]:
    succeeded = state == "verified"
    payload: dict[str, object] = {
        "schema_version": 1,
        "migration_set_id": "sha256:" + "a" * 64,
        "release_id": "release-v1",
        "cloud_commit": "b" * 40,
        "environment": "production",
        "state": state,
        "state_history": [
            {"state": "planned", "at": "2026-07-30T01:02:03Z"},
            {
                "state": "verified" if succeeded else state,
                "at": "2026-07-30T01:02:03Z",
            },
        ],
        "preflight": {
            "status": "passed",
            "remote_versions_before": ["202607120001"],
            "pending_versions": ["202607120002"],
        },
        "apply": {
            "status": "applied" if succeeded else "failed",
            "applied_versions": ["202607120002"] if succeeded else [],
        },
        "tool": {"name": "supabase", "version": "2.39.2"},
        "workflow": {
            "repository": "BicameralAI/bicameral-cloud",
            "run_id": "987654",
            "sha": "b" * 40,
            "url": "https://github.com/BicameralAI/bicameral-cloud/actions/runs/987654",
        },
        "started_at": "2026-07-30T01:02:03Z",
        "completed_at": "2026-07-30T01:02:03Z",
        "deployment_prerequisite_satisfied": succeeded,
        "authorizes_publish": False,
    }
    if succeeded:
        payload["verification"] = {
            "status": "passed",
            "remote_versions_after": ["202607120001", "202607120002"],
        }
    else:
        payload["failure"] = {
            "code": "supabase_apply_failed",
            "phase": "apply",
        }
    return {"receipt_id": canonical_digest(payload), **payload}


def run_report(
    tmp_path: Path,
    *,
    state: str = "verified",
    summary: bool = False,
) -> tuple[dict[str, str], str]:
    receipt_path = tmp_path / "receipt.json"
    summary_path = tmp_path / "summary.md"
    receipt_path.write_text(json.dumps(receipt(state=state)), encoding="utf-8")
    command = [
        sys.executable,
        str(REPORTER),
        "--receipt",
        str(receipt_path),
    ]
    if summary:
        command.extend(["--summary-path", str(summary_path)])
    result = subprocess.run(
        command,
        check=True,
        text=True,
        capture_output=True,
    )
    return json.loads(result.stdout), (
        summary_path.read_text(encoding="utf-8") if summary else ""
    )


def test_success_report_names_exact_applied_delta_and_evidence(tmp_path: Path) -> None:
    payload, _ = run_report(tmp_path)

    assert list(payload) == ["text"]
    assert "Database migration execution ✅" in payload["text"]
    assert "production · verified · 1 applied" in payload["text"]
    assert "Applied: 202607120002" in payload["text"]
    assert "Migration set: sha256:aaaaaaaa" in payload["text"]
    assert "BicameralAI/bicameral-cloud/actions/runs/987654" in payload["text"]


def test_failure_report_says_application_deployment_was_blocked(
    tmp_path: Path,
) -> None:
    payload, _ = run_report(tmp_path, state="apply_failed")

    assert "Database migration execution ❌" in payload["text"]
    assert "production · apply_failed · application deploy blocked" in payload["text"]
    assert "Failure: apply / supabase_apply_failed" in payload["text"]
    assert "verified" not in payload["text"]


def test_report_is_also_written_to_the_github_summary(tmp_path: Path) -> None:
    payload, summary = run_report(tmp_path, summary=True)

    assert "### Database migration execution ✅" in summary
    assert "production · verified · 1 applied" in summary
    assert "[workflow run]" in summary
    assert "Database migration execution ✅" in payload["text"]


def test_report_rejects_a_tampered_execution_receipt(tmp_path: Path) -> None:
    value = receipt()
    value["environment"] = "staging"
    receipt_path = tmp_path / "receipt.json"
    receipt_path.write_text(json.dumps(value), encoding="utf-8")

    result = subprocess.run(
        [
            sys.executable,
            str(REPORTER),
            "--receipt",
            str(receipt_path),
        ],
        check=False,
        text=True,
        capture_output=True,
    )

    assert result.returncode != 0
    assert "receipt_id does not bind" in result.stderr
