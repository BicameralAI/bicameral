"""Regression tests for Google native OAuth deployment wiring on Cloud main."""

from __future__ import annotations

from pathlib import Path


WORKFLOW = Path(".github/workflows/deploy-cloud-run.yml")


def test_oauth_config_is_validated_before_cloud_mutation() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    target = workflow.index("- name: Resolve deploy target")
    gate = workflow.index("- name: Validate provider OAuth configuration before deployment")
    auth = workflow.index("- name: Authenticate to Google Cloud")
    migrate = workflow.index("- name: Apply manifest-bound Supabase migration set")
    deploy = workflow.index("- name: Build and deploy")
    traffic = workflow.index("gcloud run services update-traffic")

    assert target < gate < auth < migrate < deploy < traffic
    gate_block = workflow[gate:auth]
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_ID }}" in gate_block
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_ID_STAGING }}" in gate_block
    assert "refusing to create a Cloud Run revision" in gate_block


def test_runtime_receives_bounded_public_google_app_config() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    configure = workflow.index("- name: Configure runtime auth env")
    block = workflow[configure:]

    assert "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_ID }}" in block
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_ID_STAGING }}" in block
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CONFIG_VERSION }}" in block
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CONFIG_VERSION_STAGING }}" in block
    assert "BICAMERAL_GOOGLE_OAUTH_CLIENT_ID=${GOOGLE_OAUTH_CLIENT_ID}" in block
    assert "BICAMERAL_GOOGLE_OAUTH_ENVIRONMENT=${{ steps.target.outputs.env_name }}" in block
    assert "BICAMERAL_GOOGLE_OAUTH_CONFIG_VERSION=${GOOGLE_OAUTH_CONFIG_VERSION}" in block
    assert "BICAMERAL_GOOGLE_CLIENT_SECRET" not in workflow


def test_google_desktop_credential_is_secret_manager_mounted() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    target = workflow.index("- name: Resolve deploy target")
    configure = workflow.index("- name: Configure runtime auth env")
    target_block = workflow[target:configure]
    configure_block = workflow[configure:]

    assert (
        'google_oauth_client_secret=google-oauth-desktop-client-secret-prod"'
        in target_block
    )
    assert (
        'google_oauth_client_secret=google-oauth-desktop-client-secret-staging"'
        in target_block
    )
    assert (
        "BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET=${{ steps.target.outputs.google_oauth_client_secret }}:latest"
        in configure_block
    )
    assert "secrets.BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET" not in workflow
    assert "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET" not in workflow


def test_main_only_release_hardening_is_preserved() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "BOT_CANONICAL_SOURCE_PATH: /home/runner/work/bicameral-bot/bicameral-bot" in workflow
    assert "ref: 4b74e4dce208338e12629f37dea7dd48954b9ee0" in workflow
    assert "status.traffic[0].revisionName" in workflow
    assert "Promote traffic to the candidate revision" in workflow
