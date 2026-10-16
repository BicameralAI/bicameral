"""Deployment-order guards for the Supabase migration and release-provenance gates."""

from pathlib import Path

WORKFLOW = (
    Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "deploy-cloud-run.yml"
)
CONFORMANCE_WORKFLOW = (
    Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "contract-conformance.yml"
)
MIGRATION_ATTESTATION_WORKFLOW = (
    Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "factory-attestation-migration-gate.yml"
)


def test_migrations_converge_before_cloud_run_deploy() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    compile_set = workflow.index("migration_delivery.py compile")
    migrate = workflow.index("- name: Apply manifest-bound Supabase migration set")
    deploy = workflow.index("- name: Build and deploy")

    assert compile_set < migrate < deploy
    assert "migration_delivery.py execute" in workflow
    assert "--database-url-env MIGRATION_DATABASE_URL" in workflow
    assert "--manifest release-cloud/release-manifest.json" in workflow
    assert "--migration-set release-cloud/database-migration-set.json" in workflow
    assert "npx --yes supabase@2.39.2" not in workflow
    assert 'if [ -n "${MIGRATION_DATABASE_URL}" ]; then' in workflow
    assert 'if [ -z "${MIGRATION_DATABASE_URL}" ]; then' not in workflow, (
        "the controller must emit the typed missing-secret receipt"
    )


def test_pull_request_ci_runs_the_deployment_gate_tests() -> None:
    workflow = CONFORMANCE_WORKFLOW.read_text(encoding="utf-8")
    normalized = " ".join(workflow.split())

    assert (
        "pytest -q tests/test_deploy_cloud_run_workflow.py "
        "tests/test_migration_report.py tests/test_migration_delivery.py"
    ) in normalized


def test_deployments_are_serialized_per_environment() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "group: deploy-cloud-run-${{ github.ref_name }}" in workflow
    assert "cancel-in-progress: false" in workflow


def test_deployment_is_explicit_and_requires_exact_staged_bot_commit() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "workflow_dispatch:" in workflow
    assert "\n  push:\n" not in workflow
    assert "bot_sha:" in workflow
    assert "required: true" in workflow

    resolve = workflow.index("- name: Resolve exact selected Bot commit")
    checkout = workflow.index("- name: Check out exact private Bot source")
    selector = workflow[resolve:checkout]
    assert 'bot_sha="${{ inputs.bot_sha }}"' in selector
    assert '[[ "${bot_sha}" =~ ^[0-9a-f]{40}$ ]]' in selector
    assert "version.json" not in selector

    assert "github.ref == 'refs/heads/main'" in workflow
    assert "github.ref == 'refs/heads/dev'" in workflow


def test_exact_staged_bot_bytes_are_materialized_before_manifest_and_image() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    reconstruct = workflow.index("- name: Reconstruct immutable Bot producer artifact")
    materialize = workflow.index("- name: Materialize exact staged Bot dashboard into Cloud")
    manifest = workflow.index("- name: Materialize exact release manifest and served identity")
    deploy = workflow.index("- name: Build and deploy")

    assert reconstruct < materialize < manifest < deploy
    materialization = workflow[materialize:manifest]
    assert 'scripts/vendor_dashboard.sh "${BOT_CANONICAL_SOURCE_PATH}/dashboard"' in materialization
    assert 'DASHBOARD_BOT_SHA="${BOT_SHA}"' in materialization
    assert 'DASHBOARD_ARTIFACT_SHA256="${artifact_sha}"' in materialization
    assert 'BOT_DESCRIPTOR_DIGEST="${descriptor_digest}"' in materialization
    assert "DASHBOARD_ALREADY_BUILT=1" in materialization
    assert 'json.load(open("src/bicameral_cloud/static/dashboard/version.json"))["bot_commit"]' in materialization
    assert 'test "${materialized_bot_sha}" = "${BOT_SHA}"' in materialization


def test_bot_is_reconstructed_at_its_canonical_build_path() -> None:
    """BicameralAI/bicameral-bot#1172.

    The Bot release binary embeds its absolute build path, so reconstructing it
    anywhere other than the directory the Bot's own producers build in yields
    daemon bytes that no Bot-side producer can reproduce, and the production
    byte-identity gate can then never pass for a correctly deployed commit.

    A controlled four-cell matrix on one runner VM (bicameral-bot run
    32998525832) established that the path is the sole causal variable: the
    same path rebuilt twice was byte-identical, shallow versus full checkout
    was byte-identical, and only relocating the tree changed the binary.
    """
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert (
        "BOT_CANONICAL_SOURCE_PATH: /home/runner/work/bicameral-bot/bicameral-bot"
        in workflow
    )

    checkout = workflow.index("- name: Check out exact private Bot source")
    place = workflow.index("- name: Place Bot source at its canonical build path")
    reconstruct = workflow.index("- name: Reconstruct immutable Bot producer artifact")
    assert checkout < place < reconstruct

    placement = workflow[place:reconstruct]
    assert 'cp -a _release/bot/. "${BOT_CANONICAL_SOURCE_PATH}/"' in placement
    assert 'git -C "${BOT_CANONICAL_SOURCE_PATH}" rev-parse HEAD' in placement
    assert 'test "${actual_sha}" = "${BOT_SHA}"' in placement

    materialize = workflow.index("- name: Materialize exact staged Bot dashboard into Cloud")
    build = workflow[reconstruct:materialize]
    assert 'pushd "${BOT_CANONICAL_SOURCE_PATH}"' in build
    assert "cargo build --release --locked --bin bicameral" in build

    # No release byte may be produced from the nested checkout path again.
    assert "pushd _release/bot" not in workflow
    assert "_release/bot/release-dashboard" not in workflow


def test_bot_reconstruction_toolchain_is_exactly_pinned() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "node-version: 22.23.2" in workflow
    assert "toolchain: 1.97.1" in workflow
    assert "node-version: 22\n" not in workflow
    assert "toolchain: stable" not in workflow


def test_migration_attempt_publishes_a_best_effort_slack_receipt() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    migrate = workflow.index("- name: Apply manifest-bound Supabase migration set")
    report = workflow.index("- name: Publish concise migration report")
    deploy = workflow.index("- name: Build and deploy")

    assert migrate < report < deploy
    assert "id: migrate" in workflow[migrate:report]
    assert "always() && steps.migrate.outcome != 'skipped'" in workflow[report:deploy]
    assert "continue-on-error: true" in workflow[report:deploy]
    assert "scripts/build_migration_report.py" in workflow[report:deploy]
    assert "GITHUB_STEP_SUMMARY" in workflow[report:deploy]
    assert "curl --fail-with-body" in workflow[report:deploy]


def test_exact_migration_execution_receipt_is_retained_even_on_failure() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    migrate = workflow.index("- name: Apply manifest-bound Supabase migration set")
    upload = workflow.index("- name: Upload migration execution receipt")
    deploy = workflow.index("- name: Build and deploy")

    assert migrate < upload < deploy
    assert "always() && steps.migrate.outcome != 'skipped'" in workflow[upload:deploy]
    assert (
        "release-cloud/database-migration-execution-receipt.json"
        in workflow[upload:deploy]
    )
    assert "retention-days: 90" in workflow[upload:deploy]


def test_migration_schemas_receive_exact_head_governance_validation() -> None:
    workflow = MIGRATION_ATTESTATION_WORKFLOW.read_text(encoding="utf-8")

    assert "FACTORY_REF: 456e4f6e22b4fcf8c1cabba1fe11d64303445988" in workflow
    assert "scripts/spec_governance.py validate" in workflow
    assert "--results-dir target/.bicameral/spec-governance" in workflow
    assert '--base-sha "${{ github.event.pull_request.base.sha }}"' in workflow
    assert '--head-sha "${{ github.event.pull_request.head.sha }}"' in workflow
    assert "--receipt-out /tmp/spec-governance-validation-receipt.json" in workflow
    assert "Retain spec-governance validation receipt" in workflow


def _extract_oauth_gate_script() -> str:
    """Return the pre-deploy OAuth validation run block as executable bash."""
    workflow = WORKFLOW.read_text(encoding="utf-8")
    start = workflow.index("- name: Validate provider OAuth configuration before deployment")
    end = workflow.index("- name: Authenticate to Google Cloud", start)
    step = workflow[start:end]
    run_body = step[step.index("run: |") + len("run: |"):]
    lines = [line[10:] for line in run_body.splitlines() if line.strip()]
    return "\n".join(lines)


def test_oauth_config_is_validated_before_any_deploy_or_traffic_mutation() -> None:
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

    configure = workflow.index("- name: Configure runtime auth env")
    assert "must be provisioned for the" in workflow[configure:]


def test_oauth_gate_script_fails_closed_for_missing_selected_client_id() -> None:
    import shutil
    import subprocess

    import pytest

    bash = shutil.which("bash")
    if bash is not None:
        probe = subprocess.run(
            [bash, "-c", "echo functional"], capture_output=True, text=True
        )
        if probe.returncode != 0 or "functional" not in probe.stdout:
            bash = None
    if bash is None:
        pytest.skip("functional bash unavailable on this host; gate script executes in CI")

    script = _extract_oauth_gate_script()

    def run(env_name: str, prod: str, staging: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [bash, "-c", script],
            env={
                "ENV_NAME": env_name,
                "PROD_GOOGLE_OAUTH_CLIENT_ID": prod,
                "STAGING_GOOGLE_OAUTH_CLIENT_ID": staging,
                "PATH": "/usr/bin:/bin",
            },
            capture_output=True,
            text=True,
        )

    missing_prod = run("production", "", "staging-id.apps.googleusercontent.com")
    assert missing_prod.returncode == 1
    assert "production" in missing_prod.stderr

    missing_staging = run("staging", "prod-id.apps.googleusercontent.com", "")
    assert missing_staging.returncode == 1
    assert "staging" in missing_staging.stderr

    assert run("production", "prod-id.apps.googleusercontent.com", "").returncode == 0
    assert run("staging", "", "staging-id.apps.googleusercontent.com").returncode == 0


def test_staging_served_identity_uses_fresh_explicit_id_token() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "- name: Authenticate for staging served identity" in workflow
    assert "id: served_identity_auth" in workflow
    assert "token_format: id_token" in workflow
    assert (
        "id_token_audience: ${{ steps.served_identity_target.outputs.base_url }}"
        in workflow
    )
    assert "STAGING_ID_TOKEN: ${{ steps.served_identity_auth.outputs.id_token }}" in workflow
    assert "SERVED_IDENTITY_AUTH_OUTCOME: ${{ steps.served_identity_auth.outcome }}" in workflow
    assert "gcloud auth print-identity-token" not in workflow


def test_placeholder_configuration_is_rejected_before_any_cloud_mutation():
    workflow = WORKFLOW.read_text(encoding="utf-8")

    target = workflow.index("- name: Resolve deploy target")
    reject = workflow.index("- name: Reject placeholder deployment configuration")
    oauth = workflow.index("- name: Validate provider OAuth configuration before deployment")
    auth = workflow.index("- name: Authenticate to Google Cloud")
    migrate = workflow.index("- name: Apply manifest-bound Supabase migration set")

    assert target < reject < oauth < auth < migrate
    step = workflow[reject:oauth]
    for placeholder in ('"..."', '"-"', '"TODO"', '"REPLACE_ME"'):
        assert placeholder in step
    for name in (
        "secrets.SUPABASE_URL_STAGING",
        "secrets.SUPABASE_ANON_KEY_STAGING",
        "secrets.SUPABASE_DB_URL_STAGING",
        "vars.BICAMERAL_GOOGLE_OAUTH_CLIENT_ID_STAGING",
        "vars.BICAMERAL_GOOGLE_OAUTH_CONFIG_VERSION_STAGING",
    ):
        assert name in step
    assert "postgres(ql)?://" in step
    assert ".apps.googleusercontent.com" in step

