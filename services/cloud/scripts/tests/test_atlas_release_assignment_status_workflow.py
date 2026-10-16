import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/atlas-release-assignment-status.yml"


class AtlasReleaseAssignmentStatusWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_only_remaining_pending_release_statuses_enter_resolver(self) -> None:
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

    def test_pr_time_vacuous_status_path_is_retired(self) -> None:
        self.assertNotIn("pull_request:", self.workflow)
        self.assertNotIn("vacuous-release-unit", self.workflow)
        self.assertNotIn("mode: pull_request", self.workflow)

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

    def test_status_concurrency_is_scoped_to_exact_sha_and_context(self) -> None:
        self.assertIn("github.event.sha", self.workflow)
        self.assertIn("github.event.context", self.workflow)

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


if __name__ == "__main__":
    unittest.main()
