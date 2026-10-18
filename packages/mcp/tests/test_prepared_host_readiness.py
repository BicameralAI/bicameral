"""#864: local evidence composition, not authentic-host acceptance evidence."""

import asyncio
import json
import shutil
from datetime import UTC, datetime, timedelta

import pytest
from prework_support import FakeDaemon, startup_event

from preflight_adapters import get_adapter
from preflight_adapters.clean_home import prepare_home
from preflight_adapters.cli import run_adapters_cli
from preflight_adapters.runner import run_prework

ARTIFACT = "sha256:" + "a" * 64


@pytest.fixture(params=["claude", "codex"])
def installed(request, tmp_path, monkeypatch):
    home = tmp_path / request.param
    prepare_home(home, request.param, ARTIFACT)
    adapter = get_adapter(request.param, home=home)
    assert adapter.install(consent=True, artifact_generation=ARTIFACT, host_version="test 1").ok
    # Other topology evidence is simulated here; this is not a live host run.
    monkeypatch.setattr("preflight_adapters.state._host_process_pid", lambda host, pids: 42)
    monkeypatch.setattr("preflight_adapters.state._has_terminal", lambda _: True)
    monkeypatch.setattr(
        "preflight_adapters.state._process_provenance",
        lambda: {
            "method": "ps-pid-chain",
            "writer_pid": 43,
            "parent_pid": 42,
            "ancestor_pids": [42],
        },
    )
    return adapter


def activation(adapter, daemon=None, *, session_id="sess-1"):
    asyncio.run(
        run_prework(
            adapter.host_id,
            {**startup_event(adapter.home()), "session_id": session_id},
            home=adapter.home(),
            client_factory=lambda: daemon or FakeDaemon(),
        )
    )
    return max(
        adapter._store().activation_receipt_dir.glob("*.json"), key=lambda p: p.stat().st_mtime_ns
    )


def verify(adapter, path, capsys, artifact=ARTIFACT):
    code = run_adapters_cli(
        [
            "verify-host",
            "--host",
            adapter.host_id,
            "--home",
            str(adapter.home()),
            "--receipt",
            str(path),
            "--artifact-generation",
            artifact,
            "--json",
        ]
    )
    return code, json.loads(capsys.readouterr().out)


def test_prepare_cli_never_adopts_an_existing_home(tmp_path, capsys):
    home = tmp_path / "codex"
    argv = [
        "prepare-host",
        "--host",
        "codex",
        "--home",
        str(home),
        "--artifact-generation",
        ARTIFACT,
        "--json",
    ]
    assert run_adapters_cli(argv) == 0
    evidence = json.loads(capsys.readouterr().out)["evidence"]
    assert str(tmp_path) not in json.dumps(evidence)
    assert home.stat().st_mode & 0o777 == 0o700
    before = (home / "bicameral-mcp" / "clean-home.json").read_bytes()
    assert run_adapters_cli(argv) == 1
    assert (home / "bicameral-mcp" / "clean-home.json").read_bytes() == before


def test_prepared_install_reaches_verified_without_renewing_receipt(installed, capsys):
    path = activation(installed)
    code, first = verify(installed, path, capsys)
    assert code == 0
    assert first["state"] == "active_verified"
    assert first["artifact_generation"] == ARTIFACT
    code, second = verify(installed, path, capsys)
    assert code == 0
    assert second == first


@pytest.mark.parametrize("disable_first", [False, True])
def test_same_binding_reinstall_requires_a_fresh_verified_receipt(installed, capsys, disable_first):
    old_receipt = activation(installed)
    before = old_receipt.read_bytes()
    assert verify(installed, old_receipt, capsys)[0] == 0
    if disable_first:
        assert installed.disable().ok
        assert verify(installed, old_receipt, capsys)[0] == 1
    assert installed.install(consent=True, artifact_generation=ARTIFACT, host_version="test 1").ok
    assert old_receipt.read_bytes() == before
    assert verify(installed, old_receipt, capsys)[0] == 1
    fresh_receipt = activation(installed, session_id="sess-2")
    code, result = verify(installed, fresh_receipt, capsys)
    assert code == 0
    assert result["state"] == "active_verified"
    assert result["invocation_count"] == 1


@pytest.mark.parametrize(
    ("artifact", "host_version"),
    [("sha256:" + "b" * 64, "test 1"), (ARTIFACT, "test 2")],
)
def test_reinstall_with_changed_binding_cannot_renew_home_proof(
    installed, capsys, artifact, host_version
):
    witness = installed.home() / "bicameral-mcp" / "clean-home.json"
    before = witness.read_bytes()
    assert installed.install(
        consent=True, artifact_generation=artifact, host_version=host_version
    ).ok
    assert witness.read_bytes() == before
    assert verify(installed, activation(installed), capsys, artifact=artifact)[0] == 1


@pytest.mark.parametrize(
    "change", ["artifact", "copy_home", "unprepared", "reinstall", "disabled", "package", "host"]
)
def test_changed_install_or_home_cannot_reuse_clean_evidence(installed, capsys, change, tmp_path):
    path = activation(installed)
    if change == "artifact":
        installed.update(artifact_generation="sha256:" + "b" * 64)
    elif change == "copy_home":
        copied = tmp_path / "copied"
        shutil.copytree(installed.home(), copied)
        installed = get_adapter(installed.host_id, home=copied)
        path = installed._store().activation_receipt_dir / path.name
    elif change == "unprepared":
        (installed.home() / "bicameral-mcp" / "clean-home.json").unlink()
    elif change == "reinstall":
        installed.install(consent=True, artifact_generation=ARTIFACT, host_version="test 1")
    elif change == "disabled":
        installed.disable()
    else:
        metadata = installed._store().load()
        setattr(metadata, "package_version" if change == "package" else "host_version", "different")
        installed._store().save(metadata)
    assert verify(installed, path, capsys)[0] == 1
    if change not in {"reinstall", "disabled"}:
        assert installed.install(
            consent=True, artifact_generation=ARTIFACT, host_version="test 1"
        ).ok
        assert verify(installed, activation(installed, session_id="sess-2"), capsys)[0] == 1


@pytest.mark.parametrize(
    "change", ["expired", "future", "direct", "pipe", "command", "artifact", "malformed_time"]
)
def test_negative_receipts_do_not_advance_readiness(installed, capsys, change):
    path = activation(installed)
    receipt = json.loads(path.read_text())
    if change in {"expired", "future"}:
        hours = -25 if change == "expired" else 1
        receipt["observed_at"] = (datetime.now(UTC) + timedelta(hours=hours)).isoformat()
    elif change == "malformed_time":
        receipt["observed_at"] = None
    elif change == "direct":
        receipt["lanes"][0]["direct_hook_invocation"] = True
    elif change == "pipe":
        receipt["lanes"][0]["interaction_surface"] = "pipe"
    elif change == "artifact":
        receipt["artifact_generation"] = "sha256:" + "b" * 64
    else:
        receipt["managed_command_sha256"] = "sha256:" + "b" * 64
    path.write_text(json.dumps(receipt))
    assert verify(installed, path, capsys)[0] == 1


def test_unprepared_activation_stays_false_and_uninstall_removes_witness(installed):
    installed.uninstall()
    assert not (installed.home() / "bicameral-mcp" / "clean-home.json").exists()
    installed.install(consent=True, artifact_generation=ARTIFACT, host_version="test 1")
    assert json.loads(activation(installed).read_text())["lanes"][0]["clean_home"] is False


@pytest.mark.parametrize("failure", ["protocol", "unsupported", "unavailable"])
def test_failed_handshake_preserves_witness_without_verifying(installed, capsys, failure):
    from daemon_client import DaemonConnectionError

    daemon = {
        "protocol": FakeDaemon(protocol_version="wrong"),
        "unsupported": FakeDaemon(supported=[]),
        "unavailable": FakeDaemon(raise_on_capabilities=DaemonConnectionError("offline")),
    }[failure]
    path = activation(installed, daemon)
    assert json.loads(path.read_text())["daemon_handshake_verified"] is False
    assert verify(installed, path, capsys)[0] == 1


@pytest.mark.parametrize("observation", ["host", "terminal"])
def test_missing_host_or_terminal_observation_is_not_verified(
    installed, capsys, monkeypatch, observation
):
    if observation == "host":
        monkeypatch.setattr("preflight_adapters.state._host_process_pid", lambda host, pids: None)
    else:
        monkeypatch.setattr("preflight_adapters.state._has_terminal", lambda pid: False)
    assert verify(installed, activation(installed), capsys)[0] == 1


@pytest.mark.parametrize(
    ("names", "expected"),
    [
        (["/bin/sh", "/usr/local/bin/codex"], 42),
        (["node", "codex"], None),
        (["python3", "codex"], None),
        (["claude"], None),
        (["not-codex"], None),
    ],
)
def test_host_provenance_does_not_accept_generic_process_names(monkeypatch, names, expected):
    from preflight_adapters.state import _host_process_pid

    pids = list(range(41, 41 + len(names)))
    commands = dict(zip(pids, names, strict=True))
    monkeypatch.setattr("preflight_adapters.state._process_command_name", commands.get)
    assert _host_process_pid("codex", pids) == expected
