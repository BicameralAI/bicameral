"""Supabase / Postgres schema, grant, RLS, and runtime-principal collectors.

Each collector pairs the exact ``psql`` command an authorized owner runs against a
non-production Supabase/Postgres instance with a pure parser over its JSON output.
Queries read ``information_schema`` / ``pg_catalog`` only and never mutate state. The
runtime-principal collector records how ``anon``, ``authenticated``, and
``service_role`` are permitted to read ``public`` relations, which — combined with RLS
enablement — is the principal-fidelity fact the Cloud #106 gate needs.
"""

from __future__ import annotations

from collections.abc import Callable

from bicameral_cloud.evidence import jsonutil
from bicameral_cloud.evidence.bundle import CommandSpec
from bicameral_cloud.evidence.gcp import Collector
from bicameral_cloud.evidence.outcomes import CollectionResult
from bicameral_cloud.evidence.redaction import Redactor

Parser = Callable[[str, Redactor], CollectionResult]

RUNTIME_PRINCIPALS = ("anon", "authenticated", "service_role")


def _psql(sql: str) -> list[str]:
    return ["psql", "${SUPABASE_DB_URL}", "-tAc", sql]


def _rows(text: str, context: str) -> list[dict[str, object]]:
    """Parse ``json_agg`` psql output (an array, or empty/``null`` when no rows)."""
    stripped = text.strip()
    if not stripped or stripped == "null":
        return []
    parsed = jsonutil.as_array(jsonutil.loads(stripped), context=context)
    rows: list[dict[str, object]] = []
    for row in parsed:
        rows.append(jsonutil.as_object(row, context=f"{context} row"))
    return rows


def _parse_schemas(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "schemas")
    schemas = sorted(jsonutil.get_str(row, "schema_name") for row in rows)
    return CollectionResult.collected(redactor.scrub_payload({"schemas": schemas}))


def _parse_tables(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "tables")
    tables = sorted(jsonutil.get_str(row, "table_name") for row in rows)
    return CollectionResult.collected(redactor.scrub_payload({"tables": tables}))


def _parse_grants(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "grants")
    ordered = sorted(
        rows,
        key=lambda row: (
            jsonutil.get_str(row, "grantee"),
            jsonutil.get_str(row, "table_name"),
            jsonutil.get_str(row, "privilege_type"),
        ),
    )
    grants: list[dict[str, object]] = [
        {
            "grantee": jsonutil.get_str(row, "grantee"),
            "table_name": jsonutil.get_str(row, "table_name"),
            "privilege_type": jsonutil.get_str(row, "privilege_type"),
        }
        for row in ordered
    ]
    return CollectionResult.collected(redactor.scrub_payload({"grants": grants}))


def _parse_default_privileges(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "default privileges")
    entries = sorted(
        (jsonutil.get_str(row, "default_acl") for row in rows),
        key=str,
    )
    return CollectionResult.collected(
        redactor.scrub_payload({"default_privileges": entries})
    )


def _parse_rls(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "rls")
    tables: list[dict[str, object]] = [
        {
            "table_name": jsonutil.get_str(row, "table_name"),
            "rls_enabled": jsonutil.get_bool(row, "rls_enabled"),
            "rls_forced": jsonutil.get_bool(row, "rls_forced"),
        }
        for row in sorted(rows, key=lambda row: jsonutil.get_str(row, "table_name"))
    ]
    return CollectionResult.collected(redactor.scrub_payload({"tables": tables}))


def _parse_policies(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "policies")
    ordered = sorted(
        rows,
        key=lambda row: (
            jsonutil.get_str(row, "table_name"),
            jsonutil.get_str(row, "policy_name"),
        ),
    )
    policies: list[dict[str, object]] = [
        {
            "table_name": jsonutil.get_str(row, "table_name"),
            "policy_name": jsonutil.get_str(row, "policy_name"),
            "command": jsonutil.get_str(row, "command"),
            "roles": jsonutil.get_str(row, "roles"),
        }
        for row in ordered
    ]
    return CollectionResult.collected(redactor.scrub_payload({"policies": policies}))


def _parse_runtime_principal(text: str, redactor: Redactor) -> CollectionResult:
    rows = _rows(text, "runtime principal")
    grouped: dict[str, list[dict[str, object]]] = {
        name: [] for name in RUNTIME_PRINCIPALS
    }
    for row in sorted(rows, key=lambda row: jsonutil.get_str(row, "table_name")):
        principal = jsonutil.get_str(row, "principal")
        if principal not in grouped:
            continue
        grouped[principal].append(
            {
                "table_name": jsonutil.get_str(row, "table_name"),
                "select_granted": jsonutil.get_bool(row, "select_granted"),
                "insert_granted": jsonutil.get_bool(row, "insert_granted"),
            }
        )
    principals: dict[str, object] = dict(grouped)
    return CollectionResult.collected(
        redactor.scrub_payload({"principals": principals})
    )


_SCHEMAS_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select schema_name from information_schema.schemata order by schema_name) t"
)
_TABLES_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select table_name from information_schema.tables "
    "where table_schema='public' and table_type='BASE TABLE' order by table_name) t"
)
_GRANTS_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select grantee, table_name, privilege_type "
    "from information_schema.role_table_grants "
    "where table_schema='public' order by grantee, table_name, privilege_type) t"
)
_DEFAULT_PRIV_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select pg_catalog.pg_get_userbyid(d.defaclrole) || ':' || d.defaclobjtype || ':' "
    "|| array_to_string(d.defaclacl, ',') as default_acl "
    "from pg_default_acl d join pg_namespace n on n.oid=d.defaclnamespace "
    "where n.nspname='public') t"
)
_RLS_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select c.relname as table_name, c.relrowsecurity as rls_enabled, "
    "c.relforcerowsecurity as rls_forced from pg_class c "
    "join pg_namespace n on n.oid=c.relnamespace "
    "where n.nspname='public' and c.relkind='r' order by c.relname) t"
)
_POLICIES_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from "
    "(select tablename as table_name, policyname as policy_name, cmd as command, "
    "array_to_string(roles, ',') as roles from pg_policies "
    "where schemaname='public' order by tablename, policyname) t"
)
_RUNTIME_PRINCIPAL_SQL = (
    "select coalesce(json_agg(row_to_json(t)), '[]') from ("
    "select r.rolname as principal, c.relname as table_name, "
    "has_table_privilege(r.rolname, format('public.%I', c.relname), 'SELECT') "
    "as select_granted, "
    "has_table_privilege(r.rolname, format('public.%I', c.relname), 'INSERT') "
    "as insert_granted "
    "from pg_class c join pg_namespace n on n.oid=c.relnamespace "
    "cross join (select unnest(array['anon','authenticated','service_role']) "
    "as rolname) r "
    "where n.nspname='public' and c.relkind='r' "
    "and exists (select 1 from pg_roles where rolname=r.rolname) "
    "order by principal, table_name) t"
)


COLLECTORS: tuple[Collector, ...] = (
    Collector(
        CommandSpec(
            id="supabase.schemas",
            category="supabase_schemas",
            description="Postgres schema inventory.",
            command=_psql(_SCHEMAS_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_schemas,
    ),
    Collector(
        CommandSpec(
            id="supabase.tables",
            category="supabase_tables",
            description="public schema base tables.",
            command=_psql(_TABLES_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_tables,
    ),
    Collector(
        CommandSpec(
            id="supabase.grants",
            category="supabase_grants",
            description="Role table grants on public relations.",
            command=_psql(_GRANTS_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_grants,
    ),
    Collector(
        CommandSpec(
            id="supabase.default_privileges",
            category="supabase_default_privileges",
            description="Default privileges configured for the public schema.",
            command=_psql(_DEFAULT_PRIV_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_default_privileges,
    ),
    Collector(
        CommandSpec(
            id="supabase.rls",
            category="supabase_rls",
            description="Row-level-security enablement per public table.",
            command=_psql(_RLS_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_rls,
    ),
    Collector(
        CommandSpec(
            id="supabase.policies",
            category="supabase_policies",
            description="RLS policies defined on public tables.",
            command=_psql(_POLICIES_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_policies,
    ),
    Collector(
        CommandSpec(
            id="supabase.runtime_principal",
            category="runtime_principal",
            description=(
                "SELECT/INSERT reachability of public tables for "
                "anon, authenticated, and service_role."
            ),
            command=_psql(_RUNTIME_PRINCIPAL_SQL),
            required_env=["SUPABASE_DB_URL"],
        ),
        _parse_runtime_principal,
    ),
)
