from __future__ import annotations

import importlib.util
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path
from urllib.parse import urlencode

import pytest

SCRIPT = Path(__file__).parents[1] / "scripts" / "google_provider_auth_evidence.py"
SPEC = importlib.util.spec_from_file_location("google_provider_auth_evidence", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

DAEMON_ORIGIN = "http://127.0.0.1:30123"
CLIENT_ID = "public-client.apps.googleusercontent.com"


def valid_payload() -> dict[str, object]:
    now = datetime.now(UTC)
    return {
        "schema_version": 1,
        "provider": "google",
        "client_type": "native",
        "client_id": CLIENT_ID,
        "environment": "production",
        "config_version": "google-prod-1",
        "scope_set_version": "drive-v1",
        "scopes": ["https://www.googleapis.com/auth/drive.file"],
        "issued_at": (now - timedelta(minutes=1)).isoformat().replace("+00:00", "Z"),
        "expires_at": (now + timedelta(minutes=30)).isoformat().replace("+00:00", "Z"),
        "enabled": True,
    }


DRIVE_FILE = "https://www.googleapis.com/auth/drive.file"


def valid_authorization_url(*, port: int = 30123, scope: str = DRIVE_FILE) -> str:
    query = urlencode(
        {
            "client_id": CLIENT_ID,
            "redirect_uri": (
                f"http://127.0.0.1:{port}/api/v1/connections/google/callback"
            ),
            "response_type": "code",
            "scope": scope,
            "state": "opaque-csrf-value",
            "code_challenge": "pkce-challenge-value",
            "code_challenge_method": "S256",
        }
    )
    return f"https://accounts.google.com/o/oauth2/v2/auth?{query}"


def inspect_valid_authorization_url() -> dict[str, object]:
    return MODULE.inspect_authorization_url(
        valid_authorization_url(),
        expected_client_id=CLIENT_ID,
        expected_daemon_origin=DAEMON_ORIGIN,
        expected_scopes=[DRIVE_FILE],
    )


def inspect_url_with_scopes(scope: str, expected: list[str]) -> dict[str, object]:
    return MODULE.inspect_authorization_url(
        valid_authorization_url(scope=scope),
        expected_client_id=CLIENT_ID,
        expected_daemon_origin=DAEMON_ORIGIN,
        expected_scopes=expected,
    )


def test_app_config_validation_returns_only_sanitized_evidence() -> None:
    evidence = MODULE.validate_app_config(
        valid_payload(),
        expected_environment="production",
    )
    assert evidence["provider"] == "google"
    assert evidence["environment"] == "production"
    assert str(evidence["client_id_sha256"]).startswith("sha256:")
    assert "client_id" not in evidence


def test_app_config_rejects_wrong_environment_and_expiry() -> None:
    wrong_environment = valid_payload()
    wrong_environment["environment"] = "staging"
    with pytest.raises(MODULE.EvidenceError, match="environment mismatch"):
        MODULE.validate_app_config(
            wrong_environment,
            expected_environment="production",
        )

    expired = valid_payload()
    expired["expires_at"] = (datetime.now(UTC) - timedelta(seconds=1)).isoformat()
    with pytest.raises(MODULE.EvidenceError, match="expired"):
        MODULE.validate_app_config(expired, expected_environment="production")


def test_app_config_rejects_credential_bearing_extension() -> None:
    payload = valid_payload()
    payload["client_secret"] = ""
    with pytest.raises(MODULE.EvidenceError, match="forbidden credential fields"):
        MODULE.validate_app_config(payload, expected_environment="production")


def test_fresh_install_rejects_injected_daemon_client_environment() -> None:
    MODULE.assert_no_injected_google_client_env({})
    with pytest.raises(MODULE.EvidenceError, match="forbids injected"):
        MODULE.assert_no_injected_google_client_env(
            {"BICAMERAL_GOOGLE_CLIENT_ID": "configured-locally"}
        )


def test_fresh_install_rejects_enabled_legacy_env_override() -> None:
    for enabling in ("1", "true", "yes", "TRUE", "Yes"):
        with pytest.raises(MODULE.EvidenceError, match="legacy env override"):
            MODULE.assert_legacy_env_override_disabled(
                {"BICAMERAL_GOOGLE_OAUTH_ALLOW_ENV_OVERRIDE": enabling}
            )


def test_fresh_install_accepts_absent_or_disabled_legacy_env_override() -> None:
    MODULE.assert_legacy_env_override_disabled({})
    # Mirrors the Bot resolver exactly: only lowercased "1"/"true"/"yes"
    # enable the override; untrimmed or other values do not.
    for benign in ("", "0", "false", "no", " true"):
        MODULE.assert_legacy_env_override_disabled(
            {"BICAMERAL_GOOGLE_OAUTH_ALLOW_ENV_OVERRIDE": benign}
        )


def test_start_outcome_uses_production_url_wire_field() -> None:
    assert (
        MODULE.extract_google_start_url(
            {
                "outcome": "start_url",
                "provider": "google",
                "url": "https://example.test",
            }
        )
        == "https://example.test"
    )
    with pytest.raises(MODULE.EvidenceError, match="did not contain url"):
        MODULE.extract_google_start_url(
            {
                "outcome": "start_url",
                "provider": "google",
                "authorization_url": "https://stale-shape.test",
            }
        )


def test_authorization_url_proves_pkce_loopback_without_retaining_state() -> None:
    evidence = inspect_valid_authorization_url()
    assert evidence["pkce_method"] == "S256"
    assert evidence["pkce_challenge_present"] is True
    assert evidence["state_present"] is True
    assert evidence["redirect_loopback"] is True
    assert evidence["redirect_port"] == 30123
    assert evidence["redirect_port_matches_daemon"] is True
    assert "state" not in evidence
    assert "code_challenge" not in evidence


def test_authorization_url_rejects_redirect_port_drift() -> None:
    with pytest.raises(MODULE.EvidenceError, match="port does not match daemon origin"):
        MODULE.inspect_authorization_url(
            valid_authorization_url(port=3000),
            expected_client_id=CLIENT_ID,
            expected_daemon_origin=DAEMON_ORIGIN,
            expected_scopes=[DRIVE_FILE],
        )


def test_authorization_url_rejects_verifier_or_wrong_client() -> None:
    with_verifier = valid_authorization_url() + "&code_verifier=should-not-travel"
    with pytest.raises(MODULE.EvidenceError, match="forbidden credential parameters"):
        MODULE.inspect_authorization_url(
            with_verifier,
            expected_client_id=CLIENT_ID,
            expected_daemon_origin=DAEMON_ORIGIN,
            expected_scopes=[DRIVE_FILE],
        )

    with pytest.raises(MODULE.EvidenceError, match="client_id does not match"):
        MODULE.inspect_authorization_url(
            valid_authorization_url(),
            expected_client_id="different.apps.googleusercontent.com",
            expected_daemon_origin=DAEMON_ORIGIN,
            expected_scopes=[DRIVE_FILE],
        )


# ── #283 exact scope-set binding ──────────────────────────────────


def test_scope_set_exact_equality_matrix() -> None:
    other = "https://www.googleapis.com/auth/userinfo.email"

    with pytest.raises(MODULE.EvidenceError, match="does not equal"):
        inspect_url_with_scopes(other, [DRIVE_FILE])

    with pytest.raises(MODULE.EvidenceError, match="does not equal"):
        inspect_url_with_scopes(f"{DRIVE_FILE} {other}", [DRIVE_FILE])

    with pytest.raises(MODULE.EvidenceError, match="does not equal"):
        inspect_url_with_scopes(DRIVE_FILE, [DRIVE_FILE, other])

    with pytest.raises(MODULE.EvidenceError, match="duplicate"):
        inspect_url_with_scopes(f"{DRIVE_FILE} {DRIVE_FILE}", [DRIVE_FILE])

    with pytest.raises(MODULE.EvidenceError, match="blank"):
        inspect_url_with_scopes(f"{DRIVE_FILE}  {other}", [DRIVE_FILE, other])

    evidence = inspect_url_with_scopes(f"{other} {DRIVE_FILE}", [DRIVE_FILE, other])
    assert evidence["scope_set_matches_cloud"] is True
    assert evidence["scopes_sorted"] == sorted([DRIVE_FILE, other])
    assert str(evidence["scope_set_sha256"]).startswith("sha256:")
    assert evidence["scope_count"] == 2


def test_app_config_scope_policy_is_explicit_least_privilege() -> None:
    evidence = MODULE.validate_app_config(
        valid_payload(), expected_environment="production"
    )
    assert evidence["scopes_sorted"] == [DRIVE_FILE]
    assert evidence["drive_file_least_privilege"] is True
    assert str(evidence["scope_set_sha256"]).startswith("sha256:")

    broad = valid_payload()
    broad["scopes"] = [DRIVE_FILE, "https://www.googleapis.com/auth/drive"]
    with pytest.raises(MODULE.EvidenceError, match="least-privilege"):
        MODULE.validate_app_config(broad, expected_environment="production")

    missing = valid_payload()
    missing["scopes"] = ["https://www.googleapis.com/auth/userinfo.email"]
    with pytest.raises(MODULE.EvidenceError, match="drive.file"):
        MODULE.validate_app_config(missing, expected_environment="production")

    duplicated = valid_payload()
    duplicated["scopes"] = [DRIVE_FILE, DRIVE_FILE]
    with pytest.raises(MODULE.EvidenceError, match="duplicates"):
        MODULE.validate_app_config(duplicated, expected_environment="production")


def test_preflight_rejects_token_exchange_mock_selector() -> None:
    MODULE.assert_no_token_exchange_mock({})
    for value in ("", "1", "true", "anything"):
        with pytest.raises(MODULE.EvidenceError, match="token-exchange"):
            MODULE.assert_no_token_exchange_mock(
                {"BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK": value}
            )


def test_bootstrap_receipt_cannot_claim_terminal_provider_acceptance() -> None:
    app_config = MODULE.validate_app_config(
        valid_payload(),
        expected_environment="production",
    )
    authorization = inspect_valid_authorization_url()
    receipt = MODULE.build_bootstrap_receipt(
        cloud_origin="https://app.bicameral-ai.com",
        daemon_origin=DAEMON_ORIGIN,
        expected_environment="production",
        cloud_commit="a" * 40,
        bot_commit="b" * 40,
        app_config_evidence=app_config,
        authorization_evidence=authorization,
    )
    assert receipt["terminal_acceptance"] is False
    terminal = receipt["provider_terminal"]
    assert isinstance(terminal, dict)
    assert not any(terminal.values())
    assert "daemon_restart_observed" not in terminal
    assert "refresh_observed" not in terminal
    fresh_install = receipt["fresh_install"]
    assert isinstance(fresh_install, dict)
    assert fresh_install["legacy_env_override_disabled"] is True
    MODULE.assert_receipt_safe(receipt)


def test_receipt_safety_rejects_sensitive_keys_recursively() -> None:
    with pytest.raises(MODULE.EvidenceError, match="forbidden key"):
        MODULE.assert_receipt_safe({"nested": {"access_token": ""}})


def terminal_observation() -> dict[str, object]:
    session_id = "devin-" + "1" * 32
    return {
        "schema_version": 1,
        "request_id": "sha256:" + "2" * 64,
        "attempt_id": "sha256:" + "3" * 64,
        "attempt_ordinal": 1,
        "topology_policy_digest": "sha256:" + "4" * 64,
        "provider_session_id": session_id,
        "journey_contract_digest": "sha256:" + "5" * 64,
        "host_root_pid": 100,
        "daemon_pid": 120,
        "trace_digest": "sha256:" + "6" * 64,
        "trace_uri": (
            "https://app.devin.ai/sessions/" + "1" * 32 + "?artifact=runtime-trace"
        ),
        "recording_url": (
            "https://app.devin.ai/sessions/" + "1" * 32 + "?testRecording=rec-1"
        ),
        "slack_file_id": "F012ABCDEF0",
        "slack_permalink": (
            "https://bicameralgroup.slack.com/archives/C0B3QR6EWP5/p1784892959859009"
        ),
        "message_sha256": "sha256:" + "7" * 64,
        "source_issue_url": "https://github.com/BicameralAI/bicameral-bot/issues/1132",
        "source_pr_url": "https://github.com/BicameralAI/bicameral-bot/pull/1136",
        "finished_at": "2026-08-22T12:00:00Z",
        "passed_checkpoints": list(MODULE.TERMINAL_CHECK_IDS),
        "cleanup": {
            "processes_stopped": True,
            "supabase_stopped": True,
            "ports_closed": True,
            "temporary_root_removed": True,
            "credentials_removed_with_temporary_root": True,
            "containers_remaining": [],
        },
        "sanitization": {
            "browser_payloads_scanned": True,
            "process_logs_scanned": True,
            "raw_credentials_recorded": False,
            "host_paths_recorded": False,
        },
    }


def terminal_proposal() -> dict[str, object]:
    return {
        "release_unit_id": "v0128-google-provider-auth-secure-completion",
        "proposal_digest": "sha256:" + "8" * 64,
        "topology": {
            "topology_type": "provider_auth_google",
            "validation_checklist": [
                {"check_id": check_id, "label": check_id}
                for check_id in MODULE.TERMINAL_CHECK_IDS
            ],
        },
    }


def terminal_bootstrap() -> dict[str, object]:
    return {
        "schema_version": 1,
        "evidence_type": "google_provider_auth",
        "cloud_commit": "a" * 40,
        "bot_commit": "b" * 40,
        "terminal_acceptance": False,
    }


def test_direct_attempt_claim_is_closed_and_has_no_self_referential_url() -> None:
    claim = MODULE.build_topology_attempt_claim(
        terminal_observation(),
        release_unit_id="v0128-google-provider-auth-secure-completion",
    )
    assert claim["schema_version"] == 3
    assert claim["authorization_kind"] == "topology-attempt-v1"
    assert "claim_receipt_url" not in claim
    MODULE.assert_receipt_safe(claim)


def test_terminal_candidate_covers_exact_release_checklist_and_attempt() -> None:
    assert "restart-refresh-envelope" not in MODULE.TERMINAL_CHECK_IDS
    candidate = MODULE.build_terminal_candidate(
        bootstrap=terminal_bootstrap(),
        observation=terminal_observation(),
        proposal=terminal_proposal(),
        claim_receipt_url=(
            "https://github.com/BicameralAI/bicameral-cloud/"
            "issues/263#issuecomment-1234567890"
        ),
    )
    assert candidate["status"] == "passed"
    assert candidate["terminal_acceptance"] is True
    assert candidate["terminal_outcome"] == "passed"
    assert candidate["execution_authority"]["attempt_id"] == "sha256:" + "3" * 64
    checks = candidate["release_unit"]["validation_checklist"]
    assert [item["check_id"] for item in checks] == list(MODULE.TERMINAL_CHECK_IDS)
    assert all(item["outcome"] == "passed" for item in checks)
    assert len(candidate["runtime_evidence"]["check_observations"]) == len(
        MODULE.TERMINAL_CHECK_IDS
    )
    terminal = candidate["provider_terminal"]
    assert "daemon_restart_observed" not in terminal
    assert "refresh_observed" not in terminal
    MODULE.assert_receipt_safe(candidate)


def test_terminal_candidate_rejects_stale_v0128_restart_requirement() -> None:
    proposal = terminal_proposal()
    checklist = proposal["topology"]["validation_checklist"]
    assert isinstance(checklist, list)
    checklist.insert(
        5,
        {
            "check_id": "restart-refresh-envelope",
            "label": "Restart and refresh envelope",
        },
    )
    with pytest.raises(
        MODULE.EvidenceError,
        match="deferred daemon restart continuity",
    ):
        MODULE.build_terminal_candidate(
            bootstrap=terminal_bootstrap(),
            observation=terminal_observation(),
            proposal=proposal,
            claim_receipt_url=(
                "https://github.com/BicameralAI/bicameral-cloud/"
                "issues/263#issuecomment-1234567890"
            ),
        )


def test_terminal_candidate_rejects_incomplete_or_reordered_checkpoints() -> None:
    observation = terminal_observation()
    observation["passed_checkpoints"] = list(MODULE.TERMINAL_CHECK_IDS[:-1])
    with pytest.raises(MODULE.EvidenceError, match="checkpoints"):
        MODULE.build_terminal_candidate(
            bootstrap=terminal_bootstrap(),
            observation=observation,
            proposal=terminal_proposal(),
            claim_receipt_url=(
                "https://github.com/BicameralAI/bicameral-cloud/"
                "issues/263#issuecomment-1234567890"
            ),
        )
