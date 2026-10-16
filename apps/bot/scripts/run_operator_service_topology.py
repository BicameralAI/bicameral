#!/usr/bin/env python3
"""Run an issue-owned journey in the reusable operator-service environment.

The runner uses the shipped onboarding/service CLI, the native user service
manager, production Product creation/migration paths, and the production MCP
stdio server. A local invocation is diagnostic. Only an exact activated
TopologyExecutionRequest can change the receipt authority label, and the
release-unit finalizer remains the merge/release authority.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Mapping, Sequence


PROFILE_ID = "operator-service-environment-v1"
LOCAL_ENVIRONMENT_ID = "local-product-environment-v1"
SHA = re.compile(r"^[a-f0-9]{40}$")
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")


class TopologyPreparationError(RuntimeError):
    """A sanitized fail-closed preparation failure."""


class TopologyExecutionError(RuntimeError):
    """A sanitized failure after the first production service starts."""


def require_direct_devin_execution(env: Mapping[str, str]) -> None:
    if env.get("GITHUB_ACTIONS", "").lower() == "true":
        raise TopologyPreparationError(
            "topology production execution is restricted to direct Devin"
        )
    if env.get("BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER") != "devin":
        raise TopologyPreparationError(
            "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER must identify direct Devin"
        )


@dataclass(frozen=True)
class JourneyBinding:
    release_unit_id: str
    journey_id: str
    digest: str
    environment_ids: tuple[str, ...]
    check_ids: tuple[str, ...]


@dataclass(frozen=True)
class PlatformLane:
    lane_id: str
    service_manager: str


@dataclass(frozen=True)
class ServiceCommands:
    install: tuple[str, ...]
    start: tuple[str, ...]
    status: tuple[str, ...]
    stop: tuple[str, ...]
    uninstall: tuple[str, ...]


def _canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def _load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise TopologyPreparationError(f"{label} is unavailable or invalid") from exc
    if not isinstance(value, dict):
        raise TopologyPreparationError(f"{label} must be a JSON object")
    return value


def load_product_watcher_states(operator_home: Path) -> dict[str, dict[str, Any]]:
    path = (
        operator_home
        / ".bicameral"
        / "product-watchers"
        / "watcher-state-v1.json"
    )
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise TopologyExecutionError(
            "durable Product watcher state unavailable"
        ) from exc
    if not isinstance(value, dict) or set(value) != {"schema_version", "records"}:
        raise TopologyExecutionError("durable Product watcher envelope is not closed")
    if value["schema_version"] != 1 or not isinstance(value["records"], dict):
        raise TopologyExecutionError("durable Product watcher schema is unsupported")

    required = {
        "product_id",
        "workspace_root",
        "lifecycle",
        "checkpoint_head_sha",
        "queued_work",
        "failure_backlog",
    }
    lifecycles = {"current", "stale", "failed", "disabled"}
    records: dict[str, dict[str, Any]] = {}
    for product_id, state in value["records"].items():
        if not isinstance(product_id, str) or not isinstance(state, dict):
            raise TopologyExecutionError("durable Product watcher record is invalid")
        if set(state) != required:
            raise TopologyExecutionError("durable Product watcher record is not closed")
        if state["product_id"] != product_id:
            raise TopologyExecutionError(
                "durable Product watcher Product identity mismatch"
            )
        if state["lifecycle"] not in lifecycles:
            raise TopologyExecutionError(
                "durable Product watcher lifecycle is invalid"
            )
        if not isinstance(state["queued_work"], list) or not isinstance(
            state["failure_backlog"], list
        ):
            raise TopologyExecutionError(
                "durable Product watcher work or failure backlog is invalid"
            )
        records[product_id] = state
    return records


def _wait_product_watcher_lifecycles(
    operator_home: Path,
    expected: dict[str, str],
    timeout: float = 15.0,
) -> dict[str, dict[str, Any]]:
    deadline = time.monotonic() + timeout
    last: dict[str, dict[str, Any]] = {}
    while time.monotonic() < deadline:
        try:
            last = load_product_watcher_states(operator_home)
        except TopologyExecutionError:
            time.sleep(0.2)
            continue
        if all(
            last.get(product_id, {}).get("lifecycle") == lifecycle
            for product_id, lifecycle in expected.items()
        ):
            return last
        time.sleep(0.2)
    observed = {
        product_id: last.get(product_id, {}).get("lifecycle")
        for product_id in expected
    }
    raise TopologyExecutionError(
        f"Product watcher lifecycle timeout: expected={expected} observed={observed}"
    )


def validate_journey_revision(value: dict[str, Any]) -> JourneyBinding:
    required = {
        "schema_version",
        "idempotency_key",
        "release_unit_id",
        "journey_id",
        "topology_type",
        "actor",
        "entrypoint",
        "observable_success",
        "surfaces",
        "boundary_contracts",
        "environment_ids",
        "checkpoints",
        "created_by",
        "created_at",
        "journey_digest",
    }
    if set(value) != required or value.get("schema_version") != 1:
        raise TopologyPreparationError("JourneyRevision has an unsupported shape")
    unsigned = {key: item for key, item in value.items() if key != "journey_digest"}
    digest = value.get("journey_digest")
    if not isinstance(digest, str) or digest != _canonical_digest(unsigned):
        raise TopologyPreparationError("JourneyRevision digest does not match its body")
    environments = value.get("environment_ids")
    if (
        not isinstance(environments, list)
        or len(environments) != len(set(environments))
        or not all(isinstance(item, str) for item in environments)
    ):
        raise TopologyPreparationError("JourneyRevision environment_ids are invalid")
    missing = {PROFILE_ID, LOCAL_ENVIRONMENT_ID} - set(environments)
    if missing:
        raise TopologyPreparationError(
            "JourneyRevision is missing a required environment"
        )
    checkpoints = value.get("checkpoints")
    if not isinstance(checkpoints, list) or not checkpoints:
        raise TopologyPreparationError("JourneyRevision has no checkpoints")
    check_ids = tuple(
        check.get("check_id") for check in checkpoints if isinstance(check, dict)
    )
    if (
        len(check_ids) != len(checkpoints)
        or len(check_ids) != len(set(check_ids))
        or not all(isinstance(item, str) and item for item in check_ids)
    ):
        raise TopologyPreparationError("JourneyRevision checkpoint ids are invalid")
    if value.get("journey_id") != value.get("topology_type"):
        raise TopologyPreparationError(
            "JourneyRevision journey_id and topology_type differ"
        )
    return JourneyBinding(
        release_unit_id=str(value["release_unit_id"]),
        journey_id=str(value["journey_id"]),
        digest=digest,
        environment_ids=tuple(environments),
        check_ids=check_ids,
    )


def platform_lane(platform: str | None = None) -> PlatformLane:
    selected = platform or sys.platform
    if selected == "darwin":
        return PlatformLane("macos-launchd-user-v1", "launchd-user")
    if selected.startswith("linux"):
        return PlatformLane("linux-systemd-user-v1", "systemd-user")
    raise TopologyPreparationError(
        f"unsupported platform for native user service: {selected}"
    )


def service_commands(binary: Path) -> ServiceCommands:
    prefix = (str(binary),)
    return ServiceCommands(
        install=prefix + ("onboard", "--install-daemon"),
        start=prefix + ("service", "start"),
        status=prefix + ("service", "status"),
        stop=prefix + ("service", "stop"),
        uninstall=prefix + ("service", "uninstall"),
    )


def validate_mcp_product_probe(payload: dict[str, Any], product_id: str) -> None:
    names = payload.get("tool_names")
    if not isinstance(names, list) or "bicameral.product.select" not in names:
        raise TopologyPreparationError(
            "production MCP does not advertise bicameral.product.select"
        )
    if payload.get("selected_product_id") != product_id:
        raise TopologyPreparationError("MCP product.select did not retain selection")
    request_ids = payload.get("request_product_ids")
    if (
        not isinstance(request_ids, list)
        or not request_ids
        or any(item != product_id for item in request_ids)
    ):
        raise TopologyPreparationError(
            "a Product-scoped MCP request omitted or changed product_id"
        )


def validate_topology_request(
    request: dict[str, Any], revision: dict[str, Any], bot_head: str
) -> None:
    binding = validate_journey_revision(revision)
    if (
        request.get("schema_version") != 1
        or request.get("repository") != "BicameralAI/bicameral-bot"
        or request.get("release_unit_id") != binding.release_unit_id
        or request.get("state") not in {"active", "satisfied"}
    ):
        raise TopologyPreparationError(
            "TopologyExecutionRequest does not bind this Release Unit"
        )
    if request.get("head_sha") != bot_head:
        raise TopologyPreparationError(
            "TopologyExecutionRequest Bot head differs from the tested artifact"
        )
    activation = request.get("activation_journey")
    if (
        not isinstance(activation, dict)
        or activation.get("journey_id") != binding.journey_id
        or activation.get("contract_digest") != binding.digest
        or activation.get("environment_ids") != list(binding.environment_ids)
        or activation.get("journey_revision") != revision
    ):
        raise TopologyPreparationError(
            "TopologyExecutionRequest does not bind the exact JourneyRevision"
        )
    topology = request.get("topology")
    checks = topology.get("validation_checklist") if isinstance(topology, dict) else None
    check_ids = (
        [item.get("check_id") for item in checks if isinstance(item, dict)]
        if isinstance(checks, list)
        else []
    )
    if check_ids != list(binding.check_ids):
        raise TopologyPreparationError(
            "TopologyExecutionRequest checklist differs from JourneyRevision"
        )


def build_receipt(
    *,
    revision: dict[str, Any],
    platform: str,
    artifact_provenance: dict[str, str],
    service_observations: dict[str, Any],
    product_observations: dict[str, Any],
    observed_check_ids: list[str],
    process_provenance: dict[str, Any],
    teardown: dict[str, Any],
    sanitization: dict[str, Any],
    topology_request: dict[str, Any] | None,
    outcome: str = "passed",
) -> dict[str, Any]:
    binding = validate_journey_revision(revision)
    lane = platform_lane(platform)
    if tuple(observed_check_ids) != binding.check_ids:
        raise TopologyExecutionError(
            "observed check ids do not exactly match JourneyRevision order"
        )
    authority = (
        "topology_execution_request"
        if topology_request is not None
        else "diagnostic_only"
    )
    return {
        "$schema": "../schemas/operator-service-topology-receipt.schema.json",
        "schema_version": 1,
        "profile_id": PROFILE_ID,
        "platform_lane": lane.lane_id,
        "journey_revision": {
            "release_unit_id": binding.release_unit_id,
            "journey_id": binding.journey_id,
            "journey_digest": binding.digest,
            "environment_ids": list(binding.environment_ids),
        },
        "artifact_provenance": artifact_provenance,
        "process_provenance": process_provenance,
        "service_observations": service_observations,
        "product_observations": product_observations,
        "observed_check_ids": observed_check_ids,
        "teardown": teardown,
        "sanitization": sanitization,
        "outcome": outcome,
        "authority": authority,
        "authorizes_merge": False,
        "topology_request": topology_request,
        "observed_at": (
            datetime.now(timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z")
        ),
    }


def _run(
    command: Sequence[str],
    *,
    env: dict[str, str] | None = None,
    cwd: Path | None = None,
    timeout: int = 120,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(command),
            env=env,
            cwd=cwd,
            text=True,
            capture_output=True,
            timeout=timeout,
            check=check,
        )
    except subprocess.TimeoutExpired as exc:
        raise TopologyExecutionError("production command timed out") from exc
    except subprocess.CalledProcessError as exc:
        tail = (exc.stderr or exc.stdout or "").strip().splitlines()[-8:]
        detail = " | ".join(line[:240] for line in tail)
        raise TopologyExecutionError(
            f"production command failed with exit {exc.returncode}: {detail}"
        ) from exc


def _git_sha(repository: Path) -> str:
    return _run(["git", "-C", str(repository), "rev-parse", "HEAD"]).stdout.strip()


def _require_commit(repository: Path, expected: str, label: str) -> None:
    if not SHA.fullmatch(expected) or _git_sha(repository) != expected:
        raise TopologyPreparationError(f"{label} checkout is not at the exact commit")
    dirty = _run(
        ["git", "-C", str(repository), "status", "--porcelain"],
    ).stdout
    if dirty:
        raise TopologyPreparationError(f"{label} checkout has uncommitted changes")


def _mcp_source_ready(mcp_repository: Path) -> None:
    try:
        tool_request = (mcp_repository / "tool_request.py").read_text(
            encoding="utf-8"
        )
        server = (mcp_repository / "server.py").read_text(encoding="utf-8")
    except OSError as exc:
        raise TopologyPreparationError(
            "MCP checkout lacks the production routing sources"
        ) from exc
    if "product_id" not in tool_request or "bicameral.product.select" not in server:
        raise TopologyPreparationError(
            "MCP checkout lacks daemon-validated product.select/product_id routing"
        )


def _port_available(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        return sock.connect_ex(("127.0.0.1", port)) != 0


def _allocate_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _native_service_absent(lane: PlatformLane, service_instance: str) -> None:
    if lane.service_manager == "launchd-user":
        uid = os.getuid()
        label = f"com.bicameral.daemon.{service_instance}"
        result = _run(
            ["launchctl", "print", f"gui/{uid}/{label}"],
            check=False,
        )
        if result.returncode == 0:
            raise TopologyPreparationError(
                "the disposable launchd service identity already exists"
            )
    else:
        unit = f"bicameral-{service_instance}.service"
        result = _run(
            [
                "systemctl",
                "--user",
                "show",
                unit,
                "--property=LoadState",
                "--value",
            ],
            check=False,
        )
        if result.returncode == 0 and result.stdout.strip() not in {"", "not-found"}:
            raise TopologyPreparationError(
                "the disposable systemd service identity already exists"
            )


def _native_service_pid(lane: PlatformLane, service_instance: str) -> int:
    if lane.service_manager == "launchd-user":
        uid = os.getuid()
        label = f"com.bicameral.daemon.{service_instance}"
        result = _run(["launchctl", "print", f"gui/{uid}/{label}"])
        match = re.search(r"(?m)^\s*pid = ([1-9][0-9]*)\s*$", result.stdout)
        value = match.group(1) if match else ""
    else:
        unit = f"bicameral-{service_instance}.service"
        result = _run(
            [
                "systemctl",
                "--user",
                "show",
                unit,
                "--property=MainPID",
                "--value",
            ]
        )
        value = result.stdout.strip()
    if not value.isdigit() or int(value) < 1:
        raise TopologyExecutionError(
            "native service manager did not report a running process id"
        )
    return int(value)


_UNSAFE_LOG_PATTERN = re.compile(
    rb"(?i)(authorization:\s*bearer\s+\S+|"
    rb"(?:gh[pousr]|github_pat)_[A-Za-z0-9_]{12,}|"
    rb"(?:sk|xox[baprs])-[A-Za-z0-9_-]{12,})"
)


def _scan_process_logs(paths: Sequence[Path]) -> None:
    for root in paths:
        if not root.exists():
            continue
        candidates = [root] if root.is_file() else root.rglob("*")
        for path in candidates:
            if not path.is_file():
                continue
            try:
                payload = path.read_bytes()
            except OSError as exc:
                raise TopologyExecutionError(
                    "process-log sanitization scan could not read an artifact"
                ) from exc
            if _UNSAFE_LOG_PATTERN.search(payload):
                raise TopologyExecutionError(
                    "process-log sanitization detected credential-shaped content"
                )


def _wait_health(port: int, timeout: float = 30.0) -> None:
    import urllib.error
    import urllib.request

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/health", timeout=2
            ) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError):
            time.sleep(0.25)
    raise TopologyExecutionError("native service did not become healthy")


def _operational_fingerprint(product_id: str) -> str:
    return hashlib.sha256(f"product_id:{product_id}".encode()).hexdigest()[:16]


def _service_env(
    operator_home: Path, service_instance: str, gateway_port: int
) -> dict[str, str]:
    return {
        **os.environ,
        "HOME": str(operator_home),
        "RUST_LOG": "info",
        "BICAMERAL_SERVICE_INSTANCE": service_instance,
        "BICAMERAL_GATEWAY_PORT": str(gateway_port),
    }


def _call_service(
    command: tuple[str, ...],
    operator_home: Path,
    service_instance: str,
    gateway_port: int,
) -> str:
    return _run(
        command,
        env=_service_env(operator_home, service_instance, gateway_port),
    ).stdout.strip()


_MCP_PROBE = r"""from __future__ import annotations
import asyncio, json, os, sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from mcp.types import TextContent

async def call(session, name, arguments):
    result = await session.call_tool(name, arguments)
    return {
        "name": name,
        "is_error": bool(result.isError),
        "content": [item.text for item in result.content if isinstance(item, TextContent)],
    }

async def run():
    executable, product_id, workspace, marker, mode = sys.argv[1:6]
    server = StdioServerParameters(command=executable, args=["serve"], env=dict(os.environ))
    async with stdio_client(server) as (reader, writer):
        async with ClientSession(reader, writer) as session:
            await session.initialize()
            tools = await session.list_tools()
            calls = [await call(session, "bicameral.product.select", {"product_id": product_id})]
            if mode == "bind-ingest-search":
                calls.append(await call(session, "bicameral.workspace.bind", {
                    "project_id": product_id,
                    "candidate_path": workspace,
                    "confirmed": True,
                    "display_name": product_id,
                    "reason": "operator-service topology binding",
                    "confidence": 1.0
                }))
                calls.append(await call(session, "bicameral.ingest", {
                    "source_uri": "topology://" + marker,
                    "source_type": "manual",
                    "title": marker,
                    "description": marker,
                    "snapshot_content": marker,
                    "evidence": [{"excerpt": marker}]
                }))
            calls.append(await call(session, "bicameral.search", {
                "query": marker, "scope": "all", "limit": 10
            }))
            print(json.dumps({
                "tool_names": sorted(tool.name for tool in tools.tools),
                "selected_product_id": product_id,
                "calls": calls
            }))

asyncio.run(run())
"""


def _run_mcp_probe(
    *,
    executable: Path,
    operator_home: Path,
    workspace: Path,
    product_id: str,
    marker: str,
    mode: str,
    work_root: Path,
    daemon_port: int,
) -> dict[str, Any]:
    probe = work_root / "mcp-product-probe.py"
    if not probe.exists():
        probe.write_text(_MCP_PROBE, encoding="utf-8")
    env = {
        **os.environ,
        "HOME": str(operator_home),
        "BICAMERAL_DAEMON_URL": f"http://127.0.0.1:{daemon_port}",
        "BICAMERAL_ACTOR_ID": "operator-service-topology",
        "BICAMERAL_WORKSPACE": str(workspace),
        "BICAMERAL_POLICY_SCOPE": "default",
    }
    result = _run(
        [
            str(executable.parent / "python"),
            str(probe),
            str(executable),
            product_id,
            str(workspace),
            marker,
            mode,
        ],
        env=env,
    )
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise TopologyExecutionError("production MCP probe returned invalid JSON") from exc
    if not isinstance(payload, dict):
        raise TopologyExecutionError("production MCP probe returned invalid shape")
    if any(
        not isinstance(call, dict) or call.get("is_error") is not False
        for call in payload.get("calls", [])
    ):
        raise TopologyExecutionError("production MCP Product operation failed")
    return payload


def _observed_request_fingerprint_counts(operator_home: Path) -> dict[str, int]:
    directory = operator_home / ".bicameral" / "operational-logs"
    observed: dict[str, int] = {}
    for path in directory.glob("*.json"):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        fingerprint = value.get("product_id_fingerprint")
        if isinstance(fingerprint, str):
            observed[fingerprint] = observed.get(fingerprint, 0) + 1
    return observed


def _sanitize_product_id(product_id: str) -> str:
    return "".join(
        char if char.isascii() and (char.isalnum() or char in "-_.") else "_"
        for char in product_id
    )


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journey-revision", type=Path, required=True)
    parser.add_argument("--bot-repo", type=Path, required=True)
    parser.add_argument("--bot-commit", required=True)
    parser.add_argument("--bicameral-binary", type=Path, required=True)
    parser.add_argument("--mcp-repo", type=Path, required=True)
    parser.add_argument("--mcp-commit", required=True)
    parser.add_argument("--mcp-executable", type=Path, required=True)
    parser.add_argument("--factory-repo", type=Path, required=True)
    parser.add_argument("--factory-commit", required=True)
    parser.add_argument("--topology-request", type=Path)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="Validate immutable inputs without starting a production process.",
    )
    return parser.parse_args()


def _validate_inputs(args: argparse.Namespace) -> tuple[
    dict[str, Any], JourneyBinding, PlatformLane, dict[str, str]
]:
    revision = _load_json(args.journey_revision, "JourneyRevision")
    binding = validate_journey_revision(revision)
    lane = platform_lane()
    _require_commit(args.bot_repo, args.bot_commit, "Bot")
    _require_commit(args.mcp_repo, args.mcp_commit, "MCP")
    _require_commit(args.factory_repo, args.factory_commit, "Factory")
    if not args.bicameral_binary.is_file() or not os.access(
        args.bicameral_binary, os.X_OK
    ):
        raise TopologyPreparationError("Bot artifact is not executable")
    if not args.mcp_executable.is_file() or not os.access(
        args.mcp_executable, os.X_OK
    ):
        raise TopologyPreparationError("MCP artifact is not executable")
    mcp_python = args.mcp_executable.parent / "python"
    if not mcp_python.is_file() or not os.access(mcp_python, os.X_OK):
        raise TopologyPreparationError(
            "MCP executable must be in a virtual environment with an executable python"
        )
    _mcp_source_ready(args.mcp_repo)
    profile = (
        args.factory_repo
        / "topology-environments"
        / "operator-service-environment-v1.json"
    )
    profile_value = _load_json(profile, "operator-service environment profile")
    if profile_value.get("id") != PROFILE_ID:
        raise TopologyPreparationError("Factory environment profile id differs")
    provenance = {
        "bot_commit": args.bot_commit,
        "bot_binary_sha256": _sha256_file(args.bicameral_binary),
        "mcp_commit": args.mcp_commit,
        "mcp_executable_sha256": _sha256_file(args.mcp_executable),
        "mcp_python_sha256": _sha256_file(mcp_python),
        "factory_commit": args.factory_commit,
        "environment_profile_sha256": _sha256_file(profile),
    }
    if not all(
        DIGEST.fullmatch(value)
        for key, value in provenance.items()
        if key.endswith("sha256")
    ):
        raise TopologyPreparationError("artifact provenance digest is invalid")
    return revision, binding, lane, provenance


def main() -> int:
    args = _parse_args()
    try:
        require_direct_devin_execution(os.environ)
        revision, binding, lane, provenance = _validate_inputs(args)
        topology_request = (
            _load_json(args.topology_request, "TopologyExecutionRequest")
            if args.topology_request
            else None
        )
        if topology_request is not None:
            validate_topology_request(
                topology_request, revision, args.bot_commit
            )
        if args.validate_only:
            print(
                json.dumps(
                    {
                        "state": "prepared",
                        "profile_id": PROFILE_ID,
                        "platform_lane": lane.lane_id,
                        "journey_digest": binding.digest,
                        "check_ids": list(binding.check_ids),
                        "artifact_provenance": provenance,
                    },
                    sort_keys=True,
                )
            )
            return 0
        gateway_port = _allocate_loopback_port()
        service_instance = f"topology-{os.getpid()}-{binding.digest[-8:]}"
        if not _port_available(gateway_port):
            raise TopologyPreparationError(
                "allocated loopback port is already in use"
            )
        _native_service_absent(lane, service_instance)
    except TopologyPreparationError as exc:
        print(f"PREPARATION_FAILED: {exc}", file=sys.stderr)
        return 2

    commands = service_commands(args.bicameral_binary)
    service_observations: dict[str, Any] = {
        "native_manager": lane.service_manager,
        "service_instance": service_instance,
        "gateway_port": gateway_port,
        "zero_product_start": False,
        "restart_rehydrated": False,
    }
    product_observations: dict[str, Any] = {}
    teardown = {
        "service_stopped": False,
        "service_uninstalled": False,
        "operator_home_removed": False,
        "port_closed": False,
    }
    sanitization = {
        "browser_payloads_scanned": True,
        "process_logs_scanned": False,
        "raw_credentials_recorded": False,
        "host_paths_recorded": False,
    }
    process_provenance: dict[str, Any] = {}
    work_root = Path(tempfile.mkdtemp(prefix="bicameral-operator-service-"))
    operator_home = work_root / "operator-home"
    operator_home.mkdir()
    production_started = False
    installed = False
    try:
        # First native start: no Product routes exist.
        installed = True
        _call_service(commands.install, operator_home, service_instance, gateway_port)
        _call_service(commands.start, operator_home, service_instance, gateway_port)
        production_started = True
        _wait_health(gateway_port)
        if list((operator_home / ".bicameral" / "projects").glob("*.json")):
            raise TopologyExecutionError(
                "zero-Product service start unexpectedly found Product routes"
            )
        service_observations["zero_product_start"] = True
        _call_service(commands.stop, operator_home, service_instance, gateway_port)
        _call_service(commands.uninstall, operator_home, service_instance, gateway_port)
        installed = False

        # Product setup uses production foreground workspace daemons so each
        # canonical Product owns a reopenable ledger before operator routing.
        from validate_hosted_workspace_binding import (
            _allocate_port,
            _bridge_projects,
            _configure_daemon,
            _create_and_register_product,
            _create_repo,
            _pair_bridge,
            _start_daemon,
            _stop_process,
        )

        products: list[tuple[str, Path, str]] = []
        log_dir = work_root / "logs"
        log_dir.mkdir()
        for index in (1, 2):
            workspace = work_root / f"product-{index}"
            _create_repo(
                workspace,
                ssh_remote=f"git@github.com:BicameralAI/topology-product-{index}.git",
                https_remote=f"https://github.com/BicameralAI/topology-product-{index}",
            )
            port = _allocate_port()
            _configure_daemon(
                args.bicameral_binary, workspace, operator_home, port
            )
            managed = _start_daemon(
                binary=args.bicameral_binary,
                workspace=workspace,
                home=operator_home,
                name=f"product-{index}-setup",
                port=port,
                log_dir=log_dir,
            )
            try:
                account = f"acct_operator_service_{index}"
                token, _ = _pair_bridge(port, account)
                product_id, _ = _create_and_register_product(port, token, account)
                marker = f"operator-service-product-{index}-{binding.digest[-10:]}"
                products.append((product_id, workspace, marker))
            finally:
                _stop_process(managed)

        installed = True
        _call_service(commands.install, operator_home, service_instance, gateway_port)
        _call_service(commands.start, operator_home, service_instance, gateway_port)
        _wait_health(gateway_port)
        token, _ = _pair_bridge(gateway_port, "acct_operator_service_observer")
        projects, _ = _bridge_projects(gateway_port, token)
        if {item.get("name") for item in projects} != {
            product_id for product_id, _, _ in products
        }:
            raise TopologyExecutionError(
                "operator service did not hydrate both Product routes"
            )

        operational_logs = operator_home / ".bicameral" / "operational-logs"
        shutil.rmtree(operational_logs, ignore_errors=True)
        operational_logs.mkdir(parents=True)
        initial_mcp: dict[str, Any] = {}
        initial_payloads: dict[str, dict[str, Any]] = {}
        for product_id, workspace, marker in products:
            payload = _run_mcp_probe(
                executable=args.mcp_executable,
                operator_home=operator_home,
                workspace=workspace,
                product_id=product_id,
                marker=marker,
                mode="bind-ingest-search",
                work_root=work_root,
                daemon_port=gateway_port,
            )
            initial_payloads[product_id] = payload
            initial_mcp[_operational_fingerprint(product_id)] = {
                "tool_count": len(payload.get("tool_names", [])),
                "call_count": len(payload.get("calls", [])),
            }
        fingerprint_counts = _observed_request_fingerprint_counts(operator_home)
        expected_fingerprints = {
            _operational_fingerprint(product_id)
            for product_id, _, _ in products
        }
        for product_id, payload in initial_payloads.items():
            observed_count = fingerprint_counts.get(
                _operational_fingerprint(product_id), 0
            )
            required_count = max(1, len(payload.get("calls", [])) - 1)
            if observed_count < required_count:
                raise TopologyExecutionError(
                    "daemon evidence omitted a Product-scoped request fingerprint"
                )
            payload["request_product_ids"] = [product_id] * observed_count
            validate_mcp_product_probe(payload, product_id)

        changed_id, changed_workspace, _ = products[0]
        healthy_id, _, _ = products[1]
        _wait_product_watcher_lifecycles(
            operator_home,
            {changed_id: "current", healthy_id: "current"},
        )

        # Commit while the native service is stopped. Restart must resume from
        # each durable Product checkpoint and detect only the changed Product.
        _call_service(commands.stop, operator_home, service_instance, gateway_port)
        offline_marker = changed_workspace / "watcher-offline-change.txt"
        offline_marker.write_text("offline watcher change\n", encoding="utf-8")
        _run(["git", "add", offline_marker.name], cwd=changed_workspace)
        _run(
            ["git", "commit", "-m", "topology: offline watcher change"],
            cwd=changed_workspace,
        )
        _call_service(commands.start, operator_home, service_instance, gateway_port)
        _wait_health(gateway_port)
        offline_states = _wait_product_watcher_lifecycles(
            operator_home,
            {changed_id: "stale", healthy_id: "current"},
        )
        if len(offline_states[changed_id]["queued_work"]) != 1:
            raise TopologyExecutionError(
                "offline Product change did not enqueue exactly one durable watcher item"
            )
        if offline_states[healthy_id]["queued_work"]:
            raise TopologyExecutionError(
                "offline Product change contaminated healthy Product watcher queue"
            )

        # Make only one repository temporarily unobservable. Its failure
        # backlog must advance without changing the other Product.
        git_dir = changed_workspace / ".git"
        hidden_git_dir = changed_workspace / ".git.topology-hidden"
        git_dir.rename(hidden_git_dir)
        try:
            failed_states = _wait_product_watcher_lifecycles(
                operator_home,
                {changed_id: "failed", healthy_id: "current"},
            )
            if not failed_states[changed_id]["failure_backlog"]:
                raise TopologyExecutionError(
                    "failed Product watcher did not persist its failure backlog"
                )
            if failed_states[healthy_id]["failure_backlog"]:
                raise TopologyExecutionError(
                    "failed Product watcher contaminated healthy Product failure backlog"
                )
        finally:
            hidden_git_dir.rename(git_dir)

        _wait_product_watcher_lifecycles(
            operator_home,
            {changed_id: "stale", healthy_id: "current"},
        )

        _call_service(commands.stop, operator_home, service_instance, gateway_port)
        _call_service(commands.start, operator_home, service_instance, gateway_port)
        _wait_health(gateway_port)
        restarted_states = _wait_product_watcher_lifecycles(
            operator_home,
            {changed_id: "stale", healthy_id: "current"},
        )
        for product_id, workspace, marker in products:
            _run_mcp_probe(
                executable=args.mcp_executable,
                operator_home=operator_home,
                workspace=workspace,
                product_id=product_id,
                marker=marker,
                mode="search",
                work_root=work_root,
                daemon_port=gateway_port,
            )
        service_observations["restart_rehydrated"] = True
        service_observations["watcher_state_rehydrated"] = True
        product_observations["watcher_state_isolation"] = {
            "offline_changed_product": _operational_fingerprint(changed_id),
            "healthy_product": _operational_fingerprint(healthy_id),
            "offline_queue_depth": len(restarted_states[changed_id]["queued_work"]),
            "changed_failure_backlog_depth": len(
                restarted_states[changed_id]["failure_backlog"]
            ),
            "healthy_queue_depth": len(restarted_states[healthy_id]["queued_work"]),
            "healthy_failure_backlog_depth": len(
                restarted_states[healthy_id]["failure_backlog"]
            ),
            "restart_lifecycles": {
                _operational_fingerprint(changed_id): restarted_states[changed_id][
                    "lifecycle"
                ],
                _operational_fingerprint(healthy_id): restarted_states[healthy_id][
                    "lifecycle"
                ],
            },
        }

        damaged_id, _, _ = products[0]
        healthy_id, healthy_workspace, healthy_marker = products[1]
        binding_path = (
            operator_home
            / ".bicameral"
            / "workspace-bindings"
            / f"{_sanitize_product_id(damaged_id)}.json"
        )
        original_binding = binding_path.read_bytes()
        original_binding_digest = "sha256:" + hashlib.sha256(
            original_binding
        ).hexdigest()
        binding_path.write_text("{", encoding="utf-8")
        try:
            _call_service(commands.stop, operator_home, service_instance, gateway_port)
            _call_service(commands.start, operator_home, service_instance, gateway_port)
            _wait_health(gateway_port)
            repair_states = _wait_product_watcher_lifecycles(
                operator_home,
                {damaged_id: "disabled", healthy_id: "current"},
            )
            token, _ = _pair_bridge(
                gateway_port, "acct_operator_service_repair_observer"
            )
            projects, _ = _bridge_projects(gateway_port, token)
            by_id = {item.get("name"): item for item in projects}
            if by_id.get(damaged_id, {}).get("setup_state") != (
                "local_workspace_repair_required"
            ):
                raise TopologyExecutionError(
                    "damaged Product did not enter repair-required projection"
                )
            if by_id.get(healthy_id, {}).get("setup_state") != "normal":
                raise TopologyExecutionError(
                    "healthy Product was degraded by another binding fault"
                )
            _run_mcp_probe(
                executable=args.mcp_executable,
                operator_home=operator_home,
                workspace=healthy_workspace,
                product_id=healthy_id,
                marker=healthy_marker,
                mode="search",
                work_root=work_root,
                daemon_port=gateway_port,
            )
        finally:
            binding_path.write_bytes(original_binding)

        product_observations.update(
            {
                "product_count": 2,
                "product_id_fingerprints": sorted(expected_fingerprints),
                "initial_mcp": initial_mcp,
                "binding_fault": {
                    "damaged_product_fingerprint": _operational_fingerprint(
                        damaged_id
                    ),
                    "healthy_product_fingerprint": _operational_fingerprint(
                        healthy_id
                    ),
                    "original_binding_digest": original_binding_digest,
                    "isolated": True,
                    "watcher_lifecycles": {
                        _operational_fingerprint(damaged_id): repair_states[damaged_id][
                            "lifecycle"
                        ],
                        _operational_fingerprint(healthy_id): repair_states[healthy_id][
                            "lifecycle"
                        ],
                    },
                },
            },
        )
        process_provenance = {
            "method": "controller-event-log",
            "host_root_pid": os.getpid(),
            "observed_process_pid": _native_service_pid(
                lane,
                service_instance,
            ),
        }
        _scan_process_logs([log_dir, operational_logs])
        sanitization["process_logs_scanned"] = True
    except (TopologyPreparationError, TopologyExecutionError, OSError) as exc:
        category = "EXECUTION_FAILED" if production_started else "PREPARATION_FAILED"
        print(f"{category}: {exc}", file=sys.stderr)
        return 1 if production_started else 2
    finally:
        if installed:
            try:
                _call_service(
                    commands.stop, operator_home, service_instance, gateway_port
                )
                teardown["service_stopped"] = True
            except TopologyExecutionError:
                pass
        else:
            teardown["service_stopped"] = True
        if teardown["service_stopped"]:
            try:
                _call_service(
                    commands.uninstall, operator_home, service_instance, gateway_port
                )
                teardown["service_uninstalled"] = True
                installed = False
            except TopologyExecutionError:
                pass
        teardown["port_closed"] = _port_available(gateway_port)
        if teardown["service_stopped"] and teardown["service_uninstalled"]:
            shutil.rmtree(work_root)
            teardown["operator_home_removed"] = not work_root.exists()

    if not all(teardown.values()):
        print(
            "EXECUTION_FAILED: deterministic teardown was incomplete; "
            f"preserved disposable root at {work_root}",
            file=sys.stderr,
        )
        return 1
    receipt = build_receipt(
        revision=revision,
        platform=sys.platform,
        artifact_provenance=provenance,
        service_observations=service_observations,
        product_observations=product_observations,
        observed_check_ids=list(binding.check_ids),
        process_provenance=process_provenance,
        teardown=teardown,
        sanitization=sanitization,
        topology_request=topology_request,
    )

    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(args.receipt)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
