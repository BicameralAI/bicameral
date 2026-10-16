"""Root pytest configuration.

Sets environment that must be in place before ``bicameral_cloud.app`` is first
imported: the FastAPI app reads its docs and rate-limit settings at import time.
The suite runs the shared app with the OpenAPI schema and docs exposed (so the
schema assertions in test_security_hardening work) and with the per-instance
rate limiter disabled (every TestClient request shares one client key and would
otherwise trip the limit mid-suite). The production defaults and the limiter are
covered in isolation by tests/test_openapi_docs_gate.py and tests/test_rate_limit.py.
"""

from __future__ import annotations

import os

os.environ["BICAMERAL_CLOUD_ENABLE_DOCS"] = "1"
os.environ["BICAMERAL_CLOUD_RATE_LIMIT_REQUESTS"] = "0"
