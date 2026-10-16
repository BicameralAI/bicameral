"""OpenAPI/docs exposure gate (pentest OpenAPI-exposure remediation).

The hosted API must not publish its OpenAPI schema or interactive docs by
default. These tests pin the gate logic directly, independent of the shared test
app (which enables docs via the root conftest so the schema assertions elsewhere
can run).
"""

from __future__ import annotations

from bicameral_cloud.app import _docs_kwargs
from bicameral_cloud.config import CloudSettings


def _settings(*, docs_enabled: bool) -> CloudSettings:
    return CloudSettings(
        supabase_url=None,
        supabase_jwt_secret=None,
        supabase_anon_key=None,
        supabase_service_key=None,
        docs_enabled=docs_enabled,
    )


def test_docs_disabled_hides_schema_and_ui() -> None:
    assert _docs_kwargs(_settings(docs_enabled=False)) == {
        "openapi_url": None,
        "docs_url": None,
        "redoc_url": None,
    }


def test_docs_enabled_exposes_defaults() -> None:
    assert _docs_kwargs(_settings(docs_enabled=True)) == {
        "openapi_url": "/openapi.json",
        "docs_url": "/docs",
        "redoc_url": "/redoc",
    }


def test_production_default_disables_docs() -> None:
    # A CloudSettings built without a docs override defaults to docs disabled, so
    # the production app (no BICAMERAL_CLOUD_ENABLE_DOCS) is not enumerable.
    default = CloudSettings(
        supabase_url=None,
        supabase_jwt_secret=None,
        supabase_anon_key=None,
        supabase_service_key=None,
    )
    assert default.docs_enabled is False
    assert _docs_kwargs(default) == {
        "openapi_url": None,
        "docs_url": None,
        "redoc_url": None,
    }
