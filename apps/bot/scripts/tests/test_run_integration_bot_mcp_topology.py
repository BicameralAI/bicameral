import importlib.util
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/run_integration_bot_mcp_topology.py"
WORKFLOW = ROOT / ".github/workflows/integration-bot-mcp-topology.yml"
HOSTED_WORKFLOW = ROOT / ".github/workflows/release-hosted-topology.yml"
ALPHA_WORKFLOW = ROOT / ".github/workflows/alpha-recall-real-process-topology.yml"


def _load_runner():
    spec = importlib.util.spec_from_file_location(
        "integration_bot_mcp_topology", SCRIPT
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load topology runner")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class IntegrationBotMcpTopologyTest(unittest.TestCase):
    def test_checkout_identity_requires_exact_matching_commit(self) -> None:
        runner = _load_runner()
        expected = "a" * 40

        self.assertEqual(
            runner.require_exact_commit(expected, expected, "integrations"), expected
        )
        with self.assertRaisesRegex(runner.TopologyError, "full lowercase SHA"):
            runner.require_exact_commit("main", expected, "integrations")
        with self.assertRaisesRegex(runner.TopologyError, "does not match"):
            runner.require_exact_commit(expected, "b" * 40, "integrations")

    def test_mcp_terminal_result_must_contain_the_ingested_marker(self) -> None:
        runner = _load_runner()
        marker = "journey-marker-123"
        payload = {
            "tool_names": ["bicameral.search"],
            "is_error": False,
            "content": [
                json.dumps(
                    {
                        "surface": "search",
                        "matches": [
                            {
                                "title": marker,
                                "candidate_id": "cand-1",
                                "evidence_ref_id": "ev-1",
                            }
                        ],
                    }
                )
            ],
        }

        evidence = runner.validate_mcp_terminal_result(payload, marker)
        self.assertEqual(evidence["surface"], "search")
        self.assertEqual(evidence["candidate_id"], "cand-1")
        self.assertEqual(evidence["evidence_ref_id"], "ev-1")

        payload["content"] = [json.dumps({"surface": "search", "matches": []})]
        with self.assertRaisesRegex(runner.TopologyError, "ingested marker"):
            runner.validate_mcp_terminal_result(payload, marker)

    def test_mcp_terminal_result_accepts_canonical_evidence_refs_list(self) -> None:
        runner = _load_runner()
        marker = "journey-marker-list"
        payload = {
            "tool_names": ["bicameral.search"],
            "is_error": False,
            "content": [
                json.dumps(
                    {
                        "surface": "search",
                        "matches": [
                            {
                                "title": marker,
                                "id": "cand-list-1",
                                "evidence_refs": ["ev-list-1"],
                            }
                        ],
                    }
                )
            ],
        }

        evidence = runner.validate_mcp_terminal_result(payload, marker)
        self.assertEqual(evidence["candidate_id"], "cand-list-1")
        self.assertEqual(evidence["evidence_ref_id"], "ev-list-1")
        self.assertEqual(evidence["evidence_refs"], ["ev-list-1"])

    def test_mcp_terminal_result_accepts_external_ingest_snapshot_provenance(
        self,
    ) -> None:
        runner = _load_runner()
        marker = "journey-marker-snapshot"
        payload = {
            "tool_names": ["bicameral.search"],
            "is_error": False,
            "content": [
                json.dumps(
                    {
                        "surface": "search",
                        "matches": [
                            {
                                "title": marker,
                                "id": "cand-snapshot-1",
                                "snapshot_id": "sha256:" + "a" * 64,
                                "source_uri": "https://topology.invalid/events/snapshot",
                            }
                        ],
                    }
                )
            ],
        }

        evidence = runner.validate_mcp_terminal_result(payload, marker)
        self.assertEqual(evidence["candidate_id"], "cand-snapshot-1")
        self.assertEqual(evidence["snapshot_id"], "sha256:" + "a" * 64)
        self.assertEqual(
            evidence["source_uri"], "https://topology.invalid/events/snapshot"
        )

    def test_github_actions_topology_execution_surfaces_are_absent(self) -> None:
        for workflow in (WORKFLOW, HOSTED_WORKFLOW, ALPHA_WORKFLOW):
            self.assertFalse(
                workflow.exists(),
                "topology production attempts run only in direct Devin sessions",
            )

    def test_runner_rejects_github_actions(self) -> None:
        runner = _load_runner()
        with self.assertRaisesRegex(runner.TopologyError, "Devin"):
            runner.require_direct_devin_execution({"GITHUB_ACTIONS": "true"})
        runner.require_direct_devin_execution(
            {"BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER": "devin"}
        )


if __name__ == "__main__":
    unittest.main()
