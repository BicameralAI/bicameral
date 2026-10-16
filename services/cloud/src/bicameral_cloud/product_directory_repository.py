"""Server-side durable Product directory repository (cloud#125).

Replaces the in-memory ``LocalAlphaProjectAdapter`` as the *production* source of
truth for published Product discovery and routing. Production composition selects
the Supabase-backed repository; unit/dev wiring selects the in-memory repository
explicitly (FastAPI dependency default / override).

Authority boundary: this directory is a non-canonical coordination/projection
tier. The bot/daemon stays authoritative for Product creation, membership,
roles, policy, and events (ADR-0005/0006). Only safe metadata is persisted, and
only the service-role key touches the Supabase table.
"""

from __future__ import annotations

from typing import Any, Protocol

import httpx
from pydantic import ValidationError

from bicameral_cloud.config import CloudSettings, get_settings
from bicameral_cloud.models.product_directory import (
    ProductDirectoryRecord,
    ProductPublication,
)

PRODUCT_DIRECTORY_TABLE = "product_directory"

_PERSISTED_COLUMNS = (
    "canonical_product_id",
    "display_name",
    "eligible_domain",
    "registration_ref",
    "provider_hint",
    "substrate_hint",
    "creation_commit_proof_digest",
    "creation_event_id",
    "creation_event_digest",
    "enabled",
)


class ProductDirectoryUnavailable(RuntimeError):
    """Raised when the durable directory backend cannot be reached."""


class ProductIdRemapError(ValueError):
    """Raised when a publication would silently remap an existing identifier.

    Publication is idempotent for the same canonical Product ID. It must never
    silently rebind an existing ``registration_ref`` onto a different canonical
    Product ID, rebind a canonical Product ID onto a different registration, or
    move a published Product to a different eligible domain.
    """


def _publish_result(
    existing: ProductDirectoryRecord | None,
    publication: ProductPublication,
    by_registration: ProductDirectoryRecord | None,
) -> ProductDirectoryRecord:
    """Compute the record to persist, enforcing no-silent-remap invariants.

    Returns the record that should be stored. Raises ``ProductIdRemapError``
    when the publication would remap an existing identifier.
    """
    if (
        by_registration is not None
        and by_registration.canonical_product_id != publication.canonical_product_id
    ):
        raise ProductIdRemapError(
            "registration_ref is already bound to a different canonical Product ID"
        )

    if existing is None:
        return ProductDirectoryRecord(
            canonical_product_id=publication.canonical_product_id,
            display_name=publication.display_name,
            eligible_domain=publication.eligible_domain,
            registration_ref=publication.registration_ref,
            provider_hint=publication.provider_hint,
            substrate_hint=publication.substrate_hint,
            creation_commit_proof_digest=(
                publication.first_event_commit_proof.proof_digest
            ),
            creation_event_id=publication.first_event_commit_proof.event_id,
            creation_event_digest=publication.first_event_commit_proof.event_digest,
        )

    if existing.eligible_domain != publication.eligible_domain:
        raise ProductIdRemapError(
            "cannot silently move a published Product to a different eligible domain"
        )
    if (
        publication.registration_ref is not None
        and existing.registration_ref is not None
        and existing.registration_ref != publication.registration_ref
    ):
        raise ProductIdRemapError(
            "canonical Product ID is already bound to a different registration_ref"
        )
    if (
        existing.creation_event_id is not None
        and existing.creation_event_id
        != publication.first_event_commit_proof.event_id
    ):
        raise ProductIdRemapError(
            "canonical Product ID is already bound to a different creation event"
        )
    if (
        existing.creation_event_digest is not None
        and existing.creation_event_digest
        != publication.first_event_commit_proof.event_digest
    ):
        raise ProductIdRemapError(
            "canonical Product ID is already bound to different creation event bytes"
        )

    # Idempotent re-publication: refresh safe metadata, keep identity stable.
    return existing.model_copy(
        update={
            "display_name": publication.display_name,
            "registration_ref": existing.registration_ref
            or publication.registration_ref,
            "provider_hint": publication.provider_hint,
            "substrate_hint": publication.substrate_hint,
            "creation_commit_proof_digest": (
                publication.first_event_commit_proof.proof_digest
            ),
            "creation_event_id": existing.creation_event_id
            or publication.first_event_commit_proof.event_id,
            "creation_event_digest": existing.creation_event_digest
            or publication.first_event_commit_proof.event_digest,
            "enabled": existing.enabled,
        }
    )


class ProductDirectoryRepository(Protocol):
    """Durable Product directory persistence contract."""

    def publish(
        self, publication: ProductPublication
    ) -> tuple[ProductDirectoryRecord, bool]:
        """Persist a publication idempotently.

        Returns ``(record, idempotent)`` where ``idempotent`` is ``True`` when a
        record for the canonical Product ID already existed. Raises
        ``ProductIdRemapError`` on a silent-remap attempt.
        """
        ...

    def list_for_domain(self, domain: str) -> list[ProductDirectoryRecord]:
        """Return Products a verified email domain may discover, ordered stably."""
        ...

    def get_by_canonical_id(
        self, canonical_product_id: str
    ) -> ProductDirectoryRecord | None:
        """Return the record for a canonical Product ID, if present."""
        ...


class InMemoryProductDirectoryRepository:
    """In-memory Product directory for tests and local development.

    Not a production backend. Kept behind explicit test/dev wiring so production
    composition never depends on it (cloud#125 acceptance).
    """

    def __init__(self) -> None:
        self._by_canonical: dict[str, ProductDirectoryRecord] = {}

    def _by_registration(self, ref: str) -> ProductDirectoryRecord | None:
        for record in self._by_canonical.values():
            if record.registration_ref == ref:
                return record
        return None

    def publish(
        self, publication: ProductPublication
    ) -> tuple[ProductDirectoryRecord, bool]:
        existing = self._by_canonical.get(publication.canonical_product_id)
        by_registration = (
            self._by_registration(publication.registration_ref)
            if publication.registration_ref is not None
            else None
        )
        record = _publish_result(existing, publication, by_registration)
        self._by_canonical[record.canonical_product_id] = record
        return record, existing is not None

    def list_for_domain(self, domain: str) -> list[ProductDirectoryRecord]:
        normalized = domain.strip().lower()
        if not normalized:
            return []
        matches = [
            r
            for r in self._by_canonical.values()
            if r.eligible_domain == normalized
        ]
        matches.sort(key=lambda r: r.canonical_product_id)
        return matches

    def get_by_canonical_id(
        self, canonical_product_id: str
    ) -> ProductDirectoryRecord | None:
        return self._by_canonical.get(canonical_product_id)


class SupabaseProductDirectoryRepository:
    """Supabase (PostgREST) service-role backed Product directory.

    Mirrors the ``design_partner_allowlist`` service-role read pattern: the
    service-role key is used server-side only and never returned to browsers.
    """

    def __init__(self, settings: CloudSettings) -> None:
        if not settings.supabase_url or not settings.supabase_service_key:
            raise ProductDirectoryUnavailable(
                "Product directory not configured"
            )
        self._base_url = (
            settings.supabase_url.rstrip("/") + f"/rest/v1/{PRODUCT_DIRECTORY_TABLE}"
        )
        self._service_key = settings.supabase_service_key

    def _headers(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        headers = {
            "apikey": self._service_key,
            "authorization": f"Bearer {self._service_key}",
            "accept": "application/json",
            "content-type": "application/json",
        }
        if extra:
            headers.update(extra)
        return headers

    def _get(self, params: dict[str, str]) -> list[dict[str, Any]]:
        try:
            response = httpx.get(
                self._base_url,
                params={"select": ",".join(_PERSISTED_COLUMNS), **params},
                headers=self._headers(),
                timeout=5.0,
            )
            response.raise_for_status()
            payload = response.json()
        except (httpx.HTTPError, ValueError) as exc:
            raise ProductDirectoryUnavailable(
                "Product directory unavailable"
            ) from exc
        if not isinstance(payload, list):
            raise ProductDirectoryUnavailable(
                "Product directory returned unexpected payload"
            )
        return [row for row in payload if isinstance(row, dict)]

    @staticmethod
    def _to_record(row: dict[str, Any]) -> ProductDirectoryRecord:
        try:
            return ProductDirectoryRecord.model_validate(row)
        except ValidationError as exc:
            raise ProductDirectoryUnavailable(
                "Product directory returned a malformed row"
            ) from exc

    def _first(self, column: str, value: str) -> ProductDirectoryRecord | None:
        rows = self._get({column: f"eq.{value}", "limit": "1"})
        if not rows:
            return None
        return self._to_record(rows[0])

    def get_by_canonical_id(
        self, canonical_product_id: str
    ) -> ProductDirectoryRecord | None:
        return self._first("canonical_product_id", canonical_product_id)

    def list_for_domain(self, domain: str) -> list[ProductDirectoryRecord]:
        normalized = domain.strip().lower()
        if not normalized:
            return []
        rows = self._get(
            {
                "eligible_domain": f"eq.{normalized}",
                "order": "canonical_product_id.asc",
            }
        )
        return [self._to_record(row) for row in rows]

    def publish(
        self, publication: ProductPublication
    ) -> tuple[ProductDirectoryRecord, bool]:
        existing = self.get_by_canonical_id(publication.canonical_product_id)
        by_registration = (
            self._first("registration_ref", publication.registration_ref)
            if publication.registration_ref is not None
            else None
        )
        record = _publish_result(existing, publication, by_registration)
        self._upsert(record)
        return record, existing is not None

    def _upsert(self, record: ProductDirectoryRecord) -> None:
        row = record.model_dump(include=set(_PERSISTED_COLUMNS))
        try:
            response = httpx.post(
                self._base_url,
                params={"on_conflict": "canonical_product_id"},
                headers=self._headers(
                    {"prefer": "resolution=merge-duplicates,return=minimal"}
                ),
                json=row,
                timeout=5.0,
            )
            response.raise_for_status()
        except httpx.HTTPError as exc:
            raise ProductDirectoryUnavailable(
                "Product directory write failed"
            ) from exc


# --- Composition / dependency wiring -------------------------------------------

_inmemory_singleton: InMemoryProductDirectoryRepository | None = None


def get_inmemory_product_directory_repository() -> InMemoryProductDirectoryRepository:
    """Process-wide in-memory directory (survives app/router recreation)."""
    global _inmemory_singleton  # noqa: PLW0603
    if _inmemory_singleton is None:
        _inmemory_singleton = InMemoryProductDirectoryRepository()
    return _inmemory_singleton


def reset_inmemory_product_directory_repository() -> InMemoryProductDirectoryRepository:
    """Reset and return a fresh in-memory directory (for tests)."""
    global _inmemory_singleton  # noqa: PLW0603
    _inmemory_singleton = InMemoryProductDirectoryRepository()
    return _inmemory_singleton


def select_product_directory_repository(
    settings: CloudSettings,
) -> ProductDirectoryRepository:
    """Choose the directory backend for the given settings (composition point).

    Production (service-role key configured) resolves the durable Supabase
    repository; otherwise the in-memory repository is used for tests and local
    development. This is the single place that decides the backend so the
    "production does not use the in-memory adapter" invariant is verifiable
    (cloud#125).
    """
    if settings.supabase_product_directory_enabled:
        return SupabaseProductDirectoryRepository(settings)
    return get_inmemory_product_directory_repository()


def get_product_directory_repository() -> ProductDirectoryRepository:
    """FastAPI dependency resolving the directory backend for the request.

    Unit/dev wiring may override this dependency to select the in-memory
    repository explicitly; production resolves the Supabase repository from
    settings.
    """
    return select_product_directory_repository(get_settings())
