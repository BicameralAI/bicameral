"""Behavioral tests for deterministic semantic contract pair enforcement."""

from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import semantic_contract_conformance as scc  # noqa: E402


def _fingerprint(content: bytes) -> str:
    return "sha256:" + hashlib.sha256(content).hexdigest()


def _registry(content: bytes) -> dict:
    return {
        "version": 1,
        "contracts": [
            {
                "contract_id": "hosted-bridge.capability.v1",
                "status": "active",
                "producer_repo": {"owner": "BicameralAI", "name": "bicameral-bot"},
                "consumer_repo": {"owner": "BicameralAI", "name": "bicameral-cloud"},
                "artifact_location": "contracts/hosted_bridge_contract.json",
                "current_fingerprint": _fingerprint(content),
                "fingerprint_source": {
                    "kind": "artifact_specific",
                    "source_path": "protocol/hosted-bridge/contract.json",
                    "normalization": ["artifact_specific_canonicalization"],
                    "digest": "sha256",
                },
                "enforcement_direction": "bidirectional",
            }
        ],
    }


class SemanticContractConformanceTest(unittest.TestCase):
    def test_active_producer_artifact_matches_registry(self) -> None:
        content = json.dumps({"protocol": 2}, sort_keys=True).encode()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / "protocol/hosted-bridge/contract.json"
            artifact.parent.mkdir(parents=True)
            artifact.write_bytes(content)

            problems = scc.verify_repository(
                _registry(content),
                contract_id="hosted-bridge.capability.v1",
                repository="BicameralAI/bicameral-bot",
                repo_root=root,
            )

            self.assertEqual(problems, [])

    def test_active_contract_rejects_unimplemented_fingerprint_kind(self) -> None:
        content = b'{"protocol":2}'
        registry = _registry(content)
        registry["contracts"][0]["fingerprint_source"]["kind"] = "canonical_json"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / "protocol/hosted-bridge/contract.json"
            artifact.parent.mkdir(parents=True)
            artifact.write_bytes(content)

            problems = scc.verify_repository(
                registry,
                contract_id="hosted-bridge.capability.v1",
                repository="BicameralAI/bicameral-bot",
                repo_root=root,
            )

            self.assertTrue(any("unsupported fingerprint kind" in problem for problem in problems))

    def test_bidirectional_pair_fails_when_consumer_pin_is_stale(self) -> None:
        content = b'{"protocol":2}'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            producer = root / "producer"
            consumer = root / "consumer"
            producer_artifact = producer / "protocol/hosted-bridge/contract.json"
            consumer_artifact = consumer / "contracts/hosted_bridge_contract.json"
            producer_artifact.parent.mkdir(parents=True)
            consumer_artifact.parent.mkdir(parents=True)
            producer_artifact.write_bytes(content)
            consumer_artifact.write_bytes(b'{"protocol":1}')

            problems = scc.verify_pair(
                _registry(content),
                contract_id="hosted-bridge.capability.v1",
                producer_root=producer,
                consumer_root=consumer,
            )

            self.assertTrue(any("consumer fingerprint mismatch" in problem for problem in problems))

    def test_bidirectional_pair_passes_when_both_artifacts_match(self) -> None:
        content = b'{"protocol":2}'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            producer = root / "producer"
            consumer = root / "consumer"
            for artifact in (
                producer / "protocol/hosted-bridge/contract.json",
                consumer / "contracts/hosted_bridge_contract.json",
            ):
                artifact.parent.mkdir(parents=True)
                artifact.write_bytes(content)

            problems = scc.verify_pair(
                _registry(content),
                contract_id="hosted-bridge.capability.v1",
                producer_root=producer,
                consumer_root=consumer,
            )

            self.assertEqual(problems, [])

    def test_missing_consumer_artifact_fails_closed(self) -> None:
        content = b'{"protocol":2}'
        with tempfile.TemporaryDirectory() as directory:
            problems = scc.verify_repository(
                _registry(content),
                contract_id="hosted-bridge.capability.v1",
                repository="BicameralAI/bicameral-cloud",
                repo_root=Path(directory),
            )

            self.assertTrue(any("missing consumer artifact" in problem for problem in problems))

    def test_reusable_workflow_pins_factory_and_invokes_validator(self) -> None:
        workflow = (ROOT / ".github/workflows/semantic-contract-conformance.yml").read_text(
            encoding="utf-8"
        )

        self.assertNotIn("factory_ref", workflow)
        self.assertIn("FACTORY_REF: ${{ job.workflow_sha }}", workflow)
        self.assertIn("FACTORY_WORKFLOW_REF: ${{ job.workflow_ref }}", workflow)
        self.assertIn("ref: ${{ job.workflow_sha }}", workflow)
        self.assertIn("scripts/semantic_contract_conformance.py", workflow)
        self.assertIn("--repository \"${PRODUCT_REPOSITORY}\"", workflow)

    def test_artifact_path_cannot_escape_repository_root(self) -> None:
        content = b'{"protocol":2}'
        registry = _registry(content)
        registry["contracts"][0]["fingerprint_source"]["source_path"] = "../outside.json"
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "product"
            root.mkdir()
            (parent / "outside.json").write_bytes(content)

            problems = scc.verify_repository(
                registry,
                contract_id="hosted-bridge.capability.v1",
                repository="BicameralAI/bicameral-bot",
                repo_root=root,
            )

            self.assertTrue(any("escapes repository root" in problem for problem in problems))


if __name__ == "__main__":
    unittest.main()
