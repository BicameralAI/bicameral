import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/dispatch-cloud-dashboard-vendor.yml"
RUST_WORKFLOW = ROOT / ".github/workflows/rust.yml"


class DispatchCloudDashboardVendorWorkflowTest(unittest.TestCase):
    def test_checkout_precedes_contract_fingerprint_read(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        checkout = workflow.find("uses: actions/checkout@")
        fingerprint_read = workflow.find("protocol/hosted-bridge/contract.sha256")

        self.assertGreaterEqual(
            checkout, 0, "workflow must check out the Bot repository"
        )
        self.assertGreaterEqual(
            fingerprint_read, 0, "workflow must read the contract pin"
        )
        self.assertLess(
            checkout,
            fingerprint_read,
            "checkout must happen before the contract fingerprint is read",
        )

    def test_immutable_bundle_carries_the_exact_bot_descriptor(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        emit = workflow.find("python3 scripts/emit_release_descriptor.py")
        archive = workflow.find("-czf release-dashboard/bicameral-dashboard.tar.gz")

        self.assertGreaterEqual(emit, 0, "workflow must emit the Bot descriptor")
        self.assertGreaterEqual(archive, 0, "workflow must create the immutable bundle")
        self.assertLess(
            emit,
            archive,
            "the descriptor must exist before the immutable bundle is sealed",
        )
        self.assertIn(
            "release-dashboard/bot-release-descriptor.json",
            workflow[archive : archive + 500],
            "the immutable bundle must carry the descriptor Cloud will manifest",
        )

    def test_reusable_contract_gate_uses_protected_main_without_a_caller_pin(
        self,
    ) -> None:
        workflow = RUST_WORKFLOW.read_text(encoding="utf-8")

        self.assertIn(
            "BicameralAI/bicameral-factory/.github/workflows/"
            "semantic-contract-conformance.yml@main",
            workflow,
        )
        self.assertNotIn("factory_ref:", workflow)


if __name__ == "__main__":
    unittest.main()
