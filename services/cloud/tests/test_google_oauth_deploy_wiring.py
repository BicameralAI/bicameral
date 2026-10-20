"""The public monorepo does not carry the Cloud Run deploy workflow."""

from pathlib import Path

WORKFLOW = Path(__file__).resolve().parents[1] / ".github" / "workflows" / "deploy-cloud-run.yml"


def test_public_repo_does_not_carry_deploy_workflow() -> None:
    assert not WORKFLOW.exists()
