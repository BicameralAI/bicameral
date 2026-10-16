"""Tracked-state derivation and repository-known hardening tests (cloud#141)."""

from __future__ import annotations

from pathlib import Path

from bicameral_cloud.evidence.tracked_state import derive_tracked_state

_REPO = Path(__file__).resolve().parents[2]
_MIGRATIONS = _REPO / "supabase" / "migrations"
_CONFIG = _REPO / "docs" / "evidence" / "expected-config.json"
_HARDENING = _MIGRATIONS / "202607150001_evidence_reconciliation_hardening.sql"


def test_tables_derived_from_migrations() -> None:
    state = derive_tracked_state(_MIGRATIONS, _CONFIG)
    assert "design_partner_allowlist" in state.tables
    assert "product_directory" in state.tables


def test_known_tables_expect_rls_enabled() -> None:
    state = derive_tracked_state(_MIGRATIONS, _CONFIG)
    assert state.tables["design_partner_allowlist"].rls_enabled is True
    assert state.tables["product_directory"].rls_enabled is True


def test_browser_principals_expected_no_select() -> None:
    # Repository-known fail-closed posture (principal fidelity).
    state = derive_tracked_state(_MIGRATIONS, _CONFIG)
    for table in state.tables.values():
        assert table.principal_select["anon"] is False
        assert table.principal_select["authenticated"] is False
        assert table.principal_select["service_role"] is True


def test_expected_config_pins_invoker_and_allows_hardened_ingress() -> None:
    state = derive_tracked_state(_MIGRATIONS, _CONFIG)
    assert state.cloud_run_public_invoker_expected is False
    assert "internal-and-cloud-load-balancing" in state.cloud_run_ingress_allowed


def test_hardening_migration_is_revoke_by_default() -> None:
    sql = _HARDENING.read_text(encoding="utf-8").lower()
    assert "revoke all on all tables in schema public from anon" in sql
    assert "revoke all on all tables in schema public from authenticated" in sql
    assert (
        "alter default privileges in schema public revoke all on tables from anon"
        in sql
    )
    assert "force row level security" in sql
