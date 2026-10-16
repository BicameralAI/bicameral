"""Clean, ordered validation of the Product directory migrations (cloud#127).

The hosted onboarding journey relies on the durable, service-role-only Product
directory. These checks validate the tracked Supabase migrations *in order*, as
they would apply to a clean database, without requiring a live Supabase project:

- the directory table is created before the join-eligibility migration alters
  it (a reordering or a missing table would fail here);
- the table is locked down (RLS enabled, anon/authenticated revoked) so only the
  server-side service role may read/write it;
- the join-eligibility migration only adds an ``enabled`` gate for ephemeral
  ticket issuance and introduces no membership, pending-approval, or durable
  join-lifecycle state.
"""

from __future__ import annotations

import re
from pathlib import Path

MIGRATIONS_DIR = (
    Path(__file__).resolve().parents[2] / "supabase" / "migrations"
)
DIRECTORY_TABLE = "public.product_directory"


def _ordered_migrations() -> list[tuple[str, str]]:
    files = sorted(MIGRATIONS_DIR.glob("*.sql"))
    assert files, "no migrations found"
    return [(f.name, f.read_text()) for f in files]


def _apply_order_creates_before_alters() -> tuple[int, int]:
    """Return (create_index, first_alter_index) across the ordered stream."""

    create_index = -1
    alter_index = -1
    for idx, (_name, sql) in enumerate(_ordered_migrations()):
        lowered = sql.lower()
        if create_index < 0 and re.search(
            r"create table if not exists\s+public\.product_directory", lowered
        ):
            create_index = idx
        if (
            alter_index < 0
            and "alter table" in lowered
            and "public.product_directory" in lowered
            and idx != create_index
        ):
            alter_index = idx
    return create_index, alter_index


def test_directory_table_created_before_it_is_altered() -> None:
    create_index, alter_index = _apply_order_creates_before_alters()
    assert create_index >= 0, "product_directory table is never created"
    if alter_index >= 0:
        assert create_index < alter_index, (
            "product_directory is altered before it is created: a clean-DB "
            "apply would fail"
        )


def test_directory_table_is_locked_down_to_service_role() -> None:
    sql = ""
    for _name, body in _ordered_migrations():
        if "create table if not exists public.product_directory" in body.lower():
            sql = body.lower()
            break
    assert sql, "product_directory creation migration not found"
    assert "canonical_product_id text not null unique" in sql
    assert "enable row level security" in sql
    assert "revoke all on public.product_directory from anon" in sql
    assert "revoke all on public.product_directory from authenticated" in sql


def _strip_comments(sql: str) -> str:
    return "\n".join(
        line.split("--", 1)[0] for line in sql.splitlines()
    ).lower()


def test_join_eligibility_only_adds_enabled_gate() -> None:
    join_sql = ""
    for name, body in _ordered_migrations():
        if "eligibility" in name.lower():
            join_sql = _strip_comments(body)
            break
    assert join_sql, "join-eligibility migration not found"
    assert "alter table" in join_sql and "public.product_directory" in join_sql
    assert "enabled boolean not null default true" in join_sql
    # Only a column add: no new tables or canonical membership/join lifecycle.
    assert "create table" not in join_sql
    for forbidden in (
        "membership",
        "pending",
        "join_request",
        "approval",
    ):
        assert forbidden not in join_sql, (
            f"join-eligibility migration must not introduce {forbidden!r} state"
        )
