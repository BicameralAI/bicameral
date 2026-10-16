"""The dashboard third-party notices must describe the vendored bytes."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

STATIC = Path(__file__).parents[1] / "src" / "bicameral_cloud" / "static"
NOTICES = STATIC / "DASHBOARD_THIRD_PARTY_NOTICES.md"
DASHBOARD = STATIC / "dashboard"


def test_notices_bind_the_vendored_bundle() -> None:
    notices = NOTICES.read_text(encoding="utf-8")
    version = json.loads((DASHBOARD / "version.json").read_text(encoding="utf-8"))

    # Re-vendoring a different Bot build must also re-verify the notices.
    assert version["bot_commit"] in notices
    assert version["bundle_sha256"] in notices
    scripts = sorted((DASHBOARD / "assets").glob("*.js"))
    assert scripts
    for script in scripts:
        assert script.name in notices
        assert hashlib.sha256(script.read_bytes()).hexdigest() in notices


def test_notices_carry_each_bundled_license() -> None:
    notices = NOTICES.read_text(encoding="utf-8")
    for heading, holder in (
        ("## svelte 5.56.0", "Svelte Contributors"),
        ("## esm-env 1.2.2", "Copyright 2022 Benjamin McCann"),
        ("## vite 6.4.3", "VoidZero Inc. and Vite contributors"),
    ):
        assert heading in notices
        assert holder in notices
    assert notices.count("The above copyright notice and this permission notice") == 3
