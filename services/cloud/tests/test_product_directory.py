"""Durable Supabase Product directory tests (cloud#125).

Covers the acceptance criteria for replacing the in-memory adapter with a
durable, server-side Product directory:

- repository behaviour (publish / list / idempotency / no-silent-remap);
- API behaviour for publication and discovery via ``GET /api/v1/projects``;
- domain isolation — a different-domain account cannot discover a Product;
- redaction — no receipt, registration ref, provider secret, path, or actor
  data in any browser-facing response;
- idempotency for the same canonical Product ID;
- persistence across application/repository recreation (durable substrate);
- production composition selecting the Supabase backend, never in-memory;
- the checked-in migration protecting the server-owned table.
"""

from __future__ import annotations

import time
from pathlib import Path
from typing import Any

import httpx
import jwt
import pytest

from bicameral_cloud.config import CloudSettings, reset_settings
from bicameral_cloud.models.discovery import FORBIDDEN_DISCOVERY_FIELDS
from bicameral_cloud.models.product_directory import ProductPublication
from bicameral_cloud.store import get_store
from tests.proof_fixtures import (
    enroll_identity_for_user,
    first_event_proof,
    PROOF_VERIFICATION_KEYS_JSON,
)
from bicameral_cloud.product_directory_repository import (
    InMemoryProductDirectoryRepository,
    ProductIdRemapError,
    SupabaseProductDirectoryRepository,
    select_product_directory_repository,
)
from bicameral_cloud.routes.projects import (
    DurableDirectoryProjectSource,
    LocalAlphaProjectSource,
    get_project_discovery_source,
)

SUPABASE_TEST_SECRET = "test-jwt-secret-at-least-32-chars-long"
DOMAIN = "bicameral-ai.com"
OTHER_DOMAIN = "other-partner.com"


def _publication(**overrides: Any) -> ProductPublication:
    pid = overrides.get("canonical_product_id", "prod-1")
    base: dict[str, Any] = {
        "canonical_product_id": "prod-1",
        "display_name": "Checkout",
        "eligible_domain": DOMAIN,
        "creation_receipt": "receipt-abc",
        "registration_ref": "reg-1",
        "first_event_commit_proof": first_event_proof(str(pid)),
    }
    base.update(overrides)
    if "first_event_commit_proof" not in overrides and "canonical_product_id" in overrides:
        base["first_event_commit_proof"] = first_event_proof(str(overrides["canonical_product_id"]))
    return ProductPublication(**base)


# --------------------------------------------------------------------------- #
# Repository behaviour (in-memory reference implementation)
# --------------------------------------------------------------------------- #


class TestInMemoryRepository:
    def test_publish_then_discover_by_domain(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        record, idempotent = repo.publish(_publication())

        assert idempotent is False
        assert record.canonical_product_id == "prod-1"
        assert [r.canonical_product_id for r in repo.list_for_domain(DOMAIN)] == [
            "prod-1"
        ]

    def test_list_is_domain_scoped_and_sorted(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        repo.publish(_publication(canonical_product_id="prod-b", registration_ref="b"))
        repo.publish(_publication(canonical_product_id="prod-a", registration_ref="a"))
        repo.publish(
            _publication(
                canonical_product_id="prod-other",
                registration_ref="o",
                eligible_domain=OTHER_DOMAIN,
            )
        )

        assert [r.canonical_product_id for r in repo.list_for_domain(DOMAIN)] == [
            "prod-a",
            "prod-b",
        ]
        assert [
            r.canonical_product_id for r in repo.list_for_domain(OTHER_DOMAIN)
        ] == ["prod-other"]

    def test_republish_same_id_is_idempotent(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        repo.publish(_publication())
        record, idempotent = repo.publish(_publication(display_name="Renamed"))

        assert idempotent is True
        assert record.display_name == "Renamed"
        assert len(repo.list_for_domain(DOMAIN)) == 1

    def test_registration_ref_cannot_remap_to_new_id(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        repo.publish(_publication())
        with pytest.raises(ProductIdRemapError):
            repo.publish(_publication(canonical_product_id="prod-2"))

    def test_id_cannot_be_rebound_to_new_registration(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        repo.publish(_publication())
        with pytest.raises(ProductIdRemapError):
            repo.publish(_publication(registration_ref="reg-2"))

    def test_id_cannot_be_moved_to_new_domain(self) -> None:
        repo = InMemoryProductDirectoryRepository()
        repo.publish(_publication())
        with pytest.raises(ProductIdRemapError):
            repo.publish(_publication(eligible_domain=OTHER_DOMAIN))


# --------------------------------------------------------------------------- #
# Publication metadata is validated at the boundary (no leakage into Cloud)
# --------------------------------------------------------------------------- #


class TestPublicationValidation:
    @pytest.mark.parametrize(
        "value",
        ["/etc/passwd", "C:\\secrets", "https://token@host", "provider_secret=abc"],
    )
    def test_display_name_rejects_paths_and_secrets(self, value: str) -> None:
        with pytest.raises(ValueError):
            _publication(display_name=value)

    def test_eligible_domain_is_normalized(self) -> None:
        pub = _publication(eligible_domain="@BICAMERAL-AI.COM")
        assert pub.eligible_domain == DOMAIN

    def test_empty_canonical_id_rejected(self) -> None:
        with pytest.raises(ValueError):
            _publication(canonical_product_id="   ")


# --------------------------------------------------------------------------- #
# Production composition selects Supabase, never the in-memory adapter
# --------------------------------------------------------------------------- #


class TestComposition:
    def test_production_settings_select_supabase(self) -> None:
        settings = CloudSettings(
            product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
            supabase_url="https://example.supabase.co",
            supabase_jwt_secret=SUPABASE_TEST_SECRET,
            supabase_anon_key="anon",
            supabase_service_key="service-role-key",
        )
        repo = select_product_directory_repository(settings)
        assert isinstance(repo, SupabaseProductDirectoryRepository)
        assert not isinstance(repo, InMemoryProductDirectoryRepository)

    def test_local_settings_select_in_memory(self) -> None:
        settings = CloudSettings(
            product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
            supabase_url=None,
            supabase_jwt_secret=None,
            supabase_anon_key=None,
            supabase_service_key=None,
        )
        repo = select_product_directory_repository(settings)
        assert isinstance(repo, InMemoryProductDirectoryRepository)

    def test_production_project_source_is_durable_not_local(self) -> None:
        reset_settings(
            CloudSettings(
                product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
                supabase_url="https://example.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key="anon",
                supabase_service_key="service-role-key",
            )
        )
        try:
            source = get_project_discovery_source(
                store=None,  # unused on the durable path
                directory=InMemoryProductDirectoryRepository(),
            )
        finally:
            reset_settings()
        assert isinstance(source, DurableDirectoryProjectSource)
        assert not isinstance(source, LocalAlphaProjectSource)

    def test_local_project_source_selected_without_supabase(self) -> None:
        reset_settings(
            CloudSettings(
                product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
                supabase_url=None,
                supabase_jwt_secret=None,
                supabase_anon_key=None,
                supabase_service_key=None,
            )
        )
        try:
            source = get_project_discovery_source(
                store=None,
                directory=InMemoryProductDirectoryRepository(),
            )
        finally:
            reset_settings()
        assert isinstance(source, LocalAlphaProjectSource)


# --------------------------------------------------------------------------- #
# Durable substrate: a published Product survives repository/app recreation
# --------------------------------------------------------------------------- #


class _FakeSupabaseTable:
    """Stateful in-process stand-in for the Supabase ``product_directory`` table.

    Holds rows independently of any repository instance, so recreating the
    repository (or the app) and reading again proves persistence lives in the
    durable substrate rather than in process memory.
    """

    def __init__(self) -> None:
        self.rows: dict[str, dict[str, Any]] = {}

    def get(
        self, url: str, params: dict[str, str], headers: dict[str, str], timeout: float
    ) -> httpx.Response:
        rows = list(self.rows.values())
        for key, raw in params.items():
            if key in {"select", "order", "limit"} or not raw.startswith("eq."):
                continue
            expected = raw[len("eq.") :]
            rows = [row for row in rows if str(row.get(key)) == expected]
        if "order" in params:
            rows.sort(key=lambda row: row.get("canonical_product_id", ""))
        if "limit" in params:
            rows = rows[: int(params["limit"])]
        return httpx.Response(200, json=rows, request=httpx.Request("GET", url))

    def post(
        self,
        url: str,
        params: dict[str, str],
        headers: dict[str, str],
        json: dict[str, Any],
        timeout: float,
    ) -> httpx.Response:
        self.rows[json["canonical_product_id"]] = dict(json)
        return httpx.Response(201, json=[], request=httpx.Request("POST", url))


@pytest.fixture()
def supabase_settings() -> CloudSettings:
    return CloudSettings(
        product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
        supabase_url="https://example.supabase.co",
        supabase_jwt_secret=SUPABASE_TEST_SECRET,
        supabase_anon_key="anon",
        supabase_service_key="service-role-key",
    )


class TestSupabasePersistence:
    def test_published_product_survives_repository_recreation(
        self, supabase_settings: CloudSettings, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        table = _FakeSupabaseTable()
        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.get", table.get
        )
        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.post", table.post
        )

        # Publish through one repository instance.
        writer = SupabaseProductDirectoryRepository(supabase_settings)
        writer.publish(_publication())

        # Recreate the repository (as a fresh app/process would) and read back.
        reader = SupabaseProductDirectoryRepository(supabase_settings)
        discovered = reader.list_for_domain(DOMAIN)
        assert [r.canonical_product_id for r in discovered] == ["prod-1"]

        # A different-domain reader discovers nothing.
        assert reader.list_for_domain(OTHER_DOMAIN) == []

    def test_supabase_publish_is_idempotent(
        self, supabase_settings: CloudSettings, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        table = _FakeSupabaseTable()
        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.get", table.get
        )
        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.post", table.post
        )
        repo = SupabaseProductDirectoryRepository(supabase_settings)

        _, first = repo.publish(_publication())
        _, second = repo.publish(_publication(display_name="Renamed"))

        assert first is False
        assert second is True
        assert len(table.rows) == 1


# --------------------------------------------------------------------------- #
# API: publication + discovery, domain isolation, redaction, idempotency
# --------------------------------------------------------------------------- #


class TestProductPublicationApi:
    def _mint(self, email: str) -> str:
        return jwt.encode(
            {
                "sub": f"user-{email}",
                "email": email,
                "aud": "authenticated",
                "exp": int(time.time()) + 3600,
            },
            SUPABASE_TEST_SECRET,
            algorithm="HS256",
        )

    @pytest.fixture()
    def api(self, client, monkeypatch: pytest.MonkeyPatch):
        """Client wired to a stateful fake directory table (Supabase path).

        A single ``httpx.get`` shim dispatches by URL: the design-partner
        allowlist gate always passes (isolation is enforced by the domain-scoped
        directory itself), while ``product_directory`` reads hit the stateful
        fake table.
        """
        table = _FakeSupabaseTable()

        def dispatch_get(url, params, headers, timeout):  # noqa: ANN001, ANN202
            if "product_directory" in url:
                return table.get(url, params, headers, timeout)
            return httpx.Response(
                200, json=[{"id": "row"}], request=httpx.Request("GET", url)
            )

        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.get", dispatch_get
        )
        monkeypatch.setattr(
            "bicameral_cloud.product_directory_repository.httpx.post", table.post
        )
        reset_settings(
            CloudSettings(
                product_creation_proof_verification_keys_json=PROOF_VERIFICATION_KEYS_JSON,
                supabase_url="https://example.supabase.co",
                supabase_jwt_secret=SUPABASE_TEST_SECRET,
                supabase_anon_key="anon",
                supabase_service_key="service-role-key",
            )
        )
        # Drop the suite-wide local-source pin so the real production selection
        # (durable Supabase directory) runs for these tests.
        client.app.dependency_overrides.pop(get_project_discovery_source, None)
        try:
            yield client, table
        finally:
            reset_settings()

    def _publish(self, client, token: str, **overrides: Any):
        claims = jwt.decode(
            token,
            SUPABASE_TEST_SECRET,
            algorithms=["HS256"],
            audience="authenticated",
        )
        enroll_identity_for_user(get_store(), claims["sub"])
        pid = overrides.get("canonical_product_id", "prod-1")
        body = {
            "canonical_product_id": "prod-1",
            "display_name": "Checkout",
            "eligible_domain": DOMAIN,
            "creation_receipt": "receipt-abc",
            "registration_ref": "reg-1",
            "first_event_commit_proof": first_event_proof(str(pid)).model_dump(),
        }
        body.update(overrides)
        if "first_event_commit_proof" not in overrides:
            body["first_event_commit_proof"] = first_event_proof(
                str(body["canonical_product_id"])
            ).model_dump()
        return client.post(
            "/api/v1/products",
            json=body,
            headers={"Authorization": f"Bearer {token}"},
        )

    def test_publish_then_eligible_account_discovers_product(self, api) -> None:
        client, _ = api
        publisher = self._mint(f"creator@{DOMAIN}")

        created = self._publish(client, publisher)
        assert created.status_code == 201
        assert created.json()["idempotent"] is False

        peer = self._mint(f"peer@{DOMAIN}")
        resp = client.get(
            "/api/v1/projects", headers={"Authorization": f"Bearer {peer}"}
        )
        assert resp.status_code == 200
        item = next(
            p for p in resp.json()["projects"] if p["project_id"] == "prod-1"
        )
        assert item["discovery_eligibility"] == "domain"
        assert item["membership_status"] == "not_member"
        assert item["membership_role"] is None
        assert item["team_id"] is None

    def test_different_domain_account_cannot_discover(self, api) -> None:
        client, _ = api
        self._publish(client, self._mint(f"creator@{DOMAIN}"))

        outsider = self._mint(f"out@{OTHER_DOMAIN}")
        resp = client.get(
            "/api/v1/projects", headers={"Authorization": f"Bearer {outsider}"}
        )
        assert resp.status_code == 200
        assert all(p["project_id"] != "prod-1" for p in resp.json()["projects"])

    def test_publication_is_idempotent(self, api) -> None:
        client, table = api
        token = self._mint(f"creator@{DOMAIN}")

        assert self._publish(client, token).status_code == 201
        again = self._publish(client, token, display_name="Renamed")
        assert again.status_code == 200
        assert again.json()["idempotent"] is True
        assert len(table.rows) == 1

    def test_cannot_remap_existing_id(self, api) -> None:
        client, _ = api
        token = self._mint(f"creator@{DOMAIN}")
        self._publish(client, token)

        conflict = self._publish(client, token, canonical_product_id="prod-2")
        assert conflict.status_code == 409

    def test_cannot_publish_for_foreign_domain(self, api) -> None:
        client, _ = api
        token = self._mint(f"creator@{DOMAIN}")
        resp = self._publish(client, token, eligible_domain=OTHER_DOMAIN)
        assert resp.status_code == 403

    def test_responses_contain_no_secret_or_actor_leakage(self, api) -> None:
        client, _ = api
        token = self._mint(f"creator@{DOMAIN}")
        self._publish(
            client, token, provider_hint="github", substrate_hint="postgres"
        )

        publish_body = self._publish(client, token).text.lower()
        discover_body = client.get(
            "/api/v1/projects", headers={"Authorization": f"Bearer {token}"}
        ).text.lower()

        for body in (publish_body, discover_body):
            assert "receipt-abc" not in body
            assert "reg-1" not in body
            assert "creation_receipt" not in body
            assert "registration_ref" not in body
        for field in FORBIDDEN_DISCOVERY_FIELDS:
            assert field not in discover_body


# --------------------------------------------------------------------------- #
# Checked-in migration protects the server-owned table
# --------------------------------------------------------------------------- #


def test_migration_enables_rls_and_locks_down_table() -> None:
    migration = (
        Path(__file__).resolve().parents[1]
        / "supabase"
        / "migrations"
        / "202607120001_product_directory.sql"
    )
    sql = migration.read_text().lower()
    assert "create table if not exists public.product_directory" in sql
    assert "canonical_product_id text not null unique" in sql
    assert "enable row level security" in sql
    assert "revoke all on public.product_directory from anon" in sql
    assert "revoke all on public.product_directory from authenticated" in sql
