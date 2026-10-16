"""Unit tests for hosted dashboard binding UI-state derivation (issue #89).

These pin the tested contract that the dashboard JS mirrors: the mapping from
the coarse Cloud ``BindingReadiness`` projection plus the daemon's truthful
``workspace_binding_available`` capability to the rendered next-action state,
and the fail-closed Bind CTA gating. Covers the acceptance states: unbound,
ready, repair-required, and unsupported binding.
"""

from __future__ import annotations

import pytest

from bicameral_cloud.dashboard_binding import (
    WORKSPACE_BIND_COMMAND,
    ProjectBindingUiState,
    bind_cta_available,
    derive_binding_ui_state,
    next_action_hint,
)
from bicameral_cloud.models.discovery import BindingReadiness


class TestDeriveBindingUiState:
    def test_ready_readiness_is_ready(self) -> None:
        for cap in (True, False, None):
            assert (
                derive_binding_ui_state(BindingReadiness.READY, cap)
                == ProjectBindingUiState.READY
            )

    @pytest.mark.parametrize(
        "readiness",
        [BindingReadiness.DEGRADED, BindingReadiness.UPGRADE_REQUIRED],
    )
    def test_degraded_or_upgrade_is_repair_required(
        self, readiness: BindingReadiness
    ) -> None:
        for cap in (True, False, None):
            assert (
                derive_binding_ui_state(readiness, cap)
                == ProjectBindingUiState.REPAIR_REQUIRED
            )

    @pytest.mark.parametrize(
        "readiness",
        [
            BindingReadiness.UNBOUND,
            BindingReadiness.UNPAIRED,
            BindingReadiness.UNKNOWN,
        ],
    )
    def test_unbound_with_capability_true_is_binding_available(
        self, readiness: BindingReadiness
    ) -> None:
        assert (
            derive_binding_ui_state(readiness, True)
            == ProjectBindingUiState.BINDING_AVAILABLE
        )

    @pytest.mark.parametrize(
        "readiness",
        [
            BindingReadiness.UNBOUND,
            BindingReadiness.UNPAIRED,
            BindingReadiness.UNKNOWN,
        ],
    )
    def test_unbound_with_capability_false_is_binding_unsupported(
        self, readiness: BindingReadiness
    ) -> None:
        assert (
            derive_binding_ui_state(readiness, False)
            == ProjectBindingUiState.BINDING_UNSUPPORTED
        )

    @pytest.mark.parametrize(
        "readiness",
        [
            BindingReadiness.UNBOUND,
            BindingReadiness.UNPAIRED,
            BindingReadiness.UNKNOWN,
        ],
    )
    def test_unbound_with_unknown_capability_is_discoverable_unbound(
        self, readiness: BindingReadiness
    ) -> None:
        # Fail-closed: an unknown daemon capability never becomes bindable.
        assert (
            derive_binding_ui_state(readiness, None)
            == ProjectBindingUiState.DISCOVERABLE_UNBOUND
        )


class TestBindCtaGating:
    def test_cta_only_for_binding_available(self) -> None:
        assert bind_cta_available(ProjectBindingUiState.BINDING_AVAILABLE) is True
        for state in ProjectBindingUiState:
            if state is not ProjectBindingUiState.BINDING_AVAILABLE:
                assert bind_cta_available(state) is False

    def test_unbound_no_capability_hides_cta(self) -> None:
        state = derive_binding_ui_state(BindingReadiness.UNBOUND, None)
        assert bind_cta_available(state) is False

    def test_unsupported_hides_cta_but_is_not_silent(self) -> None:
        state = derive_binding_ui_state(BindingReadiness.UNBOUND, False)
        assert bind_cta_available(state) is False
        # Fail-closed states still render a clear, non-empty next action.
        assert next_action_hint(state)


class TestNextActionHints:
    def test_every_state_has_a_hint(self) -> None:
        for state in ProjectBindingUiState:
            assert next_action_hint(state)

    def test_binding_available_hint_routes_to_mcp_daemon_path(self) -> None:
        hint = next_action_hint(ProjectBindingUiState.BINDING_AVAILABLE)
        assert WORKSPACE_BIND_COMMAND in hint

    def test_hints_do_not_leak_local_paths_or_secrets(self) -> None:
        forbidden = ["/home/", "/Users/", "api_key", "secret", "token", ".bicameral/"]
        for state in ProjectBindingUiState:
            hint = next_action_hint(state).lower()
            for token in forbidden:
                assert token.lower() not in hint
