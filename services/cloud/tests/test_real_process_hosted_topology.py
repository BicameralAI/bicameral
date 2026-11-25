"""Focused tests for the real-process topology receipt helpers."""

from __future__ import annotations

import importlib.util
import json
import sys
import zipfile
from pathlib import Path
from types import ModuleType, SimpleNamespace

import pytest

SCRIPT = Path(__file__).parents[1] / "scripts" / "run_real_process_hosted_topology.py"


def _load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("real_process_topology", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_allocated_ports_are_unique_loopback_ports() -> None:
    module = _load_script()
    ports = module._allocate_ports(("a", "b", "c"))
    assert set(ports) == {"a", "b", "c"}
    assert len(set(ports.values())) == 3
    assert all(0 < port < 65536 for port in ports.values())


def test_tree_digest_is_stable_and_path_sensitive(tmp_path: Path) -> None:
    module = _load_script()
    first = tmp_path / "first"
    second = tmp_path / "second"
    first.mkdir()
    second.mkdir()
    (first / "asset.js").write_text("same", encoding="utf-8")
    (second / "asset.js").write_text("same", encoding="utf-8")
    assert module._tree_digest(first) == module._tree_digest(second)
    (second / "nested").mkdir()
    (second / "nested" / "asset.js").write_text("same", encoding="utf-8")
    assert module._tree_digest(first) != module._tree_digest(second)


def test_vendored_dashboard_rejects_a_different_bot_commit(tmp_path: Path) -> None:
    module = _load_script()
    cloud = tmp_path / "cloud"
    bot = tmp_path / "bot"
    runtime = tmp_path / "runtime"
    dashboard = cloud / "src" / "bicameral_cloud" / "static" / "dashboard"
    contracts = cloud / "src" / "bicameral_cloud" / "contracts"
    bot_dashboard = bot / "dashboard" / "dist"
    bot_contracts = bot / "protocol" / "hosted-bridge"
    dashboard.mkdir(parents=True)
    contracts.mkdir(parents=True)
    bot_dashboard.mkdir(parents=True)
    bot_contracts.mkdir(parents=True)
    (dashboard / "index.html").write_text("same", encoding="utf-8")
    (bot_dashboard / "index.html").write_text("same", encoding="utf-8")
    bundle = module._tree_digest(dashboard, exclude=frozenset({"version.json"}))
    (dashboard / "version.json").write_text(
        json.dumps({"bot_commit": "a" * 40, "bundle_sha256": bundle}),
        encoding="utf-8",
    )
    contract = b"{}"
    (contracts / "hosted_bridge_contract.json").write_bytes(contract)
    (bot_contracts / "contract.json").write_bytes(contract)
    digest = module._sha256_bytes(contract)
    (bot_contracts / "contract.sha256").write_text(f"{digest}  contract.json\n")

    with pytest.raises(module.TopologyError, match="exact Bot commit"):
        module._prepare_cloud_runtime(
            cloud_root=cloud,
            bot_root=bot,
            runtime_root=runtime,
            bot_commit="b" * 40,
            use_vendored_dashboard=True,
        )


@pytest.mark.parametrize(
    "unsafe",
    [
        {"failure": {"message": "workspace_path leaked"}},
        {"generated_resources": {"root": "/home/user/topology"}},
        {"credential_lifecycle": {"service_role_key": "redacted"}},
        {"processes": {"daemon": {"event_store": "local"}}},
    ],
)
def test_receipt_rejects_local_or_secret_fields(unsafe: dict[str, object]) -> None:
    module = _load_script()
    with pytest.raises(module.TopologyError):
        module._assert_receipt_safe(unsafe)


def test_receipt_accepts_sanitized_resource_metadata() -> None:
    module = _load_script()
    module._assert_receipt_safe(
        {
            "schema_version": 1,
            "ports": {"cloud": 41231},
            "generated_resources": {
                "temporary_homes": 2,
                "temporary_git_repositories": 2,
            },
            "credential_lifecycle": {
                "supabase_generated": True,
                "persisted_in_receipt": False,
            },
        }
    )


def test_terminal_journey_has_only_two_controlled_identities() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    assert "OUTSIDER_DOMAIN" not in source
    assert "outsider_token" not in source


def test_terminal_acceptance_state_waits_for_human_review() -> None:
    module = _load_script()

    assert module._terminal_acceptance_state(None) == {
        "evidence_level": "browser lifecycle acceptance candidate",
        "terminal_evidence_complete": True,
        "human_product_review": "pending",
        "terminal_user_journey_acceptance": False,
    }
    assert module._terminal_acceptance_state("membership_projection") == {
        "evidence_level": "real-process integration; supporting evidence only",
        "terminal_evidence_complete": False,
        "human_product_review": "not_ready",
        "terminal_user_journey_acceptance": False,
    }


def test_redacted_network_capture_omits_query_headers_and_bodies(
    tmp_path: Path,
) -> None:
    module = _load_script()
    capture = module.RedactedNetworkCapture()
    response = SimpleNamespace(
        request=SimpleNamespace(
            method="POST",
            resource_type="fetch",
            headers={"authorization": "Bearer secret"},
            post_data="secret body",
        ),
        url="https://app.bicameral-ai.com/api/v1/products?token=secret#fragment",
        status=201,
        headers={"set-cookie": "secret"},
    )

    capture.record_response(response)
    destination = tmp_path / "network.json"
    capture.write(destination)

    payload = json.loads(destination.read_text(encoding="utf-8"))
    assert payload == {
        "schema_version": 1,
        "redaction": "method, origin, path, resource type, and status only",
        "responses": [
            {
                "method": "POST",
                "origin": "https://app.bicameral-ai.com",
                "path": "/api/v1/products",
                "resource_type": "fetch",
                "status": 201,
            }
        ],
    }
    serialized = json.dumps(payload)
    assert "secret" not in serialized
    assert "authorization" not in serialized


def test_retain_process_logs_copies_only_scanned_content(tmp_path: Path) -> None:
    module = _load_script()
    source = tmp_path / "source"
    destination = tmp_path / "retained"
    source.mkdir()
    (source / "cloud.log").write_text("cloud ready\n", encoding="utf-8")

    retained = module._retain_process_logs(
        source,
        destination,
        secret_values=["ephemeral-secret"],
        work_root=tmp_path / "disposable",
    )

    assert retained == ["cloud.log"]
    assert (destination / "cloud.log").read_text(encoding="utf-8") == "cloud ready\n"

    (source / "daemon.log").write_text("ephemeral-secret\n", encoding="utf-8")
    with pytest.raises(module.TopologyError, match="ephemeral credential"):
        module._retain_process_logs(
            source,
            destination,
            secret_values=["ephemeral-secret"],
            work_root=tmp_path / "disposable",
        )


def test_playwright_trace_archive_is_redacted_before_retention(
    tmp_path: Path,
) -> None:
    module = _load_script()
    source = tmp_path / "trace.raw.zip"
    destination = tmp_path / "trace.zip"
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr(
            "trace.network",
            "Authorization: Bearer ephemeral-secret\n"
            '{"session_token":"ephemeral-secret",'
            '"source":"/tmp/disposable/workspace"}',
        )

    module._redact_trace_archive(
        source,
        destination,
        ["ephemeral-secret"],
        [Path("/tmp/disposable")],
    )

    with zipfile.ZipFile(destination) as archive:
        retained = archive.read("trace.network")
    assert b"ephemeral-secret" not in retained
    assert b"<redacted>" in retained
    assert b"/tmp/disposable" not in retained
    assert b"<redacted-path>" in retained


def test_product_join_trust_is_scoped_to_exact_product(tmp_path: Path) -> None:
    module = _load_script()
    config = tmp_path / "config.yaml"
    config.write_text("version: 1\n", encoding="utf-8")

    digest = module._set_product_join_trust(
        config,
        product_id="product-alpha",
        signing={
            "issuer": "https://app.bicameral-ai.com",
            "key_id": "key-alpha",
            "public_key_b64": "cHVibGljLWtleQ==",
        },
    )

    content = config.read_text(encoding="utf-8")
    assert "audience: daemon:product-alpha" in content
    assert "key_id: key-alpha" in content
    assert digest == module._sha256_file(config)
    with pytest.raises(module.TopologyError):
        module._set_product_join_trust(
            config,
            product_id="product-beta",
            signing={
                "issuer": "https://app.bicameral-ai.com",
                "key_id": "key-beta",
                "public_key_b64": "cHVibGljLWtleQ==",
            },
        )


def test_event_type_count_reads_durable_git_events(tmp_path: Path) -> None:
    module = _load_script()
    events = tmp_path / ".bicameral" / "events"
    events.mkdir(parents=True)
    (events / "one.yaml").write_text(
        "event_type: product_member_added\n", encoding="utf-8"
    )
    (events / "two.yaml").write_text("event_type: create_product\n", encoding="utf-8")

    assert module._event_type_count(tmp_path, "product_member_added") == 1
    assert module._event_type_count(tmp_path, "create_product") == 1


def _pairing_responses(module, decision_message: str):
    return iter(
        (
            module.HttpResult(
                202,
                b'{"status":"pending","request_handle":"handle-1",'
                b'"poll_proof":"proof-1"}',
                {},
            ),
            module.HttpResult(
                200,
                b'{"status":"pending","local_confirmation_nonce":"local-nonce-1"}',
                {},
            ),
            module.HttpResult(
                200,
                json.dumps(
                    {"status": "approved", "message": decision_message}
                ).encode(),
                {},
            ),
            module.HttpResult(
                200,
                b'{"status":"approved","session_token":"session-1"}',
                {},
            ),
        )
    )


def test_pair_daemon_sends_team_and_token_and_requires_cloud_enrollment(
    monkeypatch,
) -> None:
    module = _load_script()
    requests: list[tuple[str, str, dict[str, object]]] = []
    responses = _pairing_responses(
        module, "Pairing approved (pairing_id=p-1). Enrollment: enrolled:created."
    )

    def fake_http(method: str, url: str, **kwargs: object):
        requests.append((method, url, kwargs))
        return next(responses)

    monkeypatch.setattr(module, "_http", fake_http)

    token, evidence = module._pair_daemon(
        37373,
        "account-1",
        team_id="team-1",
        cloud_access_token="cloud-token-1",
    )

    assert token == "session-1"
    assert evidence == b'{"status":"approved","session_token":"session-1"}'
    assert [request[0] for request in requests] == ["POST", "GET", "POST", "GET"]
    assert requests[0][2]["payload"] == {
        "origin": module.TRUSTED_BRIDGE_ORIGIN,
        "cloud_account_id": "account-1",
        "csrf_token": requests[0][2]["payload"]["csrf_token"],
        "client_protocol_version": module.HOSTED_BRIDGE_PROTOCOL_VERSION,
        "team_id": "team-1",
        "cloud_access_token": "cloud-token-1",
    }
    assert requests[1][1].endswith("/api/v1/hosted-bridge/pair/pending/handle-1")
    assert requests[2][2]["payload"] == {
        "request_handle": "handle-1",
        "action": "approve",
        "local_confirmation_nonce": "local-nonce-1",
    }
    assert "request_handle=handle-1" in requests[3][1]
    assert "poll_proof=proof-1" in requests[3][1]
    assert "cloud_account_id=account-1" in requests[3][1]
    assert requests[3][2]["headers"] == {"origin": module.TRUSTED_BRIDGE_ORIGIN}


def test_pair_daemon_fails_closed_when_cloud_enrollment_fails(monkeypatch) -> None:
    module = _load_script()
    responses = _pairing_responses(
        module,
        "Pairing approved (pairing_id=p-1). Enrollment failed closed: "
        "proof signing key unavailable: keychain not found.",
    )
    monkeypatch.setattr(module, "_http", lambda *_args, **_kwargs: next(responses))

    with pytest.raises(
        module.TopologyError, match="proof signing key unavailable"
    ) as failure:
        module._pair_daemon(
            37373,
            "account-1",
            team_id="team-1",
            cloud_access_token="cloud-token-1",
        )
    assert "cloud-token-1" not in str(failure.value)


def test_pairing_without_team_or_token_is_refused(monkeypatch) -> None:
    module = _load_script()
    sent: list[dict[str, object]] = []
    kinds = {
        "team_id": "MissingTeamId",
        "cloud_access_token": "MissingCloudAccessToken",
    }

    def fake_http(method: str, url: str, **kwargs: object):
        payload = kwargs["payload"]
        sent.append(payload)
        missing = next(field for field in kinds if field not in payload)
        return module.HttpResult(400, json.dumps({"kind": kinds[missing]}).encode(), {})

    monkeypatch.setattr(module, "_http", fake_http)

    evidence = module._check_pairing_requires_team(
        37373,
        "account-1",
        team_id="team-1",
        cloud_access_token="cloud-token-1",
    )

    assert evidence == {
        "without_team": {"status": 400, "kind": "MissingTeamId"},
        "without_cloud_credential": {
            "status": 400,
            "kind": "MissingCloudAccessToken",
        },
    }
    assert "team_id" not in sent[0] and sent[0]["cloud_access_token"]
    assert "cloud_access_token" not in sent[1] and sent[1]["team_id"]
    assert "cloud-token-1" not in json.dumps(evidence)
    module._assert_receipt_safe({"pairing_contract": evidence})


def test_pairing_accepted_without_team_fails_closed(monkeypatch) -> None:
    module = _load_script()
    monkeypatch.setattr(
        module,
        "_http",
        lambda *_args, **_kwargs: module.HttpResult(
            202, b'{"status":"pending","request_handle":"h"}', {}
        ),
    )

    with pytest.raises(module.TopologyError, match="without team_id"):
        module._check_pairing_requires_team(
            37373,
            "account-1",
            team_id="team-1",
            cloud_access_token="cloud-token-1",
        )


def test_pairing_team_comes_from_cloud_tenancy(monkeypatch) -> None:
    module = _load_script()
    calls: list[tuple[str, str, dict[str, object]]] = []

    def fake_http(method: str, url: str, **kwargs: object):
        calls.append((method, url, kwargs))
        return module.HttpResult(
            200, b'{"user_id":"u-1","team_id":"team-9","source":"created"}', {}
        )

    monkeypatch.setattr(module, "_http", fake_http)

    team = module._resolve_pairing_team("http://127.0.0.1:4000", "owner-token")

    assert team == "team-9"
    assert calls[0][0] == "POST"
    assert calls[0][1] == "http://127.0.0.1:4000/api/v1/auth/pairing-tenancy"
    assert calls[0][2]["headers"]["authorization"] == "Bearer owner-token"


def test_pair_daemon_records_enrollment_it_cannot_run(monkeypatch) -> None:
    module = _load_script()
    responses = _pairing_responses(
        module, "Pairing approved (pairing_id=p-1). Enrollment failed closed: tls."
    )
    monkeypatch.setattr(module, "_http", lambda *_args, **_kwargs: next(responses))

    token, _evidence = module._pair_daemon(
        37373,
        "account-1",
        team_id="team-1",
        cloud_access_token="cloud-token-1",
        require_enrollment=False,
    )

    assert token == "session-1"


def test_unresolved_pairing_team_fails_closed(monkeypatch) -> None:
    module = _load_script()
    monkeypatch.setattr(
        module,
        "_http",
        lambda *_args, **_kwargs: module.HttpResult(
            409, b'{"detail":{"kind":"PairingVerifiedDomainRequired"}}', {}
        ),
    )

    with pytest.raises(module.TopologyError, match="tenancy") as failure:
        module._resolve_pairing_team("http://127.0.0.1:4000", "owner-token")
    assert "owner-token" not in str(failure.value)


def test_daemon_enrolls_against_the_disposable_cloud(monkeypatch, tmp_path) -> None:
    module = _load_script()
    started: list[dict[str, str]] = []

    def fake_start(name, command, *, cwd, env, log_dir):
        started.append(env)
        return SimpleNamespace(name=name)

    monkeypatch.setattr(module, "_start_process", fake_start)
    monkeypatch.setattr(module, "_wait_http", lambda *_args, **_kwargs: None)

    module._start_daemon(
        binary=tmp_path / "bicameral",
        workspace=tmp_path,
        home=tmp_path / "home",
        name="daemon-a",
        port=37373,
        log_dir=tmp_path,
        cloud_origin="http://127.0.0.1:4000",
    )

    assert started[0]["BICAMERAL_CLOUD_ORIGIN"] == "http://127.0.0.1:4000"


def _git(cwd: Path, *arguments: str) -> str:
    import subprocess

    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *arguments],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def test_components_come_from_one_monorepo_commit(tmp_path: Path) -> None:
    module = _load_script()
    for component in ("apps/bot", "packages/mcp", "services/cloud"):
        (tmp_path / component).mkdir(parents=True)
        (tmp_path / component / "README.md").write_text("x\n", encoding="utf-8")
    _git(tmp_path, "init", "-q", "-b", "main")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-q", "-m", "init")
    head = _git(tmp_path, "rev-parse", "HEAD")

    components = module._monorepo_components(tmp_path / "services" / "cloud")

    assert components.bot == tmp_path / "apps" / "bot"
    assert components.mcp == tmp_path / "packages" / "mcp"
    assert components.commits == {"cloud": head, "bot": head, "mcp": head}

    (tmp_path / "apps" / "bot" / "README.md").write_text("changed\n", encoding="utf-8")
    with pytest.raises(module.TopologyError, match="uncommitted"):
        module._monorepo_components(tmp_path / "services" / "cloud")


def test_browser_pairing_approves_the_pending_request_locally(monkeypatch) -> None:
    module = _load_script()
    approvals: list[tuple[int, str]] = []
    interactions: list[str] = []

    class PairResponse:
        request = SimpleNamespace(method="POST")
        url = "http://localhost:37373/api/v1/hosted-bridge/pair"
        status = 202

        @staticmethod
        def json() -> dict[str, str]:
            return {"status": "pending", "request_handle": "browser-handle"}

    class ExpectedResponse:
        def __init__(self, predicate):
            self.predicate = predicate
            self.value = PairResponse()

        def __enter__(self):
            assert self.predicate(self.value)
            return self

        def __exit__(self, *_args: object) -> None:
            return None

    class Page:
        def add_init_script(self, *_args: object, **_kwargs: object) -> None:
            return None

        def route(self, *_args: object, **_kwargs: object) -> None:
            return None

        def goto(self, *_args: object, **_kwargs: object) -> None:
            interactions.append("goto")
            return None

        def wait_for_selector(self, *_args: object, **_kwargs: object) -> None:
            return None

        def locator(self, *_args: object, **_kwargs: object):
            return self

        def click(self) -> None:
            interactions.append("legacy-click")
            return None

        def expect_response(self, predicate):
            return ExpectedResponse(predicate)

        def wait_for_url(self, *_args: object, **_kwargs: object) -> None:
            return None

    page = Page()
    context = SimpleNamespace(new_page=lambda: page)
    monkeypatch.setattr(
        module,
        "_approve_daemon_pairing",
        lambda port, handle: approvals.append((port, handle)),
    )

    opened = module._open_paired_browser_page(
        context,
        access_token="token",
        hosted_origin="https://app.bicameral-ai.com:8443",
        daemon_port=37373,
    )

    assert opened is page
    assert approvals == [(37373, "browser-handle")]
    assert interactions == ["goto"]


def test_wait_for_selector_preserves_the_boundary_label() -> None:
    module = _load_script()

    class Page:
        @staticmethod
        def wait_for_selector(*_args: object, **_kwargs: object) -> None:
            raise RuntimeError("playwright details")

    with pytest.raises(
        module.TopologyError,
        match="hosted SPA paired shell timed out",
    ):
        module._wait_for_selector(
            Page(),
            '[data-testid="paired-shell"]',
            timeout=20_000,
            label="hosted SPA paired shell",
        )


def test_receipt_log_label_handles_relative_and_external_paths(tmp_path: Path) -> None:
    module = _load_script()
    cloud_root = tmp_path / "cloud"
    cloud_root.mkdir()

    assert module._receipt_log_label(
        Path("test-artifacts/receipt.json"), cloud_root
    ) == Path("test-artifacts/receipt.json")
    assert module._receipt_log_label(
        tmp_path / "outside" / "receipt.json", cloud_root
    ) == Path("receipt.json")


def test_terminal_surface_drift_fails_closed(tmp_path: Path) -> None:
    module = _load_script()
    assets = tmp_path / "assets"
    assets.mkdir()
    (assets / "index.js").write_text(
        "\n".join(module.TERMINAL_ACCEPTANCE_SURFACE), encoding="utf-8"
    )

    module._validate_terminal_acceptance_surface(tmp_path)
    (assets / "index.js").write_text('data-testid="create-product"', encoding="utf-8")
    with pytest.raises(module.TopologyError, match="drifted"):
        module._validate_terminal_acceptance_surface(tmp_path)


def test_acceptance_surface_covers_daemon_disconnect_recovery() -> None:
    module = _load_script()
    for token in (
        'data-testid="daemon-disconnect-scrim"',
        "Local daemon disconnected",
        "/api/v1/hosted-bridge/projects",
    ):
        assert token in module.TERMINAL_ACCEPTANCE_SURFACE


def test_acceptance_surface_does_not_depend_on_recovery_button_text(
    tmp_path: Path,
) -> None:
    module = _load_script()
    assets = tmp_path / "assets"
    assets.mkdir()
    surface = "\n".join(module.TERMINAL_ACCEPTANCE_SURFACE)
    for label in ("Go to pairing", "Reconnect device", "Pair again"):
        (assets / "index.js").write_text(
            f"{surface}\n<button>{label}</button>", encoding="utf-8"
        )
        module._validate_terminal_acceptance_surface(tmp_path)
    assert all(
        label not in token
        for token in module.TERMINAL_ACCEPTANCE_SURFACE
        for label in ("Go to pairing", "Reconnect device")
    )


def test_terminal_surface_drift_detects_missing_disconnect_dialog(
    tmp_path: Path,
) -> None:
    module = _load_script()
    assets = tmp_path / "assets"
    assets.mkdir()
    without_dialog = [
        token
        for token in module.TERMINAL_ACCEPTANCE_SURFACE
        if token != "Local daemon disconnected"
    ]
    (assets / "index.js").write_text("\n".join(without_dialog), encoding="utf-8")
    with pytest.raises(module.TopologyError, match="Local daemon disconnected"):
        module._validate_terminal_acceptance_surface(tmp_path)


def test_public_repo_does_not_carry_terminal_or_attestation_workflows() -> None:
    root = Path(__file__).parents[1] / ".github" / "workflows"
    assert not (root / "terminal-hosted-product-journey.yml").exists()
    assert not (root / "factory-attestation.yml").exists()


def test_factory_attestation_workflow_targets_this_validation_run() -> None:
    factory_ref = "0d972754750c45d73aae4bf3606af248034e8dbf"
    attestation = (
        Path(__file__).parents[1]
        / ".bicameral"
        / "factory-attestations"
        / f"{factory_ref}.validation-terminal-hosted-v0.1.10.json"
    )
    assert attestation.is_file()
    payload = json.loads(attestation.read_text(encoding="utf-8"))
    assert payload["fixture_kind"] == "synthetic-test-placeholder"
    assert payload["validation_run"] == "validation-terminal-hosted-v0.1.10"
    assert not (
        {
            "loaded_context",
            "context_evidence",
            "local_setup_divergence",
            "roadmap_reconciliation",
            "state_refinement",
            "end_of_session_capture",
            "attested_by",
            "agent",
            "session",
        }
        & payload.keys()
    )
