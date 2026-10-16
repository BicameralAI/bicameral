"""Design partner domain allowlist checks backed by Supabase."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import httpx

from bicameral_cloud.config import CloudSettings
from bicameral_cloud.models.tenancy import AuthenticatedUser, email_domain

DESIGN_PARTNER_ALLOWLIST_TABLE = "design_partner_allowlist"


class DesignPartnerAllowlistUnavailable(RuntimeError):
    """Raised when the authoritative allowlist cannot be checked."""


@dataclass(frozen=True)
class DesignPartnerAllowlist:
    """Server-side design partner domain allowlist policy."""

    settings: CloudSettings

    def allows(self, user: AuthenticatedUser) -> bool:
        """Return whether the user's email domain is allowed."""

        if not self.settings.supabase_auth_enabled:
            return True
        if not self.settings.supabase_design_partner_allowlist_enabled:
            raise DesignPartnerAllowlistUnavailable(
                "Design partner allowlist not configured"
            )
        return self._allows_from_supabase(user)

    def _allows_from_supabase(self, user: AuthenticatedUser) -> bool:
        domain = email_domain(user.email)
        if not domain:
            return False

        if not self.settings.supabase_url or not self.settings.supabase_service_key:
            raise DesignPartnerAllowlistUnavailable(
                "Design partner allowlist not configured"
            )

        url = (
            self.settings.supabase_url.rstrip("/")
            + f"/rest/v1/{DESIGN_PARTNER_ALLOWLIST_TABLE}"
        )
        params = {
            "select": "id",
            "enabled": "eq.true",
            "domain": f"eq.{domain}",
            "limit": "1",
        }
        headers = {
            "apikey": self.settings.supabase_service_key,
            "authorization": f"Bearer {self.settings.supabase_service_key}",
            "accept": "application/json",
        }

        try:
            response = httpx.get(url, params=params, headers=headers, timeout=5.0)
            if response.is_error:
                raise DesignPartnerAllowlistUnavailable(
                    "Design partner allowlist unavailable"
                )
            payload = response.json()
        except (httpx.HTTPError, ValueError) as exc:
            raise DesignPartnerAllowlistUnavailable(
                "Design partner allowlist unavailable"
            ) from exc

        return bool(_row_list(payload))


def _row_list(payload: Any) -> list[dict[str, Any]]:
    if not isinstance(payload, list):
        raise DesignPartnerAllowlistUnavailable(
            "Design partner allowlist returned unexpected payload"
        )
    return [row for row in payload if isinstance(row, dict)]
