"""The public monorepo does not carry a Cloud Run deploy workflow."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFORMANCE_WORKFLOW = ROOT / ".github" / "workflows" / "contract-conformance.yml"
REMOVED = (
    ROOT / ".github" / "workflows" / "deploy-cloud-run.yml",
    ROOT / ".github" / "workflows" / "dispatch-cloud-deploy.yml",
    ROOT / ".github" / "workflows" / "factory-attestation-migration-gate.yml",
    ROOT / ".github" / "workflows" / "authorize-internal-admission-migration.yml",
)


def test_public_repo_does_not_carry_deploy_workflows() -> None:
    missing = [path.name for path in REMOVED if path.exists()]
    assert missing == []


def test_pull_request_ci_runs_the_deployment_gate_tests() -> None:
    workflow = CONFORMANCE_WORKFLOW.read_text(encoding="utf-8")
    normalized = " ".join(workflow.split())
    assert (
        "pytest -q tests/test_deploy_cloud_run_workflow.py "
        "tests/test_migration_report.py tests/test_migration_delivery.py"
    ) in normalized
