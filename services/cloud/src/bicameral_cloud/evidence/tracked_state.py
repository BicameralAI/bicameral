"""Derive the repository's *tracked* expectations for reconciliation.

The tracked side of the comparison has two honest sources:

* **Supabase/Postgres** expectations are derived directly from the checked-in
  ``supabase/migrations/*.sql`` — the real source of truth for tables, RLS
  enablement, and the revoke-by-default principal posture.
* **Cloud Run / IAM** expectations come from a small reviewed descriptor
  (``docs/evidence/expected-config.json``) because the repository does not itself
  hold live GCP configuration. Every field there is explicitly pinned or ``null``
  (unpinned), so the comparator never invents an expectation the repo has not made.

An expectation of ``None`` means "the repository does not pin this", which the
comparator reports as an observation to reconcile — never as agreement.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

from bicameral_cloud.evidence import jsonutil

# Runtime principals whose read/write posture the repo pins fail-closed.
BROWSER_PRINCIPALS = ("anon", "authenticated")
SERVICE_PRINCIPAL = "service_role"

_CREATE_TABLE_RE = re.compile(
    r"create\s+table\s+(?:if\s+not\s+exists\s+)?public\.([a-z_][a-z0-9_]*)",
    re.IGNORECASE,
)
_ENABLE_RLS_RE = re.compile(
    r"alter\s+table\s+public\.([a-z_][a-z0-9_]*)\s+enable\s+row\s+level\s+security",
    re.IGNORECASE,
)
_FORCE_RLS_RE = re.compile(
    r"alter\s+table\s+public\.([a-z_][a-z0-9_]*)\s+force\s+row\s+level\s+security",
    re.IGNORECASE,
)
_CREATE_POLICY_RE = re.compile(
    r"create\s+policy\s+[^\n]*?\son\s+public\.([a-z_][a-z0-9_]*)",
    re.IGNORECASE | re.DOTALL,
)


@dataclass(frozen=True)
class TableExpectation:
    name: str
    rls_enabled: bool
    rls_forced: bool
    has_policies: bool
    # Fail-closed default posture: browser principals must not read/write; the
    # service-role backend (which bypasses RLS) may. Overridable only by an
    # explicit anon/authenticated policy in a migration.
    principal_select: dict[str, bool] = field(default_factory=dict)


@dataclass(frozen=True)
class TrackedState:
    tables: dict[str, TableExpectation]
    cloud_run_public_invoker_expected: bool | None
    cloud_run_ingress_expected: str | None
    cloud_run_ingress_allowed: tuple[str, ...]

    def table_names(self) -> set[str]:
        return set(self.tables)


def derive_tables(migrations_dir: Path) -> dict[str, TableExpectation]:
    tables: set[str] = set()
    rls_enabled: set[str] = set()
    rls_forced: set[str] = set()
    with_policies: set[str] = set()
    for path in sorted(migrations_dir.glob("*.sql")):
        sql = path.read_text(encoding="utf-8")
        tables.update(
            match.group(1).lower() for match in _CREATE_TABLE_RE.finditer(sql)
        )
        rls_enabled.update(
            match.group(1).lower() for match in _ENABLE_RLS_RE.finditer(sql)
        )
        rls_forced.update(
            match.group(1).lower() for match in _FORCE_RLS_RE.finditer(sql)
        )
        with_policies.update(
            match.group(1).lower() for match in _CREATE_POLICY_RE.finditer(sql)
        )

    expectations: dict[str, TableExpectation] = {}
    for name in sorted(tables):
        principal_select = {
            "anon": False,
            "authenticated": False,
            SERVICE_PRINCIPAL: True,
        }
        expectations[name] = TableExpectation(
            name=name,
            rls_enabled=name in rls_enabled,
            rls_forced=name in rls_forced,
            has_policies=name in with_policies,
            principal_select=principal_select,
        )
    return expectations


def load_config(config_path: Path) -> tuple[bool | None, str | None, tuple[str, ...]]:
    if not config_path.exists():
        return None, None, ()
    root = jsonutil.as_object(
        jsonutil.loads(config_path.read_text(encoding="utf-8")),
        context="expected config",
    )
    cloud_run = jsonutil.get_object(root, "cloud_run")
    invoker = cloud_run.get("public_invoker_expected")
    invoker_expected = invoker if isinstance(invoker, bool) else None
    ingress = cloud_run.get("ingress_expected")
    ingress_expected = ingress if isinstance(ingress, str) and ingress else None
    allowed = tuple(
        value
        for value in jsonutil.get_array(cloud_run, "ingress_allowed")
        if isinstance(value, str)
    )
    return invoker_expected, ingress_expected, allowed


def derive_tracked_state(migrations_dir: Path, config_path: Path) -> TrackedState:
    invoker_expected, ingress_expected, ingress_allowed = load_config(config_path)
    return TrackedState(
        tables=derive_tables(migrations_dir),
        cloud_run_public_invoker_expected=invoker_expected,
        cloud_run_ingress_expected=ingress_expected,
        cloud_run_ingress_allowed=ingress_allowed,
    )
