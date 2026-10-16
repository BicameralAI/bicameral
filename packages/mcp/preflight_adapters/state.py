"""MCP-local adapter install state and consent records.

These are MCP-owned local host configuration facts, not Decision lifecycle
state. ``installed``/``enabled`` describe whether an operator has consented to,
and switched on, a host pre-work adapter on this machine. The daemon owns all
canonical Decision, candidate, signoff, evidence, and compliance state.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from dataclasses import dataclass
from datetime import UTC, datetime
from enum import StrEnum
from pathlib import Path
from typing import Any
from uuid import uuid4

#: Contract version for the installed adapter configuration. ``update`` rewrites
#: an installed adapter when this differs from the recorded value.
ADAPTER_CONTRACT_VERSION = "3"

#: Directory (relative to a host config home) that holds MCP-owned adapter
#: metadata, consent records, and dedup markers. Kept separate from the host's
#: own configuration file so uninstalling never disturbs unrelated host state.
MCP_STATE_DIRNAME = "bicameral-mcp"

ACTIVATION_RECEIPT_SCHEMA_VERSION = 1
MAX_PROCESS_ANCESTORS = 64


def _parent_pid(pid: int) -> tuple[int | None, str]:
    """Return a process parent without retaining command lines or environment."""
    proc_stat = Path(f"/proc/{pid}/stat")
    try:
        raw = proc_stat.read_text(encoding="utf-8")
        closing_paren = raw.rfind(")")
        fields = raw[closing_paren + 2 :].split()
        return int(fields[1]), "proc-pid-chain"
    except (IndexError, OSError, ValueError):
        pass

    try:
        completed = subprocess.run(
            ["ps", "-o", "ppid=", "-p", str(pid)],
            check=True,
            capture_output=True,
            text=True,
            timeout=1,
        )
        return int(completed.stdout.strip()), "ps-pid-chain"
    except (OSError, subprocess.SubprocessError, ValueError):
        return None, "unavailable"


def _process_provenance() -> dict[str, Any]:
    """Capture a bounded PID-only ancestry chain for durable subtree proof."""
    writer_pid = os.getpid()
    ancestor_pids: list[int] = []
    methods: set[str] = set()
    seen = {writer_pid}
    current = writer_pid
    for _ in range(MAX_PROCESS_ANCESTORS):
        parent, method = _parent_pid(current)
        methods.add(method)
        if parent is None or parent <= 0 or parent in seen:
            break
        ancestor_pids.append(parent)
        seen.add(parent)
        current = parent
    usable_methods = methods - {"unavailable"}
    method = next(iter(usable_methods)) if len(usable_methods) == 1 else "unavailable"
    return {
        "method": method,
        "writer_pid": writer_pid,
        "parent_pid": ancestor_pids[0] if ancestor_pids else os.getppid(),
        "ancestor_pids": ancestor_pids,
    }


def _evidence_provenance_method(raw: str) -> str:
    """Map local process-provenance labels onto evaluate_host_activation methods."""
    if raw in {"kernel-exec", "proc-ancestry", "kernel-exec+proc-ancestry"}:
        return raw
    if raw in {"ps-pid-chain", "proc-pid-chain"}:
        return "proc-ancestry"
    return "unavailable"


def _process_command_name(pid: int) -> str:
    try:
        completed = subprocess.run(
            ["ps", "-o", "comm=", "-p", str(pid)],
            check=True,
            capture_output=True,
            text=True,
            timeout=1,
        )
        return completed.stdout.strip().lower()
    except (OSError, subprocess.SubprocessError, ValueError):
        return ""


def _host_process_pid(host: str, ancestor_pids: list[int]) -> int | None:
    """Accept the named host through shell hook wrappers, not arbitrary runners."""
    for pid in ancestor_pids:
        name = Path(_process_command_name(pid)).name
        if name == host:
            return pid
        if name not in {"sh", "bash", "zsh", "dash"}:
            break
    return None


def _has_terminal(pid: int) -> bool:
    try:
        completed = subprocess.run(
            ["ps", "-o", "tty=", "-p", str(pid)],
            check=True,
            capture_output=True,
            text=True,
            timeout=1,
        )
        return completed.stdout.strip() not in {"", "?", "??", "-"}
    except (OSError, subprocess.SubprocessError):
        return False


class AdapterState(StrEnum):
    """Local install state of a host pre-work adapter."""

    NOT_INSTALLED = "not_installed"
    INSTALLED_ENABLED = "installed_enabled"
    INSTALLED_DISABLED = "installed_disabled"
    MANUAL_FALLBACK_REQUIRED = "manual_fallback_required"
    HOST_MECHANISM_UNAVAILABLE = "host_mechanism_unavailable"
    CONFIG_INVALID_FAIL_CLOSED = "config_invalid_fail_closed"
    PACKAGE_MISSING_OR_MISMATCHED = "package_missing_or_mismatched"
    INSTALLED_BUT_HOST_DID_NOT_FIRE = "installed_but_host_did_not_fire"
    AUTHENTIC_HOST_FIRED = "authentic_host_fired"

    # Source compatibility for callers that imported the original enum names.
    ENABLED = INSTALLED_ENABLED
    DISABLED = INSTALLED_DISABLED


@dataclass(frozen=True)
class ConsentRecord:
    """Explicit operator consent to enable a host pre-work adapter."""

    granted: bool
    granted_at: str
    descriptor_summary: str

    def to_dict(self) -> dict[str, Any]:
        return {
            "granted": self.granted,
            "granted_at": self.granted_at,
            "descriptor_summary": self.descriptor_summary,
        }

    @classmethod
    def now(cls, descriptor_summary: str) -> ConsentRecord:
        return cls(
            granted=True,
            granted_at=datetime.now(UTC).isoformat().replace("+00:00", "Z"),
            descriptor_summary=descriptor_summary,
        )

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ConsentRecord:
        return cls(
            granted=bool(data.get("granted", False)),
            granted_at=str(data.get("granted_at", "")),
            descriptor_summary=str(data.get("descriptor_summary", "")),
        )


@dataclass
class AdapterMetadata:
    """Persisted MCP-owned metadata for one installed host adapter."""

    host: str
    installed: bool
    enabled: bool
    contract_version: str
    runner_invocation: str
    consent: ConsentRecord | None
    updated_at: str
    package_version: str = ""
    runner_executable: str = ""
    runner_source: str = ""
    # Release-descriptor digest recorded by Bot setup (`--artifact-generation`).
    # Empty when install was manual/legacy without a published binding.
    artifact_generation: str = ""
    # Exact host CLI version recorded at install (`--host-version`), when known.
    host_version: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "host": self.host,
            "installed": self.installed,
            "enabled": self.enabled,
            "contract_version": self.contract_version,
            "runner_invocation": self.runner_invocation,
            "consent": self.consent.to_dict() if self.consent else None,
            "updated_at": self.updated_at,
            "package_version": self.package_version,
            "runner_executable": self.runner_executable,
            "runner_source": self.runner_source,
            "artifact_generation": self.artifact_generation,
            "host_version": self.host_version,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> AdapterMetadata:
        consent_raw = data.get("consent")
        consent = ConsentRecord.from_dict(consent_raw) if isinstance(consent_raw, dict) else None
        return cls(
            host=str(data.get("host", "")),
            installed=bool(data.get("installed", False)),
            enabled=bool(data.get("enabled", False)),
            contract_version=str(data.get("contract_version", "")),
            runner_invocation=str(data.get("runner_invocation", "")),
            consent=consent,
            updated_at=str(data.get("updated_at", "")),
            package_version=str(data.get("package_version", "")),
            runner_executable=str(data.get("runner_executable", "")),
            runner_source=str(data.get("runner_source", "")),
            artifact_generation=str(data.get("artifact_generation", "")),
            host_version=str(data.get("host_version", "")),
        )


class AdapterStore:
    """Reads/writes MCP-owned adapter metadata and dedup markers on disk."""

    def __init__(self, state_dir: Path) -> None:
        self._state_dir = state_dir

    @property
    def state_dir(self) -> Path:
        return self._state_dir

    @property
    def _metadata_path(self) -> Path:
        return self._state_dir / "adapter.json"

    @property
    def _fired_dir(self) -> Path:
        return self._state_dir / "prework-fired"

    @property
    def activation_receipt_dir(self) -> Path:
        return self._state_dir / "host-activation-receipts"

    def load(self) -> AdapterMetadata | None:
        path = self._metadata_path
        if not path.is_file():
            return None
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
        if not isinstance(data, dict):
            return None
        return AdapterMetadata.from_dict(data)

    def save(self, metadata: AdapterMetadata) -> None:
        metadata.updated_at = datetime.now(UTC).isoformat().replace("+00:00", "Z")
        self._state_dir.mkdir(parents=True, exist_ok=True)
        self._metadata_path.write_text(
            json.dumps(metadata.to_dict(), indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    def clear(self) -> None:
        """Remove all MCP-owned adapter metadata and operational witnesses."""
        from .clean_home import WITNESS_FILE

        (self._state_dir / WITNESS_FILE).unlink(missing_ok=True)
        if self._metadata_path.is_file():
            self._metadata_path.unlink()
        if self._fired_dir.is_dir():
            for marker in self._fired_dir.iterdir():
                if marker.is_file():
                    marker.unlink()
            self._fired_dir.rmdir()
        if self.activation_receipt_dir.is_dir():
            for receipt in self.activation_receipt_dir.iterdir():
                if receipt.is_file():
                    receipt.unlink()
            self.activation_receipt_dir.rmdir()
        if self._state_dir.is_dir() and not any(self._state_dir.iterdir()):
            self._state_dir.rmdir()

    # --- host activation receipts (sanitized operational witnesses only) ---

    def record_activation(
        self,
        *,
        host: str,
        hook_event_name: str,
        source: str,
        correlation_id: str,
        package_version: str,
        contract_version: str,
        managed_command: str,
    ) -> Path:
        """Write a sanitized activation receipt consumable by evaluate_host_activation.

        Local operational witnesses (process ancestry, install consent, package pin)
        are projected into the product-primary ``lanes`` schema. Clean-home proof
        comes only from the separate pre-install observation. Missing topology
        or handshake facts never become successful activation by implication.
        """
        from .clean_home import clean_home_witness

        receipt_id = uuid4().hex
        process_provenance = _process_provenance()
        metadata = self.load()
        clean_home = clean_home_witness(self._state_dir.parent, metadata) if metadata else None
        host_version = (metadata.host_version if metadata is not None else "") or ""
        consented = bool(
            metadata is not None and metadata.consent is not None and metadata.consent.granted
        )
        released_artifact = bool(
            metadata is not None and (metadata.artifact_generation or "").strip()
        )
        provenance_method = _evidence_provenance_method(
            str(process_provenance.get("method") or "unavailable")
        )
        writer_pid = process_provenance.get("writer_pid")
        parent_pid = process_provenance.get("parent_pid")
        if not isinstance(writer_pid, int):
            writer_pid = os.getpid()
        if not isinstance(parent_pid, int):
            parent_pid = os.getppid()
        host_pid = _host_process_pid(host, process_provenance.get("ancestor_pids", []))
        direct_hook_invocation = host_pid is None
        interaction_surface = "pty" if host_pid is not None and _has_terminal(host_pid) else "pipe"
        hook_chain = [
            {
                "pid": writer_pid,
                "ppid": parent_pid,
                "argv_shape": f"bicameral-mcp prework-run --host {host}",
                "source": provenance_method
                if provenance_method != "unavailable"
                else "proc-ancestry",
            }
        ]
        lane = {
            "classification": "product_primary",
            "provider": host,
            "interaction_surface": interaction_surface,
            # Only the independent pre-install observation can establish this.
            "clean_home": clean_home is not None,
            "consented_install": consented,
            "released_artifact": released_artifact,
            "version_probe_ok": bool(host_version.strip()),
            "hook_written": True,
            "host_fired_managed_hook": True,
            "direct_hook_invocation": direct_hook_invocation,
            "host_pid": host_pid,
            "provenance_method": provenance_method,
            "evidence_ceiling": "authentic-host/synthetic-operator",
            "host_version": host_version,
            "hook_chain": hook_chain,
        }
        payload = {
            "schema_version": ACTIVATION_RECEIPT_SCHEMA_VERSION,
            "receipt_type": "host_activation",
            "receipt_id": receipt_id,
            # Never store raw correlation_id (embeds host session id).
            # Consumers use correlation_sha256 and receipt_id only.
            "host": host,
            "hook_event_name": hook_event_name,
            "source": source,
            "observed_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
            "correlation_sha256": "sha256:"
            + hashlib.sha256(correlation_id.encode("utf-8")).hexdigest(),
            "adapter_contract_version": contract_version,
            "package_version": package_version,
            "artifact_generation": metadata.artifact_generation if metadata else "",
            "clean_home_evidence": clean_home,
            "daemon_handshake_verified": False,
            "managed_command_sha256": "sha256:"
            + hashlib.sha256(managed_command.encode("utf-8")).hexdigest(),
            "process_provenance": process_provenance,
            "host_versions": {host: host_version} if host_version.strip() else {},
            "lanes": [lane],
            "sanitized": True,
        }
        receipt_dir = self.activation_receipt_dir
        receipt_dir.mkdir(parents=True, exist_ok=True)
        destination = receipt_dir / f"{receipt_id}.json"
        temporary = receipt_dir / f".{receipt_id}.tmp"
        with temporary.open("xb") as handle:
            handle.write((json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8"))
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, destination)
        return destination

    def confirm_activation_handshake(self, receipt: Path) -> None:
        """Complete the same operational receipt after the protocol check passes."""
        payload = json.loads(receipt.read_text(encoding="utf-8"))
        payload["daemon_handshake_verified"] = True
        temporary = receipt.with_suffix(".tmp")
        temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(receipt)

    # --- dedup / idempotency markers (operational witnesses only) ---

    def _marker_path(self, correlation_id: str) -> Path:
        safe = "".join(ch if ch.isalnum() or ch in "-._" else "_" for ch in correlation_id)
        return self._fired_dir / f"{safe}.json"

    def has_fired(self, correlation_id: str) -> bool:
        return self._marker_path(correlation_id).is_file()

    def mark_fired(self, correlation_id: str, detail: dict[str, Any]) -> None:
        self._fired_dir.mkdir(parents=True, exist_ok=True)
        payload = {
            "correlation_id": correlation_id,
            "fired_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
            **detail,
        }
        self._marker_path(correlation_id).write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
