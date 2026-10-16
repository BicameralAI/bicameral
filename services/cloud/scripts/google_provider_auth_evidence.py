#!/usr/bin/env python3
"""Collect sanitized Google provider-auth bootstrap evidence.

This is the first stage of cloud#263. It proves the daemon is using the
Cloud-owned public OAuth application configuration and producing a PKCE-bound
Google authorization request without injected daemon client credentials.

It deliberately cannot mark terminal provider acceptance true. A terminal
receipt additionally requires a real Google authorization, daemon-owned token
custody, provider-backed Drive readiness, governed cache/negative-path evidence,
and sanitized teardown. Ordinary daemon-process restart continuity is v0.1.29
scope and must not be manufactured by this v0.1.28 evidence path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from collections.abc import Mapping, Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

import httpx

SCHEMA_VERSION = 1
APP_CONFIG_PATH = "/api/v1/provider-app-config/google"
CONNECTION_START_PATH = "/api/v1/connections/google/start"

INJECTED_CLIENT_ENV = (
    "BICAMERAL_GOOGLE_CLIENT_ID",
    "BICAMERAL_GOOGLE_CLIENT_SECRET",
    "GOOGLE_CLIENT_ID",
    "GOOGLE_CLIENT_SECRET",
)
LEGACY_ENV_OVERRIDE_VAR = "BICAMERAL_GOOGLE_OAUTH_ALLOW_ENV_OVERRIDE"
# Mirrors the Bot resolver's exact enabling semantics (google_app_config.rs
# legacy_env_override_enabled): lowercased value, no trimming.
LEGACY_ENV_OVERRIDE_ENABLING_VALUES = ("1", "true", "yes")
# The historical Bot mock selector enabled synthetic token exchange on mere
# PRESENCE (std::env::var(...).is_ok()), so even an empty value must fail
# preflight. The switch is removed from Bot by bicameral-bot#1132; this
# preflight also protects runs against older daemon builds.
TOKEN_EXCHANGE_MOCK_VAR = "BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK"
# Cloud-owned least-privilege policy: drive.file must be present and no
# broader Drive scope may appear (#283).
DRIVE_FILE_SCOPE = "https://www.googleapis.com/auth/drive.file"
FORBIDDEN_BROAD_DRIVE_SCOPES = {
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/drive.metadata",
    "https://www.googleapis.com/auth/drive.metadata.readonly",
    "https://www.googleapis.com/auth/drive.appdata",
    "https://www.googleapis.com/auth/drive.scripts",
    "https://www.googleapis.com/auth/drive.photos.readonly",
}
FORBIDDEN_APP_CONFIG_FIELDS = {
    "client_secret",
    "authorization_code",
    "code_verifier",
    "access_token",
    "refresh_token",
}
FORBIDDEN_RECEIPT_KEYS = {
    "authorization_url",
    "authorization_code",
    "code",
    "code_verifier",
    "client_secret",
    "access_token",
    "refresh_token",
    "state",
    "state_token",
}
FORBIDDEN_AUTH_QUERY_KEYS = {
    "client_secret",
    "code_verifier",
    "access_token",
    "refresh_token",
}
LOOPBACK_HOSTS = {"127.0.0.1", "localhost"}
# v0.1.28 terminal evidence follows the current #263 owner boundary. Ordinary
# daemon-process restart/rehydration is deferred to v0.1.29 and is therefore
# intentionally absent. The older Release Unit proposal still contains
# restart-refresh-envelope; build_terminal_candidate fails closed on that stale
# checklist until the governed proposal is reconciled.
TERMINAL_CHECK_IDS = (
    "fresh-operator-config-bootstrap",
    "pkce-loopback-authorization",
    "daemon-only-credential-custody",
    "connection-registry-provider-truth",
    "drive-readiness-real-operation",
    "cloud-outage-cache-policy",
    "config-negative-paths",
    "sanitized-teardown",
)
DEFERRED_V0128_CHECK_IDS = frozenset({"restart-refresh-envelope"})
DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
SHA = re.compile(r"^[a-f0-9]{40}$")
DEVIN_SESSION = re.compile(r"^devin-[a-f0-9]{32}$")
COMMENT_URL = re.compile(
    r"^https://github\.com/BicameralAI/[^/]+/issues/[1-9][0-9]*"
    r"#issuecomment-[1-9][0-9]*$"
)


class EvidenceError(RuntimeError):
    """The observed provider-auth surface violates the evidence contract."""


def _parse_timestamp(value: object, field: str) -> datetime:
    if not isinstance(value, str) or not value:
        raise EvidenceError(f"{field} must be a non-empty RFC3339 timestamp")
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise EvidenceError(f"{field} is not a valid RFC3339 timestamp") from exc
    if parsed.tzinfo is None:
        raise EvidenceError(f"{field} must be timezone-aware")
    return parsed.astimezone(UTC)


def _sha256_text(value: str) -> str:
    return "sha256:" + hashlib.sha256(value.encode("utf-8")).hexdigest()


def assert_no_injected_google_client_env(env: Mapping[str, str]) -> None:
    """Reject the old daemon-injected application credential path."""

    present = sorted(name for name in INJECTED_CLIENT_ENV if env.get(name, "").strip())
    if present:
        raise EvidenceError(
            "fresh-install evidence forbids injected Google client environment: "
            + ", ".join(present)
        )


def assert_legacy_env_override_disabled(env: Mapping[str, str]) -> None:
    """Reject a daemon environment that re-enables the legacy client-env path."""

    value = env.get(LEGACY_ENV_OVERRIDE_VAR, "")
    if value.lower() in LEGACY_ENV_OVERRIDE_ENABLING_VALUES:
        raise EvidenceError(
            "fresh-install evidence forbids the enabled legacy env override: "
            + LEGACY_ENV_OVERRIDE_VAR
        )


def assert_no_token_exchange_mock(env: Mapping[str, str]) -> None:
    """Reject any environment carrying a synthetic token-exchange selector.

    Presence semantics, not truthiness: the historical Bot switch enabled
    the mock for ANY value, including the empty string (#283 /
    bicameral-bot#1132).
    """

    if TOKEN_EXCHANGE_MOCK_VAR in env:
        raise EvidenceError(
            "provider-auth evidence forbids a synthetic Google token-exchange "
            "selector in the environment: " + TOKEN_EXCHANGE_MOCK_VAR
        )


def _scope_set_evidence(scopes: Sequence[str]) -> tuple[list[str], str]:
    """Deterministic non-secret representation of a scope set."""

    scopes_sorted = sorted(scopes)
    digest = _sha256_text(json.dumps(scopes_sorted, separators=(",", ":")))
    return scopes_sorted, digest


def validate_app_config(
    payload: Mapping[str, object],
    *,
    expected_environment: str,
    now: datetime | None = None,
) -> dict[str, object]:
    """Validate the live Cloud app-config payload without retaining credentials."""

    unexpected_sensitive = sorted(FORBIDDEN_APP_CONFIG_FIELDS.intersection(payload))
    if unexpected_sensitive:
        raise EvidenceError(
            "Cloud app-config contains forbidden credential fields: "
            + ", ".join(unexpected_sensitive)
        )

    required = {
        "schema_version",
        "provider",
        "client_type",
        "client_id",
        "environment",
        "config_version",
        "scope_set_version",
        "scopes",
        "issued_at",
        "expires_at",
        "enabled",
    }
    missing = sorted(required.difference(payload))
    if missing:
        raise EvidenceError("Cloud app-config is missing fields: " + ", ".join(missing))

    if payload["schema_version"] != SCHEMA_VERSION:
        raise EvidenceError("unexpected Google app-config schema_version")
    if payload["provider"] != "google":
        raise EvidenceError("Google app-config provider mismatch")
    if payload["client_type"] != "native":
        raise EvidenceError("Google app-config must use the native/public-client path")
    if payload["environment"] != expected_environment:
        raise EvidenceError("Google app-config environment mismatch")
    if payload["enabled"] is not True:
        raise EvidenceError("Google app-config is not enabled")

    client_id = payload["client_id"]
    config_version = payload["config_version"]
    scope_set_version = payload["scope_set_version"]
    scopes = payload["scopes"]
    if not isinstance(client_id, str) or not client_id.strip():
        raise EvidenceError("Google app-config client_id is empty")
    if not isinstance(config_version, str) or not config_version.strip():
        raise EvidenceError("Google app-config config_version is empty")
    if not isinstance(scope_set_version, str) or not scope_set_version.strip():
        raise EvidenceError("Google app-config scope_set_version is empty")
    if (
        not isinstance(scopes, Sequence)
        or isinstance(scopes, (str, bytes))
        or not scopes
        or any(not isinstance(scope, str) or not scope.strip() for scope in scopes)
    ):
        raise EvidenceError("Google app-config scopes must be non-empty strings")
    scope_list = [str(scope) for scope in scopes]
    if len(set(scope_list)) != len(scope_list):
        raise EvidenceError("Google app-config scopes must not contain duplicates")
    if DRIVE_FILE_SCOPE not in scope_list:
        raise EvidenceError(
            "Google app-config scopes must include the drive.file least-privilege scope"
        )
    broad = sorted(FORBIDDEN_BROAD_DRIVE_SCOPES.intersection(scope_list))
    if broad:
        raise EvidenceError(
            "Google app-config scopes exceed the drive.file least-privilege policy: "
            + ", ".join(broad)
        )

    issued_at = _parse_timestamp(payload["issued_at"], "issued_at")
    expires_at = _parse_timestamp(payload["expires_at"], "expires_at")
    observed_at = (now or datetime.now(UTC)).astimezone(UTC)
    if expires_at <= issued_at:
        raise EvidenceError("Google app-config expires_at must follow issued_at")
    if expires_at <= observed_at:
        raise EvidenceError("Google app-config is expired")

    scopes_sorted, scope_set_sha256 = _scope_set_evidence(scope_list)
    return {
        "schema_version": SCHEMA_VERSION,
        "provider": "google",
        "client_type": "native",
        "environment": expected_environment,
        "config_version": config_version,
        "scope_set_version": scope_set_version,
        "scope_count": len(scope_list),
        "scopes_sorted": scopes_sorted,
        "scope_set_sha256": scope_set_sha256,
        "drive_file_least_privilege": True,
        "client_id_sha256": _sha256_text(client_id),
        "issued_at": issued_at.isoformat().replace("+00:00", "Z"),
        "expires_at": expires_at.isoformat().replace("+00:00", "Z"),
        "enabled": True,
    }


def extract_google_start_url(start_obj: Mapping[str, object]) -> str:
    """Read the production ConnectionStartOutcome wire shape without retaining it."""

    if start_obj.get("outcome") != "start_url" or start_obj.get("provider") != "google":
        raise EvidenceError("daemon did not return a Google start_url outcome")
    value = start_obj.get("url")
    if not isinstance(value, str) or not value:
        raise EvidenceError("daemon Google start_url outcome did not contain url")
    return value


def inspect_authorization_url(
    authorization_url: str,
    *,
    expected_client_id: str,
    expected_daemon_origin: str,
    expected_scopes: Sequence[str],
) -> dict[str, object]:
    """Inspect Google authorization parameters and retain only non-secret evidence."""

    daemon = urlparse(expected_daemon_origin)
    if (
        daemon.scheme != "http"
        or daemon.hostname not in LOOPBACK_HOSTS
        or daemon.port is None
    ):
        raise EvidenceError(
            "expected daemon origin must be loopback HTTP with explicit port"
        )

    parsed = urlparse(authorization_url)
    if parsed.scheme != "https" or parsed.netloc != "accounts.google.com":
        raise EvidenceError("authorization URL is not the Google HTTPS endpoint")
    query = parse_qs(parsed.query, keep_blank_values=True)

    leaked = sorted(FORBIDDEN_AUTH_QUERY_KEYS.intersection(query))
    if leaked:
        raise EvidenceError(
            "authorization URL contains forbidden credential parameters: "
            + ", ".join(leaked)
        )

    client_ids = query.get("client_id", [])
    if client_ids != [expected_client_id]:
        raise EvidenceError(
            "authorization URL client_id does not match Cloud app-config"
        )
    states = query.get("state", [])
    challenges = query.get("code_challenge", [])
    methods = query.get("code_challenge_method", [])
    redirect_uris = query.get("redirect_uri", [])
    scopes = query.get("scope", [])
    if len(states) != 1 or not states[0]:
        raise EvidenceError("authorization URL must carry one opaque state value")
    if len(challenges) != 1 or not challenges[0]:
        raise EvidenceError("authorization URL must carry one PKCE challenge")
    if methods != ["S256"]:
        raise EvidenceError("authorization URL must use PKCE S256")
    if len(redirect_uris) != 1:
        raise EvidenceError("authorization URL must carry one redirect_uri")
    redirect = urlparse(redirect_uris[0])
    if redirect.scheme != "http" or redirect.hostname not in LOOPBACK_HOSTS:
        raise EvidenceError(
            "Google native OAuth redirect_uri must be local loopback HTTP"
        )
    if redirect.port != daemon.port:
        raise EvidenceError("Google redirect_uri port does not match daemon origin")
    if redirect.path != "/api/v1/connections/google/callback":
        raise EvidenceError("Google redirect_uri does not target the daemon callback")
    if len(scopes) != 1 or not scopes[0].strip():
        raise EvidenceError("authorization URL must carry the approved Google scopes")

    requested = scopes[0].split(" ")
    if any(not scope for scope in requested):
        raise EvidenceError("authorization URL scope list contains blank entries")
    if len(set(requested)) != len(requested):
        raise EvidenceError("authorization URL scope list contains duplicate scopes")
    if not expected_scopes:
        raise EvidenceError("expected Cloud scope set must be non-empty")
    if sorted(requested) != sorted(expected_scopes):
        raise EvidenceError(
            "authorization URL scope set does not equal the Cloud-published scope set"
        )
    scopes_sorted, scope_set_sha256 = _scope_set_evidence(requested)

    return {
        "authorization_host": parsed.netloc,
        "pkce_method": "S256",
        "pkce_challenge_present": True,
        "state_present": True,
        "redirect_loopback": True,
        "redirect_port": redirect.port,
        "redirect_port_matches_daemon": True,
        "scope_count": len(requested),
        "scopes_sorted": scopes_sorted,
        "scope_set_sha256": scope_set_sha256,
        "scope_set_matches_cloud": True,
        "client_id_sha256": _sha256_text(expected_client_id),
        "forbidden_query_keys_absent": True,
    }


def assert_receipt_safe(value: object) -> None:
    """Reject receipt shapes that could persist provider credentials or CSRF
    material.
    """

    if isinstance(value, Mapping):
        for key, nested in value.items():
            if not isinstance(key, str):
                raise EvidenceError("receipt keys must be strings")
            if key.lower() in FORBIDDEN_RECEIPT_KEYS:
                raise EvidenceError(f"receipt contains forbidden key: {key}")
            assert_receipt_safe(nested)
    elif isinstance(value, Sequence) and not isinstance(value, (str, bytes)):
        for nested in value:
            assert_receipt_safe(nested)


def build_bootstrap_receipt(
    *,
    cloud_origin: str,
    daemon_origin: str,
    expected_environment: str,
    cloud_commit: str,
    bot_commit: str,
    app_config_evidence: Mapping[str, object],
    authorization_evidence: Mapping[str, object],
) -> dict[str, object]:
    """Build a non-terminal receipt for the bootstrap/PKCE portion of #263."""

    receipt: dict[str, object] = {
        "schema_version": 1,
        "evidence_type": "google_provider_auth",
        "observed_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "cloud_commit": cloud_commit,
        "bot_commit": bot_commit,
        "expected_environment": expected_environment,
        "cloud_origin": cloud_origin.rstrip("/"),
        "daemon_origin": daemon_origin.rstrip("/"),
        "fresh_install": {
            "injected_google_client_env_absent": True,
            "legacy_env_override_disabled": True,
            "token_exchange_mock_absent": True,
        },
        "cloud_app_config": dict(app_config_evidence),
        "authorization_request": dict(authorization_evidence),
        "provider_terminal": {
            "real_google_authorization_observed": False,
            "connection_registry_connected": False,
            "drive_root_verified": False,
        },
        "terminal_acceptance": False,
    }
    assert_receipt_safe(receipt)
    return receipt


def collect_bootstrap_evidence(
    *,
    cloud_origin: str,
    daemon_origin: str,
    expected_environment: str,
    cloud_commit: str,
    bot_commit: str,
    env: Mapping[str, str],
) -> dict[str, object]:
    """Collect live Cloud bootstrap and daemon authorization-request evidence."""

    assert_no_injected_google_client_env(env)
    assert_legacy_env_override_disabled(env)
    assert_no_token_exchange_mock(env)
    with httpx.Client(timeout=10.0, follow_redirects=False) as client:
        app_response = client.get(cloud_origin.rstrip("/") + APP_CONFIG_PATH)
        app_response.raise_for_status()
        payload_obj: Any = app_response.json()
        if not isinstance(payload_obj, dict):
            raise EvidenceError("Cloud app-config response must be a JSON object")
        app_config = validate_app_config(
            payload_obj,
            expected_environment=expected_environment,
        )
        client_id_obj = payload_obj.get("client_id")
        if not isinstance(client_id_obj, str):
            raise EvidenceError("Cloud app-config client_id is not a string")
        scopes_obj = payload_obj.get("scopes")
        if not isinstance(scopes_obj, list) or not all(
            isinstance(scope, str) for scope in scopes_obj
        ):
            raise EvidenceError("Cloud app-config scopes are not a string list")

        start_response = client.post(
            daemon_origin.rstrip("/") + CONNECTION_START_PATH,
            json={},
        )
        start_response.raise_for_status()
        start_obj: Any = start_response.json()
        if not isinstance(start_obj, dict):
            raise EvidenceError(
                "daemon connection-start response must be a JSON object"
            )
        authorization_url = extract_google_start_url(start_obj)
        authorization = inspect_authorization_url(
            authorization_url,
            expected_client_id=client_id_obj,
            expected_daemon_origin=daemon_origin,
            expected_scopes=scopes_obj,
        )

    return build_bootstrap_receipt(
        cloud_origin=cloud_origin,
        daemon_origin=daemon_origin,
        expected_environment=expected_environment,
        cloud_commit=cloud_commit,
        bot_commit=bot_commit,
        app_config_evidence=app_config,
        authorization_evidence=authorization,
    )


def _attempt_authority(observation: Mapping[str, object]) -> dict[str, object]:
    authority = {
        "authorization_kind": "topology-attempt-v1",
        "provider": "devin",
        "execution_path": "devin-session-direct",
        "request_id": observation.get("request_id"),
        "attempt_id": observation.get("attempt_id"),
        "attempt_ordinal": observation.get("attempt_ordinal"),
        "topology_policy_digest": observation.get("topology_policy_digest"),
        "provider_session_id": observation.get("provider_session_id"),
    }
    if (
        not DIGEST.fullmatch(str(authority["request_id"]))
        or not DIGEST.fullmatch(str(authority["attempt_id"]))
        or not isinstance(authority["attempt_ordinal"], int)
        or int(authority["attempt_ordinal"]) <= 0
        or not DIGEST.fullmatch(str(authority["topology_policy_digest"]))
        or not DEVIN_SESSION.fullmatch(str(authority["provider_session_id"]))
    ):
        raise EvidenceError(
            "terminal observation has invalid topology attempt authority"
        )
    return authority


def build_topology_attempt_claim(
    observation: Mapping[str, object], *, release_unit_id: str
) -> dict[str, object]:
    """Build the claim body posted before its eventual GitHub URL is known."""

    if not release_unit_id:
        raise EvidenceError("release_unit_id is required")
    claim = {
        "schema_version": 3,
        **_attempt_authority(observation),
        "release_unit_id": release_unit_id,
    }
    assert_receipt_safe(claim)
    return claim


def _terminal_observation(
    observation: Mapping[str, object],
) -> dict[str, object]:
    required = {
        "schema_version",
        "request_id",
        "attempt_id",
        "attempt_ordinal",
        "topology_policy_digest",
        "provider_session_id",
        "journey_contract_digest",
        "host_root_pid",
        "daemon_pid",
        "trace_digest",
        "trace_uri",
        "recording_url",
        "slack_file_id",
        "slack_permalink",
        "message_sha256",
        "source_issue_url",
        "source_pr_url",
        "finished_at",
        "passed_checkpoints",
        "cleanup",
        "sanitization",
    }
    if set(observation) != required or observation.get("schema_version") != 1:
        raise EvidenceError("terminal observation fields are not closed")
    _attempt_authority(observation)
    if observation.get("passed_checkpoints") != list(TERMINAL_CHECK_IDS):
        raise EvidenceError(
            "terminal observation checkpoints are incomplete or reordered"
        )
    if not DIGEST.fullmatch(str(observation.get("journey_contract_digest"))):
        raise EvidenceError("terminal observation journey digest is invalid")
    if not DIGEST.fullmatch(
        str(observation.get("trace_digest"))
    ) or not DIGEST.fullmatch(str(observation.get("message_sha256"))):
        raise EvidenceError("terminal observation evidence digest is invalid")
    for field in ("host_root_pid", "daemon_pid"):
        value = observation.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            raise EvidenceError(f"terminal observation {field} is invalid")
    session = str(observation["provider_session_id"]).removeprefix("devin-")
    run_url = f"https://app.devin.ai/sessions/{session}"
    if not str(observation.get("trace_uri", "")).startswith(run_url + "?"):
        raise EvidenceError("terminal observation trace does not bind Devin session")
    recording_url = str(observation.get("recording_url", ""))
    if (
        not recording_url.startswith(run_url + "?")
        or "testRecording=" not in recording_url
    ):
        raise EvidenceError(
            "terminal observation recording does not bind Devin session"
        )
    if not re.fullmatch(r"F[A-Z0-9]{8,}", str(observation.get("slack_file_id"))):
        raise EvidenceError("terminal observation Slack file id is invalid")
    if not re.fullmatch(
        r"https://bicameralgroup\.slack\.com/archives/C0B3QR6EWP5/p[0-9]+",
        str(observation.get("slack_permalink")),
    ):
        raise EvidenceError("terminal observation Slack permalink is invalid")
    _parse_timestamp(observation.get("finished_at"), "finished_at")
    cleanup = observation.get("cleanup")
    if (
        not isinstance(cleanup, Mapping)
        or any(
            cleanup.get(field) is not True
            for field in (
                "processes_stopped",
                "supabase_stopped",
                "ports_closed",
                "temporary_root_removed",
                "credentials_removed_with_temporary_root",
            )
        )
        or cleanup.get("containers_remaining") != []
    ):
        raise EvidenceError("terminal observation teardown is incomplete")
    sanitization = observation.get("sanitization")
    if not isinstance(sanitization, Mapping) or (
        sanitization.get("browser_payloads_scanned") is not True
        or sanitization.get("process_logs_scanned") is not True
        or sanitization.get("raw_credentials_recorded") is not False
        or sanitization.get("host_paths_recorded") is not False
    ):
        raise EvidenceError("terminal observation sanitization is incomplete")
    return dict(observation)


def build_terminal_candidate(
    *,
    bootstrap: Mapping[str, object],
    observation: Mapping[str, object],
    proposal: Mapping[str, object],
    claim_receipt_url: str,
) -> dict[str, object]:
    """Compose terminal Direct-Devin evidence from bounded observations."""

    observed = _terminal_observation(observation)
    if not COMMENT_URL.fullmatch(claim_receipt_url):
        raise EvidenceError("claim receipt URL is invalid")
    if bootstrap.get("terminal_acceptance") is not False:
        raise EvidenceError("bootstrap receipt must remain non-terminal")
    cloud_commit = bootstrap.get("cloud_commit")
    bot_commit = bootstrap.get("bot_commit")
    if not SHA.fullmatch(str(cloud_commit)) or not SHA.fullmatch(str(bot_commit)):
        raise EvidenceError("bootstrap receipt does not bind exact component commits")
    topology = proposal.get("topology")
    if not isinstance(topology, Mapping):
        raise EvidenceError("proposal topology is invalid")
    checklist = topology.get("validation_checklist")
    if not isinstance(checklist, list):
        raise EvidenceError("proposal validation checklist is invalid")
    proposal_check_ids = [
        item.get("check_id") for item in checklist if isinstance(item, Mapping)
    ]
    deferred = sorted(DEFERRED_V0128_CHECK_IDS.intersection(proposal_check_ids))
    if deferred:
        raise EvidenceError(
            "v0.1.28 proposal still requires deferred daemon restart continuity: "
            + ", ".join(deferred)
            + "; reconcile the governed Release Unit before terminal acceptance"
        )
    if (
        proposal_check_ids != list(TERMINAL_CHECK_IDS)
        or proposal.get("release_unit_id")
        != "v0128-google-provider-auth-secure-completion"
        or not DIGEST.fullmatch(str(proposal.get("proposal_digest")))
        or topology.get("topology_type") != "provider_auth_google"
    ):
        raise EvidenceError(
            "proposal does not match the terminal provider-auth contract"
        )

    authority = {
        **_attempt_authority(observed),
        "claim_receipt_url": claim_receipt_url,
    }
    finished = _parse_timestamp(observed["finished_at"], "finished_at")
    video_attempt_id = (
        finished.strftime("%Y%m%dT%H%M%SZ-")
        + str(observed["attempt_id"]).removeprefix("sha256:")[:8]
    )
    trace_digest = str(observed["trace_digest"])
    cleanup = observed["cleanup"]
    sanitization = observed["sanitization"]
    assert isinstance(cleanup, Mapping)
    assert isinstance(sanitization, Mapping)
    runtime_evidence = {
        "schema_version": 1,
        "runtime_path": "authentic-runtime",
        "operator_class": "synthetic-operator",
        "initiation": "normal-user-flow",
        "direct_invocation": False,
        "process_provenance": {
            "method": "kernel-exec+proc-ancestry",
            "host_root_pid": observed["host_root_pid"],
            "observed_process_pid": observed["daemon_pid"],
            "ancestry_digest": trace_digest,
        },
        "artifacts": [
            {
                "kind": "raw-process-trace",
                "digest": trace_digest,
                "uri": observed["trace_uri"],
            }
        ],
        "check_observations": [
            {
                "check_id": check_id,
                "observation_kind": "runtime-outcome",
                "observed_count": 1,
                "artifact_digests": [trace_digest],
            }
            for check_id in TERMINAL_CHECK_IDS
        ],
    }
    candidate: dict[str, object] = {
        "schema_version": 2,
        "status": "passed",
        "evidence_level": "trusted-controller-runtime-evidence-v2",
        "terminal_evidence_complete": True,
        "terminal_user_journey_acceptance": True,
        "terminal_acceptance": True,
        "terminal_outcome": "passed",
        "components": {
            "BicameralAI/bicameral-bot": bot_commit,
            "BicameralAI/bicameral-cloud": cloud_commit,
        },
        "release_unit": {
            "release_unit_id": proposal["release_unit_id"],
            "topology_type": topology["topology_type"],
            "proposal_digest": proposal["proposal_digest"],
            "validation_checklist": [
                {"check_id": check_id, "outcome": "passed"}
                for check_id in TERMINAL_CHECK_IDS
            ],
        },
        "execution_authority": authority,
        "runtime_evidence": runtime_evidence,
        "video_evidence": {
            "attempt_id": video_attempt_id,
            "topology_class": "provider-auth-google-v1",
            "journey_id": "provider_auth_google_v1",
            "journey_contract_digest": observed["journey_contract_digest"],
            "capture_mode": "headed-live-journey",
            "result": "passed",
            "validated_checkpoints": list(TERMINAL_CHECK_IDS),
            "gap_checkpoints": [],
            "recording_url": observed["recording_url"],
            "delivery_mode": "slack-upload",
            "slack_file_id": observed["slack_file_id"],
            "slack_channel_id": "C0B3QR6EWP5",
            "slack_permalink": observed["slack_permalink"],
            "message_format": "topology-footage-v1",
            "message_sha256": observed["message_sha256"],
            "source_issue_url": observed["source_issue_url"],
            "source_pr_url": observed["source_pr_url"],
        },
        "provider_terminal": {
            "real_google_authorization_observed": True,
            "connection_registry_connected": True,
            "drive_root_verified": True,
        },
        "cleanup": dict(cleanup),
        "sanitization": dict(sanitization),
        "finished_at": observed["finished_at"],
    }
    assert_receipt_safe(candidate)
    return candidate


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode", choices=("bootstrap", "claim", "terminal"), default="bootstrap"
    )
    parser.add_argument("--cloud-origin")
    parser.add_argument("--daemon-origin")
    parser.add_argument("--expected-environment")
    parser.add_argument("--cloud-commit")
    parser.add_argument("--bot-commit")
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--bootstrap-receipt", type=Path)
    parser.add_argument("--observation", type=Path)
    parser.add_argument("--proposal", type=Path)
    parser.add_argument("--claim-receipt-url")
    parser.add_argument("--claim-output", type=Path)
    parser.add_argument("--candidate-output", type=Path)
    return parser.parse_args()


def _required(args: argparse.Namespace, *names: str) -> None:
    missing = [name for name in names if getattr(args, name) is None]
    if missing:
        labels = ", ".join(name.replace("_", "-") for name in missing)
        raise EvidenceError(f"{args.mode} mode requires: {labels}")


def _load_object(path: Path, *, label: str) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be one JSON object")
    return value


def main() -> int:
    args = _parse_args()
    if args.mode == "claim":
        _required(args, "observation", "proposal", "claim_output")
        observation = _load_object(args.observation, label="observation")
        proposal = _load_object(args.proposal, label="proposal")
        release_unit_id = proposal.get("release_unit_id")
        if not isinstance(release_unit_id, str):
            raise EvidenceError("proposal release_unit_id is invalid")
        claim = build_topology_attempt_claim(
            observation, release_unit_id=release_unit_id
        )
        args.claim_output.write_text(
            json.dumps(claim, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(args.claim_output)
        return 0
    if args.mode == "terminal":
        _required(
            args,
            "bootstrap_receipt",
            "observation",
            "proposal",
            "claim_receipt_url",
            "candidate_output",
        )
        candidate = build_terminal_candidate(
            bootstrap=_load_object(args.bootstrap_receipt, label="bootstrap receipt"),
            observation=_load_object(args.observation, label="observation"),
            proposal=_load_object(args.proposal, label="proposal"),
            claim_receipt_url=args.claim_receipt_url,
        )
        args.candidate_output.write_text(
            json.dumps(candidate, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(args.candidate_output)
        return 0

    _required(
        args,
        "cloud_origin",
        "daemon_origin",
        "expected_environment",
        "cloud_commit",
        "bot_commit",
        "receipt",
    )
    receipt = collect_bootstrap_evidence(
        cloud_origin=args.cloud_origin,
        daemon_origin=args.daemon_origin,
        expected_environment=args.expected_environment,
        cloud_commit=args.cloud_commit,
        bot_commit=args.bot_commit,
        env=os.environ,
    )
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(args.receipt)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
