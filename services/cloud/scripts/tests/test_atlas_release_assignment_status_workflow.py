import unittest
from pathlib import Path

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/atlas-release-assignment-status.yml"


class AtlasReleaseAssignmentStatusWorkflowTests(unittest.TestCase):
    def test_public_repo_does_not_carry_the_status_workflow(self) -> None:
        self.assertFalse(WORKFLOW.exists())


if __name__ == "__main__":
    unittest.main()
