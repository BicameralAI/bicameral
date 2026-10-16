"""CLI surface for MCP-distributed host pre-work adapters.

Wired into the ``bicameral-mcp`` console entrypoint:

    bicameral-mcp adapters status  [--host claude|codex] [--json]
    bicameral-mcp adapters prepare-host --host claude|codex --home NEW_CONFIG_HOME
        --artifact-generation sha256:...
    bicameral-mcp adapters install --host claude|codex --consent
        [--artifact-generation sha256:…] [--host-version …]
    bicameral-mcp adapters update  --host claude|codex
        [--artifact-generation sha256:…] [--host-version …]
    bicameral-mcp adapters disable --host claude|codex
    bicameral-mcp adapters uninstall --host claude|codex
    bicameral-mcp adapters verify-host --host claude|codex --receipt receipt.json
        [--artifact-generation sha256:…]
    bicameral-mcp prework-run --host claude|codex   # reads host event JSON on stdin

``prework-run`` is what an installed host hook invokes; it is not typically run
by hand.

``--artifact-generation`` / ``--host-version`` are the Bot setup controller
contract (Bot v0.1.19+). They must be accepted so published wheels remain
compatible with the bound Bot release.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from .base import AdapterActionResult, HostConfigError, _normalize_artifact_generation
from .clean_home import clean_home_witness, prepare_home
from .evidence import HostEvidenceError, evaluate_host_activation
from .registry import get_adapter, supported_hosts
from .runner import run_prework


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="bicameral-mcp adapters",
        description="Manage MCP-distributed host pre-work adapters.",
    )
    sub = parser.add_subparsers(dest="adapter_command", required=True)

    prepare = sub.add_parser("prepare-host", help="Create a fresh host-config evidence root.")
    prepare.add_argument("--host", choices=list(supported_hosts()), required=True)
    prepare.add_argument(
        "--home", type=Path, required=True, help="New, nonexistent host config home."
    )
    prepare.add_argument("--artifact-generation", required=True)
    prepare.add_argument("--json", action="store_true")

    for name in ("status", "install", "update", "disable", "uninstall"):
        p = sub.add_parser(name)
        p.add_argument(
            "--host",
            choices=list(supported_hosts()),
            required=(name != "status"),
            help="Target host (required for all actions except status).",
        )
        p.add_argument("--home", help="Override the host config home (for testing).")
        p.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")
        if name == "install":
            p.add_argument(
                "--consent",
                action="store_true",
                help="Explicit operator consent to enable the adapter.",
            )
        if name in ("install", "update"):
            p.add_argument(
                "--artifact-generation",
                default="",
                help=(
                    "Bot release-descriptor digest (sha256:<64 hex>) that this "
                    "managed install binds. Required by Bot setup controller."
                ),
            )
            p.add_argument(
                "--host-version",
                default="",
                help="Exact host CLI version string recorded by Bot setup.",
            )
    verify = sub.add_parser(
        "verify-host",
        help="Evaluate a sanitized external authentic-host receipt.",
    )
    verify.add_argument("--host", choices=list(supported_hosts()), required=True)
    verify.add_argument("--receipt", type=Path, required=True)
    verify.add_argument("--home", help="Host config home used for setup and activation.")
    verify.add_argument(
        "--artifact-generation",
        default="",
        help="Bot release-descriptor digest the receipt must bind (when provided).",
    )
    verify.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")
    return parser


def run_adapters_cli(argv: list[str]) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    home = Path(args.home) if getattr(args, "home", None) else None
    command = args.adapter_command

    if command == "prepare-host":
        try:
            witness = prepare_home(
                args.home, args.host, _normalize_artifact_generation(args.artifact_generation)
            )
            payload = {"host": args.host, "state": "prepared", "evidence": witness}
            code = 0
        except (OSError, ValueError) as exc:
            payload = {"host": args.host, "state": "preparation_rejected", "error": str(exc)}
            code = 1
        print(json.dumps(payload, indent=2, sort_keys=True) if args.json else payload)
        return code

    if command == "verify-host":
        return _verify_host_receipt(
            args.host,
            args.receipt,
            artifact_generation=str(getattr(args, "artifact_generation", "") or ""),
            as_json=args.json,
            home=home,
        )

    if command == "status":
        hosts = [args.host] if args.host else list(supported_hosts())
        payloads = [get_adapter(h, home=home).status().to_dict() for h in hosts]
        if args.json:
            print(json.dumps(payloads, indent=2, sort_keys=True))
        else:
            for payload in payloads:
                _print_status(payload)
        return 0

    adapter = get_adapter(args.host, home=home)
    try:
        if command == "install":
            result = adapter.install(
                consent=bool(getattr(args, "consent", False)),
                artifact_generation=str(getattr(args, "artifact_generation", "") or ""),
                host_version=str(getattr(args, "host_version", "") or ""),
            )
        elif command == "update":
            result = adapter.update(
                artifact_generation=str(getattr(args, "artifact_generation", "") or ""),
                host_version=str(getattr(args, "host_version", "") or ""),
            )
        elif command == "disable":
            result = adapter.disable()
        elif command == "uninstall":
            result = adapter.uninstall()
        else:  # pragma: no cover - argparse guarantees a valid command
            parser.error(f"unknown command {command!r}")
    except HostConfigError as exc:
        status = adapter.status()
        result = AdapterActionResult(
            host=args.host,
            action=command,
            ok=False,
            state=status.state,
            message=str(exc),
            package_provenance=status.package_provenance,
        )

    if args.json:
        print(json.dumps(result.to_dict(), indent=2, sort_keys=True))
    else:
        print(f"[{result.host}] {result.action}: {result.message}")
    return 0 if result.ok else 1


def run_prework_cli(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="bicameral-mcp prework-run",
        description=(
            "Invoked by a host hook at a SessionStart pre-work boundary or a "
            "Planning-complete boundary."
        ),
    )
    parser.add_argument("--host", choices=list(supported_hosts()), required=True)
    parser.add_argument("--home", help="Override the host config home (for testing).")
    args = parser.parse_args(argv)
    home = Path(args.home) if args.home else None

    payload = _read_event_payload()
    result = asyncio.run(run_prework(args.host, payload, home=home))

    # The message is always visible so the operator sees whether preflight ran.
    # Fallbacks are written to stderr and never claim preflight succeeded.
    stream = sys.stderr if (result.is_fallback or not result.preflight_invoked) else sys.stdout
    print(f"[bicameral pre-work / {result.host}] {result.message}", file=stream)
    # Exit 0 so a failed/absent daemon never blocks the host session.
    return 0


def _read_event_payload() -> dict[str, Any]:
    raw = sys.stdin.read().strip()
    if not raw:
        return {}
    try:
        data = json.loads(raw)
    except ValueError:
        return {}
    return data if isinstance(data, dict) else {}


def _print_status(payload: dict[str, Any]) -> None:
    print(f"host: {payload['host']}")
    print(f"  state: {payload['state']}")
    print(f"  mechanism: {payload['mechanism']}")
    print(f"  config_path: {payload['config_path']}")
    print(f"  hook_present: {payload['hook_present']}")
    print(f"  capability_supported: {payload['capability_supported']}")
    print(f"  consent_granted: {payload['consent_granted']}")
    print(f"  package_matches: {payload['package_matches']}")
    print(f"  artifact_generation: {payload.get('artifact_generation', '')}")
    print(f"  host_version: {payload.get('host_version', '')}")
    print(f"  activation_receipt_dir: {payload['activation_receipt_dir']}")
    provenance = payload.get("package_provenance")
    if provenance:
        print(f"  package_version: {provenance['package_version']}")
        print(f"  runner_executable: {provenance['executable_path']}")
    if payload.get("manual_preflight"):
        print(f"  manual_preflight: {payload['manual_preflight']}")
    print(f"  detail: {payload['detail']}")


def _verify_host_receipt(
    host: str,
    receipt_path: Path,
    *,
    artifact_generation: str = "",
    as_json: bool,
    home: Path | None = None,
) -> int:
    try:
        artifact = _normalize_artifact_generation(artifact_generation)
        receipt_bytes = receipt_path.read_bytes()
        receipt = json.loads(receipt_bytes.decode("utf-8"))
        if not isinstance(receipt, dict):
            raise HostEvidenceError("host receipt must be a JSON object")
        if artifact and receipt.get("artifact_generation") != artifact:
            raise HostEvidenceError("receipt does not bind the requested artifact generation")
        now = datetime.now(UTC)
        verified_at = now
        if receipt.get("receipt_type") == "host_activation":
            if receipt.get("daemon_handshake_verified") is not True:
                raise HostEvidenceError("host activation requires a successful daemon handshake")
            adapter = get_adapter(host, home=home)
            store = adapter._store()
            metadata = store.load()
            witness = clean_home_witness(adapter.home(), metadata) if metadata else None
            if (
                metadata is None
                or metadata.consent is None
                or witness is None
                or receipt.get("clean_home_evidence") != witness
                or receipt_path.resolve().parent != store.activation_receipt_dir.resolve()
                or receipt.get("artifact_generation") != metadata.artifact_generation
                or receipt.get("package_version") != metadata.package_version
                or receipt.get("adapter_contract_version") != metadata.contract_version
                or receipt.get("managed_command_sha256")
                != "sha256:"
                + hashlib.sha256(
                    adapter._managed_command(metadata.runner_invocation).encode()
                ).hexdigest()
                or not adapter.status().package_matches
                or not metadata.enabled
            ):
                raise HostEvidenceError(
                    "receipt lacks matching current prepared-home/install evidence"
                )
            verified_at = datetime.fromisoformat(receipt["observed_at"])
            if (
                verified_at.tzinfo is None
                or not now - timedelta(hours=24) < verified_at <= now
                or verified_at < datetime.fromisoformat(metadata.consent.granted_at)
            ):
                raise HostEvidenceError("host activation receipt is expired or future-dated")
        result = evaluate_host_activation(host, receipt)
        # Bot setup deserializes a local activation projection; include both the
        # historical evidence fields and the setup-controller fields.
        receipt_digest = "sha256:" + hashlib.sha256(receipt_bytes).hexdigest()
        payload: dict[str, Any] = result.to_dict()
        payload.update(
            {
                "verified": result.authentic,
                "artifact_generation": receipt.get("artifact_generation", artifact),
                "correlation_id": receipt.get("correlation_id")
                or receipt.get("receipt_id")
                or f"{host}:verify-host",
                "receipt_digest": receipt_digest,
                "verified_at": verified_at.isoformat().replace("+00:00", "Z"),
                "expires_at": (verified_at + timedelta(hours=24))
                .isoformat()
                .replace("+00:00", "Z"),
            }
        )
        if result.authentic:
            payload["state"] = "active_verified"
        exit_code = 0 if result.authentic else 1
    except (OSError, ValueError, KeyError, TypeError) as exc:
        payload = {
            "host": host,
            "state": "evidence_rejected",
            "verified": False,
            "authentic": False,
            "exactly_once": False,
            "invocation_count": 0,
            "provenance_method": "unavailable",
            "artifact_generation": (artifact_generation or "").strip(),
            "correlation_id": None,
            "receipt_digest": "",
            "verified_at": None,
            "expires_at": None,
            "error": str(exc),
        }
        exit_code = 1
    if as_json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        detail = payload.get("error") or payload["state"]
        print(f"[{host}] verify-host: {detail}")
    return exit_code
