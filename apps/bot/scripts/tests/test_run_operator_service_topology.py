"""Contract tests for the reusable operator-service topology runner."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

try:
    from jsonschema import Draft202012Validator
except ModuleNotFoundError:
    Draft202012Validator = None


SCRIPT = Path(__file__).parents[1] / "run_operator_service_topology.py"
SPEC = importlib.util.spec_from_file_location("run_operator_service_topology", SCRIPT)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)


def _digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def journey_revision() -> dict:
    value = {
        "schema_version": 1,
        "idempotency_key": "release-unit:v0114-operator-scoped-multi-product-daemon",
        "release_unit_id": "v0114-operator-scoped-multi-product-daemon",
        "journey_id": "daemon_service_multi_product_v1",
        "topology_type": "daemon_service_multi_product_v1",
        "actor": "local operator",
        "entrypoint": "bicameral onboard --install-daemon",
        "observable_success": (
            "One installed daemon routes two Products across restart while one "
            "damaged binding remains isolated."
        ),
        "surfaces": [
            "bicameral-cli",
            "native-user-service",
            "operator-daemon",
            "bicameral-mcp",
        ],
        "boundary_contracts": [
            "ToolRequest.product_id",
            "ProjectEventStoreRouter",
            "LocalWorkspaceBinding",
        ],
        "environment_ids": [
            "operator-service-environment-v1",
            "local-product-environment-v1",
        ],
        "checkpoints": [
            {"check_id": "first", "label": "First issue-owned check"},
            {"check_id": "second", "label": "Second issue-owned check"},
        ],
        "created_by": "github:jinhongkuan",
        "created_at": "2026-07-28T00:00:00Z",
    }
    value["journey_digest"] = _digest(value)
    return value


class OperatorServiceTopologyRunnerTest(unittest.TestCase):
    def test_production_execution_rejects_github_actions(self) -> None:
        with self.assertRaisesRegex(
            RUNNER.TopologyPreparationError, "direct Devin"
        ):
            RUNNER.require_direct_devin_execution({"GITHUB_ACTIONS": "true"})
        RUNNER.require_direct_devin_execution(
            {"BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER": "devin"}
        )

    def test_durable_product_watcher_states_are_closed_and_product_keyed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            operator_home = Path(temp)
            state_dir = operator_home / ".bicameral" / "product-watchers"
            state_dir.mkdir(parents=True)
            (state_dir / "watcher-state-v1.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "records": {
                            "prd_a": {
                                "product_id": "prd_a",
                                "workspace_root": "/tmp/a",
                                "lifecycle": "failed",
                                "checkpoint_head_sha": "a" * 40,
                                "queued_work": [],
                                "failure_backlog": [{"reason_code": "head_unavailable"}],
                            },
                            "prd_b": {
                                "product_id": "prd_b",
                                "workspace_root": "/tmp/b",
                                "lifecycle": "current",
                                "checkpoint_head_sha": "b" * 40,
                                "queued_work": [],
                                "failure_backlog": [],
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )

            states = RUNNER.load_product_watcher_states(operator_home)

            self.assertEqual("failed", states["prd_a"]["lifecycle"])
            self.assertEqual("current", states["prd_b"]["lifecycle"])

    def test_durable_product_watcher_state_rejects_cross_product_record(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            operator_home = Path(temp)
            state_dir = operator_home / ".bicameral" / "product-watchers"
            state_dir.mkdir(parents=True)
            (state_dir / "watcher-state-v1.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "records": {
                            "prd_a": {
                                "product_id": "prd_b",
                                "workspace_root": "/tmp/a",
                                "lifecycle": "current",
                                "checkpoint_head_sha": "a" * 40,
                                "queued_work": [],
                                "failure_backlog": [],
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(
                RUNNER.TopologyExecutionError, "Product identity"
            ):
                RUNNER.load_product_watcher_states(operator_home)

    def test_revision_is_digest_bound_and_check_ids_remain_issue_owned(self) -> None:
        revision = journey_revision()
        binding = RUNNER.validate_journey_revision(revision)

        self.assertEqual(("first", "second"), binding.check_ids)
        self.assertEqual(revision["journey_digest"], binding.digest)

    def test_revision_requires_both_composed_environments(self) -> None:
        revision = journey_revision()
        revision["environment_ids"].remove("local-product-environment-v1")
        unsigned = {key: value for key, value in revision.items() if key != "journey_digest"}
        revision["journey_digest"] = _digest(unsigned)

        with self.assertRaisesRegex(
            RUNNER.TopologyPreparationError, "required environment"
        ):
            RUNNER.validate_journey_revision(revision)

    def test_native_lane_selection_is_explicit(self) -> None:
        self.assertEqual(
            "macos-launchd-user-v1", RUNNER.platform_lane("darwin").lane_id
        )
        self.assertEqual(
            "linux-systemd-user-v1", RUNNER.platform_lane("linux").lane_id
        )
        with self.assertRaisesRegex(
            RUNNER.TopologyPreparationError, "unsupported platform"
        ):
            RUNNER.platform_lane("win32")

    def test_production_commands_use_shipped_onboarding_and_service_cli(self) -> None:
        commands = RUNNER.service_commands(Path("/release/bicameral"))

        self.assertEqual(
            ("/release/bicameral", "onboard", "--install-daemon"),
            commands.install,
        )
        self.assertEqual(
            ("/release/bicameral", "service", "start"), commands.start
        )
        self.assertEqual(
            ("/release/bicameral", "service", "stop"), commands.stop
        )
        self.assertEqual(
            ("/release/bicameral", "service", "uninstall"),
            commands.uninstall,
        )

    def test_native_service_run_has_an_isolated_identity_home_and_port(self) -> None:
        environment = RUNNER._service_env(
            Path("/tmp/operator-home"), "topology-42-deadbeef", 41_042
        )

        self.assertEqual("/tmp/operator-home", environment["HOME"])
        self.assertEqual(
            "topology-42-deadbeef", environment["BICAMERAL_SERVICE_INSTANCE"]
        )
        self.assertEqual("41042", environment["BICAMERAL_GATEWAY_PORT"])

    def test_mcp_probe_requires_product_selection_and_scoped_requests(self) -> None:
        good = {
            "tool_names": ["bicameral.product.select", "bicameral.search"],
            "selected_product_id": "prd_a",
            "request_product_ids": ["prd_a", "prd_a"],
        }
        RUNNER.validate_mcp_product_probe(good, "prd_a")

        for mutation, reason in (
            (
                {**good, "tool_names": ["bicameral.search"]},
                "product.select",
            ),
            (
                {**good, "request_product_ids": ["prd_a", None]},
                "product_id",
            ),
        ):
            with self.subTest(reason=reason):
                with self.assertRaisesRegex(RUNNER.TopologyPreparationError, reason):
                    RUNNER.validate_mcp_product_probe(mutation, "prd_a")

    def test_local_receipt_never_claims_release_authority(self) -> None:
        revision = journey_revision()
        receipt = RUNNER.build_receipt(
            revision=revision,
            platform="darwin",
            artifact_provenance={
                "bot_commit": "a" * 40,
                "bot_binary_sha256": "sha256:" + "b" * 64,
                "mcp_commit": "c" * 40,
                "mcp_executable_sha256": "sha256:" + "d" * 64,
                "factory_commit": "e" * 40,
                "environment_profile_sha256": "sha256:" + "f" * 64,
            },
            service_observations={"native_manager": True},
            product_observations={"isolated": True},
            observed_check_ids=["first", "second"],
            process_provenance={
                "method": "controller-event-log",
                "host_root_pid": 100,
                "observed_process_pid": 200,
            },
            teardown={
                "service_stopped": True,
                "service_uninstalled": True,
                "operator_home_removed": True,
                "port_closed": True,
            },
            sanitization={
                "browser_payloads_scanned": True,
                "process_logs_scanned": True,
                "raw_credentials_recorded": False,
                "host_paths_recorded": False,
            },
            topology_request=None,
        )

        self.assertEqual("diagnostic_only", receipt["authority"])
        self.assertFalse(receipt["authorizes_merge"])
        self.assertEqual(["first", "second"], receipt["observed_check_ids"])
        self.assertEqual(200, receipt["process_provenance"]["observed_process_pid"])
        self.assertTrue(receipt["sanitization"]["process_logs_scanned"])
        if Draft202012Validator is not None:
            schema = json.loads(
                (
                    Path(__file__).parents[2]
                    / "schemas/operator-service-topology-receipt.schema.json"
                ).read_text(encoding="utf-8")
            )
            Draft202012Validator(schema).validate(receipt)

    def test_topology_request_must_bind_the_exact_revision_and_bot_head(self) -> None:
        revision = journey_revision()
        request = {
            "schema_version": 1,
            "repository": "BicameralAI/bicameral-bot",
            "head_sha": "a" * 40,
            "release_unit_id": revision["release_unit_id"],
            "state": "active",
            "topology": {
                "validation_checklist": revision["checkpoints"],
            },
            "activation_journey": {
                "journey_id": revision["journey_id"],
                "contract_digest": revision["journey_digest"],
                "environment_ids": revision["environment_ids"],
                "journey_revision": revision,
            },
        }
        RUNNER.validate_topology_request(request, revision, "a" * 40)

        request["head_sha"] = "b" * 40
        with self.assertRaisesRegex(
            RUNNER.TopologyPreparationError, "Bot head"
        ):
            RUNNER.validate_topology_request(request, revision, "a" * 40)


if __name__ == "__main__":
    unittest.main()
