"""Cross-suite MCP session defaults.

Legacy transport tests exercise individual Product-scoped tools directly. Give
those tests an explicit synthetic selection so they continue to test their
original behavior; product-selection tests own the unselected boundary.
"""

from __future__ import annotations

import pytest

import server


@pytest.fixture(autouse=True)
def _explicit_test_product(monkeypatch: pytest.MonkeyPatch, request: pytest.FixtureRequest) -> None:
    if request.path.name != "test_product_selection.py":
        monkeypatch.setattr(server, "_selected_product_id", "prd_test")
