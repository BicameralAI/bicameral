#!/usr/bin/env python3
"""Run the disposable real-process hosted topology for cloud issue #135."""

from __future__ import annotations

import argparse
import base64
import contextlib
import hashlib
import json
import os
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import IO, NoReturn

from bicameral_cloud.contracts import HOSTED_BRIDGE_PROTOCOL_VERSION

SUPABASE_CLI_VERSION = "2.39.2"
TRUSTED_BRIDGE_ORIGIN = "https://app.bicameral-ai.com"
OWNER_DOMAIN = "bicameral-ai.com"
PRODUCT_JOIN_ISSUER = "https://app.bicameral-ai.com"
PRODUCT_JOIN_KEY_ID = "topology-join-key"
TERMINAL_ACCEPTANCE_SURFACE = (
    'data-testid="create-product"',
    'data-testid="create-project-name"',
    'data-testid="create-project-submit"',
    'data-testid="registration-retry-needed"',
    'data-testid="join-action"',
    'data-testid="join-retry"',
    'data-testid="daemon-disconnect-scrim"',
    "Local daemon disconnected",
    "Go to pairing",
    "bicameral_hosted_product_create_v1:",
    "/api/v1/hosted-bridge/command",
    "/api/v1/hosted-bridge/product-join",
    "/api/v1/hosted-bridge/projects",
    "/api/v1/products",
)
FORBIDDEN_RECEIPT_TOKENS = (
    "service_role_key",
    "jwt_secret",
    "refresh_token",
    "access_token",
    "password",
    "session_token",
    "source_body",
    "source_content",
    "event_store",
    ".bicameral/",
    "/home/",
    "local_path",
    "filesystem_path",
    "workspace_path",
)
GENERATED_DASHBOARD_METADATA = frozenset({"version.json", "release-identity.json"})


class TopologyError(RuntimeError):
    """A sanitized orchestration failure."""


@dataclass
class ManagedProcess:
    name: str
    process: subprocess.Popen[str]
    log: IO[str]


@dataclass(frozen=True)
class HttpResult:
    status: int
    body: bytes
    headers: dict[str, str]

    def json(self) -> object:
        return json.loads(self.body)


class RedactedNetworkCapture:
    """Retain browser response metadata without headers, bodies, or queries."""

    def __init__(self) -> None:
        self.responses: list[dict[str, object]] = []

    def attach(self, context: object) -> None:
        context.on("response", self.record_response)  # type: ignore[attr-defined]

    def record_response(self, response: object) -> None:
        if len(self.responses) >= 2_000:
            return
        parsed = urllib.parse.urlsplit(response.url)  # type: ignore[attr-defined]
        if parsed.scheme not in {"http", "https"}:
            return
        request = response.request  # type: ignore[attr-defined]
        self.responses.append(
            {
                "method": request.method,
                "origin": f"{parsed.scheme}://{parsed.netloc}",
                "path": parsed.path or "/",
                "resource_type": request.resource_type,
                "status": response.status,  # type: ignore[attr-defined]
            }
        )

    def write(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "redaction": (
                        "method, origin, path, resource type, and status only"
                    ),
                    "responses": self.responses,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )


def _redact_trace_archive(
    source: Path,
    destination: Path,
    secret_values: Sequence[str],
    redacted_paths: Sequence[Path] = (),
) -> None:
    """Copy a Playwright trace while removing every known ephemeral secret."""

    destination.parent.mkdir(parents=True, exist_ok=True)
    with (
        zipfile.ZipFile(source, "r") as input_archive,
        zipfile.ZipFile(
            destination, "w", compression=zipfile.ZIP_DEFLATED
        ) as output_archive,
    ):
        for entry in input_archive.infolist():
            content = input_archive.read(entry.filename)
            for value in secret_values:
                if value:
                    content = content.replace(value.encode(), b"<redacted>")
            for path in redacted_paths:
                content = content.replace(str(path).encode(), b"<redacted-path>")
            content = re.sub(
                rb"(?i)(authorization[^\r\n]{0,80}bearer\s+)[A-Za-z0-9._~+/=-]+",
                rb"\1<redacted>",
                content,
            )
            for value in secret_values:
                if value and value.encode() in content:
                    raise TopologyError(
                        "sanitized Playwright trace retained a credential"
                    )
            for path in redacted_paths:
                if str(path).encode() in content:
                    raise TopologyError("sanitized Playwright trace retained a path")
            output_archive.writestr(entry, content)


def _record_browser_handoff_secrets(
    page: object,
    secret_values: list[str],
) -> None:
    handoff = page.evaluate(  # type: ignore[attr-defined]
        "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
    )
    if not isinstance(handoff, str) or not handoff:
        return
    try:
        payload = json.loads(handoff)
    except json.JSONDecodeError:
        return

    def collect(value: object, key: str = "") -> None:
        if isinstance(value, dict):
            for child_key, child_value in value.items():
                collect(child_value, str(child_key))
        elif isinstance(value, list):
            for child in value:
                collect(child, key)
        elif isinstance(value, str) and "token" in key.lower() and value:
            secret_values.append(value)

    collect(payload)


def _terminal_acceptance_state(
    blocking_contract: object | None,
) -> dict[str, object]:
    if blocking_contract:
        return {
            "evidence_level": "real-process integration; supporting evidence only",
            "terminal_evidence_complete": False,
            "human_product_review": "not_ready",
            "terminal_user_journey_acceptance": False,
        }
    return {
        "evidence_level": "browser lifecycle acceptance candidate",
        "terminal_evidence_complete": True,
        "human_product_review": "pending",
        "terminal_user_journey_acceptance": False,
    }


def _now() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _run_id() -> str:
    return datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ") + "-" + secrets.token_hex(4)


def _run(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = 120,
    input_text: str | None = None,
    label: str,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(command),
            cwd=cwd,
            env=env,
            input=input_text,
            text=True,
            capture_output=True,
            check=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise TopologyError(f"{label} timed out") from exc
    except subprocess.CalledProcessError as exc:
        raise TopologyError(f"{label} failed with exit code {exc.returncode}") from exc


def _allocate_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _allocate_ports(names: Sequence[str]) -> dict[str, int]:
    ports: dict[str, int] = {}
    while len(ports) < len(names):
        port = _allocate_port()
        if port not in ports.values():
            ports[names[len(ports)]] = port
    return ports


def _sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _sha256_file(path: Path) -> str:
    return _sha256_bytes(path.read_bytes())


def _tree_digest(root: Path, *, exclude: frozenset[str] = frozenset()) -> str:
    hasher = hashlib.sha256()
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        relative = path.relative_to(root).as_posix()
        if relative in exclude:
            continue
        hasher.update(relative.encode())
        hasher.update(b"\0")
        hasher.update(path.read_bytes())
    return hasher.hexdigest()


def _http(
    method: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
    payload: object | None = None,
    timeout: float = 10.0,
    verify_tls: bool = True,
) -> HttpResult:
    body = None
    request_headers = dict(headers or {})
    if payload is not None:
        body = json.dumps(payload).encode()
        request_headers.setdefault("content-type", "application/json")
    request = urllib.request.Request(
        url,
        data=body,
        headers=request_headers,
        method=method,
    )
    try:
        context = None
        if url.startswith("https://") and not verify_tls:
            context = ssl._create_unverified_context()  # noqa: S323 # nosec B323 # opt-in only via verify_tls=False for local/self-signed journey targets; TLS verified by default
        with urllib.request.urlopen(  # nosec B310 # internal process-harness request; scheme is not attacker-controlled
            request, timeout=timeout, context=context
        ) as response:
            return HttpResult(
                status=response.status,
                body=response.read(),
                headers={key.lower(): value for key, value in response.headers.items()},
            )
    except urllib.error.HTTPError as exc:
        return HttpResult(
            status=exc.code,
            body=exc.read(),
            headers={key.lower(): value for key, value in exc.headers.items()},
        )
    except urllib.error.URLError as exc:
        raise TopologyError(
            "HTTP request could not reach a required loopback process"
        ) from exc


def _expect_status(result: HttpResult, expected: Sequence[int], label: str) -> None:
    if result.status not in expected:
        raise TopologyError(f"{label} returned HTTP {result.status}")


def _wait_http(url: str, *, timeout: float, label: str) -> HttpResult:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = _http("GET", url, timeout=2)
        except TopologyError:
            time.sleep(0.4)
            continue
        if 200 <= result.status < 300:
            return result
        time.sleep(0.4)
    raise TopologyError(f"{label} did not become healthy")


def _wait_port_closed(port: int, *, timeout: float = 10.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.25)
            if sock.connect_ex(("127.0.0.1", port)) != 0:
                return True
        time.sleep(0.2)
    return False


def _start_process(
    name: str,
    command: Sequence[str],
    *,
    cwd: Path,
    env: dict[str, str],
    log_dir: Path,
) -> ManagedProcess:
    log = (log_dir / f"{name}.log").open("w", encoding="utf-8")
    process = subprocess.Popen(
        list(command),
        cwd=cwd,
        env=env,
        text=True,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    return ManagedProcess(name=name, process=process, log=log)


def _stop_process(managed: ManagedProcess) -> None:
    if managed.process.poll() is None:
        managed.process.terminate()
        try:
            managed.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            managed.process.kill()
            managed.process.wait(timeout=10)
    managed.log.close()


def _clone_or_validate(
    *,
    supplied: Path | None,
    destination: Path,
    repository: str,
    ref: str,
    label: str,
) -> Path:
    if supplied is not None:
        checkout = supplied.resolve()
        if not (checkout / ".git").is_dir():
            raise TopologyError(f"{label} checkout is not a Git repository")
        status = _run(
            ["git", "status", "--porcelain"],
            cwd=checkout,
            label=f"inspect {label} checkout",
        ).stdout
        if status.strip():
            raise TopologyError(f"{label} checkout contains uncommitted changes")
        return checkout
    _run(
        [
            "git",
            "clone",
            "--depth",
            "1",
            "--branch",
            ref,
            f"https://github.com/{repository}.git",
            str(destination),
        ],
        timeout=300,
        label=f"clone {label}",
    )
    return destination


def _git_sha(repository: Path) -> str:
    value = _run(
        ["git", "rev-parse", "HEAD"],
        cwd=repository,
        label="read component commit",
    ).stdout.strip()
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise TopologyError("component commit was not a full SHA")
    return value


def _git_commit(repository: Path, message: str, paths: Sequence[str]) -> str:
    _run(["git", "add", "--", *paths], cwd=repository, label="stage topology state")
    _run(
        [
            "git",
            "-c",
            "user.name=Bicameral Topology",
            "-c",
            "user.email=topology@invalid.example",
            "commit",
            "-m",
            message,
        ],
        cwd=repository,
        label="commit topology state",
    )
    return _git_sha(repository)


def _create_workspace(path: Path, name: str) -> None:
    path.mkdir(parents=True)
    _run(["git", "init", "-b", "main"], cwd=path, label=f"initialize {name} repository")
    (path / "README.md").write_text(f"# {name}\n", encoding="utf-8")
    _git_commit(path, f"Initialize {name}", ["README.md"])


def _configure_workspace(
    *,
    binary: Path,
    workspace: Path,
    home: Path,
    port: int,
    name: str,
) -> str:
    home.mkdir(parents=True)
    env = {**os.environ, "HOME": str(home)}
    _run(
        [str(binary), "init", str(workspace)],
        cwd=workspace,
        env=env,
        timeout=60,
        label=f"initialize {name}",
    )
    config_path = workspace / ".bicameral" / "config.yaml"
    config = config_path.read_text(encoding="utf-8")
    config = re.sub(
        r"^workspace_name:.*$",
        f"workspace_name: {name}",
        config,
        flags=re.M,
    )
    config = re.sub(r"^  port: \d+$", f"  port: {port}", config, flags=re.M)
    if f"port: {port}" not in config:
        raise TopologyError(f"{name} gateway port was not configured")
    config_path.write_text(config, encoding="utf-8")
    return _sha256_file(config_path)


def _set_product_join_trust(
    config_path: Path,
    *,
    product_id: str,
    signing: dict[str, str],
) -> str:
    """Add the exact daemon-local trust anchor after Product identity exists."""

    config = config_path.read_text(encoding="utf-8")
    if "\nproduct_join:" in config:
        raise TopologyError("daemon Product join trust was already configured")
    config += (
        "\nproduct_join:\n"
        f"  issuer: {signing['issuer']}\n"
        f"  audience: daemon:{product_id}\n"
        "  verifying_keys:\n"
        f"    - key_id: {signing['key_id']}\n"
        f"      public_key_b64: {signing['public_key_b64']}\n"
    )
    config_path.write_text(config, encoding="utf-8")
    return _sha256_file(config_path)


def _sync_product_events_via_git(
    *, creator_workspace: Path, peer_workspace: Path, remote: Path
) -> str:
    """Push/fetch canonical Product events through a disposable Git remote."""

    _git_commit(
        creator_workspace,
        "Commit canonical hosted Product state",
        [".bicameral/events"],
    )
    _run(
        ["git", "init", "--bare", str(remote)],
        timeout=60,
        label="initialize disposable canonical Product remote",
    )
    _run(
        ["git", "remote", "add", "product-origin", str(remote)],
        cwd=creator_workspace,
        label="configure creator Product remote",
    )
    _run(
        ["git", "push", "product-origin", "main"],
        cwd=creator_workspace,
        timeout=60,
        label="push canonical Product events",
    )
    _run(
        ["git", "remote", "add", "product-origin", str(remote)],
        cwd=peer_workspace,
        label="configure peer Product remote",
    )
    _run(
        ["git", "fetch", "product-origin", "main"],
        cwd=peer_workspace,
        timeout=60,
        label="fetch canonical Product events",
    )
    _run(
        [
            "git",
            "checkout",
            "product-origin/main",
            "--",
            ".bicameral/events",
        ],
        cwd=peer_workspace,
        label="materialize fetched canonical Product events",
    )
    return _git_commit(
        peer_workspace,
        "Materialize canonical Product state from Git",
        [".bicameral/events"],
    )


def _start_daemon(
    *,
    binary: Path,
    workspace: Path,
    home: Path,
    name: str,
    port: int,
    log_dir: Path,
) -> ManagedProcess:
    env = {**os.environ, "HOME": str(home), "RUST_LOG": "info"}
    managed = _start_process(
        name,
        [str(binary), "--workspace", str(workspace), "gateway", "start"],
        cwd=workspace,
        env=env,
        log_dir=log_dir,
    )
    try:
        _wait_http(
            f"http://127.0.0.1:{port}/health",
            timeout=30,
            label=name,
        )
    except TopologyError:
        _stop_process(managed)
        raise
    return managed


def _write_supabase_config(root: Path, ports: dict[str, int], project_id: str) -> None:
    supabase = root / "supabase"
    migrations = supabase / "migrations"
    migrations.mkdir(parents=True)
    config = f"""project_id = "{project_id}"

[api]
enabled = true
port = {ports["supabase_api"]}
schemas = ["public", "graphql_public"]
extra_search_path = ["public", "extensions"]
max_rows = 1000

[api.tls]
enabled = false

[db]
port = {ports["supabase_db"]}
shadow_port = {ports["supabase_shadow"]}
major_version = 17

[db.pooler]
enabled = false
port = {ports["supabase_pooler"]}
pool_mode = "transaction"
default_pool_size = 20
max_client_conn = 100

[db.migrations]
enabled = true
schema_paths = []

[db.seed]
enabled = false
sql_paths = []

[realtime]
enabled = false

[studio]
enabled = false
port = {ports["supabase_studio"]}
api_url = "http://127.0.0.1"

[inbucket]
enabled = false
port = {ports["supabase_mail"]}

[storage]
enabled = false
file_size_limit = "50MiB"

[auth]
enabled = true
site_url = "http://127.0.0.1:{ports["cloud"]}"
additional_redirect_urls = []
jwt_expiry = 3600
enable_refresh_token_rotation = true
refresh_token_reuse_interval = 10
enable_signup = true
enable_anonymous_sign_ins = false
enable_manual_linking = false
minimum_password_length = 8
password_requirements = ""

[auth.email]
enable_signup = true
double_confirm_changes = true
enable_confirmations = false
secure_password_change = false
max_frequency = "1s"
otp_length = 6
otp_expiry = 3600

[auth.sms]
enable_signup = false
enable_confirmations = false
template = "Your code is {{{{ .Code }}}}"
max_frequency = "5s"

[edge_runtime]
enabled = false
policy = "oneshot"
inspector_port = {ports["supabase_inspector"]}
deno_version = 2

[analytics]
enabled = false
port = {ports["supabase_analytics"]}
backend = "postgres"
"""
    (supabase / "config.toml").write_text(config, encoding="utf-8")


def _supabase_command(*arguments: str) -> list[str]:
    return ["npx", "--yes", f"supabase@{SUPABASE_CLI_VERSION}", *arguments]


def _status_value(status: dict[str, object], *names: str) -> str:
    for name in names:
        value = status.get(name)
        if isinstance(value, str) and value:
            return value
    raise TopologyError("Supabase status omitted a required runtime value")


def _start_supabase(
    *,
    root: Path,
    cloud_root: Path,
    ports: dict[str, int],
    project_id: str,
) -> dict[str, str]:
    _write_supabase_config(root, ports, project_id)
    source_migrations = cloud_root / "supabase" / "migrations"
    destination_migrations = root / "supabase" / "migrations"
    for migration in sorted(source_migrations.glob("*.sql")):
        shutil.copy2(migration, destination_migrations / migration.name)
    started = False
    try:
        _run(
            _supabase_command(
                "start",
                "--workdir",
                str(root),
                "-x",
                (
                    "realtime,storage-api,imgproxy,studio,postgres-meta,mailpit,"
                    "edge-runtime,logflare,vector,supavisor"
                ),
            ),
            timeout=900,
            label="start disposable Supabase",
        )
        started = True
        _run(
            _supabase_command("db", "reset", "--workdir", str(root)),
            timeout=600,
            label="apply Cloud migrations",
        )
        raw_status = _run(
            _supabase_command("status", "--workdir", str(root), "-o", "json"),
            timeout=60,
            label="read Supabase status",
        ).stdout
        status_object = json.loads(raw_status)
        if not isinstance(status_object, dict):
            raise TopologyError("Supabase status was not a JSON object")
        status = {str(key): value for key, value in status_object.items()}
        return {
            "api_url": _status_value(status, "API_URL", "api_url"),
            "db_url": _status_value(status, "DB_URL", "db_url"),
            "anon_key": _status_value(status, "ANON_KEY", "anon_key"),
            "service_key": _status_value(
                status,
                "SERVICE_ROLE_KEY",
                "service_role_key",
            ),
            "jwt_secret": _status_value(status, "JWT_SECRET", "jwt_secret"),
        }
    except json.JSONDecodeError as exc:
        if started:
            _stop_supabase(root)
        raise TopologyError("Supabase status was not valid JSON") from exc
    except TopologyError:
        if started:
            _stop_supabase(root)
        raise


def _stop_supabase(root: Path) -> bool:
    try:
        _run(
            _supabase_command("stop", "--workdir", str(root), "--no-backup"),
            timeout=300,
            label="stop disposable Supabase",
        )
        return True
    except TopologyError:
        return False


def _supabase_db_container(db_port: int) -> str:
    result = _run(
        [
            "docker",
            "ps",
            "--filter",
            f"publish={db_port}",
            "--format",
            "{{.Names}}",
        ],
        label="locate disposable Postgres",
    ).stdout.splitlines()
    names = [name.strip() for name in result if name.strip()]
    if len(names) != 1:
        raise TopologyError("disposable Postgres container was not uniquely identified")
    return names[0]


def _supabase_container_names(db_port: int) -> list[str]:
    database = _supabase_db_container(db_port)
    project = _run(
        [
            "docker",
            "inspect",
            database,
            "--format",
            '{{index .Config.Labels "com.supabase.cli.project"}}',
        ],
        label="identify disposable Supabase project",
    ).stdout.strip()
    if not project:
        raise TopologyError(
            "disposable Supabase containers omitted their project label"
        )
    names = _run(
        [
            "docker",
            "ps",
            "--filter",
            f"label=com.supabase.cli.project={project}",
            "--format",
            "{{.Names}}",
        ],
        label="list disposable Supabase containers",
    ).stdout.splitlines()
    return sorted(name.strip() for name in names if name.strip())


def _migration_versions(db_port: int) -> list[str]:
    container = _supabase_db_container(db_port)
    result = _run(
        [
            "docker",
            "exec",
            container,
            "psql",
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-Atc",
            (
                "select version from supabase_migrations.schema_migrations "
                "order by version"
            ),
        ],
        label="query applied migrations",
    )
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def _postgres_json(db_port: int, sql: str, *, label: str) -> object:
    container = _supabase_db_container(db_port)
    result = _run(
        [
            "docker",
            "exec",
            container,
            "psql",
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-Atc",
            sql,
        ],
        label=label,
    )
    try:
        return json.loads(result.stdout.strip() or "null")
    except json.JSONDecodeError as exc:
        raise TopologyError(f"{label} returned malformed JSON") from exc


def _migration_rls_evidence(
    db_port: int,
    *,
    applied_versions: Sequence[str],
    tracked_versions: Sequence[str],
) -> dict[str, object]:
    tables = _postgres_json(
        db_port,
        """
        select coalesce(json_agg(row_to_json(e) order by e.table_name), '[]'::json)
        from (
          select c.relname as table_name,
                 c.relrowsecurity as rls_enabled,
                 c.relforcerowsecurity as rls_forced
          from pg_class c
          join pg_namespace n on n.oid = c.relnamespace
          where n.nspname = 'public' and c.relkind = 'r'
        ) e
        """,
        label="query live RLS table state",
    )
    policies = _postgres_json(
        db_port,
        """
        select coalesce(
          json_agg(row_to_json(e) order by e.tablename, e.policyname),
          '[]'::json
        )
        from (
          select tablename, policyname, roles, cmd, permissive
          from pg_policies
          where schemaname = 'public'
        ) e
        """,
        label="query live RLS policies",
    )
    direct_grants = _postgres_json(
        db_port,
        """
        select coalesce(
          json_agg(
            row_to_json(e) order by e.table_name, e.grantee, e.privilege_type
          ),
          '[]'::json
        )
        from (
          select table_name, grantee, privilege_type
          from information_schema.role_table_grants
          where table_schema = 'public'
            and grantee in ('anon', 'authenticated')
        ) e
        """,
        label="query live public API grants",
    )
    return {
        "schema_version": 1,
        "migrations": {
            "tracked_versions": list(tracked_versions),
            "applied_versions": list(applied_versions),
            "exact_match": list(applied_versions) == list(tracked_versions),
        },
        "rls_tables": tables,
        "rls_policies": policies,
        "anon_authenticated_direct_grants": direct_grants,
    }


def _supabase_headers(key: str, token: str | None = None) -> dict[str, str]:
    return {
        "apikey": key,
        "authorization": f"Bearer {token or key}",
        "accept": "application/json",
    }


def _create_identity(
    api_url: str,
    service_key: str,
    *,
    email: str,
    password: str,
) -> str:
    result = _http(
        "POST",
        f"{api_url}/auth/v1/admin/users",
        headers=_supabase_headers(service_key),
        payload={
            "email": email,
            "password": password,
            "email_confirm": True,
            "user_metadata": {"full_name": "Topology Identity"},
        },
    )
    _expect_status(result, [200, 201], "create controlled identity")
    payload = result.json()
    if not isinstance(payload, dict) or not isinstance(payload.get("id"), str):
        raise TopologyError("controlled identity response omitted its id")
    return payload["id"]


def _sign_in(api_url: str, anon_key: str, *, email: str, password: str) -> str:
    result = _http(
        "POST",
        f"{api_url}/auth/v1/token?grant_type=password",
        headers=_supabase_headers(anon_key),
        payload={"email": email, "password": password},
    )
    _expect_status(result, [200], "sign in controlled identity")
    payload = result.json()
    if not isinstance(payload, dict) or not isinstance(
        payload.get("access_token"), str
    ):
        raise TopologyError("controlled sign-in response omitted its access token")
    return payload["access_token"]


def _validate_database_behavior(
    *,
    api_url: str,
    anon_key: str,
    service_key: str,
    owner_token: str,
) -> dict[str, object]:
    product = {
        "canonical_product_id": "topology-product",
        "display_name": "Topology Product",
        "eligible_domain": OWNER_DOMAIN,
        "registration_ref": "topology-registration",
        "provider_hint": "github",
        "substrate_hint": "git",
        "enabled": True,
    }
    service_insert = _http(
        "POST",
        f"{api_url}/rest/v1/product_directory",
        headers={
            **_supabase_headers(service_key),
            "content-type": "application/json",
            "prefer": "return=representation",
        },
        payload=product,
    )
    _expect_status(service_insert, [200, 201], "service-role directory insert")
    service_select = _http(
        "GET",
        (
            f"{api_url}/rest/v1/product_directory?"
            + urllib.parse.urlencode(
                {
                    "select": "canonical_product_id,eligible_domain,enabled",
                    "canonical_product_id": "eq.topology-product",
                }
            )
        ),
        headers=_supabase_headers(service_key),
    )
    _expect_status(service_select, [200], "service-role directory select")
    selected = service_select.json()
    if not isinstance(selected, list) or len(selected) != 1:
        raise TopologyError("service-role directory row was not observable")

    anon_select = _http(
        "GET",
        f"{api_url}/rest/v1/product_directory?select=canonical_product_id",
        headers=_supabase_headers(anon_key),
    )
    authenticated_select = _http(
        "GET",
        f"{api_url}/rest/v1/product_directory?select=canonical_product_id",
        headers=_supabase_headers(anon_key, owner_token),
    )
    if anon_select.status not in (401, 403):
        raise TopologyError("anonymous Product directory access did not fail closed")
    if authenticated_select.status not in (401, 403):
        raise TopologyError(
            "authenticated Product directory access did not fail closed"
        )

    allowlist = _http(
        "GET",
        (
            f"{api_url}/rest/v1/design_partner_allowlist?"
            + urllib.parse.urlencode(
                {"select": "domain,enabled", "domain": f"eq.{OWNER_DOMAIN}"}
            )
        ),
        headers=_supabase_headers(service_key),
    )
    _expect_status(allowlist, [200], "service-role allowlist select")
    rows = allowlist.json()
    if not isinstance(rows, list) or len(rows) != 1:
        raise TopologyError("seeded design-partner domain was not observable")
    return {
        "service_role_insert": True,
        "service_role_select": True,
        "anonymous_denied": True,
        "authenticated_denied": True,
        "allowlist_seed_observed": True,
    }


def _ensure_python_environment(
    *,
    work_root: Path,
    cloud_root: Path,
    mcp_root: Path,
    include_journey: bool = False,
) -> Path:
    venv = work_root / "python-env"
    cloud_requirement = str(cloud_root)
    if include_journey:
        cloud_requirement += "[journey]"
    _run(
        ["uv", "venv", "--python", "3.11", str(venv)],
        timeout=300,
        label="create Python 3.11 environment",
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
            cloud_requirement,
            "-e",
            str(mcp_root),
        ],
        timeout=600,
        label="install Cloud and MCP runtime",
    )
    return python


def _expose_python_environment(python: Path) -> None:
    """Expose the disposable 3.11 packages to this 3.11 orchestrator."""

    result = _run(
        [
            str(python),
            "-c",
            "import site; print(site.getsitepackages()[0])",
        ],
        timeout=30,
        label="locate disposable Python packages",
    )
    site_packages = result.stdout.strip()
    if not site_packages:
        raise TopologyError("disposable Python environment omitted site-packages")
    sys.path.insert(0, site_packages)


def _generate_tls_material(work_root: Path) -> tuple[Path, Path]:
    """Create a disposable certificate for the exact production hosted origin."""

    key = work_root / "hosted-origin.key"
    certificate = work_root / "hosted-origin.crt"
    _run(
        [
            "openssl",
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=app.bicameral-ai.com",
            "-addext",
            "subjectAltName=DNS:app.bicameral-ai.com",
            "-keyout",
            str(key),
            "-out",
            str(certificate),
        ],
        timeout=60,
        label="create disposable hosted-origin certificate",
    )
    return key, certificate


def _generate_product_join_signing_material(work_root: Path) -> dict[str, str]:
    """Generate an ephemeral Cloud signer and daemon verification key."""

    private_pem = work_root / "product-join-private.pem"
    private_der = work_root / "product-join-private.der"
    public_der = work_root / "product-join-public.der"
    _run(
        [
            "openssl",
            "genpkey",
            "-algorithm",
            "ED25519",
            "-out",
            str(private_pem),
        ],
        timeout=60,
        label="generate disposable Product join key",
    )
    _run(
        [
            "openssl",
            "pkey",
            "-in",
            str(private_pem),
            "-outform",
            "DER",
            "-out",
            str(private_der),
        ],
        timeout=60,
        label="encode disposable Product join private key",
    )
    _run(
        [
            "openssl",
            "pkey",
            "-in",
            str(private_pem),
            "-pubout",
            "-outform",
            "DER",
            "-out",
            str(public_der),
        ],
        timeout=60,
        label="encode disposable Product join public key",
    )
    private_raw = private_der.read_bytes()[-32:]
    public_raw = public_der.read_bytes()[-32:]
    if len(private_raw) != 32 or len(public_raw) != 32:
        raise TopologyError("OpenSSL did not produce raw Ed25519 key material")
    return {
        "issuer": PRODUCT_JOIN_ISSUER,
        "key_id": PRODUCT_JOIN_KEY_ID,
        "private_key_b64": base64.b64encode(private_raw).decode(),
        "public_key_b64": base64.b64encode(public_raw).decode(),
    }


def _build_bot(bot_root: Path) -> Path:
    _run(
        ["npm", "ci", "--silent"],
        cwd=bot_root / "dashboard",
        timeout=600,
        label="install dashboard dependencies",
    )
    _run(
        ["npm", "run", "build"],
        cwd=bot_root / "dashboard",
        timeout=600,
        label="build dashboard SPA",
    )
    _run(
        ["cargo", "build", "--release", "--locked", "--bin", "bicameral"],
        cwd=bot_root,
        timeout=1800,
        label="build production bot binary",
    )
    binary = bot_root / "target" / "release" / "bicameral"
    if not binary.is_file():
        raise TopologyError("production bot binary was not built")
    return binary


def _prepare_cloud_runtime(
    *,
    cloud_root: Path,
    bot_root: Path,
    runtime_root: Path,
    bot_commit: str,
    validate_terminal_surface: bool = False,
    use_vendored_dashboard: bool = False,
) -> dict[str, str]:
    source_package = cloud_root / "src" / "bicameral_cloud"
    runtime_package = runtime_root / "bicameral_cloud"
    shutil.copytree(source_package, runtime_package)

    dashboard_source = bot_root / "dashboard" / "dist"
    dashboard_destination = runtime_package / "static" / "dashboard"
    built_bundle_digest = _tree_digest(
        dashboard_source,
        exclude=GENERATED_DASHBOARD_METADATA,
    )
    if use_vendored_dashboard:
        metadata_path = dashboard_destination / "version.json"
        try:
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise TopologyError("vendored Cloud dashboard provenance is malformed") from exc
        if not isinstance(metadata, dict):
            raise TopologyError("vendored Cloud dashboard provenance is malformed")
        if metadata.get("bot_commit") != bot_commit:
            raise TopologyError(
                "vendored Cloud dashboard does not select the exact Bot commit"
            )
        vendored_digest = _tree_digest(
            dashboard_destination,
            exclude=GENERATED_DASHBOARD_METADATA,
        )
        if metadata.get("bundle_sha256") != vendored_digest:
            raise TopologyError("vendored Cloud dashboard digest does not verify")
        if vendored_digest != built_bundle_digest:
            raise TopologyError(
                "vendored Cloud dashboard differs from the exact Bot build"
            )
    else:
        if dashboard_destination.exists():
            shutil.rmtree(dashboard_destination)
        shutil.copytree(dashboard_source, dashboard_destination)
    if validate_terminal_surface:
        _validate_terminal_acceptance_surface(dashboard_destination)

    contract_source = bot_root / "protocol" / "hosted-bridge" / "contract.json"
    recorded_source = bot_root / "protocol" / "hosted-bridge" / "contract.sha256"
    if not contract_source.is_file() or not recorded_source.is_file():
        raise TopologyError("Bot hosted-bridge contract manifest is missing")
    contract_digest = _sha256_file(contract_source)
    recorded_digest = recorded_source.read_text(encoding="utf-8").split()[0]
    if contract_digest != recorded_digest:
        raise TopologyError("Bot hosted-bridge contract fingerprint did not verify")

    contracts_destination = runtime_package / "contracts"
    contracts_destination.mkdir(exist_ok=True)
    if use_vendored_dashboard:
        vendored_contract = contracts_destination / "hosted_bridge_contract.json"
        if _sha256_file(vendored_contract) != contract_digest:
            raise TopologyError(
                "vendored Cloud bridge contract differs from the exact Bot contract"
            )
    else:
        shutil.copy2(
            contract_source,
            contracts_destination / "hosted_bridge_contract.json",
        )
        (contracts_destination / "hosted_bridge_contract.sha256").write_text(
            f"{contract_digest}  hosted_bridge_contract.json\n",
            encoding="utf-8",
        )

    bundle_digest = _tree_digest(
        dashboard_destination,
        exclude=GENERATED_DASHBOARD_METADATA,
    )
    metadata = {
        "source_repository": "BicameralAI/bicameral-bot",
        "bot_commit": bot_commit,
        "bundle_sha256": bundle_digest,
        "hosted_bridge_contract_sha256": contract_digest,
        "source_run_url": f"https://github.com/BicameralAI/bicameral-bot/commit/{bot_commit}",
        "built_at": _now(),
    }
    if not use_vendored_dashboard:
        (dashboard_destination / "version.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    return {
        "bundle_sha256": bundle_digest,
        "contract_sha256": contract_digest,
    }


def _validate_terminal_acceptance_surface(dashboard: Path) -> None:
    """Fail when the built SPA drifts from cloud#137's browser contract."""

    scripts = sorted((dashboard / "assets").glob("*.js"))
    content = "\n".join(
        script.read_text(encoding="utf-8", errors="replace") for script in scripts
    )
    missing = [item for item in TERMINAL_ACCEPTANCE_SURFACE if item not in content]
    if missing:
        raise TopologyError(
            "built dashboard drifted from the terminal acceptance surface: "
            + ", ".join(missing)
        )


def _start_cloud(
    *,
    python: Path,
    runtime_root: Path,
    cloud_root: Path,
    port: int,
    supabase: dict[str, str],
    log_dir: Path,
    daemon_port: int | None = None,
    product_join_signing: dict[str, str] | None = None,
    tls_key: Path | None = None,
    tls_certificate: Path | None = None,
) -> ManagedProcess:
    env = {
        **os.environ,
        "PYTHONPATH": os.pathsep.join([str(runtime_root), str(cloud_root / "src")]),
        "SUPABASE_URL": supabase["api_url"],
        "SUPABASE_JWT_SECRET": supabase["jwt_secret"],
        "SUPABASE_ANON_KEY": supabase["anon_key"],
        "SUPABASE_SERVICE_KEY": supabase["service_key"],
    }
    if daemon_port is not None:
        env["BICAMERAL_DAEMON_PORT"] = str(daemon_port)
    if product_join_signing is not None:
        env.update(
            {
                "PRODUCT_JOIN_TICKET_ISSUER": product_join_signing["issuer"],
                "PRODUCT_JOIN_SIGNING_KEY_ID": product_join_signing["key_id"],
                "PRODUCT_JOIN_SIGNING_PRIVATE_KEY_B64": product_join_signing[
                    "private_key_b64"
                ],
                "PRODUCT_JOIN_TICKET_TTL_SECONDS": "300",
            }
        )
    command = [
        str(python),
        "-m",
        "uvicorn",
        "bicameral_cloud.app:app",
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
        "--no-access-log",
    ]
    if tls_key is not None and tls_certificate is not None:
        command.extend(
            [
                "--ssl-keyfile",
                str(tls_key),
                "--ssl-certfile",
                str(tls_certificate),
            ]
        )
    managed = _start_process(
        "cloud",
        command,
        cwd=cloud_root,
        env=env,
        log_dir=log_dir,
    )
    try:
        if tls_key is None:
            _wait_http(
                f"http://127.0.0.1:{port}/health",
                timeout=30,
                label="Cloud",
            )
        else:
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
                    if sock.connect_ex(("127.0.0.1", port)) == 0:
                        break
                time.sleep(0.1)
            else:
                raise TopologyError("Cloud HTTPS listener did not become ready")
    except TopologyError:
        _stop_process(managed)
        raise
    return managed


def _validate_cloud(
    *,
    cloud_port: int,
    owner_token: str,
    expected_dashboard: dict[str, str],
    base_url: str | None = None,
    verify_tls: bool = True,
) -> tuple[dict[str, object], list[bytes]]:
    base_url = base_url or f"http://127.0.0.1:{cloud_port}"
    request = lambda method, url, **kwargs: _http(  # noqa: E731
        method, url, verify_tls=verify_tls, **kwargs
    )
    health = request("GET", f"{base_url}/health")
    _expect_status(health, [200], "Cloud health")
    app = request("GET", f"{base_url}/app")
    _expect_status(app, [200], "Cloud dashboard SPA")
    if b"<html" not in app.body.lower() and b"<!doctype html" not in app.body.lower():
        raise TopologyError("Cloud dashboard route did not serve the built SPA")
    version = request("GET", f"{base_url}/app/version.json")
    _expect_status(version, [200], "Cloud dashboard provenance")
    version_payload = version.json()
    if not isinstance(version_payload, dict):
        raise TopologyError("Cloud dashboard provenance was malformed")
    for key in ("bundle_sha256", "hosted_bridge_contract_sha256"):
        if (
            version_payload.get(key)
            != expected_dashboard[
                "bundle_sha256" if key == "bundle_sha256" else "contract_sha256"
            ]
        ):
            raise TopologyError(
                "Cloud dashboard provenance did not match the built SPA"
            )

    projects = request(
        "GET",
        f"{base_url}/api/v1/projects",
        headers={"authorization": f"Bearer {owner_token}"},
    )
    _expect_status(projects, [200], "same-domain Cloud discovery")
    project_payload = projects.json()
    if not isinstance(project_payload, dict):
        raise TopologyError("Cloud discovery returned a malformed payload")
    project_rows = project_payload.get("projects")
    if not isinstance(project_rows, list) or not any(
        isinstance(row, dict) and row.get("project_id") == "topology-product"
        for row in project_rows
    ):
        raise TopologyError("Cloud did not read the Supabase-backed Product directory")

    return (
        {
            "health": True,
            "dashboard_spa": True,
            "dashboard_provenance": True,
            "supabase_directory_selected": True,
            "same_domain_discovery": True,
        },
        [health.body, app.body, version.body, projects.body],
    )


def _daemon_capabilities(port: int) -> dict[str, object]:
    result = _http("GET", f"http://127.0.0.1:{port}/v2/capabilities")
    _expect_status(result, [200], "daemon capabilities")
    payload = result.json()
    if not isinstance(payload, dict):
        raise TopologyError("daemon capabilities were malformed")
    return {str(key): value for key, value in payload.items()}


def _start_daemon_pairing(port: int, cloud_account_id: str) -> tuple[str, str]:
    pending = _http(
        "POST",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/pair",
        headers={"origin": TRUSTED_BRIDGE_ORIGIN},
        payload={
            "origin": TRUSTED_BRIDGE_ORIGIN,
            "cloud_account_id": cloud_account_id,
            "csrf_token": secrets.token_urlsafe(24),
            "client_protocol_version": HOSTED_BRIDGE_PROTOCOL_VERSION,
        },
    )
    _expect_status(pending, [202], "daemon hosted-bridge pairing request")
    pending_payload = pending.json()
    if (
        not isinstance(pending_payload, dict)
        or pending_payload.get("status") != "pending"
    ):
        raise TopologyError("daemon did not create a pending bridge request")
    request_handle = pending_payload.get("request_handle")
    poll_proof = pending_payload.get("poll_proof")
    if not isinstance(request_handle, str) or not request_handle:
        raise TopologyError("daemon pairing response omitted its request handle")
    if not isinstance(poll_proof, str) or not poll_proof:
        raise TopologyError("daemon pairing response omitted its poll proof")
    return request_handle, poll_proof


def _approve_daemon_pairing(port: int, request_handle: str) -> None:
    local_view = _http(
        "GET",
        (
            f"http://127.0.0.1:{port}/api/v1/hosted-bridge/pair/pending/"
            f"{urllib.parse.quote(request_handle, safe='')}"
        ),
    )
    _expect_status(local_view, [200], "daemon local pairing confirmation view")
    local_payload = local_view.json()
    local_nonce = (
        local_payload.get("local_confirmation_nonce")
        if isinstance(local_payload, dict)
        else None
    )
    if not isinstance(local_nonce, str) or not local_nonce:
        raise TopologyError("daemon local pairing view omitted its confirmation nonce")

    decision = _http(
        "POST",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/pair/decision",
        payload={
            "request_handle": request_handle,
            "action": "approve",
            "local_confirmation_nonce": local_nonce,
        },
    )
    _expect_status(decision, [200], "daemon local pairing approval")
    decision_payload = decision.json()
    if (
        not isinstance(decision_payload, dict)
        or decision_payload.get("status") != "approved"
    ):
        raise TopologyError("daemon did not approve the local pairing request")


def _pair_daemon(port: int, cloud_account_id: str) -> tuple[str, bytes]:
    request_handle, poll_proof = _start_daemon_pairing(port, cloud_account_id)
    _approve_daemon_pairing(port, request_handle)

    query = urllib.parse.urlencode(
        {
            "request_handle": request_handle,
            "poll_proof": poll_proof,
            "cloud_account_id": cloud_account_id,
        }
    )
    status = _http(
        "GET",
        f"http://127.0.0.1:{port}/api/v1/hosted-bridge/pair/status?{query}",
        headers={"origin": TRUSTED_BRIDGE_ORIGIN},
    )
    _expect_status(status, [200], "daemon hosted-bridge pairing status")
    status_payload = status.json()
    if (
        not isinstance(status_payload, dict)
        or status_payload.get("status") != "approved"
    ):
        raise TopologyError("daemon pairing status did not report local approval")
    token = status_payload.get("session_token")
    if not isinstance(token, str) or not token:
        raise TopologyError("daemon pairing status omitted its session")
    return token, status.body


def _write_mcp_probe(path: Path) -> None:
    path.write_text(
        """from __future__ import annotations

import asyncio
import json
import os
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from mcp.types import TextContent


async def run() -> None:
    executable, tool_name, raw_arguments = sys.argv[1:4]
    server = StdioServerParameters(
        command=executable,
        args=["serve"],
        env=dict(os.environ),
    )
    async with stdio_client(server) as (read_stream, write_stream):
        async with ClientSession(read_stream, write_stream) as session:
            await session.initialize()
            tools = await session.list_tools()
            result = await session.call_tool(tool_name, json.loads(raw_arguments))
            content = [
                item.text
                for item in result.content
                if isinstance(item, TextContent)
            ]
            print(json.dumps({
                "tool_names": sorted(tool.name for tool in tools.tools),
                "is_error": bool(result.isError),
                "content": content,
            }))


asyncio.run(run())
""",
        encoding="utf-8",
    )


def _mcp_call(
    *,
    python: Path,
    probe: Path,
    daemon_port: int,
    workspace: Path,
    tool_name: str,
    arguments: dict[str, object],
) -> dict[str, object]:
    executable = python.parent / "bicameral-mcp"
    env = {
        **os.environ,
        "BICAMERAL_DAEMON_URL": f"http://127.0.0.1:{daemon_port}",
        "BICAMERAL_ACTOR_ID": "topology-mcp-client",
        "BICAMERAL_WORKSPACE": str(workspace),
        "BICAMERAL_POLICY_SCOPE": "default",
    }
    result = _run(
        [
            str(python),
            str(probe),
            str(executable),
            tool_name,
            json.dumps(arguments),
        ],
        env=env,
        timeout=90,
        label=f"MCP {tool_name}",
    )
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise TopologyError("MCP client process returned malformed output") from exc
    if not isinstance(payload, dict) or payload.get("is_error") is not False:
        raise TopologyError(f"MCP tool {tool_name} failed")
    names = payload.get("tool_names")
    if not isinstance(names, list) or tool_name not in names:
        raise TopologyError("MCP server did not advertise the invoked tool")
    return {str(key): value for key, value in payload.items()}


def _mcp_content(payload: dict[str, object]) -> str:
    content = payload.get("content")
    if not isinstance(content, list):
        raise TopologyError("MCP response content was malformed")
    return "\n".join(item for item in content if isinstance(item, str))


def _event_state(workspace: Path) -> tuple[int, str]:
    events = workspace / ".bicameral" / "events"
    files = sorted(events.glob("*.yaml"))
    return len(files), _tree_digest(events)


def _event_type_count(workspace: Path, event_type: str) -> int:
    """Count durable Git events by their serialized snake-case type."""

    marker = f"event_type: {event_type}"
    return sum(
        marker in path.read_text(encoding="utf-8", errors="replace")
        for path in (workspace / ".bicameral" / "events").glob("*.yaml")
    )


def _supabase_session_stub(access_token: str) -> str:
    """Return the narrow SDK surface the production dashboard consumes.

    The token is not fabricated: it was issued by the disposable Supabase Auth
    process after a password sign-in. The stub only avoids a third-party CDN in
    an otherwise local acceptance topology.
    """

    token = json.dumps(access_token)
    return f"""
window.supabase = {{
  createClient: function () {{
    return {{ auth: {{
      getSession: async function () {{
        return {{ data: {{ session: {{ access_token: {token} }} }}, error: null }};
      }},
      signOut: async function () {{ return {{ error: null }}; }}
    }} }};
  }}
}};
"""


def _wait_for_selector(
    page: object,
    selector: str,
    *,
    timeout: int,
    label: str,
    state: str | None = None,
) -> None:
    options: dict[str, object] = {"timeout": timeout}
    if state is not None:
        options["state"] = state
    try:
        page.wait_for_selector(selector, **options)  # type: ignore[attr-defined]
    except Exception as exc:
        raise TopologyError(f"{label} timed out waiting for {selector}") from exc


def _open_paired_browser_page(
    context: object,
    *,
    access_token: str,
    hosted_origin: str,
    daemon_port: int,
    setup_page: object | None = None,
    secret_values: list[str] | None = None,
) -> object:
    """Drive the production /dashboard authentication and pairing handoff."""

    page = context.new_page()  # type: ignore[attr-defined]
    stub = _supabase_session_stub(access_token)
    daemon_origin = f"http://localhost:{daemon_port}"
    page.add_init_script(  # type: ignore[attr-defined]
        "window.localStorage.setItem('bicameral_daemon_url', "
        f"{json.dumps(daemon_origin)});"
    )

    def bridge_transport(route: object) -> None:
        """Preserve the exact hosted Origin through a high-port TLS topology.

        A developer machine cannot bind privileged port 443. The request still
        reaches the real daemon and its real CORS/PNA middleware with the exact
        production Origin; only the browser-facing ACAO value is translated
        back to the disposable high-port origin.
        """

        headers = dict(route.request.headers)  # type: ignore[attr-defined]
        headers["origin"] = TRUSTED_BRIDGE_ORIGIN
        response = route.fetch(headers=headers)  # type: ignore[attr-defined]
        response_headers = dict(response.headers)
        response_headers["access-control-allow-origin"] = hosted_origin
        route.fulfill(  # type: ignore[attr-defined]
            response=response, headers=response_headers
        )

    page.route(  # type: ignore[attr-defined]
        f"{daemon_origin}/api/v1/hosted-bridge/**", bridge_transport
    )
    page.route(  # type: ignore[attr-defined]
        "https://cdn.jsdelivr.net/npm/@supabase/supabase-js*",
        lambda route: route.fulfill(
            status=200,
            content_type="application/javascript; charset=utf-8",
            body=stub,
        ),
    )
    if setup_page is not None:
        setup_page(page)  # type: ignore[operator]
    # The current Cloud dashboard begins the pending-pairing request as soon as
    # its authenticated session is ready. Capture that production request
    # during navigation; there is no separate legacy #pair-btn click.
    with page.expect_response(  # type: ignore[attr-defined]
        lambda response: (
            response.request.method == "POST"
            and response.url.endswith("/api/v1/hosted-bridge/pair")
        )
    ) as pending_response:
        page.goto(  # type: ignore[attr-defined]
            f"{hosted_origin}/dashboard", wait_until="networkidle"
        )
    pending = pending_response.value
    if pending.status != 202:
        raise TopologyError(
            f"browser hosted-bridge pairing returned HTTP {pending.status}"
        )
    pending_payload = pending.json()
    request_handle = (
        pending_payload.get("request_handle")
        if isinstance(pending_payload, dict)
        else None
    )
    if not isinstance(request_handle, str) or not request_handle:
        raise TopologyError("browser pairing response omitted its request handle")
    _approve_daemon_pairing(daemon_port, request_handle)
    page.wait_for_url(  # type: ignore[attr-defined]
        f"{hosted_origin}/app", timeout=20_000
    )
    _wait_for_selector(
        page,
        '[data-testid="paired-shell"]',
        state="visible",
        timeout=20_000,
        label="hosted SPA paired shell",
    )
    if secret_values is not None:
        _record_browser_handoff_secrets(page, secret_values)
    return page


def _browser_directory_item(
    page: object, access_token: str, display_name: str
) -> dict[str, object]:
    payload = page.evaluate(  # type: ignore[attr-defined]
        """async ({token}) => {
          const response = await fetch('/api/v1/projects', {
            headers: {Authorization: 'Bearer ' + token}
          });
          return {status: response.status, body: await response.json()};
        }""",
        {"token": access_token},
    )
    if not isinstance(payload, dict) or payload.get("status") != 200:
        raise TopologyError("browser could not read authenticated Product discovery")
    body = payload.get("body")
    rows = body.get("projects") if isinstance(body, dict) else None
    if not isinstance(rows, list):
        raise TopologyError("browser Product discovery response was malformed")
    matches = [
        row
        for row in rows
        if isinstance(row, dict) and row.get("display_name") == display_name
    ]
    if len(matches) != 1:
        raise TopologyError("browser did not observe exactly one created Product")
    return {str(key): value for key, value in matches[0].items()}


def _run_terminal_user_journey(
    *,
    artifact_dir: Path,
    work_root: Path,
    owner_token: str,
    peer_token: str,
    ports: dict[str, int],
    workspaces: list[Path],
    homes: list[Path],
    bot_binary: Path,
    log_dir: Path,
    signing: dict[str, str],
    hosted_origin: str,
    daemon_a: ManagedProcess,
    daemon_b: ManagedProcess,
    processes: list[ManagedProcess],
    secret_values: list[str],
) -> tuple[dict[str, object], ManagedProcess]:
    """Run the terminal browser boundary and return evidence plus daemon B.

    Extends cloud#137's Create/Join recovery with cloud#155's local-daemon
    disconnect recovery against bot#808: while a paired Product screen stays
    rendered, the paired daemon is terminated, a real daemon-bound request then
    fails at the network layer, and the SPA must raise exactly one blocking
    "Local daemon disconnected" dialog over the preserved, inert prior screen
    before "Go to pairing" clears the stale hosted handoff and loads the
    canonical Cloud /dashboard pairing surface.
    """

    from playwright.sync_api import Error as PlaywrightError
    from playwright.sync_api import sync_playwright

    artifact_dir.mkdir(parents=True, exist_ok=True)
    display_name = f"Alpha Product {_run_id()}"
    screenshots = 0
    network_capture = RedactedNetworkCapture()

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(
            headless=True,
            args=[
                "--host-resolver-rules=MAP app.bicameral-ai.com 127.0.0.1",
                "--no-proxy-server",
            ],
        )
        try:
            owner_context = browser.new_context(
                ignore_https_errors=True,
                viewport={"width": 1280, "height": 800},
                record_video_dir=str(artifact_dir / "video-owner"),
            )
            network_capture.attach(owner_context)
            owner_context.tracing.start(screenshots=True, snapshots=True, sources=True)
            first_create_lost = {"observed": False, "status": None}

            def lose_first_create(page: object) -> None:
                def handle(route: object) -> None:
                    request = route.request  # type: ignore[attr-defined]
                    try:
                        body = request.post_data_json
                    except Exception:
                        body = None
                    name = (
                        body.get("command", {}).get("name")
                        if isinstance(body, dict)
                        and isinstance(body.get("command"), dict)
                        else None
                    )
                    if request.method == "POST" and name == "product.create":
                        headers = dict(request.headers)
                        headers["origin"] = TRUSTED_BRIDGE_ORIGIN
                        response = route.fetch(  # type: ignore[attr-defined]
                            headers=headers
                        )
                        first_create_lost["status"] = response.status
                        first_create_lost["observed"] = True
                        route.abort("failed")  # type: ignore[attr-defined]
                        return
                    route.continue_()  # type: ignore[attr-defined]

                page.route(  # type: ignore[attr-defined]
                    f"http://localhost:{ports['daemon_a']}"
                    "/api/v1/hosted-bridge/command",
                    handle,
                )

            owner_page = _open_paired_browser_page(
                owner_context,
                access_token=owner_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_a"],
                setup_page=lose_first_create,
                secret_values=secret_values,
            )
            owner_page.locator('[data-testid="create-product"]').click()
            owner_page.locator('[data-testid="create-project-name"]').fill(display_name)
            owner_page.locator('[data-testid="create-project-submit"]').click()
            _wait_for_selector(
                owner_page,
                '[data-testid="registration-retry-needed"]',
                timeout=30_000,
                label="lost create-response recovery",
            )
            if not first_create_lost["observed"]:
                raise TopologyError("create-response loss fault did not execute")
            if first_create_lost["status"] != 200:
                raise TopologyError("real daemon product.create did not commit")
            if _event_type_count(workspaces[0], "create_product") != 1:
                raise TopologyError(
                    "lost create response did not leave one canonical Product"
                )
            owner_page.screenshot(
                path=str(artifact_dir / "01-create-response-lost.png"), full_page=True
            )
            screenshots += 1
            owner_page.close()

            cloud_response_lost = {"observed": False, "status": None}

            def lose_first_cloud_publication(page: object) -> None:
                def handle(route: object) -> None:
                    if route.request.method == "POST":  # type: ignore[attr-defined]
                        request = route.request  # type: ignore[attr-defined]
                        request_body = request.post_data_json
                        authorization = request.headers.get("authorization")
                        if not isinstance(request_body, dict) or not authorization:
                            raise TopologyError(
                                "Cloud publication fault omitted its real request"
                            )
                        response = _http(
                            "POST",
                            f"https://127.0.0.1:{ports['cloud']}/api/v1/products",
                            headers={
                                "authorization": authorization,
                                "content-type": "application/json",
                            },
                            payload=request_body,
                            verify_tls=False,
                        )
                        cloud_response_lost["status"] = response.status
                        cloud_response_lost["observed"] = True
                        route.abort("failed")  # type: ignore[attr-defined]
                        return
                    route.continue_()  # type: ignore[attr-defined]

                page.route(  # type: ignore[attr-defined]
                    f"{hosted_origin}/api/v1/products", handle
                )

            owner_page = _open_paired_browser_page(
                owner_context,
                access_token=owner_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_a"],
                setup_page=lose_first_cloud_publication,
                secret_values=secret_values,
            )
            _wait_for_selector(
                owner_page,
                '[data-testid="registration-retry-needed"]',
                timeout=30_000,
                label="lost Cloud-publication response recovery",
            )
            if not cloud_response_lost["observed"]:
                raise TopologyError("Cloud-response loss fault did not execute")
            if cloud_response_lost["status"] not in (200, 201):
                raise TopologyError("real Cloud Product publication failed")
            owner_page.screenshot(
                path=str(artifact_dir / "02-cloud-response-lost.png"), full_page=True
            )
            screenshots += 1
            owner_page.close()

            owner_page = _open_paired_browser_page(
                owner_context,
                access_token=owner_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_a"],
                secret_values=secret_values,
            )
            row = owner_page.locator('[data-testid="project-row"]').filter(
                has_text=display_name
            )
            row.first.wait_for(state="visible", timeout=30_000)
            directory_item = _browser_directory_item(
                owner_page, owner_token, display_name
            )
            product_id = directory_item.get("project_id")
            if not isinstance(product_id, str) or not product_id:
                raise TopologyError("created Product omitted its canonical identity")
            if _event_type_count(workspaces[0], "create_product") != 1:
                raise TopologyError(
                    "Create recovery duplicated canonical Product creation"
                )
            owner_page.screenshot(
                path=str(artifact_dir / "03-owner-created-product.png"), full_page=True
            )
            screenshots += 1
            owner_page.close()
            owner_trace = work_root / "trace-owner.raw.zip"
            owner_context.tracing.stop(path=str(owner_trace))
            _redact_trace_archive(
                owner_trace,
                artifact_dir / "trace-owner.zip",
                secret_values,
                [work_root, Path(__file__).resolve().parents[1]],
            )
            owner_context.close()

            _stop_process(daemon_b)
            processes.remove(daemon_b)
            if not _wait_port_closed(ports["daemon_b"]):
                raise TopologyError("peer daemon port did not close for trust setup")
            _sync_product_events_via_git(
                creator_workspace=workspaces[0],
                peer_workspace=workspaces[1],
                remote=work_root / "canonical-product.git",
            )
            _set_product_join_trust(
                workspaces[1] / ".bicameral" / "config.yaml",
                product_id=product_id,
                signing=signing,
            )
            _git_commit(
                workspaces[1],
                "Configure disposable Product join trust",
                [".bicameral/config.yaml"],
            )
            daemon_b = _start_daemon(
                binary=bot_binary,
                workspace=workspaces[1],
                home=homes[1],
                name="daemon-b-join-ready",
                port=ports["daemon_b"],
                log_dir=log_dir,
            )
            processes.append(daemon_b)

            peer_context = browser.new_context(
                ignore_https_errors=True,
                viewport={"width": 1280, "height": 800},
                record_video_dir=str(artifact_dir / "video-peer"),
            )
            network_capture.attach(peer_context)
            peer_context.tracing.start(screenshots=True, snapshots=True, sources=True)
            join_response_lost = {
                "observed": False,
                "status": None,
                "tool_status": None,
                "outcome": None,
                "failure_code": None,
            }

            def lose_first_join(page: object) -> None:
                def handle(route: object) -> None:
                    if route.request.method == "POST":  # type: ignore[attr-defined]
                        headers = dict(route.request.headers)  # type: ignore[attr-defined]
                        headers["origin"] = TRUSTED_BRIDGE_ORIGIN
                        response = route.fetch(  # type: ignore[attr-defined]
                            headers=headers
                        )
                        join_response_lost["status"] = response.status
                        payload = response.json()
                        result = payload.get("result") or {}
                        failure = result.get("failure") or {}
                        tool_status = payload.get("status")
                        outcome = result.get("outcome")
                        failure_code = failure.get("code")
                        join_response_lost["tool_status"] = (
                            tool_status
                            if tool_status in {"ok", "rejected", "error"}
                            else "unexpected"
                        )
                        join_response_lost["outcome"] = (
                            outcome
                            if outcome in {"granted", "rejected"}
                            else "unexpected"
                        )
                        join_response_lost["failure_code"] = (
                            failure_code
                            if failure_code
                            in {
                                "expired_ticket",
                                "invalid_signature",
                                "missing_pairing",
                                "not_yet_valid",
                                "policy_changed",
                                "replayed_ticket",
                                "wrong_account",
                                "wrong_audience",
                                "wrong_domain",
                                "wrong_product",
                            }
                            else "none"
                        )
                        join_response_lost["observed"] = True
                        route.abort("failed")  # type: ignore[attr-defined]
                        return
                    route.continue_()  # type: ignore[attr-defined]

                page.route(  # type: ignore[attr-defined]
                    f"http://localhost:{ports['daemon_b']}"
                    "/api/v1/hosted-bridge/product-join",
                    handle,
                )

            peer_page = _open_paired_browser_page(
                peer_context,
                access_token=peer_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_b"],
                setup_page=lose_first_join,
                secret_values=secret_values,
            )
            available_row = peer_page.locator(
                '[data-testid="project-row"][data-membership="available"]'
            ).filter(has_text=display_name)
            available_row.first.wait_for(state="visible", timeout=30_000)
            available_row.locator('[data-testid="join-action"]').click()

            # The daemon commits the membership, then the browser loses the
            # response. Per product authority (bot#808), any lost daemon
            # response is a transport disconnect: the SPA must raise the single
            # blocking disconnect overlay and route recovery to pairing rather
            # than an inline in-place retry.
            join_disconnect_dialog = peer_page.get_by_role(
                "dialog", name="Local daemon disconnected"
            )
            join_disconnect_dialog.wait_for(state="visible", timeout=30_000)
            join_dialogs = peer_page.get_by_role("dialog").count()
            if join_dialogs != 1:
                raise TopologyError(
                    "expected exactly one dialog on lost join response, saw "
                    f"{join_dialogs}"
                )
            peer_page.get_by_role("button", name="Go to pairing").wait_for(
                state="visible", timeout=10_000
            )
            if not join_response_lost["observed"]:
                raise TopologyError("join-response loss fault did not execute")
            if join_response_lost["status"] != 200:
                raise TopologyError(
                    "real daemon product.join failed with HTTP "
                    f"{join_response_lost['status']}"
                )
            if (
                join_response_lost["tool_status"] != "ok"
                or join_response_lost["outcome"] != "granted"
            ):
                raise TopologyError(
                    "real daemon product.join did not grant membership: "
                    f"status={join_response_lost['tool_status']}, "
                    f"outcome={join_response_lost['outcome']}, "
                    f"failure={join_response_lost['failure_code']}"
                )
            if _event_type_count(workspaces[1], "product_member_added") != 1:
                raise TopologyError(
                    "lost join response did not materialize one membership"
                )
            join_scrim_visible = peer_page.locator(
                '[data-testid="daemon-disconnect-scrim"]'
            ).is_visible()
            join_prior_visible = peer_page.locator(
                '[data-testid="hosted-dashboard"]'
            ).is_visible()
            join_app_shell = peer_page.locator(".app-shell")
            if not (join_scrim_visible and join_prior_visible):
                raise TopologyError(
                    "prior Product screen was not preserved beneath the join "
                    "disconnect scrim"
                )
            if not (
                join_app_shell.get_attribute("inert") is not None
                and join_app_shell.get_attribute("aria-hidden") == "true"
            ):
                raise TopologyError(
                    "app background was not inert/aria-hidden under the join "
                    "disconnect dialog"
                )
            if not peer_page.evaluate(
                "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
            ):
                raise TopologyError(
                    "hosted handoff was cleared before join-recovery pairing"
                )
            peer_page.screenshot(
                path=str(artifact_dir / "04-join-response-lost-disconnect.png"),
                full_page=True,
            )
            screenshots += 1

            peer_page.unroute(
                f"http://localhost:{ports['daemon_b']}"
                "/api/v1/hosted-bridge/product-join"
            )
            peer_page.get_by_role("button", name="Go to pairing").click()
            peer_page.wait_for_url(f"{hosted_origin}/dashboard", timeout=20_000)
            _wait_for_selector(
                peer_page,
                "#status-card",
                state="visible",
                timeout=20_000,
                label="join-recovery dashboard pairing surface",
            )
            if (
                peer_page.evaluate(
                    "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
                )
                is not None
            ):
                raise TopologyError(
                    "stale hosted handoff survived join-recovery pairing"
                )
            peer_page.screenshot(
                path=str(artifact_dir / "05-join-recovery-pairing.png"),
                full_page=True,
            )
            screenshots += 1
            peer_page.close()

            # Re-pair and reissue Join. The daemon already committed the
            # membership during the lost-response attempt, so recovery plus any
            # reissue must remain idempotent: exactly one canonical membership,
            # and the peer renders as a member.
            reloaded = _open_paired_browser_page(
                peer_context,
                access_token=peer_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_b"],
                secret_values=secret_values,
            )
            reloaded_row = reloaded.locator('[data-testid="project-row"]').filter(
                has_text=display_name
            )
            reloaded_row.first.wait_for(state="visible", timeout=30_000)
            reissue_available = reloaded.locator(
                '[data-testid="project-row"][data-membership="available"]'
            ).filter(has_text=display_name)
            if reissue_available.count() == 1:
                reissue_available.locator('[data-testid="join-action"]').click()
                reloaded.wait_for_timeout(1500)
            if _event_type_count(workspaces[1], "product_member_added") != 1:
                raise TopologyError(
                    "join recovery/reissue duplicated canonical Product membership"
                )
            member_rows = reloaded.locator(
                '[data-testid="project-row"][data-membership="member"]'
            ).filter(has_text=display_name)
            member_rows.first.wait_for(state="visible", timeout=30_000)
            member_after_reload = member_rows.count() == 1
            reloaded.screenshot(
                path=str(artifact_dir / "06-peer-after-recovery.png"), full_page=True
            )
            screenshots += 1
            reloaded.close()
            peer_trace = work_root / "trace-peer.raw.zip"
            peer_context.tracing.stop(path=str(peer_trace))
            _redact_trace_archive(
                peer_trace,
                artifact_dir / "trace-peer.zip",
                secret_values,
                [work_root, Path(__file__).resolve().parents[1]],
            )
            peer_context.close()

            # ── Local daemon disconnect recovery (cloud#155 / bot#808) ──────
            # Re-pair the owner against the still-live daemon A, keep the paired
            # Product screen rendered, then terminate that daemon and let a real
            # daemon-bound request fail at the network layer.
            disconnect_context = browser.new_context(
                ignore_https_errors=True,
                viewport={"width": 1280, "height": 800},
                record_video_dir=str(artifact_dir / "video-disconnect"),
            )
            network_capture.attach(disconnect_context)
            disconnect_context.tracing.start(
                screenshots=True, snapshots=True, sources=True
            )
            disconnect_page = _open_paired_browser_page(
                disconnect_context,
                access_token=owner_token,
                hosted_origin=hosted_origin,
                daemon_port=ports["daemon_a"],
                secret_values=secret_values,
            )
            disconnect_row = disconnect_page.locator(
                '[data-testid="project-row"]'
            ).filter(has_text=display_name)
            disconnect_row.first.wait_for(state="visible", timeout=30_000)
            if not disconnect_page.locator(
                '[data-testid="hosted-dashboard"]'
            ).is_visible():
                raise TopologyError(
                    "paired hosted Product screen was not rendered before disconnect"
                )
            handoff_before = disconnect_page.evaluate(
                "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
            )
            if not handoff_before:
                raise TopologyError(
                    "paired screen did not retain the hosted bridge handoff"
                )
            disconnect_page.screenshot(
                path=str(artifact_dir / "07-paired-before-disconnect.png"),
                full_page=True,
            )
            screenshots += 1

            _stop_process(daemon_a)
            processes.remove(daemon_a)
            if not _wait_port_closed(ports["daemon_a"]):
                raise TopologyError(
                    "paired daemon port did not close for the disconnect scenario"
                )
            # Drop the bridge proxy so the browser connects directly to the now
            # closed loopback port: a genuine transport loss, not a stubbed one.
            disconnect_page.unroute(
                f"http://localhost:{ports['daemon_a']}/api/v1/hosted-bridge/**"
            )
            disconnect_page.reload(wait_until="domcontentloaded")

            disconnect_dialog = disconnect_page.get_by_role(
                "dialog", name="Local daemon disconnected"
            )
            disconnect_dialog.wait_for(state="visible", timeout=30_000)
            visible_dialogs = disconnect_page.get_by_role("dialog").count()
            if visible_dialogs != 1:
                raise TopologyError(
                    "expected exactly one daemon-disconnect dialog, saw "
                    f"{visible_dialogs}"
                )
            pairing_action = disconnect_page.get_by_role("button", name="Go to pairing")
            pairing_action.wait_for(state="visible", timeout=10_000)

            scrim_visible = disconnect_page.locator(
                '[data-testid="daemon-disconnect-scrim"]'
            ).is_visible()
            prior_screen_visible = disconnect_page.locator(
                '[data-testid="hosted-dashboard"]'
            ).is_visible()
            app_shell = disconnect_page.locator(".app-shell")
            background_inert = app_shell.get_attribute("inert") is not None
            background_aria_hidden = app_shell.get_attribute("aria-hidden") == "true"
            if not (scrim_visible and prior_screen_visible):
                raise TopologyError(
                    "prior paired screen was not preserved beneath the scrim"
                )
            if not (background_inert and background_aria_hidden):
                raise TopologyError(
                    "app background was not inert/aria-hidden under the dialog"
                )
            handoff_during = disconnect_page.evaluate(
                "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
            )
            if not handoff_during:
                raise TopologyError(
                    "hosted handoff was cleared before pairing was requested"
                )
            disconnect_page.screenshot(
                path=str(artifact_dir / "08-daemon-disconnected-modal.png"),
                full_page=True,
            )
            screenshots += 1

            pairing_action.click()
            disconnect_page.wait_for_url(f"{hosted_origin}/dashboard", timeout=20_000)
            _wait_for_selector(
                disconnect_page,
                "#status-card",
                state="visible",
                timeout=20_000,
                label="disconnect-recovery dashboard pairing surface",
            )
            handoff_after = disconnect_page.evaluate(
                "() => window.sessionStorage.getItem('bicameral_hosted_bridge')"
            )
            if handoff_after is not None:
                raise TopologyError(
                    "stale hosted handoff survived the pairing recovery"
                )
            disconnect_page.screenshot(
                path=str(artifact_dir / "09-recovered-pairing-surface.png"),
                full_page=True,
            )
            screenshots += 1
            disconnect_page.close()
            disconnect_trace = work_root / "trace-disconnect.raw.zip"
            disconnect_context.tracing.stop(path=str(disconnect_trace))
            _redact_trace_archive(
                disconnect_trace,
                artifact_dir / "trace-disconnect.zip",
                secret_values,
                [work_root, Path(__file__).resolve().parents[1]],
            )
            disconnect_context.close()
            network_capture.write(artifact_dir / "network-redacted.json")

            result: dict[str, object] = {
                "exact_hosted_origin_reaches_daemon": True,
                "high_port_tls_origin_translation": True,
                "real_supabase_issued_sessions": True,
                "dashboard_authentication_and_pairing": True,
                "daemon_canonical_create_count": 1,
                "create_response_loss_recovered": True,
                "cloud_response_loss_recovered": True,
                "durable_directory_product_observed": True,
                "daemon_member_event_count": 1,
                "join_response_loss_recovered_idempotently": True,
                "member_projection_after_reload": member_after_reload,
                "paired_screen_rendered_before_disconnect": True,
                "real_daemon_bound_request_failed_after_termination": True,
                "single_local_daemon_disconnected_modal": True,
                "prior_screen_preserved_beneath_scrim": True,
                "background_inert_and_aria_hidden": True,
                "hosted_handoff_retained_until_pairing_action": True,
                "go_to_pairing_cleared_stale_hosted_bridge": True,
                "canonical_dashboard_pairing_surface_reloaded": True,
                "screenshots": screenshots,
                "playwright_traces": 3,
                "redacted_network_capture": True,
                "raw_credentials_recorded": False,
            }
            if not member_after_reload:
                result["blocking_contract"] = "hosted_membership_projection"
                result["blocking_authority"] = (
                    "Factory must assign the daemon-to-Cloud/SPA membership "
                    "projection boundary"
                )
            return result, daemon_b
        except PlaywrightError as exc:
            summary = str(exc).splitlines()[0] or type(exc).__name__
            raise TopologyError(
                f"terminal browser automation failed: {summary}"
            ) from exc
        finally:
            with contextlib.suppress(PlaywrightError):
                browser.close()


def _scan_payloads(payloads: Sequence[bytes], secret_values: Sequence[str]) -> None:
    combined = b"\n".join(payloads)
    for value in secret_values:
        if value and value.encode() in combined:
            raise TopologyError(
                "browser-safe payload contained ephemeral credential material"
            )
    text = combined.decode(errors="ignore")
    for token in (
        "/home/",
        "workspace_path",
        "filesystem_path",
        "source_body",
        "source_content",
        "provider_secret",
    ):
        if token in text:
            raise TopologyError(
                "browser-safe payload contained a forbidden local field"
            )


def _validate_process_log_content(
    content: str,
    secret_values: Sequence[str],
    work_root: Path,
) -> None:
    for value in secret_values:
        if value and value in content:
            raise TopologyError("process log contained ephemeral credential material")
    if str(work_root) in content:
        raise TopologyError("process log contained the disposable local root")


def _scan_logs(log_dir: Path, secret_values: Sequence[str], work_root: Path) -> None:
    for path in log_dir.glob("*.log"):
        _validate_process_log_content(
            path.read_text(encoding="utf-8", errors="ignore"),
            secret_values,
            work_root,
        )


def _retain_process_logs(
    log_dir: Path,
    destination: Path,
    *,
    secret_values: Sequence[str],
    work_root: Path,
) -> list[str]:
    retained: list[str] = []
    destination.mkdir(parents=True, exist_ok=True)
    for source in sorted(log_dir.glob("*.log")):
        content = source.read_text(encoding="utf-8", errors="ignore")
        _validate_process_log_content(content, secret_values, work_root)
        (destination / source.name).write_text(content, encoding="utf-8")
        retained.append(source.name)
    return retained


def _assert_receipt_safe(receipt: dict[str, object]) -> None:
    serialized = json.dumps(receipt, sort_keys=True).lower()
    for token in FORBIDDEN_RECEIPT_TOKENS:
        if token in serialized:
            raise TopologyError(f"receipt contained forbidden token: {token}")


def _write_receipt(path: Path, receipt: dict[str, object]) -> None:
    _assert_receipt_safe(receipt)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _write_redacted_evidence(
    path: Path,
    payload: object,
    *,
    secret_values: Sequence[str],
    work_root: Path,
) -> None:
    serialized = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    for value in secret_values:
        if value and value in serialized:
            raise TopologyError("retained evidence contained an ephemeral credential")
    if str(work_root) in serialized:
        raise TopologyError("retained evidence contained the disposable local root")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(serialized, encoding="utf-8")


def _receipt_log_label(receipt_path: Path, cloud_root: Path) -> Path:
    """Return a repo-relative receipt label without leaking a host path."""
    resolved_receipt = (
        receipt_path if receipt_path.is_absolute() else cloud_root / receipt_path
    ).resolve()
    try:
        return resolved_receipt.relative_to(cloud_root.resolve())
    except ValueError:
        return Path(receipt_path.name)


def _containers_remaining(names: Sequence[str]) -> int:
    if not names:
        return 0
    try:
        result = _run(
            [
                "docker",
                "ps",
                "-a",
                "--format",
                "{{.Names}}",
            ],
            label="verify disposable containers removed",
        )
    except TopologyError:
        return -1
    remaining = {line.strip() for line in result.stdout.splitlines() if line.strip()}
    return len(remaining.intersection(names))


def _fail(message: str) -> NoReturn:
    print(f"real-process topology failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Boot, validate, restart/replay, and tear down the real hosted topology."
        )
    )
    parser.add_argument("--bot-ref", default="main")
    parser.add_argument("--mcp-ref", default="main")
    parser.add_argument("--bot-repo", type=Path)
    parser.add_argument("--mcp-repo", type=Path)
    parser.add_argument("--factory-commit")
    parser.add_argument("--receipt", type=Path)
    parser.add_argument(
        "--terminal-user-journey",
        action="store_true",
        help=(
            "also drive cloud#137 Create/Join recovery in a real browser from "
            "the exact hosted origin"
        ),
    )
    parser.add_argument(
        "--use-vendored-dashboard",
        action="store_true",
        help="exercise Cloud's committed SPA bytes and require them to equal the exact Bot build",
    )
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    if args.terminal_user_journey and sys.version_info[:2] != (3, 11):
        raise TopologyError("terminal user journey must be launched with Python 3.11")
    cloud_root = Path(__file__).resolve().parents[1]
    run_id = _run_id()
    work_root = Path.home() / ".cache" / "bicameral-cloud" / "topology-runs" / run_id
    receipt_path = args.receipt or (
        cloud_root
        / "test-artifacts"
        / "real-process-hosted-topology"
        / run_id
        / "receipt.json"
    )
    work_root.parent.mkdir(parents=True, exist_ok=True)
    work_root.mkdir()
    log_dir = work_root / "logs"
    log_dir.mkdir()
    processes: list[ManagedProcess] = []
    supabase_started = False
    supabase_containers: list[str] = []
    ports = _allocate_ports(
        (
            "cloud",
            "daemon_a",
            "daemon_b",
            "supabase_api",
            "supabase_db",
            "supabase_shadow",
            "supabase_pooler",
            "supabase_studio",
            "supabase_mail",
            "supabase_inspector",
            "supabase_analytics",
        )
    )
    project_id = f"bicameral-topology-{run_id.lower()}"
    supabase_root = work_root / project_id
    started_at = _now()
    receipt: dict[str, object] = {
        "schema_version": 1,
        "run_id": run_id,
        "evidence_level": "real-process integration; supporting evidence only",
        "terminal_user_journey_acceptance": False,
        "started_at": started_at,
        "status": "failed",
        "ports": ports,
    }
    error: TopologyError | None = None
    secret_values: list[str] = []

    try:
        commands = ["docker", "git", "node", "npm", "cargo", "npx", "uv"]
        if args.terminal_user_journey:
            commands.append("openssl")
        for command in commands:
            if shutil.which(command) is None:
                raise TopologyError(f"required executable is unavailable: {command}")
        _run(["docker", "info"], timeout=30, label="verify Docker")

        bot_root = _clone_or_validate(
            supplied=args.bot_repo,
            destination=work_root / "bicameral-bot",
            repository="BicameralAI/bicameral-bot",
            ref=args.bot_ref,
            label="bot",
        )
        mcp_root = _clone_or_validate(
            supplied=args.mcp_repo,
            destination=work_root / "bicameral-mcp",
            repository="BicameralAI/bicameral-mcp",
            ref=args.mcp_ref,
            label="MCP",
        )
        commits = {
            "cloud": _git_sha(cloud_root),
            "bot": _git_sha(bot_root),
            "mcp": _git_sha(mcp_root),
        }
        if args.factory_commit:
            if not re.fullmatch(r"[0-9a-f]{40}", args.factory_commit):
                raise TopologyError("factory commit must be a full lowercase SHA")
            commits["factory"] = args.factory_commit

        bot_binary = _build_bot(bot_root)
        python = _ensure_python_environment(
            work_root=work_root,
            cloud_root=cloud_root,
            mcp_root=mcp_root,
            include_journey=args.terminal_user_journey,
        )
        if args.terminal_user_journey:
            _expose_python_environment(python)
        dashboard = _prepare_cloud_runtime(
            cloud_root=cloud_root,
            bot_root=bot_root,
            runtime_root=work_root / "cloud-runtime",
            bot_commit=commits["bot"],
            validate_terminal_surface=args.terminal_user_journey,
            use_vendored_dashboard=args.use_vendored_dashboard,
        )

        supabase = _start_supabase(
            root=supabase_root,
            cloud_root=cloud_root,
            ports=ports,
            project_id=project_id,
        )
        supabase_started = True
        supabase_containers = _supabase_container_names(ports["supabase_db"])
        secret_values.extend(
            [
                supabase["anon_key"],
                supabase["service_key"],
                supabase["jwt_secret"],
            ]
        )
        migrations = _migration_versions(ports["supabase_db"])
        expected_migrations = [
            path.stem.split("_", 1)[0]
            for path in sorted((cloud_root / "supabase" / "migrations").glob("*.sql"))
        ]
        if migrations != expected_migrations:
            raise TopologyError(
                "applied migration versions did not match tracked Cloud migrations"
            )
        migration_rls = _migration_rls_evidence(
            ports["supabase_db"],
            applied_versions=migrations,
            tracked_versions=expected_migrations,
        )

        owner_email = f"topology-owner-{run_id.lower()}@{OWNER_DOMAIN}"
        peer_email = f"topology-peer-{run_id.lower()}@{OWNER_DOMAIN}"
        owner_password = secrets.token_urlsafe(24)
        peer_password = secrets.token_urlsafe(24)
        secret_values.extend([owner_password, peer_password])
        owner_id = _create_identity(
            supabase["api_url"],
            supabase["service_key"],
            email=owner_email,
            password=owner_password,
        )
        peer_id = _create_identity(
            supabase["api_url"],
            supabase["service_key"],
            email=peer_email,
            password=peer_password,
        )
        owner_token = _sign_in(
            supabase["api_url"],
            supabase["anon_key"],
            email=owner_email,
            password=owner_password,
        )
        peer_token = _sign_in(
            supabase["api_url"],
            supabase["anon_key"],
            email=peer_email,
            password=peer_password,
        )
        secret_values.extend([owner_token, peer_token])
        database_behavior = _validate_database_behavior(
            api_url=supabase["api_url"],
            anon_key=supabase["anon_key"],
            service_key=supabase["service_key"],
            owner_token=owner_token,
        )
        migration_rls["behavioral_validation"] = database_behavior
        _write_redacted_evidence(
            receipt_path.parent / "migration-rls-evidence.json",
            migration_rls,
            secret_values=secret_values,
            work_root=work_root,
        )

        signing = (
            _generate_product_join_signing_material(work_root)
            if args.terminal_user_journey
            else None
        )
        tls_key: Path | None = None
        tls_certificate: Path | None = None
        if args.terminal_user_journey:
            tls_key, tls_certificate = _generate_tls_material(work_root)
            secret_values.append(signing["private_key_b64"] if signing else "")

        cloud_process = _start_cloud(
            python=python,
            runtime_root=work_root / "cloud-runtime",
            cloud_root=cloud_root,
            port=ports["cloud"],
            supabase=supabase,
            log_dir=log_dir,
            daemon_port=(ports["daemon_a"] if args.terminal_user_journey else None),
            product_join_signing=signing,
            tls_key=tls_key,
            tls_certificate=tls_certificate,
        )
        processes.append(cloud_process)
        cloud_behavior, browser_payloads = _validate_cloud(
            cloud_port=ports["cloud"],
            owner_token=owner_token,
            expected_dashboard=dashboard,
            base_url=(
                f"https://127.0.0.1:{ports['cloud']}"
                if args.terminal_user_journey
                else None
            ),
            verify_tls=not args.terminal_user_journey,
        )

        homes = [work_root / "home-a", work_root / "home-b"]
        workspaces = [work_root / "workspace-a", work_root / "workspace-b"]
        for index, workspace in enumerate(workspaces):
            _create_workspace(workspace, f"topology-daemon-{index + 1}")
        config_digests = [
            _configure_workspace(
                binary=bot_binary,
                workspace=workspaces[index],
                home=homes[index],
                port=ports["daemon_a" if index == 0 else "daemon_b"],
                name=f"topology-daemon-{index + 1}",
            )
            for index in range(2)
        ]
        for index, workspace in enumerate(workspaces):
            _git_commit(
                workspace,
                f"Configure topology daemon {index + 1}",
                [".bicameral/config.yaml"],
            )

        daemon_a = _start_daemon(
            binary=bot_binary,
            workspace=workspaces[0],
            home=homes[0],
            name="daemon-a",
            port=ports["daemon_a"],
            log_dir=log_dir,
        )
        processes.append(daemon_a)
        daemon_b = _start_daemon(
            binary=bot_binary,
            workspace=workspaces[1],
            home=homes[1],
            name="daemon-b",
            port=ports["daemon_b"],
            log_dir=log_dir,
        )
        processes.append(daemon_b)
        capabilities_a = _daemon_capabilities(ports["daemon_a"])
        capabilities_b = _daemon_capabilities(ports["daemon_b"])

        bridge_a, pair_payload_a = _pair_daemon(ports["daemon_a"], owner_id)
        bridge_b, pair_payload_b = _pair_daemon(ports["daemon_b"], peer_id)
        if bridge_a == bridge_b:
            raise TopologyError("daemon bridge sessions were not isolated")

        probe = work_root / "mcp_probe.py"
        _write_mcp_probe(probe)
        ingest = _mcp_call(
            python=python,
            probe=probe,
            daemon_port=ports["daemon_a"],
            workspace=workspaces[0],
            tool_name="bicameral.ingest",
            arguments={
                "source_uri": "topology://replay-marker",
                "source_type": "local",
                "label": "Real-process topology replay marker",
                "title": "Real-process topology replay marker",
                "description": (
                    "Decision: we will use this topology replay marker as the "
                    "durable disposable-run state."
                ),
                "suggested_level": "implementation",
                "snapshot_content": "real-process topology replay marker",
                "evidence": [
                    {
                        "excerpt": "real-process topology replay marker",
                        "confidence": 1.0,
                    }
                ],
            },
        )
        if "status" not in _mcp_content(ingest):
            raise TopologyError("MCP ingest did not return a ToolResponse")
        before_count, before_digest = _event_state(workspaces[0])
        daemon_b_count, _ = _event_state(workspaces[1])
        if before_count == 0 or daemon_b_count != 0:
            raise TopologyError("daemon Git event stores were not isolated")
        event_commit = _git_commit(
            workspaces[0],
            "Commit topology replay state",
            [".bicameral/events"],
        )
        search_before = _mcp_call(
            python=python,
            probe=probe,
            daemon_port=ports["daemon_a"],
            workspace=workspaces[0],
            tool_name="bicameral.search",
            arguments={"query": "real-process topology replay marker", "limit": 10},
        )
        if "replay marker" not in _mcp_content(search_before).lower():
            raise TopologyError(
                "daemon search did not observe committed topology state"
            )

        _stop_process(daemon_a)
        processes.remove(daemon_a)
        if not _wait_port_closed(ports["daemon_a"]):
            raise TopologyError("daemon port did not close for restart")
        daemon_a = _start_daemon(
            binary=bot_binary,
            workspace=workspaces[0],
            home=homes[0],
            name="daemon-a-restarted",
            port=ports["daemon_a"],
            log_dir=log_dir,
        )
        processes.append(daemon_a)
        after_count, after_digest = _event_state(workspaces[0])
        if (after_count, after_digest) != (before_count, before_digest):
            raise TopologyError("Git event store changed across daemon restart")
        restarted_config_digest = _sha256_file(
            workspaces[0] / ".bicameral" / "config.yaml"
        )
        if restarted_config_digest != config_digests[0]:
            raise TopologyError("daemon configuration changed across restart")
        if _git_sha(workspaces[0]) != event_commit:
            raise TopologyError("committed daemon state changed across restart")
        search_after = _mcp_call(
            python=python,
            probe=probe,
            daemon_port=ports["daemon_a"],
            workspace=workspaces[0],
            tool_name="bicameral.search",
            arguments={"query": "real-process topology replay marker", "limit": 10},
        )
        if "replay marker" not in _mcp_content(search_after).lower():
            raise TopologyError(
                "daemon replay did not reconstruct committed topology state"
            )

        terminal_journey: dict[str, object] | None = None
        if args.terminal_user_journey:
            if signing is None:
                raise TopologyError("Product join signing material was not generated")
            terminal_journey, daemon_b = _run_terminal_user_journey(
                artifact_dir=receipt_path.parent / "browser-evidence",
                work_root=work_root,
                owner_token=owner_token,
                peer_token=peer_token,
                ports=ports,
                workspaces=workspaces,
                homes=homes,
                bot_binary=bot_binary,
                log_dir=log_dir,
                signing=signing,
                hosted_origin=(f"https://app.bicameral-ai.com:{ports['cloud']}"),
                daemon_a=daemon_a,
                daemon_b=daemon_b,
                processes=processes,
                secret_values=secret_values,
            )
            bridge_b, pair_payload_b = _pair_daemon(ports["daemon_b"], peer_id)

        restart_evidence: dict[str, object] = {
            "schema_version": 1,
            "daemon_restart": {
                "pre_restart_event_count": before_count,
                "post_restart_event_count": after_count,
                "pre_restart_event_digest": before_digest,
                "post_restart_event_digest": after_digest,
                "committed_git_revision": event_commit,
                "configuration_digest_unchanged": (
                    restarted_config_digest == config_digests[0]
                ),
                "search_reconstructed_committed_state": True,
            },
        }
        if terminal_journey is not None:
            restart_evidence["browser_recovery"] = {
                "create_response_loss_recovered": terminal_journey.get(
                    "create_response_loss_recovered"
                ),
                "cloud_response_loss_recovered": terminal_journey.get(
                    "cloud_response_loss_recovered"
                ),
                "join_response_loss_recovered_idempotently": terminal_journey.get(
                    "join_response_loss_recovered_idempotently"
                ),
                "daemon_member_event_count": terminal_journey.get(
                    "daemon_member_event_count"
                ),
                "member_projection_after_reload": terminal_journey.get(
                    "member_projection_after_reload"
                ),
            }
        _write_redacted_evidence(
            receipt_path.parent / "restart-replay-evidence.json",
            restart_evidence,
            secret_values=secret_values,
            work_root=work_root,
        )

        for managed in processes:
            managed.log.flush()
        _scan_payloads(
            [*browser_payloads, pair_payload_a, pair_payload_b],
            secret_values,
        )
        _scan_logs(log_dir, [*secret_values, bridge_a, bridge_b], work_root)
        retained_process_logs = _retain_process_logs(
            log_dir,
            receipt_path.parent / "process-logs",
            secret_values=[*secret_values, bridge_a, bridge_b],
            work_root=work_root,
        )
        retained_evidence: dict[str, object] = {
            "process_logs": retained_process_logs,
            "migration_rls": "migration-rls-evidence.json",
            "restart_replay": "restart-replay-evidence.json",
        }
        if terminal_journey is not None:
            retained_evidence.update(
                {
                    "playwright_traces": [
                        "trace-owner.zip",
                        "trace-peer.zip",
                        "trace-disconnect.zip",
                    ],
                    "redacted_network_capture": "network-redacted.json",
                }
            )

        supported_a = capabilities_a.get("supported_commands")
        supported_b = capabilities_b.get("supported_commands")
        receipt.update(
            {
                "status": "passed",
                "components": commits,
                "dashboard": dashboard,
                "migrations": {
                    "versions": migrations,
                    "behavioral_validation": database_behavior,
                },
                "identities": [
                    {
                        "id": owner_id,
                        "domain": OWNER_DOMAIN,
                        "role": "same-domain",
                    },
                    {
                        "id": peer_id,
                        "domain": OWNER_DOMAIN,
                        "role": "same-domain-peer",
                    },
                ],
                "credential_lifecycle": {
                    "supabase_generated": True,
                    "identity_credentials_generated": True,
                    "key_fingerprints": {
                        "public_api": _sha256_bytes(supabase["anon_key"].encode()),
                        "privileged_api": _sha256_bytes(
                            supabase["service_key"].encode()
                        ),
                        "token_signing_material": _sha256_bytes(
                            supabase["jwt_secret"].encode()
                        ),
                    },
                    "persisted_in_receipt": False,
                    "scheduled_for_removal": True,
                },
                "processes": {
                    "cloud": cloud_behavior,
                    "daemon_a": {
                        "health": True,
                        "capability_contract": True,
                        "supported_command_count": (
                            len(supported_a) if isinstance(supported_a, list) else 0
                        ),
                        "bridge_paired": True,
                        "bridge_session_fingerprint": _sha256_bytes(bridge_a.encode()),
                    },
                    "daemon_b": {
                        "health": True,
                        "capability_contract": True,
                        "supported_command_count": (
                            len(supported_b) if isinstance(supported_b, list) else 0
                        ),
                        "bridge_paired": True,
                        "bridge_session_fingerprint": _sha256_bytes(bridge_b.encode()),
                    },
                    "mcp": {
                        "external_client_process": True,
                        "external_server_process": True,
                        "ingest_reached_daemon_a": True,
                        "search_reached_daemon_a": True,
                    },
                },
                "isolation": {
                    "loopback_only": True,
                    "distinct_ports": len(set(ports.values())) == len(ports),
                    "distinct_homes": True,
                    "distinct_git_repositories": True,
                    "distinct_bridge_sessions": True,
                    "daemon_b_event_count_after_daemon_a_ingest": daemon_b_count,
                },
                "restart_replay": {
                    "config_reopened": True,
                    "git_commit": event_commit,
                    "event_count": after_count,
                    "event_digest": after_digest,
                    "committed_state_preserved": True,
                    "replay_reconstructed_state": True,
                },
                "generated_resources": {
                    "supabase_project": project_id,
                    "containers": "disposable-supabase-stack",
                    "temporary_homes": 2,
                    "temporary_git_repositories": 2,
                    "bridge_sessions": 2,
                    "controlled_identities": 2,
                },
                "sanitization": {
                    "browser_payloads_scanned": True,
                    "process_logs_scanned": True,
                    "raw_credentials_recorded": False,
                    "host_paths_recorded": False,
                },
                "retained_evidence": retained_evidence,
            }
        )
        if terminal_journey is not None:
            receipt["terminal_user_journey"] = terminal_journey
            receipt.update(
                _terminal_acceptance_state(terminal_journey.get("blocking_contract"))
            )
            if terminal_journey.get("blocking_contract"):
                receipt["status"] = "blocked"
                raise TopologyError(
                    "hosted membership projection remained unavailable after a "
                    "daemon-confirmed idempotent join and browser reload"
                )
    except TopologyError as exc:
        error = exc
    finally:
        for managed in reversed(processes):
            _stop_process(managed)
        supabase_stopped = _stop_supabase(supabase_root) if supabase_started else True
        port_cleanup = {
            name: _wait_port_closed(port)
            for name, port in ports.items()
            if name in {"cloud", "daemon_a", "daemon_b", "supabase_api", "supabase_db"}
        }
        containers_remaining = _containers_remaining(supabase_containers)
        shutil.rmtree(work_root, ignore_errors=True)
        cleanup = {
            "processes_stopped": all(
                port_cleanup.get(name, True)
                for name in ("cloud", "daemon_a", "daemon_b")
            ),
            "supabase_stopped": supabase_stopped,
            "containers_remaining": containers_remaining,
            "ports_closed": all(port_cleanup.values()),
            "temporary_root_removed": not work_root.exists(),
            "credentials_removed_with_temporary_root": not work_root.exists(),
        }
        receipt["cleanup"] = cleanup
        receipt["finished_at"] = _now()
        if error is not None:
            receipt["failure"] = {"message": str(error)}
        try:
            _write_receipt(receipt_path, receipt)
        except TopologyError as receipt_error:
            if error is None:
                error = receipt_error

    if error is not None:
        _fail(str(error))
    print("real-process topology passed")
    print(f"receipt={_receipt_log_label(receipt_path, cloud_root)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
