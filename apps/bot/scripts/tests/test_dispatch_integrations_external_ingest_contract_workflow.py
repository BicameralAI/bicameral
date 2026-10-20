import unittest
from pathlib import Path

WORKFLOW = (
    Path(__file__).resolve().parents[2]
    / ".github/workflows/dispatch-integrations-external-ingest-contract.yml"
)


class DispatchIntegrationsExternalIngestContractWorkflowTests(unittest.TestCase):
    def test_public_repo_does_not_carry_the_dispatch_workflow(self) -> None:
        self.assertFalse(WORKFLOW.exists())


if __name__ == "__main__":
    unittest.main()
