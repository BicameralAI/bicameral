"""Redaction and digesting for the reconciliation evidence bundle.

The produced evidence bundle is attached to the owner-only Cloud #106 gate, so it
must never carry secrets, tokens, confidential project identifiers, raw customer
data, or private source bodies. Two defences run together:

* structural redaction of secret-shaped keys/values; and
* caller-supplied identifier redaction (project id, Supabase project ref) so the
  bundle can be shared without leaking the environment it was taken from.

Every sanitized payload is fingerprinted with :func:`sanitized_digest` so a bundle
carries a stable content digest of exactly the redacted bytes it exposes.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass, field

REDACTED = "<redacted>"

# Key names whose values are always secret-shaped and must never be retained.
_SECRET_KEY_TOKENS = (
    "password",
    "passwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "authorization",
    "credential",
    "private_key",
    "privatekey",
    "client_secret",
    "access_key",
    "service_key",
    "connection_string",
    "dsn",
)

# Value shapes that look like credentials regardless of the key they appear under.
_SECRET_VALUE_PATTERNS = (
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{36,}\b"),
    re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b"),
    # JWT (Supabase anon/service keys are JWTs); three base64url segments.
    re.compile(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b"),
)


def _key_is_secret(key: str) -> bool:
    normalized = key.replace("-", "").replace("_", "").lower()
    return any(
        token.replace("-", "").replace("_", "") in normalized
        for token in _SECRET_KEY_TOKENS
    )


@dataclass(frozen=True)
class Redactor:
    """Replaces caller-declared sensitive identifiers with typed placeholders.

    ``identifiers`` maps a literal substring (e.g. a confidential project id) to a
    stable placeholder label. Matching is literal and case-insensitive so the same
    identifier is scrubbed wherever it appears — service-account emails, resource
    names, connection hosts.
    """

    identifiers: dict[str, str] = field(default_factory=dict)

    def scrub_text(self, text: str) -> str:
        result = text
        for literal, label in self.identifiers.items():
            if not literal:
                continue
            result = re.sub(
                re.escape(literal), f"<redacted:{label}>", result, flags=re.IGNORECASE
            )
        for pattern in _SECRET_VALUE_PATTERNS:
            result = pattern.sub(REDACTED, result)
        return result

    def scrub_value(self, key: str | None, value: object) -> object:
        if key is not None and _key_is_secret(key):
            return REDACTED
        if isinstance(value, str):
            return self.scrub_text(value)
        if isinstance(value, dict):
            return {k: self.scrub_value(k, v) for k, v in value.items()}
        if isinstance(value, list):
            return [self.scrub_value(None, item) for item in value]
        return value

    def scrub_payload(self, payload: dict[str, object]) -> dict[str, object]:
        scrubbed = self.scrub_value(None, payload)
        assert isinstance(scrubbed, dict)  # nosec B101 — payload is a dict by construction
        return scrubbed


def canonical_json(value: object) -> str:
    """Deterministic JSON serialization used for digesting and on-disk bundles."""
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def sanitized_digest(payload: object) -> str:
    """SHA-256 (``sha256:`` prefixed) of the canonicalized sanitized payload."""
    digest = hashlib.sha256(canonical_json(payload).encode("utf-8")).hexdigest()
    return f"sha256:{digest}"
