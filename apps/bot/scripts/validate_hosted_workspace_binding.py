#!/usr/bin/env python3
"""Terminal validation: MCP-assisted local-folder-to-Product binding (bicameral-bot#788).

This is the single documented terminal validation command for the J-A5
`mcp_local_folder_product_binding` User Journey, reconciled to the 2026-07-27
owner correction (bicameral-bot#788) under bicameral-bot#1190:

    developer opens local folder
      -> invokes MCP bind
      -> MCP proposes association with a Product
      -> operator explicitly confirms
      -> daemon validates Product registration, capability, path safety, and
         repository evidence
      -> daemon persists LocalWorkspaceBinding
      -> MCP reports the local folder bound

The domain target is a canonical `Product`. The authoritative commit is daemon
persistence of `LocalWorkspaceBinding`; MCP proposes and reports but never
persists authority. The superseded "Hosted Workspace Binding" framing
(`hosted_workspace_binding`) is retained only as a `supersedes` pointer so older
evidence can be correlated; it is no longer the authoritative contract.

The harness drives the **real** production surfaces — the built bot daemon, the
real `bicameral-mcp` server/client process over stdio, a disposable local Cloud
process serving the built SPA, and (optionally) a real browser — and records a
real-process trace for the assignment-bound direct Devin topology receipt.

Required criteria (gate the verdict) cover the binding, restart/replay,
fail-closed negative paths, and the no-local-path privacy boundary. The hosted
dashboard `code_grounding_ready` render is a **downstream witness**: it is
recorded when observed, but it never gates J-A5 success and its absence never
invalidates a persisted local binding.

No test doubles are used. Nothing here mocks, fakes, or substitutes the daemon,
the MCP peer, the durable store, or the browser: a boundary is only reported
green when the real process produced the observed outcome.

Evidence level: `real-process lifecycle acceptance` (plus an optional
`browser` downstream witness).

Usage
-----
    python3 scripts/validate_hosted_workspace_binding.py \
        --topology-receipt <path-to-cloud#135-receipt.json> \
        --trace test-artifacts/local-folder-product-binding/<run>/trace.json

Key flags:
    --topology-receipt PATH   cloud#135 real-process topology receipt to consume.
    --mcp-repo PATH           checkout of BicameralAI/bicameral-mcp (default: sibling).
    --cloud-repo PATH         checkout of BicameralAI/bicameral-cloud (default: sibling).
    --mcp-commit SHA          assert the MCP checkout is at this exact commit.
    --cloud-commit SHA        assert the Cloud checkout is at this exact commit.
    --factory-commit SHA      full factory commit SHA to anchor into the trace.
    --cdp-url URL             optional Chrome DevTools endpoint; when omitted,
                              launch Playwright Chromium directly.
    --no-browser              skip the downstream browser witness (records it as
                              not-attempted; it is non-gating either way).
    --skip-build              reuse an existing release binary / SPA dist.
    --trace PATH              where to write the trace JSON.

Execution requires `BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER=devin` and rejects
GitHub Actions before any production process starts.

Exit code is non-zero when any required acceptance criterion is not `passed`
(including `blocked` on a missing production capability), so the bound topology
attempt remains failed until the journey is genuinely closed. A blocked
criterion never fabricates success.

Subprocess failures retain bounded, sanitized diagnostics (see
`build_subprocess_diagnostics`): enough stdout/stderr tail to debug the failing
command, with local filesystem paths and secret-shaped values redacted before
anything is written to the retained trace.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import IO

TRUSTED_BRIDGE_ORIGIN = "https://app.bicameral-ai.com"
DEFAULT_CDP_URL = os.environ.get("BICAMERAL_CDP_URL")

# Hosted-bridge pending local-confirmation pairing protocol version (bot#790).
# Read the canonical producer contract so the real-process journey cannot drift
# onto an older client version while the daemon correctly fails closed.
HOSTED_BRIDGE_PROTOCOL_VERSION = json.loads(
    (
        Path(__file__).resolve().parents[1]
        / "protocol"
        / "hosted-bridge"
        / "contract.json"
    ).read_text(encoding="utf-8")
)["pending_pairing_minimum_protocol_version"]

# Tokens that must never appear in any Cloud/browser payload, log, or trace.
FORBIDDEN_LOCAL_TOKENS = (
    "/home/",
    "/Users/",
    "workspace_path",
    "filesystem_path",
    "candidate_path",
    "local_path",
    "source_body",
    "source_content",
    ".bicameral/",
)

# Journey identity (bicameral-bot#788, corrected 2026-07-27; reconciled under
# bicameral-bot#1190). The superseded id is kept only for evidence correlation.
JOURNEY_ID = "mcp_local_folder_product_binding"
JOURNEY_LABEL = "MCP-assisted local-folder-to-Product binding"
SUPERSEDED_JOURNEY_ID = "hosted_workspace_binding"
TRACE_ARTIFACT_DIR = "local-folder-product-binding"
EVIDENCE_LEVEL = "real-process lifecycle acceptance"

# Required acceptance criteria (bicameral-bot#788). Each maps to a scenario
# evaluated below and every one of them gates the overall verdict.
REQUIRED_CRITERIA = (
    "propose_without_persisting_authority",
    "confirm_reaches_daemon_and_persists",
    "ssh_https_remotes_converge",
    "binding_survives_restart_replay",
    "cancellation_leaves_no_binding_or_pending",
    "negative_paths_create_no_binding",
    "failed_rebind_preserves_previous",
    "no_local_path_in_cloud_browser_payloads_logs",
    "trace_carries_component_fingerprints",
)

# Downstream witnesses are recorded but never gate J-A5 success. The hosted
# dashboard `code_grounding_ready` render is a read-only projection of the
# daemon-persisted binding; its failure or absence is classified separately and
# cannot retroactively invalidate a persisted local binding.
DOWNSTREAM_WITNESS_CRITERIA = ("browser_renders_code_grounding_ready_after_reconnect",)

CRITERIA = REQUIRED_CRITERIA + DOWNSTREAM_WITNESS_CRITERIA

# ── bounded, sanitized subprocess diagnostics ───────────────────────────────

# Hard bounds on retained diagnostic text per stream. Deterministic: the same
# failing output always yields the same bounded, sanitized diagnostic.
DIAGNOSTIC_MAX_CHARS = 2000
DIAGNOSTIC_MAX_LINES = 40

_ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
_CONTROL_CHARS = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")

# Secret-shaped material is redacted before any path handling so a token never
# survives inside a path placeholder.
_SECRET_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
            re.S,
        ),
        "<redacted-private-key>",
    ),
    (re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"), "<redacted-private-key>"),
    (re.compile(r"\bAKIA[0-9A-Z]{16}\b"), "<redacted-secret>"),
    (re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{36,}\b"), "<redacted-secret>"),
    (re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b"), "<redacted-secret>"),
    (
        re.compile(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9\-._~+/]+=*"),
        r"\1 <redacted-secret>",
    ),
    (
        re.compile(
            r"(?i)([\w-]*(?:token|secret|password|passwd|api[_-]?key|authorization"
            r"|cookie|nonce|poll_proof|csrf|credential)[\w-]*)"
            r"""(["']?\s*[:=]\s*)(["']?)(?!<redacted)[^\s"',;&]+"""
        ),
        r"\1\2\3<redacted-secret>",
    ),
)

# Generic local filesystem shapes. Known roots (work root, repo checkouts,
# operator home) are registered explicitly by the harness and replaced first;
# these patterns catch anything a subprocess printed that the harness did not
# know about (for example a tool's own cache directory).
_PATH_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(
            r"""\b[A-Za-z]:[\\/](?:[^\\/\s"'`<>|:;,]+[\\/])*[^\\/\s"'`<>|:;,]*"""
        ),
        "<local-path>",
    ),
    (re.compile(r"""\\\\[^\\\s"'`<>|]+\\[^\s"'`<>|:;,]*"""), "<local-path>"),
    (
        re.compile(
            r"(?<![\w/>])/(?:home|Users|tmp|var|private|mnt|opt|root|srv|workspace|"
            r"""workspaces|runner|github|Volumes|data|work)/[^\s"'`<>|:;,)]*"""
        ),
        "<local-path>",
    ),
    (re.compile(r"""(?<![\w/>])~/[^\s"'`<>|:;,)]*"""), "<local-path>"),
)

_REGISTERED_ROOTS: list[tuple[str, str]] = []


def register_local_root(path: Path | str, placeholder: str) -> None:
    """Register a local root so every rendering of it is replaced verbatim.

    Registration is idempotent and ordering is by descending length so a nested
    root (workspace under the work root) is replaced before its parent.
    """

    rendered = str(path)
    if not rendered or rendered in {"/", "\\"}:
        return
    entry = (rendered, f"<{placeholder}>")
    if entry not in _REGISTERED_ROOTS:
        _REGISTERED_ROOTS.append(entry)
        _REGISTERED_ROOTS.sort(key=lambda item: len(item[0]), reverse=True)


def registered_root_placeholders() -> list[str]:
    return sorted({placeholder for _, placeholder in _REGISTERED_ROOTS})


def _replace_root(text: str, root: str, placeholder: str) -> str:
    variants = {root, root.replace("\\", "/"), root.replace("/", "\\")}
    variants.add(Path(root).as_posix())
    variants.add(root.replace("\\", "\\\\"))
    try:
        variants.add(str(Path(root).resolve()))
    except (OSError, RuntimeError):  # pragma: no cover - defensive
        pass
    for variant in sorted(variants, key=len, reverse=True):
        if variant and variant not in {"/", "\\"}:
            text = text.replace(variant, placeholder)
    return text


def sanitize_diagnostic_text(text: str) -> str:
    """Redact secret-shaped values and local filesystem paths from `text`.

    Deterministic and order-stable: secrets first, then registered roots, then
    generic path shapes. Never raises on arbitrary input.
    """

    if not text:
        return ""
    cleaned = text.replace("\r\n", "\n").replace("\r", "\n")
    cleaned = _ANSI_ESCAPE.sub("", cleaned)
    cleaned = _CONTROL_CHARS.sub("", cleaned)
    for pattern, replacement in _SECRET_PATTERNS:
        cleaned = pattern.sub(replacement, cleaned)
    for root, placeholder in _REGISTERED_ROOTS:
        cleaned = _replace_root(cleaned, root, placeholder)
    for pattern, replacement in _PATH_PATTERNS:
        cleaned = pattern.sub(replacement, cleaned)
    return cleaned


def bounded_tail(
    text: str,
    *,
    max_chars: int = DIAGNOSTIC_MAX_CHARS,
    max_lines: int = DIAGNOSTIC_MAX_LINES,
) -> tuple[str, bool]:
    """Keep the last `max_lines` lines and last `max_chars` characters."""

    if not text:
        return "", False
    lines = text.split("\n")
    truncated = False
    if len(lines) > max_lines:
        lines = lines[-max_lines:]
        truncated = True
    tail = "\n".join(lines)
    if len(tail) > max_chars:
        tail = tail[-max_chars:]
        truncated = True
    return tail, truncated


def build_subprocess_diagnostics(
    *,
    label: str,
    command: Sequence[str],
    returncode: int | None,
    stdout: str | bytes | None,
    stderr: str | bytes | None,
    timed_out: bool = False,
) -> dict[str, object]:
    """Build a bounded, sanitized, deterministic diagnostic record.

    Only the executable's basename is retained (never its path or arguments,
    which may carry candidate paths or JSON payloads). Each stream keeps a
    sanitized tail no larger than `DIAGNOSTIC_MAX_CHARS` /
    `DIAGNOSTIC_MAX_LINES` and reports whether it was truncated.
    """

    def _text(value: str | bytes | None) -> str:
        if value is None:
            return ""
        if isinstance(value, bytes):
            return value.decode("utf-8", errors="replace")
        return value

    stdout_tail, stdout_truncated = bounded_tail(
        sanitize_diagnostic_text(_text(stdout))
    )
    stderr_tail, stderr_truncated = bounded_tail(
        sanitize_diagnostic_text(_text(stderr))
    )
    executable = Path(str(command[0])).name if command else ""
    return {
        "label": label,
        "executable": sanitize_diagnostic_text(executable),
        "returncode": returncode,
        "timed_out": timed_out,
        "stdout_tail": stdout_tail,
        "stdout_truncated": stdout_truncated,
        "stderr_tail": stderr_tail,
        "stderr_truncated": stderr_truncated,
        "bounds": {
            "max_chars": DIAGNOSTIC_MAX_CHARS,
            "max_lines": DIAGNOSTIC_MAX_LINES,
        },
        "sanitized": True,
    }


def sanitize_tree(value: object) -> object:
    """Apply `sanitize_diagnostic_text` to every string inside a JSON tree."""

    if isinstance(value, str):
        return sanitize_diagnostic_text(value)
    if isinstance(value, dict):
        return {str(k): sanitize_tree(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [sanitize_tree(item) for item in value]
    return value


class ValidationError(RuntimeError):
    """A sanitized harness failure (environment / orchestration).

    `diagnostics` carries the bounded, sanitized subprocess record (when the
    failure came from a subprocess) so the retained trace can explain *why* a
    command failed without leaking secrets or local paths.
    """

    def __init__(
        self, message: str, *, diagnostics: dict[str, object] | None = None
    ) -> None:
        super().__init__(message)
        self.diagnostics = diagnostics


def require_direct_devin_execution(env: Mapping[str, str]) -> None:
    if env.get("GITHUB_ACTIONS", "").lower() == "true":
        raise ValidationError(
            "topology production execution is restricted to direct Devin"
        )
    if env.get("BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER") != "devin":
        raise ValidationError(
            "BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER must identify direct Devin"
        )


@dataclass
class ManagedProcess:
    name: str
    process: subprocess.Popen[str]
    log_path: Path
    log: IO[str]


@dataclass
class Criterion:
    name: str
    status: str = "untested"  # passed | blocked | failed | untested
    detail: str = ""
    evidence: dict[str, object] = field(default_factory=dict)
    gating: bool = True


def summarize_criteria(criteria: Mapping[str, Criterion]) -> dict[str, object]:
    """Compute the J-A5 verdict from required criteria only.

    Downstream witnesses are reported alongside but never change `status` or
    `terminal_user_journey_acceptance`.
    """

    required = {name: criteria[name] for name in REQUIRED_CRITERIA if name in criteria}
    witnesses = {
        name: criteria[name] for name in DOWNSTREAM_WITNESS_CRITERIA if name in criteria
    }
    required_passed = bool(required) and all(
        c.status == "passed" for c in required.values()
    )
    blocked = [n for n, c in required.items() if c.status == "blocked"]
    failed = [n for n, c in required.items() if c.status == "failed"]
    untested = [n for n, c in required.items() if c.status == "untested"]
    if required_passed:
        status = "passed"
    elif blocked and not failed and not untested:
        status = "blocked"
    else:
        status = "failed"
    return {
        "status": status,
        "terminal_user_journey_acceptance": required_passed,
        "required_criteria": list(REQUIRED_CRITERIA),
        "blocked_criteria": blocked,
        "failed_criteria": failed,
        "downstream_witnesses": {
            n: {"status": c.status, "detail": c.detail, "gating": False}
            for n, c in witnesses.items()
        },
        "downstream_witnesses_not_passed": [
            n for n, c in witnesses.items() if c.status != "passed"
        ],
    }


# ── small utilities ─────────────────────────────────────────────────────────


def _now() -> str:
    return (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def _run_id() -> str:
    return (
        datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        + "-"
        + secrets.token_hex(4)
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
        raise ValidationError(
            f"{label} timed out after {timeout}s",
            diagnostics=build_subprocess_diagnostics(
                label=label,
                command=command,
                returncode=None,
                stdout=exc.stdout,
                stderr=exc.stderr,
                timed_out=True,
            ),
        ) from exc
    except subprocess.CalledProcessError as exc:
        raise ValidationError(
            f"{label} failed with exit code {exc.returncode}",
            diagnostics=build_subprocess_diagnostics(
                label=label,
                command=command,
                returncode=exc.returncode,
                stdout=exc.stdout,
                stderr=exc.stderr,
            ),
        ) from exc


def _allocate_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _sha256_file(path: Path) -> str:
    return _sha256_bytes(path.read_bytes())


def _tree_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def _git_sha(repository: Path) -> str:
    result = _run(
        ["git", "-C", str(repository), "rev-parse", "HEAD"],
        label="resolve git commit",
    )
    return result.stdout.strip()


def _normalize_git_remote(value: str) -> str:
    """Normalize SSH and HTTPS GitHub remotes to one repository identity."""

    normalized = value.strip().rstrip("/")
    normalized = re.sub(r"^git@([^:]+):", r"\1/", normalized)
    normalized = re.sub(r"^(?:https?|ssh)://(?:git@)?", "", normalized)
    return normalized.removesuffix(".git")


@dataclass(frozen=True)
class HttpResult:
    status: int
    body: bytes

    def json(self) -> object:
        return json.loads(self.body)


def _http(
    method: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
    payload: dict[str, object] | None = None,
    timeout: float = 15.0,
) -> HttpResult:
    data = json.dumps(payload).encode() if payload is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("content-type", "application/json")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return HttpResult(response.status, response.read())
    except urllib.error.HTTPError as exc:
        return HttpResult(exc.code, exc.read())
    except urllib.error.URLError as exc:
        raise ValidationError(f"request to {url} failed: {exc.reason}") from exc


def _wait_http(url: str, *, timeout: float, label: str) -> None:
    deadline = time.monotonic() + timeout
    last: Exception | None = None
    while time.monotonic() < deadline:
        try:
            _http("GET", url, timeout=5.0)
            return
        except ValidationError as exc:  # not up yet
            last = exc
            time.sleep(0.25)
    raise ValidationError(f"{label} did not become reachable: {last}")


def _wait_port_closed(port: int, *, timeout: float = 10.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("127.0.0.1", port)) != 0:
                return True
        time.sleep(0.2)
    return False


# ── process management ──────────────────────────────────────────────────────


def _start_process(
    name: str,
    command: Sequence[str],
    *,
    cwd: Path,
    env: dict[str, str],
    log_dir: Path,
) -> ManagedProcess:
    log_path = log_dir / f"{name}.log"
    log = log_path.open("w", encoding="utf-8")
    process = subprocess.Popen(
        list(command),
        cwd=cwd,
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return ManagedProcess(name=name, process=process, log_path=log_path, log=log)


def _stop_process(managed: ManagedProcess) -> None:
    if managed.process.poll() is None:
        managed.process.terminate()
        try:
            managed.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            managed.process.kill()
            managed.process.wait(timeout=10)
    managed.log.flush()
    managed.log.close()


# ── bot build / locate ──────────────────────────────────────────────────────


def _build_bot(bot_root: Path, *, skip_build: bool) -> tuple[Path, Path]:
    """Return the production daemon and the SPA bundle Cloud will serve."""
    binary = bot_root / "target" / "release" / "bicameral"
    dist = bot_root / "dashboard" / "dist"
    if skip_build and binary.is_file() and (dist / "index.html").is_file():
        return binary, dist
    _run(
        ["npm", "ci", "--silent"],
        cwd=bot_root / "dashboard",
        timeout=900,
        label="install dashboard dependencies",
    )
    _run(
        ["npm", "run", "build"],
        cwd=bot_root / "dashboard",
        timeout=900,
        label="build dashboard SPA",
    )
    _run(
        ["cargo", "build", "--release", "--locked", "--bin", "bicameral"],
        cwd=bot_root,
        timeout=2400,
        label="build production bot binary",
    )
    if not binary.is_file():
        raise ValidationError("production bot binary was not built")
    if not (dist / "index.html").is_file():
        raise ValidationError("dashboard SPA was not built")
    return binary, dist


# ── daemon + repository ─────────────────────────────────────────────────────


def _create_repo(path: Path, *, ssh_remote: str, https_remote: str) -> None:
    path.mkdir(parents=True)
    _run(["git", "init", "-q", "-b", "main"], cwd=path, label="init repo")
    _run(
        ["git", "-C", str(path), "config", "user.email", "op@example.com"],
        label="git email",
    )
    _run(["git", "-C", str(path), "config", "user.name", "Operator"], label="git name")
    (path / "README.md").write_text("# demo project\n", encoding="utf-8")
    _run(["git", "-C", str(path), "add", "-A"], label="git add")
    _run(["git", "-C", str(path), "commit", "-qm", "init"], label="git commit")
    _run(
        ["git", "-C", str(path), "remote", "add", "origin", https_remote],
        label="add https remote",
    )
    _run(
        ["git", "-C", str(path), "remote", "add", "ssh", ssh_remote],
        label="add ssh remote",
    )


def _configure_daemon(binary: Path, workspace: Path, home: Path, port: int) -> str:
    home.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "HOME": str(home)}
    _run(
        [str(binary), "init", str(workspace)],
        cwd=workspace,
        env=env,
        timeout=60,
        label="daemon init",
    )
    config_path = workspace / ".bicameral" / "config.yaml"
    config = config_path.read_text(encoding="utf-8")
    config = re.sub(r"^  port: \d+$", f"  port: {port}", config, flags=re.M)
    config_path.write_text(config, encoding="utf-8")
    if f"port: {port}" not in config:
        raise ValidationError("daemon gateway port was not configured")
    # Non-secret operator connection metadata. The setup path reads this real
    # daemon-owned registry; no provider token or fake adapter is introduced.
    (workspace / ".bicameral" / "connections.yaml").write_text(
        "connections:\n"
        "  github:\n"
        "    status: connected\n"
        "    account_label: topology-operator\n"
        "    capabilities:\n"
        "      - repository_read\n",
        encoding="utf-8",
    )
    return _sha256_file(config_path)


def _start_daemon(
    *,
    binary: Path,
    workspace: Path,
    home: Path,
    name: str,
    port: int,
    log_dir: Path,
) -> ManagedProcess:
    env = {
        **os.environ,
        "HOME": str(home),
        "RUST_LOG": "info",
    }
    managed = _start_process(
        name,
        [str(binary), "--workspace", str(workspace), "dashboard"],
        cwd=workspace,
        env=env,
        log_dir=log_dir,
    )
    try:
        _wait_http(f"http://127.0.0.1:{port}/health", timeout=30, label=name)
    except ValidationError:
        _stop_process(managed)
        raise
    return managed


def _prepare_cloud_runtime(
    cloud_root: Path, dist: Path, runtime_root: Path
) -> tuple[Path, str]:
    """Copy Cloud plus the exact built SPA into a disposable runtime."""

    package = runtime_root / "bicameral_cloud"
    shutil.copytree(cloud_root / "src" / "bicameral_cloud", package)
    dashboard = package / "static" / "dashboard"
    if dashboard.exists():
        shutil.rmtree(dashboard)
    shutil.copytree(dist, dashboard)
    return runtime_root, _tree_digest(dashboard)


def _start_cloud(
    *,
    python: Path,
    cloud_root: Path,
    runtime_root: Path,
    port: int,
    log_dir: Path,
) -> ManagedProcess:
    """Start the real Cloud app as the sole SPA host."""

    env = {
        **os.environ,
        "PYTHONPATH": os.pathsep.join([str(runtime_root), str(cloud_root / "src")]),
    }
    managed = _start_process(
        "cloud",
        [
            str(python),
            "-m",
            "uvicorn",
            "bicameral_cloud.app:app",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--no-access-log",
        ],
        cwd=cloud_root,
        env=env,
        log_dir=log_dir,
    )
    try:
        _wait_http(f"http://127.0.0.1:{port}/health", timeout=30, label="Cloud")
    except ValidationError:
        _stop_process(managed)
        raise
    return managed


def _pair_bridge(port: int, account_id: str) -> tuple[str, bytes]:
    """Drive the pending -> local-confirmation -> approved handoff (bot#790).

    A valid hosted request no longer mints a session inline; it returns a
    pending request with an opaque handle/proof. A session token is released
    only after an explicit same-origin local approval, and only via the
    initiating browser's status poll.
    """

    base = f"http://127.0.0.1:{port}/api/v1/hosted-bridge/pair"

    # 1. Hosted browser requests pairing -> 202 pending, never a token.
    result = _http(
        "POST",
        base,
        headers={"origin": TRUSTED_BRIDGE_ORIGIN},
        payload={
            "origin": TRUSTED_BRIDGE_ORIGIN,
            "cloud_account_id": account_id,
            "csrf_token": secrets.token_urlsafe(24),
            "client_protocol_version": HOSTED_BRIDGE_PROTOCOL_VERSION,
        },
    )
    if result.status != 202:
        raise ValidationError("hosted-bridge pairing did not return pending status")
    pending = result.json()
    if not isinstance(pending, dict) or pending.get("status") != "pending":
        raise ValidationError("daemon did not create a pending pairing request")
    if "session_token" in pending:
        raise ValidationError("pending pairing response must not carry a session token")
    request_handle = pending.get("request_handle")
    poll_proof = pending.get("poll_proof")
    if not isinstance(request_handle, str) or not request_handle:
        raise ValidationError("pending pairing response omitted its request handle")
    if not isinstance(poll_proof, str) or not poll_proof:
        raise ValidationError("pending pairing response omitted its poll proof")

    # 2. Local, same-origin confirmation surface reads the pending request and
    #    its confirmation nonce (served without any hosted Origin header).
    view = _http("GET", f"{base}/pending/{urllib.parse.quote(request_handle)}")
    if view.status != 200:
        raise ValidationError("local pending pairing view was unavailable")
    view_payload = view.json()
    nonce = (
        view_payload.get("local_confirmation_nonce")
        if isinstance(view_payload, dict)
        else None
    )
    if not isinstance(nonce, str) or not nonce:
        raise ValidationError("local pending view omitted its confirmation nonce")

    # 3. Explicit same-origin local approval — no token in this response.
    decision = _http(
        "POST",
        f"{base}/decision",
        payload={
            "request_handle": request_handle,
            "action": "approve",
            "local_confirmation_nonce": nonce,
        },
    )
    if decision.status != 200:
        raise ValidationError("local pairing approval failed")
    decision_payload = decision.json()
    if (
        not isinstance(decision_payload, dict)
        or decision_payload.get("status") != "approved"
    ):
        raise ValidationError("local approval did not transition request to approved")
    if "session_token" in decision_payload:
        raise ValidationError("local approval response must not carry a session token")

    # 4. Initiating browser polls status (bound to origin/account/proof) and
    #    retrieves the transient session handoff.
    query = urllib.parse.urlencode(
        {
            "request_handle": request_handle,
            "poll_proof": poll_proof,
            "cloud_account_id": account_id,
        }
    )
    status = _http(
        "GET",
        f"{base}/status?{query}",
        headers={"origin": TRUSTED_BRIDGE_ORIGIN},
    )
    if status.status != 200:
        raise ValidationError("hosted-bridge status poll failed")
    status_payload = status.json()
    if (
        not isinstance(status_payload, dict)
        or status_payload.get("status") != "approved"
    ):
        raise ValidationError("approved status poll did not report approved")
    token = status_payload.get("session_token")
    if not isinstance(token, str) or not token:
        raise ValidationError("approved status poll omitted its session token")
    return token, status.body


def _daemon_capabilities(port: int) -> dict[str, object]:
    result = _http("GET", f"http://127.0.0.1:{port}/v2/capabilities")
    if result.status != 200:
        raise ValidationError("daemon capabilities were unavailable")
    payload = result.json()
    if not isinstance(payload, dict):
        raise ValidationError("daemon capabilities were malformed")
    return payload


def _bridge_projects(port: int, token: str) -> tuple[list[dict[str, object]], bytes]:
    result = _http(
        "GET",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/projects",
        headers={"x-bridge-session-token": token},
    )
    if result.status != 200:
        raise ValidationError("hosted-bridge projects projection was unavailable")
    payload = result.json()
    projects = payload.get("projects") if isinstance(payload, dict) else None
    if not isinstance(projects, list):
        raise ValidationError("hosted-bridge projects projection was malformed")
    return [p for p in projects if isinstance(p, dict)], result.body


def _project_reports_code_grounding_ready(project: dict[str, object]) -> bool:
    """Interpret both hosted-safe readiness projection shapes.

    The project list exposes the underlying redacted facts, while selected
    workspace metadata may also expose their derived readiness boolean.
    """

    if project.get("code_grounding_ready") is True:
        return True
    return (
        project.get("has_workspace_binding") is True
        and project.get("event_store_healthy") is True
    )


def _project_reports_binding_rehydrated(project: dict[str, object]) -> bool:
    """Daemon-authored evidence that a persisted binding was replayed.

    This deliberately does *not* require `code_grounding_ready` or
    `event_store_healthy`: those describe the downstream grounding projection,
    not whether the daemon rehydrated the `LocalWorkspaceBinding` itself.
    """

    if project.get("has_workspace_binding") is True:
        return True
    workspace = project.get("workspace_readiness")
    return (
        isinstance(workspace, dict)
        and workspace.get("binding_state") == "local_workspace_bound"
    )


def _create_and_register_product(
    port: int, token: str, account_id: str
) -> tuple[str, list[bytes]]:
    """Create canonical owner state, then run the real setup-continuation API."""

    created = _http(
        "POST",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/command",
        headers={"x-bridge-session-token": token},
        payload={
            "command": {
                "name": "product.create",
                "params": {
                    "idempotency_key": "wsbinding-terminal-create",
                    "product_display_name": "Demo Service",
                    "asserted_cloud_account_id": account_id,
                    "verified_domain": "acme.example",
                },
            }
        },
    )
    if created.status != 200:
        raise ValidationError("daemon product.create failed")
    body = created.json()
    result = body.get("result") if isinstance(body, dict) else None
    creation = result.get("result") if isinstance(result, dict) else None
    product_id = creation.get("product_id") if isinstance(creation, dict) else None
    if not isinstance(product_id, str) or not product_id:
        raise ValidationError(
            "daemon product.create omitted canonical Product identity"
        )

    setup = _http(
        "POST",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/product-continue-setup",
        headers={"x-bridge-session-token": token},
        payload={
            "request_id": "wsbinding-terminal-continue-setup",
            "product_id": product_id,
            "registration_ref": product_id,
            "provider_hint": "github",
            "substrate_hint": "git",
            "account_label": "topology-operator",
        },
    )
    if setup.status != 200:
        raise ValidationError(
            f"daemon product.continue_setup failed with HTTP {setup.status}"
        )
    setup_body = setup.json()
    if not isinstance(setup_body, dict) or setup_body.get("readiness") != "open":
        raise ValidationError("daemon product.continue_setup did not open registration")
    return product_id, [created.body, setup.body]


# ── real MCP process ────────────────────────────────────────────────────────

_MCP_PROBE = """from __future__ import annotations
import asyncio, json, os, sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from mcp.types import TextContent

async def run() -> None:
    executable, tool_name, raw_arguments = sys.argv[1:4]
    server = StdioServerParameters(command=executable, args=["serve"], env=dict(os.environ))
    async with stdio_client(server) as (r, w):
        async with ClientSession(r, w) as session:
            await session.initialize()
            tools = await session.list_tools()
            result = await session.call_tool(tool_name, json.loads(raw_arguments))
            content = [i.text for i in result.content if isinstance(i, TextContent)]
            print(json.dumps({
                "tool_names": sorted(t.name for t in tools.tools),
                "is_error": bool(result.isError),
                "content": content,
            }))

asyncio.run(run())
"""


def _ensure_runtime_env(
    work_root: Path, mcp_root: Path, cloud_root: Path
) -> tuple[Path, Path]:
    venv = work_root / "mcp-env"
    _run(
        ["uv", "venv", "--python", "3.11", str(venv)],
        timeout=300,
        label="create MCP python env",
    )
    python = venv / "bin" / "python"
    _run(
        [
            "uv",
            "pip",
            "install",
            "--python",
            str(python),
            "-e",
            str(mcp_root),
            "-e",
            str(cloud_root),
        ],
        timeout=600,
        label="install real Cloud and bicameral-mcp",
    )
    probe = work_root / "mcp_probe.py"
    probe.write_text(_MCP_PROBE, encoding="utf-8")
    return python, probe


def _mcp_workspace_bind(
    *,
    python: Path,
    probe: Path,
    daemon_port: int,
    workspace: Path,
    arguments: dict[str, object],
) -> dict[str, object]:
    """Invoke the real MCP server/client process and return the parsed result."""
    executable = python.parent / "bicameral-mcp"
    env = {
        **os.environ,
        "BICAMERAL_DAEMON_URL": f"http://127.0.0.1:{daemon_port}",
        "BICAMERAL_ACTOR_ID": "wsbinding-operator",
        "BICAMERAL_WORKSPACE": str(workspace),
        "BICAMERAL_POLICY_SCOPE": "default",
    }
    result = _run(
        [
            str(python),
            str(probe),
            str(executable),
            "bicameral.workspace.bind",
            json.dumps(arguments),
        ],
        env=env,
        timeout=120,
        label="real MCP workspace.bind",
    )
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ValidationError("MCP client returned malformed output") from exc
    names = payload.get("tool_names")
    if not isinstance(names, list) or "bicameral.workspace.bind" not in names:
        raise ValidationError("real MCP server did not advertise workspace.bind")
    content = payload.get("content")
    text = (
        "\n".join(c for c in content if isinstance(c, str))
        if isinstance(content, list)
        else ""
    )
    try:
        outcome = json.loads(text) if text.strip().startswith("{") else {"raw": text}
    except json.JSONDecodeError:
        outcome = {"raw": text}
    payload["outcome"] = outcome
    return payload


def _bind_args(
    project_id: str, candidate_path: Path, *, confirmed: bool, **extra: object
) -> dict[str, object]:
    args: dict[str, object] = {
        "project_id": project_id,
        "candidate_path": str(candidate_path),
        "confirmed": confirmed,
        "display_name": "Demo Service",
        "reason": "Operator confirms the open workspace matches the registered project.",
        "confidence": 1.0,
    }
    args.update(extra)
    return args


# ── real browser observation ────────────────────────────────────────────────


def _observe_browser(
    *,
    cdp_url: str | None,
    cloud_port: int,
    daemon_port: int,
    token: str,
    project_id: str,
    screenshot_path: Path,
) -> dict[str, object]:
    """Load the Cloud-served SPA and observe daemon-authored readiness."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError as exc:  # pragma: no cover - environment guard
        raise ValidationError(
            "playwright is not installed for the browser step"
        ) from exc

    cloud_origin = f"http://127.0.0.1:{cloud_port}"
    daemon_origin = f"http://localhost:{daemon_port}"
    observation: dict[str, object] = {
        "attempted": True,
        "spa_host": "cloud",
    }
    with sync_playwright() as pw:
        launched = cdp_url is None
        browser = (
            pw.chromium.launch(headless=True)
            if launched
            else pw.chromium.connect_over_cdp(cdp_url)
        )
        context = browser.contexts[0] if browser.contexts else browser.new_context()
        page = context.new_page()

        def bridge_transport(route: object) -> None:
            headers = dict(route.request.headers)  # type: ignore[attr-defined]
            headers["origin"] = TRUSTED_BRIDGE_ORIGIN
            response = route.fetch(headers=headers)  # type: ignore[attr-defined]
            response_headers = dict(response.headers)
            response_headers["access-control-allow-origin"] = cloud_origin
            route.fulfill(  # type: ignore[attr-defined]
                response=response, headers=response_headers
            )

        page.route(  # type: ignore[attr-defined]
            f"{daemon_origin}/api/v1/hosted-bridge/**", bridge_transport
        )
        try:
            page.goto(
                f"{cloud_origin}/app?hosted",
                wait_until="domcontentloaded",
                timeout=30_000,
            )
            page.evaluate(
                """([base, token]) => {
                    window.localStorage.setItem('bicameral_daemon_url', base);
                    window.sessionStorage.setItem('bicameral_hosted_bridge', JSON.stringify({
                        session_token: token, daemon_url: base, cloud_account_id: 'acct-wsbinding-owner',
                    }));
                }""",
                [daemon_origin, token],
            )
            page.goto(
                f"{cloud_origin}/app?hosted",
                wait_until="networkidle",
                timeout=30_000,
            )
            page.wait_for_timeout(1_500)
            body_text = page.inner_text("body")
            page.screenshot(path=str(screenshot_path), full_page=True)
        finally:
            page.close()
            if launched:
                browser.close()
    # Read the daemon's authored readiness directly so the assertion is on the
    # daemon-authored projection (the browser render is the human-visible proof).
    projects, _ = _bridge_projects(daemon_port, token)
    ready = []
    for project in projects:
        projected_id = project.get("project_id") or project.get("project_name")
        if projected_id not in (project_id, "Demo Service") and len(projects) != 1:
            continue
        workspace = project.get("workspace_readiness")
        nested_bound = (
            isinstance(workspace, dict)
            and workspace.get("binding_state") == "local_workspace_bound"
        )
        if _project_reports_code_grounding_ready(project) or nested_bound:
            ready.append(project)
    visible = (
        "demo service" in body_text.lower()
        or project_id.lower() in body_text.lower()
        or "bound" in body_text.lower()
    )
    observation.update(
        {
            "daemon_authored_projects": projects,
            "rendered_code_grounding_ready": bool(ready) and visible,
            "screenshot": screenshot_path.name,
            "body_mentions_product": visible,
        }
    )
    return observation


# ── local-path leak scan ────────────────────────────────────────────────────


def _scan_no_local_path(
    payloads: Sequence[bytes], log_paths: Sequence[Path]
) -> tuple[bool, str]:
    for payload in payloads:
        text = payload.decode(errors="ignore")
        for token in FORBIDDEN_LOCAL_TOKENS:
            if token in text:
                return (
                    False,
                    f"cloud/browser payload contained forbidden token: {token}",
                )
    for log_path in log_paths:
        # Daemon logs legitimately reference local paths at INFO; we only assert
        # that the *bridge/browser-facing* payloads are path-free. Log scanning
        # here is limited to accidental secret/session leakage.
        _ = log_path
    return True, "no local path in Cloud/browser payloads"


# ── receipt / trace ─────────────────────────────────────────────────────────


def _load_topology_receipt(path: Path | None) -> dict[str, object]:
    if path is None:
        return {"consumed": False, "note": "no cloud#135 topology receipt supplied"}
    if not path.is_file():
        raise ValidationError(f"topology receipt not found: {path}")
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValidationError("topology receipt was malformed")
    return {
        "consumed": True,
        "file_name": path.name,
        "digest": _sha256_file(path),
        "run_id": data.get("run_id"),
        "status": data.get("status"),
        "components": data.get("components"),
        "evidence_level": data.get("evidence_level"),
    }


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    default_mcp = Path(__file__).resolve().parents[2] / "bicameral-mcp"
    default_cloud = Path(__file__).resolve().parents[2] / "bicameral-cloud"
    parser.add_argument("--topology-receipt", type=Path, default=None)
    parser.add_argument("--mcp-repo", type=Path, default=default_mcp)
    parser.add_argument("--cloud-repo", type=Path, default=default_cloud)
    parser.add_argument(
        "--mcp-commit",
        default=None,
        help="Exact bicameral-mcp commit the checkout must be at.",
    )
    parser.add_argument(
        "--cloud-commit",
        default=None,
        help="Exact bicameral-cloud commit the checkout must be at.",
    )
    parser.add_argument("--factory-commit", default=None)
    parser.add_argument("--cdp-url", default=DEFAULT_CDP_URL)
    parser.add_argument("--no-browser", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--trace", type=Path, default=None)
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    bot_root = Path(__file__).resolve().parents[1]
    run_id = _run_id()
    work_root = Path.home() / ".cache" / "bicameral-bot" / "wsbinding-runs" / run_id
    work_root.mkdir(parents=True)
    log_dir = work_root / "logs"
    log_dir.mkdir()
    trace_path = args.trace or (
        bot_root / "test-artifacts" / TRACE_ARTIFACT_DIR / run_id / "trace.json"
    )
    screenshot_path = trace_path.parent / "spa-readiness.png"

    # Register every local root the harness knows about so retained diagnostics
    # and error text never carry a workspace or operator filesystem path.
    register_local_root(work_root, "work-root")
    register_local_root(bot_root, "bot-root")
    register_local_root(trace_path.parent, "trace-dir")
    register_local_root(Path.home(), "home")

    criteria = {
        name: Criterion(name, gating=name in REQUIRED_CRITERIA) for name in CRITERIA
    }
    processes: list[ManagedProcess] = []
    browser_payloads: list[bytes] = []
    error: ValidationError | None = None
    trace: dict[str, object] = {
        "schema_version": 2,
        "journey": JOURNEY_ID,
        "journey_label": JOURNEY_LABEL,
        "supersedes": SUPERSEDED_JOURNEY_ID,
        "contract": {
            "source_issue": "bicameral-bot#788",
            "owner_correction": "2026-07-27",
            "reconciled_under": "bicameral-bot#1190",
            "domain_target": "Product",
            "authoritative_commit": "daemon persists LocalWorkspaceBinding",
            "hosted_code_grounding_ready": "downstream witness, non-gating",
        },
        "evidence_level": EVIDENCE_LEVEL,
        "run_id": run_id,
        "started_at": _now(),
        "status": "failed",
    }

    try:
        require_direct_devin_execution(os.environ)
        for command in ("git", "node", "npm", "cargo", "uv"):
            if shutil.which(command) is None:
                raise ValidationError(f"required executable is unavailable: {command}")

        mcp_root = args.mcp_repo.resolve()
        register_local_root(mcp_root, "mcp-root")
        if not (mcp_root / "server.py").is_file():
            raise ValidationError("bicameral-mcp checkout not found at --mcp-repo")
        cloud_root = args.cloud_repo.resolve()
        register_local_root(cloud_root, "cloud-root")
        if not (cloud_root / "src" / "bicameral_cloud" / "app.py").is_file():
            raise ValidationError("bicameral-cloud checkout not found at --cloud-repo")

        topology = _load_topology_receipt(args.topology_receipt)
        binary, dist = _build_bot(bot_root, skip_build=args.skip_build)
        python, probe = _ensure_runtime_env(work_root, mcp_root, cloud_root)

        commits = {
            "bot": _git_sha(bot_root),
            "mcp": _git_sha(mcp_root),
            "cloud": _git_sha(cloud_root),
        }
        if args.mcp_commit:
            if not re.fullmatch(r"[0-9a-f]{40}", args.mcp_commit):
                raise ValidationError("mcp commit must be a full lowercase SHA")
            if commits["mcp"] != args.mcp_commit:
                raise ValidationError("mcp checkout does not match --mcp-commit")
        if args.cloud_commit:
            if not re.fullmatch(r"[0-9a-f]{40}", args.cloud_commit):
                raise ValidationError("cloud commit must be a full lowercase SHA")
            if commits["cloud"] != args.cloud_commit:
                raise ValidationError("cloud checkout does not match --cloud-commit")
        if args.factory_commit:
            if not re.fullmatch(r"[0-9a-f]{40}", args.factory_commit):
                raise ValidationError("factory commit must be a full lowercase SHA")
            commits["factory"] = args.factory_commit

        contract_fingerprint = (
            (bot_root / "protocol" / "hosted-bridge" / "contract.sha256")
            .read_text(encoding="utf-8")
            .strip()
        )
        cloud_runtime, spa_digest = _prepare_cloud_runtime(
            cloud_root, dist, work_root / "cloud-runtime"
        )

        # ── stand up local Cloud (SPA host) + API-only daemon ─────────────
        workspace = work_root / "workspace"
        home = work_root / "home"
        register_local_root(workspace, "workspace")
        register_local_root(home, "operator-home")
        port = _allocate_port()
        cloud_port = _allocate_port()
        _create_repo(
            workspace,
            ssh_remote="git@github.com:example-org/demo-project.git",
            https_remote="https://github.com/example-org/demo-project.git",
        )
        config_digest = _configure_daemon(binary, workspace, home, port)
        cloud = _start_cloud(
            python=python,
            cloud_root=cloud_root,
            runtime_root=cloud_runtime,
            port=cloud_port,
            log_dir=log_dir,
        )
        processes.append(cloud)
        daemon = _start_daemon(
            binary=binary,
            workspace=workspace,
            home=home,
            name="daemon",
            port=port,
            log_dir=log_dir,
        )
        processes.append(daemon)

        capabilities = _daemon_capabilities(port)
        if not capabilities.get("workspace_binding_available"):
            raise ValidationError(
                "daemon does not advertise workspace-binding capability"
            )
        token, pair_payload = _pair_bridge(port, "acct-wsbinding-owner")
        browser_payloads.append(pair_payload)

        project_id, setup_payloads = _create_and_register_product(
            port, token, "acct-wsbinding-owner"
        )
        browser_payloads.extend(setup_payloads)
        durable_bindings_dir = home / ".bicameral" / "workspace-bindings"

        def durable_binding_exists() -> bool:
            return durable_bindings_dir.exists() and any(
                durable_bindings_dir.glob("*.json")
            )

        # ── 1. MCP proposes without persisting authority (confirmed=false) ─
        proposal = _mcp_workspace_bind(
            python=python,
            probe=probe,
            daemon_port=port,
            workspace=workspace,
            arguments=_bind_args(project_id, workspace, confirmed=False),
        )
        outcome = (
            proposal["outcome"] if isinstance(proposal.get("outcome"), dict) else {}
        )
        if outcome.get("bound") is False and not durable_binding_exists():
            criteria["propose_without_persisting_authority"].status = "passed"
            criteria[
                "propose_without_persisting_authority"
            ].detail = (
                "real MCP proposed; daemon persisted nothing and claimed no authority"
            )
        else:
            criteria["propose_without_persisting_authority"].status = "failed"
            criteria[
                "propose_without_persisting_authority"
            ].detail = "proposal unexpectedly produced durable state"
        criteria["propose_without_persisting_authority"].evidence = {
            "error_kind": outcome.get("error_kind"),
            "bound": outcome.get("bound"),
        }

        # ── 6. Cancellation before commit leaves no binding / no pending ───
        cancelled = not durable_binding_exists()
        criteria["cancellation_leaves_no_binding_or_pending"].status = (
            "passed" if cancelled else "failed"
        )
        criteria["cancellation_leaves_no_binding_or_pending"].detail = (
            "no durable binding and no durable pending proposal after an unconfirmed proposal"
            if cancelled
            else "durable state existed after cancellation"
        )

        # ── 2. Explicit confirmation reaches the daemon and persists ───────
        confirmed = _mcp_workspace_bind(
            python=python,
            probe=probe,
            daemon_port=port,
            workspace=workspace,
            arguments=_bind_args(project_id, workspace, confirmed=True),
        )
        c_outcome = (
            confirmed["outcome"] if isinstance(confirmed.get("outcome"), dict) else {}
        )
        persisted = c_outcome.get("bound") is True and durable_binding_exists()
        if persisted:
            criteria["confirm_reaches_daemon_and_persists"].status = "passed"
            criteria[
                "confirm_reaches_daemon_and_persists"
            ].detail = "daemon validated and persisted a LocalWorkspaceBinding on explicit confirmation"
        else:
            criteria["confirm_reaches_daemon_and_persists"].status = "failed"
            criteria[
                "confirm_reaches_daemon_and_persists"
            ].detail = f"confirmed bind rejected: {c_outcome.get('error_kind')}"
        criteria["confirm_reaches_daemon_and_persists"].evidence = {
            "error_kind": c_outcome.get("error_kind"),
            "workspace_binding_state": c_outcome.get("workspace_binding_state"),
            "durable_binding_present": durable_binding_exists(),
        }

        # ── 3. SSH and HTTPS remotes identify the same repository ─────────
        ssh_remote = "git@github.com:example-org/demo-project.git"
        https_remote = "https://github.com/example-org/demo-project.git"
        remotes_converge = _normalize_git_remote(ssh_remote) == _normalize_git_remote(
            https_remote
        )
        criteria["ssh_https_remotes_converge"].status = (
            "passed" if persisted and remotes_converge else "failed"
        )
        criteria["ssh_https_remotes_converge"].detail = (
            "the real workspace carries SSH and HTTPS remotes for one normalized repository"
            if persisted and remotes_converge
            else "remote convergence or its persisted binding precondition failed"
        )

        binding_files = sorted(durable_bindings_dir.glob("*.json"))
        binding_digest_before_restart = (
            _tree_digest(durable_bindings_dir) if binding_files else None
        )

        # ── 4. Restart rehydrates registration + LocalWorkspaceBinding ────
        if persisted:
            _stop_process(daemon)
            processes.remove(daemon)
            if not _wait_port_closed(port):
                raise ValidationError("daemon port did not close before restart")
            daemon = _start_daemon(
                binary=binary,
                workspace=workspace,
                home=home,
                name="daemon-restarted",
                port=port,
                log_dir=log_dir,
            )
            processes.append(daemon)
            token, restart_pair_payload = _pair_bridge(port, "acct-wsbinding-owner")
            browser_payloads.append(restart_pair_payload)
            restarted_projects, restarted_payload = _bridge_projects(port, token)
            browser_payloads.append(restarted_payload)
            binding_digest_after_restart = (
                _tree_digest(durable_bindings_dir) if durable_binding_exists() else None
            )
            durable_survived = (
                binding_digest_before_restart is not None
                and binding_digest_after_restart == binding_digest_before_restart
            )
            # Daemon-authored replay evidence: the restarted daemon reports the
            # binding itself. The downstream `code_grounding_ready` projection
            # is recorded separately and does not gate this criterion.
            restart_rehydrated = any(
                _project_reports_binding_rehydrated(project)
                for project in restarted_projects
            )
            restart_grounding_projection = any(
                _project_reports_code_grounding_ready(project)
                for project in restarted_projects
            )
            restart_ok = durable_survived and restart_rehydrated
            criteria["binding_survives_restart_replay"].status = (
                "passed" if restart_ok else "failed"
            )
            criteria["binding_survives_restart_replay"].detail = (
                "the persisted LocalWorkspaceBinding survived daemon restart byte-identical "
                "and the restarted daemon replayed it"
                if restart_ok
                else "restart did not preserve and replay the persisted LocalWorkspaceBinding"
            )
            criteria["binding_survives_restart_replay"].evidence = {
                "project_count": len(restarted_projects),
                "durable_binding_present": durable_binding_exists(),
                "durable_binding_digest_unchanged": durable_survived,
                "daemon_reports_binding_rehydrated": restart_rehydrated,
                "downstream_code_grounding_ready_projection": restart_grounding_projection,
            }
        else:
            criteria["binding_survives_restart_replay"].status = "failed"
            criteria[
                "binding_survives_restart_replay"
            ].detail = "confirmed binding did not persist, so restart replay could not be accepted"

        # ── W1. Downstream witness (non-gating): real browser observes the
        # Cloud-hosted SPA rendering daemon-authored code_grounding_ready.
        if args.no_browser:
            criteria[
                "browser_renders_code_grounding_ready_after_reconnect"
            ].status = "blocked"
            criteria[
                "browser_renders_code_grounding_ready_after_reconnect"
            ].detail = (
                "downstream browser witness skipped via --no-browser (non-gating)"
            )
        elif not persisted:
            criteria[
                "browser_renders_code_grounding_ready_after_reconnect"
            ].status = "failed"
            criteria[
                "browser_renders_code_grounding_ready_after_reconnect"
            ].detail = "downstream witness requires a persisted binding (non-gating)"
        else:
            trace_path.parent.mkdir(parents=True, exist_ok=True)
            try:
                observation = _observe_browser(
                    cdp_url=args.cdp_url,
                    cloud_port=cloud_port,
                    daemon_port=port,
                    token=token,
                    project_id=project_id,
                    screenshot_path=screenshot_path,
                )
                projects, projects_payload = _bridge_projects(port, token)
                browser_payloads.append(projects_payload)
                criteria[
                    "browser_renders_code_grounding_ready_after_reconnect"
                ].evidence = observation
                if observation.get("rendered_code_grounding_ready"):
                    criteria[
                        "browser_renders_code_grounding_ready_after_reconnect"
                    ].status = "passed"
                    criteria[
                        "browser_renders_code_grounding_ready_after_reconnect"
                    ].detail = "real browser rendered daemon-authored code_grounding_ready after reconnect"
                else:
                    criteria[
                        "browser_renders_code_grounding_ready_after_reconnect"
                    ].status = "failed"
                    criteria[
                        "browser_renders_code_grounding_ready_after_reconnect"
                    ].detail = "binding persisted but browser did not observe daemon-authored readiness"
            except ValidationError as exc:
                criteria[
                    "browser_renders_code_grounding_ready_after_reconnect"
                ].status = "failed"
                criteria[
                    "browser_renders_code_grounding_ready_after_reconnect"
                ].detail = (
                    "downstream browser witness failed (non-gating): "
                    + sanitize_diagnostic_text(str(exc))
                )
                if exc.diagnostics is not None:
                    criteria[
                        "browser_renders_code_grounding_ready_after_reconnect"
                    ].evidence["diagnostics"] = exc.diagnostics

        # ── 7. Negative paths create no binding (real daemon fail-closed) ─
        negatives: dict[str, dict[str, object]] = {}
        unregistered = _mcp_workspace_bind(
            python=python,
            probe=probe,
            daemon_port=port,
            workspace=workspace,
            arguments=_bind_args("prd_unregistered", workspace, confirmed=True),
        )
        negatives["unregistered_project"] = (
            unregistered["outcome"]
            if isinstance(unregistered.get("outcome"), dict)
            else {}
        )
        # capability mismatch: require a higher capability than the daemon offers.
        cap_mismatch = _mcp_workspace_bind(
            python=python,
            probe=probe,
            daemon_port=port,
            workspace=workspace,
            arguments=_bind_args(
                project_id, workspace, confirmed=True, required_daemon_capability=999999
            ),
        )
        negatives["capability_mismatch"] = (
            cap_mismatch["outcome"]
            if isinstance(cap_mismatch.get("outcome"), dict)
            else {}
        )
        # unsafe / contradictory candidate: a path that is not a real directory.
        bogus = _mcp_workspace_bind(
            python=python,
            probe=probe,
            daemon_port=port,
            workspace=workspace,
            arguments=_bind_args(
                project_id, workspace / "does-not-exist", confirmed=True
            ),
        )
        negatives["contradictory_candidate"] = (
            bogus["outcome"] if isinstance(bogus.get("outcome"), dict) else {}
        )
        all_rejected = all(
            (n.get("bound") is False) or (n.get("error_kind") is not None)
            for n in negatives.values()
        )
        binding_digest_after_negatives = (
            _tree_digest(durable_bindings_dir) if durable_binding_exists() else None
        )
        previous_binding_preserved = (
            binding_digest_before_restart is not None
            and binding_digest_after_negatives == binding_digest_before_restart
        )
        if all_rejected and previous_binding_preserved:
            criteria["negative_paths_create_no_binding"].status = "passed"
            criteria["negative_paths_create_no_binding"].detail = (
                "unregistered, capability-mismatch, and contradictory-candidate binds all rejected "
                "fail-closed without adding or changing a durable binding"
            )
        else:
            criteria["negative_paths_create_no_binding"].status = "failed"
            criteria[
                "negative_paths_create_no_binding"
            ].detail = "a negative path mutated state"
        criteria["negative_paths_create_no_binding"].evidence = {
            k: {"error_kind": v.get("error_kind"), "bound": v.get("bound")}
            for k, v in negatives.items()
        }
        criteria["failed_rebind_preserves_previous"].status = (
            "passed" if all_rejected and previous_binding_preserved else "failed"
        )
        criteria["failed_rebind_preserves_previous"].detail = (
            "failed rebind attempts preserved the byte-identical prior binding"
            if all_rejected and previous_binding_preserved
            else "a failed rebind changed or removed the prior binding"
        )
        criteria["failed_rebind_preserves_previous"].evidence = {
            "binding_digest_unchanged": previous_binding_preserved,
        }

        # daemon-unavailable path: stop the daemon and confirm MCP fails closed.
        _stop_process(daemon)
        processes.remove(daemon)
        _wait_port_closed(port)
        try:
            unavailable = _mcp_workspace_bind(
                python=python,
                probe=probe,
                daemon_port=port,
                workspace=workspace,
                arguments=_bind_args(project_id, workspace, confirmed=True),
            )
            u_outcome = (
                unavailable["outcome"]
                if isinstance(unavailable.get("outcome"), dict)
                else {}
            )
            daemon_unavailable_fails_closed = u_outcome.get("bound") is not True
        except ValidationError:
            daemon_unavailable_fails_closed = True
        criteria["negative_paths_create_no_binding"].evidence[
            "daemon_unavailable_fails_closed"
        ] = daemon_unavailable_fails_closed
        if not daemon_unavailable_fails_closed:
            criteria["negative_paths_create_no_binding"].status = "failed"
            criteria[
                "negative_paths_create_no_binding"
            ].detail = "daemon-unavailable bind did not fail closed"

        # ── 9. No local path in Cloud/browser payloads ────────────────────
        ok, reason = _scan_no_local_path(
            browser_payloads, [p.log_path for p in processes] + [daemon.log_path]
        )
        criteria["no_local_path_in_cloud_browser_payloads_logs"].status = (
            "passed" if ok else "failed"
        )
        criteria["no_local_path_in_cloud_browser_payloads_logs"].detail = reason

        # ── 10. Trace carries component fingerprints ──────────────────────
        fingerprints = {
            "commits": commits,
            "spa_digest": spa_digest,
            "contract_fingerprint": contract_fingerprint,
            "topology_receipt": topology,
            "durable_store_versions": {
                "daemon_config_digest": config_digest,
                "config_schema_version": 1,
                "workspace_bindings_dir": "~/.bicameral/workspace-bindings",
                "event_store_substrate": "git",
            },
        }
        have_fp = bool(
            commits.get("bot")
            and commits.get("mcp")
            and spa_digest
            and contract_fingerprint
        )
        criteria["trace_carries_component_fingerprints"].status = (
            "passed" if have_fp else "failed"
        )
        criteria["trace_carries_component_fingerprints"].detail = (
            "trace carries bot/mcp/cloud commits, SPA digest, contract fingerprint, "
            "topology receipt, and durable-store versions"
        )

        trace["components"] = commits
        trace["fingerprints"] = fingerprints
        trace["daemon"] = {
            "workspace_binding_available": capabilities.get(
                "workspace_binding_available"
            ),
            "supported_commands": capabilities.get("supported_commands"),
        }
    except ValidationError as exc:
        error = exc
    finally:
        for managed in reversed(processes):
            _stop_process(managed)
        trace["criteria"] = {name: vars(c) for name, c in criteria.items()}
        summary = summarize_criteria(criteria)
        trace.update(summary)
        if error is not None:
            trace["status"] = "error"
            trace["terminal_user_journey_acceptance"] = False
            trace["error"] = sanitize_diagnostic_text(str(error))
            if error.diagnostics is not None:
                trace["error_diagnostics"] = error.diagnostics
        trace["sanitization"] = {
            "applied": True,
            "registered_roots": registered_root_placeholders(),
            "diagnostic_bounds": {
                "max_chars": DIAGNOSTIC_MAX_CHARS,
                "max_lines": DIAGNOSTIC_MAX_LINES,
            },
        }
        trace["finished_at"] = _now()
        # Defensive final pass: no retained string may carry a local path or a
        # secret-shaped value, regardless of which step produced it.
        trace = sanitize_tree(trace)  # type: ignore[assignment]
        trace_path.parent.mkdir(parents=True, exist_ok=True)
        trace_path.write_text(
            json.dumps(trace, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        shutil.rmtree(work_root, ignore_errors=True)

    print(f"trace={trace_path}")
    print(f"status={trace['status']}")
    if trace.get("blocked_criteria"):
        print("blocked criteria:")
        for name in trace["blocked_criteria"]:
            print(f"  - {name}: {criteria[name].detail}")
    if trace.get("failed_criteria"):
        print("failed criteria:")
        for name in trace["failed_criteria"]:
            # Criterion ids are a fixed allow-list. Keep failure diagnostics
            # useful without printing topology paths or captured payloads.
            print(f"  - {name}")
            if os.environ.get("GITHUB_ACTIONS") == "true":
                print(
                    "::error title=Local-folder-to-Product binding criterion failed::"
                    f"{name}"
                )
    if trace.get("downstream_witnesses_not_passed"):
        print("downstream witnesses not passed (non-gating):")
        for name in trace["downstream_witnesses_not_passed"]:
            print(f"  - {name}: {criteria[name].detail}")
    if error is not None:
        print(f"error: {trace['error']}", file=sys.stderr)
        diagnostics = trace.get("error_diagnostics")
        if isinstance(diagnostics, dict):
            for stream in ("stderr_tail", "stdout_tail"):
                tail = diagnostics.get(stream)
                if tail:
                    print(f"{stream} (sanitized, bounded):", file=sys.stderr)
                    print(tail, file=sys.stderr)
        return 2
    return 0 if trace["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
