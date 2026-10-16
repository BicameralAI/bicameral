import unittest
from pathlib import Path

from scripts.verify_hosted_bridge_contract import verify


class HostedBridgeContractTest(unittest.TestCase):
    def test_manifest_fingerprint_and_implementations_are_in_sync(self) -> None:
        root = Path(__file__).resolve().parents[2]
        self.assertEqual(verify(root), [])


if __name__ == "__main__":
    unittest.main()
