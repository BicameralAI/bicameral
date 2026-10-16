"""Redaction and digest tests (cloud#141).

No secret, token, JWT, or caller-declared confidential identifier may survive into a
produced evidence payload.
"""

from __future__ import annotations

from bicameral_cloud.evidence.redaction import REDACTED, Redactor, sanitized_digest


def test_secret_shaped_keys_are_redacted() -> None:
    redactor = Redactor()
    payload = {
        "service_account": "runtime@example.iam.gserviceaccount.com",
        "api_key": "super-secret-value",
        "nested": {"authorization": "Bearer abc123", "safe": "keep"},
    }
    scrubbed = redactor.scrub_payload(payload)
    assert scrubbed["api_key"] == REDACTED
    nested = scrubbed["nested"]
    assert isinstance(nested, dict)
    assert nested["authorization"] == REDACTED
    assert nested["safe"] == "keep"


def test_jwt_and_private_key_values_are_redacted() -> None:
    redactor = Redactor()
    jwt = (
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.aaaabbbbcccc"
    )
    pem = "-----BEGIN " + "PRIVATE KEY-----\nMIIB"
    payload = {"note": f"leaked {jwt} here", "pem": pem}
    scrubbed = redactor.scrub_payload(payload)
    assert jwt not in str(scrubbed["note"])
    assert REDACTED in str(scrubbed["note"])
    assert REDACTED in str(scrubbed["pem"])


def test_declared_identifiers_are_scrubbed_everywhere() -> None:
    redactor = Redactor(identifiers={"example-project": "gcp_project"})
    payload = {"sa": "runtime@example-project.iam.gserviceaccount.com"}
    scrubbed = redactor.scrub_payload(payload)
    assert "example-project" not in str(scrubbed["sa"])
    assert "<redacted:gcp_project>" in str(scrubbed["sa"])


def test_digest_is_stable_and_prefixed() -> None:
    digest = sanitized_digest({"b": 1, "a": 2})
    assert digest.startswith("sha256:")
    # Key order does not change the canonical digest.
    assert digest == sanitized_digest({"a": 2, "b": 1})
