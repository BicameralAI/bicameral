from pathlib import Path
import unittest


ROOT = Path(__file__).parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "terminal-hosted-product-journey.yml"


class TerminalHostedProductJourneyWorkflowTests(unittest.TestCase):
    def test_deploy_workflow_changes_do_not_trigger_unrelated_product_journey(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn('      - ".github/workflows/deploy-cloud-run.yml"', workflow)
        self.assertIn('      - "release-inputs/**"', workflow)
        self.assertIn('      - "src/bicameral_cloud/static/dashboard/**"', workflow)
        self.assertIn("workflow_call:", workflow)


if __name__ == "__main__":
    unittest.main()
