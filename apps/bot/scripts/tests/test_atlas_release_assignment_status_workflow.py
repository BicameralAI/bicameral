import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/atlas-release-assignment-status.yml"


class AtlasReleaseAssignmentStatusWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_only_remaining_pending_statuses_enter_resolver(self) -> None:
        self.assertIn("status:", self.workflow)
        self.assertIn(
            "github.event.context == 'release-unit / assigned'", self.workflow
        )
        self.assertIn(
            "github.event.context == 'stage-version / authorized'", self.workflow
        )
        self.assertNotIn("release-unit / merge-eligible", self.workflow)
        self.assertIn("github.event.state == 'pending'", self.workflow)
        self.assertNotIn("schedule:", self.workflow)

    def test_factory_resolver_uses_protected_main_without_a_caller_pin(self) -> None:
        self.assertIn(
            "BicameralAI/bicameral-factory/.github/workflows/"
            "atlas-release-unit-status.yml@main",
            self.workflow,
        )
        self.assertNotIn("factory_ref:", self.workflow)

    def test_wrapper_grants_only_read_receipt_and_status_permissions(self) -> None:
        self.assertIn("pull-requests: read", self.workflow)
        self.assertIn("id-token: write", self.workflow)
        self.assertIn("statuses: write", self.workflow)
        self.assertNotIn("actions: write", self.workflow)
        self.assertNotIn("contents: write", self.workflow)
        self.assertIn("atlas-receipts", self.workflow)
        self.assertIn("BICAMERAL_CROSS_REPO_TOKEN", self.workflow)
        self.assertNotIn("gcloud storage", self.workflow)
        self.assertNotIn("gh api", self.workflow)

    def test_resolver_uses_dedicated_release_unit_status_app(self) -> None:
        self.assertIn(
            "release_status_app_id: ${{ vars.RELEASE_UNIT_STATUS_APP_ID }}",
            self.workflow,
        )
        self.assertIn(
            "release_status_app_private_key: "
            "${{ secrets.RELEASE_UNIT_STATUS_APP_PRIVATE_KEY }}",
            self.workflow,
        )

    def test_pr_time_vacuous_status_path_is_retired(self) -> None:
        self.assertNotIn("pull_request_target:", self.workflow)
        self.assertNotIn("pull_request:", self.workflow)
        self.assertNotIn("github.rest.pulls.listFiles", self.workflow)
        self.assertNotIn("github.rest.repos.createCommitStatus", self.workflow)
        self.assertNotIn("No Release Unit asserted by this pull request", self.workflow)

    def test_status_concurrency_is_scoped_to_exact_sha_and_context(self) -> None:
        self.assertIn("github.event.sha", self.workflow)
        self.assertIn("github.event.context", self.workflow)

    def test_github_actions_contains_no_topology_execution_workflow(self) -> None:
        import re

        provenance = (ROOT / ".github/workflows/release-provenance.yml").read_text(
            encoding="utf-8"
        )
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
