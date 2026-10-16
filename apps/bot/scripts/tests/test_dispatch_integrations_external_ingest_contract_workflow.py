import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/dispatch-integrations-external-ingest-contract.yml"


class DispatchIntegrationsContractWorkflowTest(unittest.TestCase):
    def test_dispatch_carries_immutable_contract_coordinates(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        checkout = workflow.find("uses: actions/checkout@")
        contract_read = workflow.find("protocol/external-ingest/contract.sha256")
        self.assertGreaterEqual(checkout, 0)
        self.assertGreaterEqual(contract_read, 0)
        self.assertLess(checkout, contract_read)
        for field in (
            "bot_sha",
            "contract_sha256",
            "request_schema_sha256",
            "source_run_url",
        ):
            self.assertIn(f"client_payload[{field}]", workflow)
        self.assertIn("external_ingest_contract_released", workflow)


if __name__ == "__main__":
    unittest.main()
