"""Tests for the dual-platform operator-service topology finalizer."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "finalize_operator_service_topology.py"
SPEC = importlib.util.spec_from_file_location(
    "finalize_operator_service_topology",
    SCRIPT,
)
assert SPEC and SPEC.loader
FINALIZER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = FINALIZER
SPEC.loader.exec_module(FINALIZER)


def digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def proposal() -> dict:
    value = {
        "schema_version": 2,
        "release_unit_id": "v0114-operator-scoped-multi-product-daemon-r3",
        "label": "Operator service topology",
        "repository": "BicameralAI/bicameral-bot",
        "scope_ref": "BicameralAI/bicameral-bot#908",
        "release_unit_type": "bot_daemon",
        "target_version": "0.1.14",
        "topology": {
            "required": True,
            "topology_type": "daemon_service_multi_product_v1",
            "validation_checklist": [
                {"check_id": "first", "label": "First native observation"},
                {"check_id": "second", "label": "Second native observation"},
            ],
            "evidence_contract": {
                "schema_version": 1,
                "runtime_path": "authentic-runtime",
                "operator_policy": "synthetic-or-human",
                "initiation": "native-service-flow",
                "artifact_policy": "raw-or-reproducible",
                "required_execution_lanes": [
                    "macos-launchd-user-v1",
                    "linux-systemd-user-v1",
                ],
                "check_requirements": [
                    {
                        "check_id": "first",
                        "observation_kind": "native-service-event",
                        "minimum": 1,
                        "maximum": 1,
                    },
                    {
                        "check_id": "second",
                        "observation_kind": "runtime-outcome",
                        "minimum": 1,
                        "maximum": 1,
                    },
                ],
            },
        },
        "journey_revision": {
            "comment_id": 5130404214,
            "journey_id": "daemon_service_multi_product_v1",
            "journey_digest": "sha256:" + "1" * 64,
        },
        "proposed_by": "collaborator:jinhongkuan",
        "proposed_at": "2026-07-30T12:39:00Z",
    }
    value["proposal_digest"] = digest(value)
    return value


def raw_receipt(lane_id: str, *, observed_pid: int) -> dict:
    value = proposal()
    topology_request = {
        "schema_version": 1,
        "repository": "BicameralAI/bicameral-bot",
        "head_sha": "a" * 40,
        "release_unit_id": value["release_unit_id"],
        "state": "active",
        "topology": value["topology"],
        "activation_journey": {
            "journey_id": value["journey_revision"]["journey_id"],
            "contract_digest": value["journey_revision"]["journey_digest"],
            "environment_ids": [
                "operator-service-environment-v1",
                "local-product-environment-v1",
            ],
            "journey_revision": {"journey_digest": "sha256:" + "1" * 64},
        },
    }
    return {
        "$schema": "../schemas/operator-service-topology-receipt.schema.json",
        "schema_version": 1,
        "profile_id": "operator-service-environment-v1",
        "platform_lane": lane_id,
        "journey_revision": {
            "release_unit_id": value["release_unit_id"],
            "journey_id": value["journey_revision"]["journey_id"],
            "journey_digest": value["journey_revision"]["journey_digest"],
            "environment_ids": [
                "operator-service-environment-v1",
                "local-product-environment-v1",
            ],
        },
        "artifact_provenance": {
            "bot_commit": "a" * 40,
            "bot_binary_sha256": "sha256:" + "b" * 64,
            "mcp_commit": "c" * 40,
            "mcp_executable_sha256": "sha256:" + "d" * 64,
            "mcp_python_sha256": "sha256:" + "e" * 64,
            "factory_commit": "f" * 40,
            "environment_profile_sha256": "sha256:" + "2" * 64,
        },
        "process_provenance": {
            "method": "controller-event-log",
            "host_root_pid": 100,
            "observed_process_pid": observed_pid,
        },
        "service_observations": {"native_manager": True},
        "product_observations": {"isolated": True},
        "observed_check_ids": ["first", "second"],
        "teardown": {
            "service_stopped": True,
            "service_uninstalled": True,
            "operator_home_removed": True,
            "port_closed": True,
        },
        "sanitization": {
            "browser_payloads_scanned": True,
            "process_logs_scanned": True,
            "raw_credentials_recorded": False,
            "host_paths_recorded": False,
        },
        "outcome": "passed",
        "authority": "topology_execution_request",
        "authorizes_merge": False,
        "topology_request": topology_request,
        "observed_at": "2026-07-31T10:00:00Z",
    }


def lane_bindings() -> list[dict]:
    return [
        {
            "lane_id": "macos-launchd-user-v1",
            "provider_session_id": "devin-" + "1" * 32,
            "recording_url": (
                "https://app.devin.ai/sessions/" + "1" * 32 + "?testRecording=macos"
            ),
            "artifact_url": (
                "https://app.devin.ai/sessions/"
                + "1" * 32
                + "?artifact=operator-service-receipt"
            ),
        },
        {
            "lane_id": "linux-systemd-user-v1",
            "provider_session_id": "devin-" + "2" * 32,
            "recording_url": (
                "https://app.devin.ai/sessions/" + "2" * 32 + "?testRecording=linux"
            ),
            "artifact_url": (
                "https://app.devin.ai/sessions/"
                + "2" * 32
                + "?artifact=operator-service-linux-receipt"
            ),
        },
    ]


def authority() -> dict:
    return {
        "authorization_kind": "topology-execution-grant-v2",
        "provider": "devin",
        "execution_path": "devin-session-direct",
        "grant_id": "teg_operator_service_aggregate_20260731",
        "grant_digest": "sha256:" + "3" * 64,
        "launch_ordinal": 1,
        "provider_session_id": "devin-" + "2" * 32,
        "launch_receipt_url": (
            "https://github.com/BicameralAI/bicameral-bot/"
            "issues/908#issuecomment-5136000001"
        ),
        "claim_receipt_url": (
            "https://github.com/BicameralAI/bicameral-bot/"
            "issues/908#issuecomment-5136000003"
        ),
        "lane_authorities": [
            {
                "lane_id": "macos-launchd-user-v1",
                "provider_session_id": "devin-" + "1" * 32,
                "claim_receipt_url": (
                    "https://github.com/BicameralAI/bicameral-bot/"
                    "issues/908#issuecomment-5136000002"
                ),
            },
            {
                "lane_id": "linux-systemd-user-v1",
                "provider_session_id": "devin-" + "2" * 32,
                "claim_receipt_url": (
                    "https://github.com/BicameralAI/bicameral-bot/"
                    "issues/908#issuecomment-5136000003"
                ),
            },
        ],
    }


def video() -> list[dict]:
    return [
        {
            "lane_id": binding["lane_id"],
            "attempt_id": f"20260731T10000{index}Z-{index + 1:08x}",
            "topology_class": "operator-service-environment-v1",
            "journey_id": "daemon_service_multi_product_v1",
            "journey_contract_digest": "sha256:" + "1" * 64,
            "capture_mode": "headed-live-journey",
            "result": "passed",
            "validated_checkpoints": ["first", "second"],
            "gap_checkpoints": [],
            "recording_url": binding["recording_url"],
            "delivery_mode": "slack-upload",
            "slack_file_id": f"F012ABCDE{index}",
            "slack_channel_id": "C0B3QR6EWP5",
            "slack_permalink": (
                "https://bicameralgroup.slack.com/archives/"
                f"C0B3QR6EWP5/p178550000000000{index}"
            ),
            "message_format": "topology-footage-v1",
            "message_sha256": "sha256:" + str(index + 4) * 64,
            "source_issue_url": (
                "https://github.com/BicameralAI/bicameral-bot/issues/908"
            ),
            "source_pr_url": (
                "https://github.com/BicameralAI/bicameral-bot/pull/945"
            ),
        }
        for index, binding in enumerate(lane_bindings())
    ]


class FinalizeOperatorServiceTopologyTests(unittest.TestCase):
    def test_composes_exact_dual_lane_candidate(self) -> None:
        value = proposal()
        receipts = [
            raw_receipt("macos-launchd-user-v1", observed_pid=101),
            raw_receipt("linux-systemd-user-v1", observed_pid=202),
        ]
        receipts[1]["artifact_provenance"]["bot_binary_sha256"] = "sha256:" + "8" * 64
        receipts[1]["artifact_provenance"]["mcp_executable_sha256"] = (
            "sha256:" + "9" * 64
        )

        result = FINALIZER.finalize(
            proposal=value,
            receipts=receipts,
            lane_bindings=lane_bindings(),
            execution_authority=authority(),
            video_evidence=video(),
        )

        self.assertEqual(2, result["schema_version"])
        self.assertEqual(
            ["macos-launchd-user-v1", "linux-systemd-user-v1"],
            [lane["lane_id"] for lane in result["runtime_evidence"]["execution_lanes"]],
        )
        self.assertEqual(
            {"bot": "a" * 40, "mcp": "c" * 40, "factory": "f" * 40},
            result["components"],
        )
        self.assertFalse(result["terminal_user_journey_acceptance"])
        self.assertNotIn("evidence_class", result)
        self.assertNotIn("authorizes_merge", result)

    def test_composes_pre_controller_grant_bound_dual_lane_candidate(self) -> None:
        value = proposal()
        receipts = [
            raw_receipt("macos-launchd-user-v1", observed_pid=101),
            raw_receipt("linux-systemd-user-v1", observed_pid=202),
        ]
        for receipt in receipts:
            receipt["authority"] = "diagnostic_only"
            receipt["topology_request"] = None

        result = FINALIZER.finalize(
            proposal=value,
            receipts=receipts,
            lane_bindings=lane_bindings(),
            execution_authority=authority(),
            video_evidence=video(),
        )

        self.assertEqual("passed", result["status"])
        self.assertEqual(authority(), result["execution_authority"])
        self.assertEqual(
            ["macos-launchd-user-v1", "linux-systemd-user-v1"],
            [lane["lane_id"] for lane in result["runtime_evidence"]["execution_lanes"]],
        )

    def test_rejects_mixed_request_and_pre_controller_lane_authority(self) -> None:
        value = proposal()
        receipts = [
            raw_receipt("macos-launchd-user-v1", observed_pid=101),
            raw_receipt("linux-systemd-user-v1", observed_pid=202),
        ]
        receipts[0]["authority"] = "diagnostic_only"
        receipts[0]["topology_request"] = None

        with self.assertRaisesRegex(ValueError, "lane receipt authorities differ"):
            FINALIZER.finalize(
                proposal=value,
                receipts=receipts,
                lane_bindings=lane_bindings(),
                execution_authority=authority(),
                video_evidence=video(),
            )

    def test_rejects_misaligned_lane_authority_or_video(self) -> None:
        value = proposal()
        receipts = [
            raw_receipt("macos-launchd-user-v1", observed_pid=101),
            raw_receipt("linux-systemd-user-v1", observed_pid=202),
        ]

        wrong_authority = authority()
        wrong_authority["lane_authorities"] = list(
            reversed(wrong_authority["lane_authorities"])
        )
        with self.assertRaisesRegex(ValueError, "ordered lane cohort"):
            FINALIZER.finalize(
                proposal=value,
                receipts=receipts,
                lane_bindings=lane_bindings(),
                execution_authority=wrong_authority,
                video_evidence=video(),
            )

        duplicate_handoff = video()
        duplicate_handoff[1]["slack_permalink"] = duplicate_handoff[0][
            "slack_permalink"
        ]
        with self.assertRaisesRegex(ValueError, "handoffs"):
            FINALIZER.finalize(
                proposal=value,
                receipts=receipts,
                lane_bindings=lane_bindings(),
                execution_authority=authority(),
                video_evidence=duplicate_handoff,
            )

        wrong_video_lane = video()
        wrong_video_lane[1]["lane_id"] = "macos-launchd-user-v1"
        with self.assertRaisesRegex(ValueError, "lane order"):
            FINALIZER.finalize(
                proposal=value,
                receipts=receipts,
                lane_bindings=lane_bindings(),
                execution_authority=authority(),
                video_evidence=wrong_video_lane,
            )

    def test_rejects_missing_stale_or_unsafe_lane(self) -> None:
        value = proposal()
        receipts = [
            raw_receipt("macos-launchd-user-v1", observed_pid=101),
            raw_receipt("linux-systemd-user-v1", observed_pid=202),
        ]
        cases: list[tuple[str, list[dict], list[dict]]] = []

        cases.append(("exact required lane order", receipts[:1], lane_bindings()[:1]))

        mixed = copy.deepcopy(receipts)
        mixed[1]["artifact_provenance"]["mcp_commit"] = "9" * 40
        cases.append(("artifact provenance", mixed, lane_bindings()))

        stale = copy.deepcopy(receipts)
        stale[1]["topology_request"]["head_sha"] = "9" * 40
        cases.append(("TopologyExecutionRequest", stale, lane_bindings()))

        dirty = copy.deepcopy(receipts)
        dirty[1]["teardown"]["port_closed"] = False
        cases.append(("teardown", dirty, lane_bindings()))

        split_controller = lane_bindings()
        split_controller[1]["provider_session_id"] = "devin-" + "1" * 32
        split_controller[1]["recording_url"] = (
            "https://app.devin.ai/sessions/" + "1" * 32 + "?testRecording=linux"
        )
        split_controller[1]["artifact_url"] = (
            "https://app.devin.ai/sessions/"
            + "1" * 32
            + "?artifact=operator-service-linux-receipt"
        )
        cases.append(("unique direct Devin session", receipts, split_controller))

        bad_recording = lane_bindings()
        bad_recording[1]["recording_url"] = (
            "https://app.devin.ai/sessions/" + "2" * 32 + "?testRecording=wrong"
        )
        cases.append(("recording", receipts, bad_recording))

        for expected, candidate_receipts, bindings in cases:
            with (
                self.subTest(expected=expected),
                self.assertRaisesRegex(
                    ValueError,
                    expected,
                ),
            ):
                FINALIZER.finalize(
                    proposal=value,
                    receipts=candidate_receipts,
                    lane_bindings=bindings,
                    execution_authority=authority(),
                    video_evidence=video(),
                )


if __name__ == "__main__":
    unittest.main()
