#!/usr/bin/env python3
"""Compose exact macOS and Linux operator-service receipts for Atlas intake."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any


SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
DEVIN_SESSION = re.compile(r"^devin-[a-f0-9]{32}$")
PROFILE_ID = "operator-service-environment-v1"
LANE_BINDING_KEYS = {
    "lane_id",
    "provider_session_id",
    "recording_url",
    "artifact_url",
}
LEGACY_AUTHORITY_KEYS = {
    "authorization_kind",
    "provider",
    "execution_path",
    "grant_id",
    "grant_digest",
    "launch_ordinal",
    "provider_session_id",
    "launch_receipt_url",
    "claim_receipt_url",
}
LANE_AUTHORITY_KEYS = {
    "lane_id",
    "provider_session_id",
    "claim_receipt_url",
}
AUTHORITY_KEYS = LEGACY_AUTHORITY_KEYS | {"lane_authorities"}
VIDEO_KEYS = {
    "attempt_id",
    "topology_class",
    "journey_id",
    "journey_contract_digest",
    "capture_mode",
    "result",
    "validated_checkpoints",
    "gap_checkpoints",
    "recording_url",
    "delivery_mode",
    "slack_file_id",
    "slack_channel_id",
    "slack_permalink",
    "message_format",
    "message_sha256",
    "source_issue_url",
    "source_pr_url",
}
PROVENANCE_KEYS = {
    "bot_commit",
    "bot_binary_sha256",
    "mcp_commit",
    "mcp_executable_sha256",
    "mcp_python_sha256",
    "factory_commit",
    "environment_profile_sha256",
}


def canonical_digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _closed(value: object, keys: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{label} fields are not closed")
    return value


def _required_contract(proposal: object) -> tuple[dict, dict, list[str]]:
    if not isinstance(proposal, dict):
        raise ValueError("proposal must be one JSON object")
    unsigned = {key: item for key, item in proposal.items() if key != "proposal_digest"}
    if proposal.get("proposal_digest") != canonical_digest(unsigned):
        raise ValueError("proposal digest does not bind exact proposal")
    topology = proposal.get("topology")
    contract = topology.get("evidence_contract") if isinstance(topology, dict) else None
    lanes = (
        contract.get("required_execution_lanes") if isinstance(contract, dict) else None
    )
    if (
        not isinstance(topology, dict)
        or topology.get("required") is not True
        or not isinstance(contract, dict)
        or contract.get("runtime_path") != "authentic-runtime"
        or contract.get("initiation") != "native-service-flow"
        or not isinstance(lanes, list)
        or not lanes
        or len(lanes) != len(set(lanes))
    ):
        raise ValueError("proposal lacks a closed required execution-lane contract")
    checks = topology.get("validation_checklist")
    requirements = contract.get("check_requirements")
    check_ids = (
        [item.get("check_id") for item in checks if isinstance(item, dict)]
        if isinstance(checks, list)
        else []
    )
    requirement_ids = (
        [item.get("check_id") for item in requirements if isinstance(item, dict)]
        if isinstance(requirements, list)
        else []
    )
    if not check_ids or requirement_ids != check_ids:
        raise ValueError("proposal checklist and evidence requirements differ")
    return topology, contract, lanes


def _components(provenance: object) -> dict[str, str]:
    value = _closed(provenance, PROVENANCE_KEYS, "artifact provenance")
    components = {
        "bot": value["bot_commit"],
        "mcp": value["mcp_commit"],
        "factory": value["factory_commit"],
    }
    if any(
        not isinstance(commit, str) or not SHA.fullmatch(commit)
        for commit in components.values()
    ):
        raise ValueError("artifact provenance does not bind exact commits")
    for field in (
        "bot_binary_sha256",
        "mcp_executable_sha256",
        "mcp_python_sha256",
        "environment_profile_sha256",
    ):
        if not DIGEST.fullmatch(str(value[field])):
            raise ValueError(f"artifact provenance {field} is invalid")
    return components


def _provenance_identity(provenance: dict[str, Any]) -> dict[str, str]:
    return {
        "bot_commit": str(provenance["bot_commit"]),
        "mcp_commit": str(provenance["mcp_commit"]),
        "factory_commit": str(provenance["factory_commit"]),
        "environment_profile_sha256": str(provenance["environment_profile_sha256"]),
    }


def _validate_authority(
    authority: object,
    *,
    required_lanes: list[str],
    lane_bindings: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    value = _closed(authority, AUTHORITY_KEYS, "execution authority")
    lane_authorities = value["lane_authorities"]
    if (
        value["authorization_kind"] != "topology-execution-grant-v2"
        or value["provider"] != "devin"
        or value["execution_path"] != "devin-session-direct"
        or value["launch_ordinal"] != 1
        or not DIGEST.fullmatch(str(value["grant_digest"]))
        or not isinstance(lane_authorities, list)
        or len(lane_authorities) != len(required_lanes)
    ):
        raise ValueError("execution authority is not a bound direct Devin lane")
    expected: list[dict[str, Any]] = []
    claim_urls: set[str] = set()
    for index, lane_id in enumerate(required_lanes):
        lane = _closed(
            lane_authorities[index],
            LANE_AUTHORITY_KEYS,
            f"lane authority {index}",
        )
        binding = lane_bindings[lane_id]
        claim_url = str(lane["claim_receipt_url"])
        if (
            lane["lane_id"] != lane_id
            or lane["provider_session_id"] != binding["provider_session_id"]
            or not re.fullmatch(
                r"https://github\.com/BicameralAI/[^/]+/issues/"
                r"[1-9][0-9]*#issuecomment-[1-9][0-9]*",
                claim_url,
            )
            or claim_url in claim_urls
        ):
            raise ValueError(
                "execution authority must exactly bind ordered lane cohort"
            )
        claim_urls.add(claim_url)
        expected.append(dict(lane))
    finalizer = expected[-1]
    if (
        value["provider_session_id"] != finalizer["provider_session_id"]
        or value["claim_receipt_url"] != finalizer["claim_receipt_url"]
    ):
        raise ValueError(
            "execution authority top-level identity must bind finalizer lane"
        )
    return value


def _validate_video(
    video: object,
    *,
    authority: dict[str, Any],
    proposal: dict,
    check_ids: list[str],
) -> dict[str, Any]:
    value = _closed(video, VIDEO_KEYS, "video evidence")
    session = str(authority["provider_session_id"]).removeprefix("devin-")
    if (
        value["journey_id"] != proposal["journey_revision"]["journey_id"]
        or value["journey_contract_digest"]
        != proposal["journey_revision"]["journey_digest"]
        or value["result"] != "passed"
        or value["validated_checkpoints"] != check_ids
        or value["gap_checkpoints"] != []
        or not str(value["recording_url"]).startswith(
            f"https://app.devin.ai/sessions/{session}?"
        )
        or "testRecording=" not in str(value["recording_url"])
        or value["slack_channel_id"] != "C0B3QR6EWP5"
        or value["message_format"] != "topology-footage-v1"
    ):
        raise ValueError("video evidence does not bind the exact passed journey")
    return value


def _validate_videos(
    videos: object,
    *,
    required_lanes: list[str],
    lane_bindings: dict[str, dict[str, Any]],
    proposal: dict,
    check_ids: list[str],
) -> list[dict[str, Any]]:
    if not isinstance(videos, list) or len(videos) != len(required_lanes):
        raise ValueError("video evidence must contain one ordered lane video")
    validated: list[dict[str, Any]] = []
    slack_permalinks: set[str] = set()
    message_digests: set[str] = set()
    for index, lane_id in enumerate(required_lanes):
        video = _closed(
            videos[index],
            VIDEO_KEYS | {"lane_id"},
            f"video evidence {index}",
        )
        if (
            video["lane_id"] != lane_id
            or video["recording_url"]
            != lane_bindings[lane_id]["recording_url"]
        ):
            raise ValueError(
                "video evidence must use exact lane order and recording"
            )
        plain_video = {
            key: item for key, item in video.items() if key != "lane_id"
        }
        _validate_video(
            plain_video,
            authority={
                "provider_session_id": lane_bindings[lane_id][
                    "provider_session_id"
                ]
            },
            proposal=proposal,
            check_ids=check_ids,
        )
        if (
            str(video["slack_permalink"]) in slack_permalinks
            or str(video["message_sha256"]) in message_digests
        ):
            raise ValueError(
                "lane video Slack handoffs and message digests must be distinct"
            )
        slack_permalinks.add(str(video["slack_permalink"]))
        message_digests.add(str(video["message_sha256"]))
        validated.append(dict(video))
    return validated


def finalize(
    *,
    proposal: object,
    receipts: object,
    lane_bindings: object,
    execution_authority: object,
    video_evidence: object,
) -> dict[str, object]:
    """Build one non-authoritative candidate; Atlas computes merge authority."""
    topology, contract, required_lanes = _required_contract(proposal)
    assert isinstance(proposal, dict)
    if not isinstance(receipts, list) or not isinstance(lane_bindings, list):
        raise ValueError("receipts and lane bindings must be lists")
    receipt_lanes = [
        receipt.get("platform_lane")
        for receipt in receipts
        if isinstance(receipt, dict)
    ]
    binding_lanes = [
        binding.get("lane_id") for binding in lane_bindings if isinstance(binding, dict)
    ]
    if receipt_lanes != required_lanes or binding_lanes != required_lanes:
        raise ValueError("receipts and bindings must use exact required lane order")

    binding_by_lane: dict[str, dict[str, Any]] = {}
    lane_sessions: list[str] = []
    artifact_urls: list[str] = []
    for index, binding in enumerate(lane_bindings):
        value = _closed(
            binding,
            LANE_BINDING_KEYS,
            f"lane binding {index}",
        )
        provider_session_id = str(value["provider_session_id"])
        bare_session = provider_session_id.removeprefix("devin-")
        session_url = f"https://app.devin.ai/sessions/{bare_session}"
        if not DEVIN_SESSION.fullmatch(provider_session_id):
            raise ValueError("every lane must bind the direct Devin controller")
        if not str(value["recording_url"]).startswith(
            session_url + "?"
        ) or "testRecording=" not in str(value["recording_url"]):
            raise ValueError("lane recording does not bind its direct Devin session")
        if not str(value["artifact_url"]).startswith(session_url + "?"):
            raise ValueError("lane artifact does not bind its direct Devin session")
        lane_sessions.append(provider_session_id)
        artifact_urls.append(str(value["artifact_url"]))
        binding_by_lane[str(value["lane_id"])] = value
    if len(lane_sessions) != len(set(lane_sessions)):
        raise ValueError("every lane must bind one unique direct Devin session")
    if len(artifact_urls) != len(set(artifact_urls)):
        raise ValueError("lane artifacts must be distinct")

    authority = _validate_authority(
        execution_authority,
        required_lanes=required_lanes,
        lane_bindings=binding_by_lane,
    )
    checks = topology["validation_checklist"]
    check_ids = [check["check_id"] for check in checks]
    videos = _validate_videos(
        video_evidence,
        required_lanes=required_lanes,
        lane_bindings=binding_by_lane,
        proposal=proposal,
        check_ids=check_ids,
    )

    first_provenance_identity: dict[str, str] | None = None
    first_request: dict[str, Any] | None = None
    first_receipt_authority: str | None = None
    components: dict[str, str] | None = None
    execution_lanes: list[dict[str, object]] = []
    observed_at: list[str] = []
    for index, receipt in enumerate(receipts):
        if not isinstance(receipt, dict):
            raise ValueError(f"receipt {index} must be one JSON object")
        lane_id = str(receipt["platform_lane"])
        receipt_authority = receipt.get("authority")
        request = receipt.get("topology_request")
        if receipt_authority == "topology_execution_request":
            if not isinstance(request, dict):
                raise ValueError(
                    f"{lane_id} requested topology lacks TopologyExecutionRequest"
                )
        elif receipt_authority == "diagnostic_only":
            if request is not None:
                raise ValueError(
                    f"{lane_id} pre-controller topology unexpectedly has a request"
                )
        else:
            raise ValueError(f"{lane_id} receipt authority is invalid")
        if first_receipt_authority is None:
            first_receipt_authority = str(receipt_authority)
        elif receipt_authority != first_receipt_authority:
            raise ValueError("lane receipt authorities differ")
        if (
            receipt.get("schema_version") != 1
            or receipt.get("profile_id") != PROFILE_ID
            or receipt.get("outcome") != "passed"
            or receipt.get("authorizes_merge") is not False
            or receipt.get("observed_check_ids") != check_ids
        ):
            raise ValueError(f"{lane_id} receipt is not a passed requested topology")
        journey = receipt.get("journey_revision")
        if (
            not isinstance(journey, dict)
            or journey.get("release_unit_id") != proposal["release_unit_id"]
            or journey.get("journey_id") != proposal["journey_revision"]["journey_id"]
            or journey.get("journey_digest")
            != proposal["journey_revision"]["journey_digest"]
        ):
            raise ValueError(f"{lane_id} receipt JourneyRevision is stale")
        provenance = receipt.get("artifact_provenance")
        lane_components = _components(provenance)
        assert isinstance(provenance, dict)
        provenance_identity = _provenance_identity(provenance)
        if first_provenance_identity is None:
            first_provenance_identity = provenance_identity
            components = lane_components
        elif (
            provenance_identity != first_provenance_identity
            or lane_components != components
        ):
            raise ValueError(
                "lane artifact provenance must bind identical source commits "
                "and environment profile"
            )
        assert components is not None
        if isinstance(request, dict):
            if first_request is None:
                first_request = request
            elif request != first_request:
                raise ValueError("lane TopologyExecutionRequest values differ")
            if (
                request.get("repository") != proposal["repository"]
                or request.get("release_unit_id") != proposal["release_unit_id"]
                or request.get("head_sha") != components["bot"]
                or request.get("state") != "active"
            ):
                raise ValueError("TopologyExecutionRequest is stale or mismatched")
        teardown = receipt.get("teardown")
        if (
            not isinstance(teardown, dict)
            or not teardown
            or any(item is not True for item in teardown.values())
        ):
            raise ValueError(f"{lane_id} teardown is incomplete")
        sanitization = receipt.get("sanitization")
        if (
            not isinstance(sanitization, dict)
            or sanitization.get("browser_payloads_scanned") is not True
            or sanitization.get("process_logs_scanned") is not True
            or sanitization.get("raw_credentials_recorded") is not False
            or sanitization.get("host_paths_recorded") is not False
        ):
            raise ValueError(f"{lane_id} sanitization is incomplete")
        process = receipt.get("process_provenance")
        if (
            not isinstance(process, dict)
            or set(process) != {"method", "host_root_pid", "observed_process_pid"}
            or process.get("method") != "controller-event-log"
            or any(
                not isinstance(process.get(field), int)
                or isinstance(process.get(field), bool)
                or process[field] < 1
                for field in ("host_root_pid", "observed_process_pid")
            )
        ):
            raise ValueError(f"{lane_id} process provenance is invalid")
        binding = binding_by_lane[lane_id]
        receipt_digest = canonical_digest(receipt)
        runtime = {
            "schema_version": 1,
            "runtime_path": "authentic-runtime",
            "operator_class": "synthetic-operator",
            "initiation": "native-service-flow",
            "direct_invocation": False,
            "process_provenance": {
                **process,
                "ancestry_digest": receipt_digest,
            },
            "artifacts": [
                {
                    "kind": "reproduction-bundle",
                    "digest": receipt_digest,
                    "uri": binding["artifact_url"],
                }
            ],
            "check_observations": [
                {
                    "check_id": requirement["check_id"],
                    "observation_kind": requirement["observation_kind"],
                    "observed_count": 1,
                    "artifact_digests": [receipt_digest],
                }
                for requirement in contract["check_requirements"]
            ],
        }
        execution_lanes.append(
            {
                "lane_id": lane_id,
                "provider_session_id": binding["provider_session_id"],
                "run_url": (
                    "https://app.devin.ai/sessions/"
                    + str(binding["provider_session_id"]).removeprefix("devin-")
                ),
                "recording_url": binding["recording_url"],
                "component_commits": dict(components),
                "proposal_digest": proposal["proposal_digest"],
                "outcome": "passed",
                "terminal_evidence_complete": True,
                "teardown_complete": True,
                "sanitization_complete": True,
                "runtime_evidence": runtime,
            }
        )
        if not isinstance(receipt.get("observed_at"), str):
            raise ValueError(f"{lane_id} observed_at is invalid")
        observed_at.append(receipt["observed_at"])

    assert components is not None
    candidate: dict[str, object] = {
        "schema_version": 2,
        "status": "passed",
        "evidence_level": "trusted-controller-runtime-evidence-v2",
        "terminal_evidence_complete": True,
        "terminal_user_journey_acceptance": False,
        "components": components,
        "release_unit": {
            "release_unit_id": proposal["release_unit_id"],
            "topology_type": topology["topology_type"],
            "proposal_digest": proposal["proposal_digest"],
            "validation_checklist": [
                {"check_id": check_id, "outcome": "passed"} for check_id in check_ids
            ],
        },
        "runtime_evidence": {
            "schema_version": 2,
            "runtime_path": "authentic-runtime",
            "operator_class": "synthetic-operator",
            "initiation": "native-service-flow",
            "direct_invocation": False,
            "execution_lanes": execution_lanes,
        },
        "sanitization": {
            "browser_payloads_scanned": True,
            "process_logs_scanned": True,
            "raw_credentials_recorded": False,
            "host_paths_recorded": False,
        },
        "cleanup": {
            "processes_stopped": True,
            "supabase_stopped": True,
            "containers_remaining": [],
            "ports_closed": True,
            "temporary_root_removed": True,
            "credentials_removed_with_temporary_root": True,
        },
        "finished_at": max(observed_at),
        "execution_authority": dict(authority),
        "video_evidence": [dict(video) for video in videos],
    }
    return candidate


def _load(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proposal", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, action="append", required=True)
    parser.add_argument("--lane-bindings", type=Path, required=True)
    parser.add_argument("--execution-authority", type=Path, required=True)
    parser.add_argument("--video-evidence", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = finalize(
        proposal=_load(args.proposal),
        receipts=[_load(path) for path in args.receipt],
        lane_bindings=_load(args.lane_bindings),
        execution_authority=_load(args.execution_authority),
        video_evidence=_load(args.video_evidence),
    )
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
