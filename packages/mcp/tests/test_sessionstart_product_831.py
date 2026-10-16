"""SessionStart Product routing via daemon LocalWorkspaceBinding (mcp#831).

Contract tests: unique bound folder → product_id; unbound / repair_required /
ambiguous fail closed. The hook never walks ``~/.bicameral/workspace-bindings/``.
``BICAMERAL_PRODUCT_ID`` is an optional override *after* daemon lookup.

One real-process test runs production ``bicameral-mcp prework-run`` (or
``python -m server prework-run``) against a protocol-faithful HTTP daemon
subprocess and a disposable host home. That is supporting evidence for the
hook CLI path; production ``FileWorkspaceBindingStore`` reverse-lookup lives
in the paired bot PR. Authentic host SessionStart fire remains #736 / #762.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest
from prework_support import HOSTS
from prework_support import FakeDaemon as _FakeDaemon
from prework_support import install as _install
from prework_support import startup_event as _startup_event

from preflight_adapters.runner import PreworkOutcome, run_prework

_RUNNER = Path(__file__).resolve().parents[1] / "preflight_adapters" / "runner.py"


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_unique_binding_invokes_preflight_without_env(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon(resolved_product_id="prd_bound")
    event = _startup_event(tmp_path)

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.INVOKED
    assert result.preflight_invoked is True
    resolve_req, preflight_req = daemon.requests
    assert resolve_req["command"]["name"] == "workspace.resolve"
    assert resolve_req["command"]["params"]["candidate_path"] == str(tmp_path)
    assert "product_id" not in resolve_req
    assert preflight_req["command"]["name"] == "preflight.run"
    assert preflight_req["product_id"] == "prd_bound"


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.parametrize(
    ("resolve_error", "resolve_state"),
    [
        ("unbound", "local_workspace_unbound"),
        ("repair_required", "local_workspace_repair_required"),
        ("ambiguous", "local_workspace_bound"),
    ],
)
@pytest.mark.asyncio
async def test_typed_miss_fails_closed(
    host: str, tmp_path: Path, resolve_error: str, resolve_state: str
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon(resolve_error=resolve_error, resolve_state=resolve_state)

    result = await run_prework(
        host, _startup_event(tmp_path), home=adapter.home(), client_factory=lambda: daemon
    )

    assert result.outcome == PreworkOutcome.FALLBACK_PRODUCT_CONTEXT_REQUIRED
    assert result.preflight_invoked is False
    assert result.details["resolve_error"] == resolve_error
    assert [req["command"]["name"] for req in daemon.requests] == ["workspace.resolve"]


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_env_override_applies_only_after_daemon_miss(
    host: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon(resolve_error="unbound")
    monkeypatch.setenv("BICAMERAL_PRODUCT_ID", "prd_env")

    result = await run_prework(
        host, _startup_event(tmp_path), home=adapter.home(), client_factory=lambda: daemon
    )

    assert result.outcome == PreworkOutcome.INVOKED
    assert daemon.requests[0]["command"]["name"] == "workspace.resolve"
    assert daemon.requests[1]["product_id"] == "prd_env"


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_daemon_unique_binding_wins_over_env(
    host: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon(resolved_product_id="prd_bound")
    monkeypatch.setenv("BICAMERAL_PRODUCT_ID", "prd_env")

    result = await run_prework(
        host, _startup_event(tmp_path), home=adapter.home(), client_factory=lambda: daemon
    )

    assert result.outcome == PreworkOutcome.INVOKED
    assert daemon.requests[1]["product_id"] == "prd_bound"


@pytest.mark.parametrize("host", HOSTS)
@pytest.mark.asyncio
async def test_payload_product_id_is_not_an_identity_source(host: str, tmp_path: Path) -> None:
    adapter, _ = _install(host, tmp_path)
    daemon = _FakeDaemon()
    event = _startup_event(tmp_path)
    event["product_id"] = "prd_from_host_json"

    result = await run_prework(host, event, home=adapter.home(), client_factory=lambda: daemon)

    assert result.outcome == PreworkOutcome.FALLBACK_PRODUCT_CONTEXT_REQUIRED
    assert result.preflight_invoked is False


def test_hook_source_does_not_walk_binding_store() -> None:
    source = _RUNNER.read_text(encoding="utf-8")
    assert "workspace-bindings" not in source
    assert ".bicameral" not in source
    assert "BICAMERAL_HOME" not in source


_HTTP_DAEMON_SCRIPT = r"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

port = int(sys.argv[1])
product_id = sys.argv[2]
folder = sys.argv[3]
log_path = Path(sys.argv[4])
ready_path = Path(sys.argv[5])


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        return

    def _read_json(self):
        length = int(self.headers.get("content-length") or "0")
        raw = self.rfile.read(length) if length else b"{}"
        payload = json.loads(raw.decode("utf-8") or "{}")
        return payload if isinstance(payload, dict) else {}

    def _write_json(self, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/v2/capabilities":
            self._write_json(
                {
                    "toolrequest_protocol_version": "v2",
                    "supported_commands": ["preflight.run", "workspace.resolve"],
                    "deferred_commands": [],
                }
            )
            return
        self.send_error(404)

    def do_POST(self):
        if self.path != "/v2/tool-requests":
            self.send_error(404)
            return
        request = self._read_json()
        with log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(request) + "\n")
        name = (request.get("command") or {}).get("name")
        request_id = request.get("request_id")
        if name == "workspace.resolve":
            path = ((request.get("command") or {}).get("params") or {}).get("candidate_path")
            if path == folder:
                self._write_json(
                    {
                        "request_id": request_id,
                        "status": "ok",
                        "result": {
                            "status": "resolved",
                            "outcome": {
                                "product_id": product_id,
                                "state": "local_workspace_bound",
                                "display": {"display_name": "Bound Product"},
                            },
                        },
                    }
                )
                return
            self._write_json(
                {
                    "request_id": request_id,
                    "status": "rejected",
                    "result": {
                        "error": "unbound",
                        "message": "No local workspace binding matches this folder.",
                        "state": "local_workspace_unbound",
                        "match_count": 0,
                    },
                    "message": "unbound: No local workspace binding matches this folder.",
                }
            )
            return
        if name == "preflight.run":
            self._write_json({"request_id": request_id, "status": "ok", "result": {"status": "ok"}})
            return
        self._write_json(
            {"request_id": request_id, "status": "error", "message": "unsupported_command"}
        )


httpd = HTTPServer(("127.0.0.1", port), Handler)
ready_path.write_text("ready", encoding="utf-8")
httpd.serve_forever()
"""


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def test_production_prework_cli_resolves_product_over_http(tmp_path: Path) -> None:
    """Production hook CLI + separate HTTP daemon process (not in-process FakeDaemon)."""
    host = "claude"
    adapter, _ = _install(host, tmp_path)
    port = _free_port()
    log_path = tmp_path / "daemon-requests.jsonl"
    ready_path = tmp_path / "daemon-ready"
    log_path.write_text("", encoding="utf-8")
    daemon = subprocess.Popen(
        [
            sys.executable,
            "-c",
            _HTTP_DAEMON_SCRIPT,
            str(port),
            "prd_http",
            str(tmp_path),
            str(log_path),
            str(ready_path),
        ],
        cwd=str(Path(__file__).resolve().parents[1]),
    )
    try:
        for _ in range(50):
            if ready_path.exists():
                break
            time.sleep(0.05)
        else:
            raise AssertionError("HTTP daemon did not become ready")

        env = os.environ.copy()
        env["BICAMERAL_DAEMON_URL"] = f"http://127.0.0.1:{port}"
        env.pop("BICAMERAL_PRODUCT_ID", None)
        env["PYTHONPATH"] = str(Path(__file__).resolve().parents[1])
        proc = subprocess.run(
            [
                sys.executable,
                "-m",
                "server",
                "prework-run",
                "--host",
                host,
                "--home",
                str(adapter.home()),
            ],
            input=json.dumps(_startup_event(tmp_path)),
            capture_output=True,
            text=True,
            cwd=str(Path(__file__).resolve().parents[1]),
            env=env,
            check=False,
            timeout=20,
        )
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=5)
        except subprocess.TimeoutExpired:
            daemon.kill()

    assert proc.returncode == 0, proc.stderr
    combined = proc.stdout + proc.stderr
    assert "Invoked bicameral.preflight once" in combined
    requests = [
        json.loads(line)
        for line in log_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    names = [req["command"]["name"] for req in requests]
    assert names == ["workspace.resolve", "preflight.run"]
    assert requests[1]["product_id"] == "prd_http"
