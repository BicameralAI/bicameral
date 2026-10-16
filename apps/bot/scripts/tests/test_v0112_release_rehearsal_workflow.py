import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/v0112-release-rehearsal.yml"
CONTRACT = ROOT / "release-rehearsals/v0.1.12-retroactive-simulation.json"


class V0112ReleaseRehearsalWorkflowTest(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")
        self.contract = json.loads(CONTRACT.read_text(encoding="utf-8"))

    def test_factory_controller_is_immutable_and_matches_contract(self) -> None:
        match = re.search(r"FACTORY_COMMIT: ([0-9a-f]{40})", self.workflow)
        self.assertIsNotNone(match)
        self.assertEqual(
            match.group(1), self.contract["evaluated_under"]["factory_commit"]
        )
        self.assertIn("ref: ${{ env.FACTORY_COMMIT }}", self.workflow)
        self.assertIn('git -C _factory rev-parse HEAD', self.workflow)

    def test_workflow_has_no_production_write_authority(self) -> None:
        self.assertIn("permissions:\n  contents: read", self.workflow)
        self.assertNotIn("contents: write", self.workflow)
        self.assertNotIn("deployments: write", self.workflow)
        self.assertNotIn("id-token: write", self.workflow)
        self.assertNotIn("git push", self.workflow)
        self.assertNotIn("gh release", self.workflow)

    def test_rehearsal_fail_closes_every_production_action(self) -> None:
        self.assertEqual(
            self.contract["publication"],
            {
                "git_tag": False,
                "github_release": False,
                "production_r2_latest": False,
                "cloud_deploy": False,
            },
        )
        self.assertIn('result["state"] == "awaiting_assignment"', self.workflow)
        self.assertIn('result["authorizes_merge"] is False', self.workflow)
        self.assertIn("actions/upload-artifact@", self.workflow)


if __name__ == "__main__":
    unittest.main()
