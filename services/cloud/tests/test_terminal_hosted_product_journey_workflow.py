"""The public monorepo does not carry the hosted-product journey workflow."""

from pathlib import Path
import unittest

WORKFLOW = (
    Path(__file__).parents[1] / ".github" / "workflows" / "terminal-hosted-product-journey.yml"
)


class TerminalHostedProductJourneyWorkflowTests(unittest.TestCase):
    def test_public_repo_does_not_carry_the_journey_workflow(self) -> None:
        self.assertFalse(WORKFLOW.exists())


if __name__ == "__main__":
    unittest.main()
