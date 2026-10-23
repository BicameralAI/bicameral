import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/atlas-release-assignment-status.yml"


class AtlasReleaseAssignmentStatusWorkflowTests(unittest.TestCase):
    def test_public_repo_does_not_carry_the_status_workflow(self) -> None:
        self.assertFalse(WORKFLOW.exists())

    def test_github_actions_contains_no_topology_execution_workflow(self) -> None:
        provenance = (ROOT / ".github/workflows/release-provenance.yml").read_text(encoding="utf-8")
        self.assertNotIn("pull_request:", provenance)
        self.assertIn("workflow_dispatch:", provenance)
        for name in (
            "alpha-recall-real-process-topology.yml",
            "hosted-workspace-binding-journey.yml",
            "integration-bot-mcp-topology.yml",
            "release-hosted-topology.yml",
            "provider-product-creation-edge.yml",
        ):
            self.assertFalse((ROOT / ".github/workflows" / name).exists(), name)
        production_runner = re.compile(
            r"scripts/(?:run_[A-Za-z0-9_-]*topology|"
            r"validate_hosted_workspace_binding)\.py"
        )
        for workflow in sorted((ROOT / ".github/workflows").glob("*.yml")):
            self.assertIsNone(
                production_runner.search(workflow.read_text(encoding="utf-8")),
                workflow.name,
            )


if __name__ == "__main__":
    unittest.main()
