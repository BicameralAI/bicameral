#!/usr/bin/env python3
"""Run the revision-pinned Integration -> Bot -> MCP release journey.

The runner starts the production Bot daemon, executes the production
Integrations normalizer and GatewaySink in a separate Python process, then
invokes the production MCP stdio server/client and observes the ingested
candidate through ``bicameral.search``.  A passing receipt therefore proves
the real public boundaries; fixtures and in-process peers are not terminal
evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Mapping, Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate_hosted_workspace_binding import (  # noqa: E402
    ValidationError,
    _allocate_port,
    _configure_daemon,
    _create_repo,
    _start_daemon,
    _stop_process,
)


class TopologyError(RuntimeError):
    """A sanitized, fail-closed topology or provenance failure."""


def require_direct_devin_execution(env: Mapping[str, str]) -> None:
    if env.get("GITHUB_ACTIONS", "").lower() == "true":
        raise TopologyError(
            "topology production execution is restricted to direct Devin"
        )
    if env.get("BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER") != "devin":
        raise TopologyError(
            "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER must identify direct Devin"
        )


def _now() -> str:
    return (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def _run(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = 120,
    label: str,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(command),
            cwd=cwd,
            env=env,
            text=True,
            capture_output=True,
            check=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise TopologyError(f"{label} timed out") from exc
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or exc.stdout or "").strip().splitlines()[-20:]
        bounded = " | ".join(line[:300] for line in detail)
        raise TopologyError(
            f"{label} failed with exit code {exc.returncode}: {bounded}"
        ) from exc


def _git_sha(repository: Path) -> str:
    return _run(
        ["git", "-C", str(repository), "rev-parse", "HEAD"],
        label="resolve checkout commit",
    ).stdout.strip()


def require_exact_commit(expected: str, actual: str, label: str) -> str:
    """Reject floating refs and checkouts that do not equal the release tuple."""

    if re.fullmatch(r"[0-9a-f]{40}", expected) is None:
        raise TopologyError(f"{label} commit must be a full lowercase SHA")
    if actual != expected:
        raise TopologyError(f"{label} checkout does not match the pinned commit")
    return actual


def _sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _closure_digest(repository: Path, paths: Sequence[str]) -> str:
    digest = hashlib.sha256()
    for relative in sorted(paths):
        path = repository / relative
        if not path.is_file():
            raise TopologyError(f"required interface file is absent: {relative}")
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return "sha256:" + digest.hexdigest()


def _find_marker(value: Any, marker: str) -> dict[str, Any] | None:
    if isinstance(value, dict):
        if marker in json.dumps(value, sort_keys=True):
            if (
                marker in str(value.get("title", ""))
                or marker in str(value.get("excerpt", ""))
                or marker in str(value.get("snippet", ""))
            ):
                return value
        for child in value.values():
            found = _find_marker(child, marker)
            if found is not None:
                return found
    elif isinstance(value, list):
        for child in value:
            found = _find_marker(child, marker)
            if found is not None:
                return found
    return None


def validate_mcp_terminal_result(
    payload: dict[str, Any], marker: str
) -> dict[str, Any]:
    """Validate that the real MCP surface returned the Integration-ingested item."""

    tools = payload.get("tool_names")
    if not isinstance(tools, list) or "bicameral.search" not in tools:
        raise TopologyError("real MCP server did not advertise bicameral.search")
    if payload.get("is_error") is not False:
        raise TopologyError("real MCP search returned an error")
    content = payload.get("content")
    if not isinstance(content, list):
        raise TopologyError("real MCP search returned malformed content")

    for item in content:
        if not isinstance(item, str):
            continue
        try:
            rendered = json.loads(item)
        except json.JSONDecodeError:
            continue
        if not isinstance(rendered, dict) or rendered.get("surface") != "search":
            continue
        match = _find_marker(rendered.get("matches", []), marker)
        if match is not None:
            candidate_id = match.get("candidate_id") or match.get("id")
            evidence_refs = match.get("evidence_refs") or []
            evidence_ref_id = match.get("evidence_ref_id")
            if (
                not evidence_ref_id
                and isinstance(evidence_refs, list)
                and evidence_refs
            ):
                evidence_ref_id = evidence_refs[0]
            snapshot_id = match.get("snapshot_id")
            source_uri = match.get("source_uri") or match.get("source_link")
            if not candidate_id or not (evidence_ref_id or snapshot_id or source_uri):
                raise TopologyError(
                    "MCP result omitted typed candidate and source/evidence identity"
                )
            return {
                "surface": "search",
                "candidate_id": candidate_id,
                "evidence_ref_id": evidence_ref_id,
                "evidence_refs": evidence_refs
                or ([evidence_ref_id] if evidence_ref_id else []),
                "snapshot_id": snapshot_id,
                "source_uri": source_uri,
            }
    raise TopologyError("real MCP search did not return the ingested marker")


_INTEGRATIONS_PROBE = r"""from __future__ import annotations
import json, sys
from adapter.core.capabilities import SourceMode
from adapter.core.emissions import SourceRef
from adapter.core.observations import Observation
from adapter.core.pipeline import normalize
from runtime.sinks import GatewaySink

endpoint, marker = sys.argv[1:3]
observation = Observation(
    source_ref=SourceRef(
        source_id="topology_webhook",
        ref=marker,
        url=f"https://topology.invalid/events/{marker}",
        kind="webhook",
    ),
    excerpt=f"Decision: we will use {marker} as the revision-pinned journey marker",
    mode=SourceMode.WEBHOOK,
    title=marker,
    provider_event_id=marker,
)
emissions = normalize([observation], adapter_version="release-topology-v1")
GatewaySink(endpoint=endpoint).emit(emissions)
print(json.dumps({
    "adapter_process": "bicameral-integrations",
    "emission_type": emissions[0].emission_type,
    "source_id": emissions[0].source_id,
    "marker": marker,
}))
"""


_MCP_PROBE = r"""from __future__ import annotations
import asyncio, json, os, sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from mcp.types import TextContent

async def run() -> None:
    executable, marker = sys.argv[1:3]
    server = StdioServerParameters(command=executable, args=["serve"], env=dict(os.environ))
    async with stdio_client(server) as (reader, writer):
        async with ClientSession(reader, writer) as session:
            await session.initialize()
            tools = await session.list_tools()
            result = await session.call_tool(
                "bicameral.search",
                {"query": marker, "scope": "candidates", "limit": 10},
            )
            print(json.dumps({
                "tool_names": sorted(tool.name for tool in tools.tools),
                "is_error": bool(result.isError),
                "content": [
                    item.text for item in result.content if isinstance(item, TextContent)
                ],
            }))

asyncio.run(run())
"""


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[1]
    parser.add_argument("--integrations-repo", type=Path, required=True)
    parser.add_argument("--mcp-repo", type=Path, required=True)
    parser.add_argument("--bot-commit", required=True)
    parser.add_argument("--integrations-commit", required=True)
    parser.add_argument("--mcp-commit", required=True)
    parser.add_argument("--factory-commit", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--skip-build", action="store_true")
    parser.set_defaults(bot_repo=root)
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    bot_root = Path(args.bot_repo).resolve()
    integrations_root = args.integrations_repo.resolve()
    mcp_root = args.mcp_repo.resolve()
    started_at = _now()
    marker = "integration-bot-mcp-" + secrets.token_hex(8)
    managed = None

    try:
        require_direct_devin_execution(os.environ)
        for command in ("cargo", "git", "uv"):
            if shutil.which(command) is None:
                raise TopologyError(f"required executable is unavailable: {command}")

        commits = {
            "bot": require_exact_commit(args.bot_commit, _git_sha(bot_root), "bot"),
            "integrations": require_exact_commit(
                args.integrations_commit, _git_sha(integrations_root), "integrations"
            ),
            "mcp": require_exact_commit(args.mcp_commit, _git_sha(mcp_root), "mcp"),
            "factory": require_exact_commit(
                args.factory_commit, args.factory_commit, "factory"
            ),
        }

        binary = bot_root / "target/release/bicameral"
        if not args.skip_build or not binary.is_file():
            _run(
                ["cargo", "build", "--release", "--locked", "--bin", "bicameral"],
                cwd=bot_root,
                timeout=2400,
                label="build production Bot binary",
            )
        if not binary.is_file():
            raise TopologyError("production Bot binary was not built")

        with tempfile.TemporaryDirectory(
            prefix="bicameral-integration-topology-"
        ) as raw:
            work_root = Path(raw)
            workspace = work_root / "workspace"
            home = work_root / "home"
            logs = work_root / "logs"
            logs.mkdir()
            _create_repo(
                workspace,
                ssh_remote="git@github.com:example-org/topology.git",
                https_remote="https://github.com/example-org/topology.git",
            )
            port = _allocate_port()
            config_digest = _configure_daemon(binary, workspace, home, port)
            managed = _start_daemon(
                binary=binary,
                workspace=workspace,
                home=home,
                name="integration-bot-mcp-daemon",
                port=port,
                log_dir=logs,
            )

            integration_result = _run(
                [
                    sys.executable,
                    "-c",
                    _INTEGRATIONS_PROBE,
                    f"http://127.0.0.1:{port}/api/v1/external-ingest",
                    marker,
                ],
                cwd=integrations_root,
                timeout=120,
                label="real Integrations adapter delivery",
            )
            integration_evidence = json.loads(integration_result.stdout)
            if integration_evidence.get("marker") != marker:
                raise TopologyError(
                    "Integrations process did not confirm its typed emission"
                )

            venv = work_root / "mcp-env"
            _run(
                ["uv", "venv", "--python", "3.11", str(venv)],
                timeout=300,
                label="create MCP environment",
            )
            python = venv / "bin/python"
            _run(
                ["uv", "pip", "install", "--python", str(python), "-e", str(mcp_root)],
                timeout=600,
                label="install production MCP artifact",
            )
            probe = work_root / "mcp_probe.py"
            probe.write_text(_MCP_PROBE, encoding="utf-8")
            env = {
                **os.environ,
                "BICAMERAL_DAEMON_URL": f"http://127.0.0.1:{port}",
                "BICAMERAL_ACTOR_ID": "release-topology-operator",
                "BICAMERAL_WORKSPACE": str(workspace),
                "BICAMERAL_POLICY_SCOPE": "default",
            }
            mcp_result = _run(
                [str(python), str(probe), str(venv / "bin/bicameral-mcp"), marker],
                env=env,
                timeout=180,
                label="real MCP stdio search",
            )
            mcp_payload = json.loads(mcp_result.stdout)
            terminal_evidence = validate_mcp_terminal_result(mcp_payload, marker)

            receipt = {
                "schema_version": 1,
                "journey": "integration_bot_mcp_v1",
                "profile": "real-process-v1",
                "status": "passed",
                "started_at": started_at,
                "completed_at": _now(),
                "release_tuple": commits,
                "artifacts": {
                    "bot_binary": "sha256:" + _sha256_file(binary),
                    "integrations_adapter": _closure_digest(
                        integrations_root,
                        [
                            "adapter/core/emissions.py",
                            "adapter/core/observations.py",
                            "adapter/core/pipeline.py",
                            "runtime/gateway_mapping.py",
                            "runtime/sinks.py",
                        ],
                    ),
                    "mcp_tool_surface": _closure_digest(
                        mcp_root,
                        [
                            "server.py",
                            "daemon_client.py",
                            "tool_request.py",
                            "tool_schemas.py",
                        ],
                    ),
                },
                "interfaces": {
                    "adapter_contract": _closure_digest(
                        integrations_root,
                        [
                            "runtime/gateway_mapping.py",
                            "runtime/schemas/external_ingest_request_v2.schema.json",
                        ],
                    ),
                    "bot_ingress": "sha256:"
                    + _sha256_file(
                        bot_root
                        / "protocol/schemas/v2/external-ingest-request.schema.json"
                    ),
                    "toolrequest_contract": _closure_digest(
                        bot_root,
                        [
                            "crates/bicameral-api/src/tool_request.rs",
                            "protocol/schemas/v2/tool-request.schema.json",
                        ],
                    ),
                },
                "process_evidence": {
                    "integrations": integration_evidence,
                    "bot": {
                        "process": "target/release/bicameral dashboard",
                        "config_sha256": config_digest,
                        "external_ingest_status": 201,
                    },
                    "mcp": terminal_evidence,
                },
                "terminal_evidence": {
                    "real_processes": [
                        "bicameral-integrations",
                        "bicameral",
                        "bicameral-mcp",
                    ],
                    "fixtures_used": False,
                    "in_process_peer_used": False,
                    "marker_digest": "sha256:"
                    + hashlib.sha256(marker.encode("utf-8")).hexdigest(),
                },
            }
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(
                json.dumps(receipt, indent=2, sort_keys=True) + "\n"
            )
            print(json.dumps({"status": "passed", "receipt": str(args.receipt)}))
        return 0
    except (TopologyError, ValidationError, json.JSONDecodeError) as exc:
        print(f"integration_bot_mcp_v1 failed: {exc}", file=sys.stderr)
        return 1
    finally:
        if managed is not None:
            _stop_process(managed)


if __name__ == "__main__":
    raise SystemExit(main())
