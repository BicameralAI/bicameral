from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "verify_external_ingest_contract",
    ROOT / "scripts/verify_external_ingest_contract.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ExternalIngestContractVerifierTest(unittest.TestCase):
    def test_repository_contract_is_in_sync(self) -> None:
        self.assertEqual(MODULE.verify(ROOT), [])

    def test_schema_digest_drift_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            copy = Path(directory)
            (copy / "protocol/external-ingest").mkdir(parents=True)
            (copy / "protocol/schemas/v2").mkdir(parents=True)
            (copy / "crates/bicameral-api/src").mkdir(parents=True)
            for relative in [
                "protocol/external-ingest/contract.json",
                "protocol/external-ingest/contract.sha256",
                "protocol/schemas/v2/external-ingest-request.schema.json",
                "crates/bicameral-api/src/external_ingest.rs",
            ]:
                target = copy / relative
                target.write_bytes((ROOT / relative).read_bytes())

            schema_path = copy / "protocol/schemas/v2/external-ingest-request.schema.json"
            schema = json.loads(schema_path.read_text())
            schema["description"] = "drift"
            schema_path.write_text(json.dumps(schema))

            self.assertIn(
                "request schema digest differs from contract manifest",
                MODULE.verify(copy),
            )


if __name__ == "__main__":
    unittest.main()
