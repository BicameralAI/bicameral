"""Managed MCP candidate-selection transport and display contract.

This module implements the MCP-owned portion of BicameralAI/bicameral-mcp#785.
It preserves daemon-authored CandidateSet, preview, receipt, failure, heartbeat,
and end payloads while adding accessible display metadata. It never evaluates
selection closure, grants authority, expands a subset, confirms for a human, or
writes canonical Product state.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from copy import deepcopy
from dataclasses import dataclass
from typing import Any
from uuid import uuid4

CANDIDATE_SELECTION_CONTRACT_VERSION = 1
CANDIDATE_SESSION_CONTRACT_VERSION = 1

PREPARE_TOOL = "bicameral.candidate_selection.prepare"
COMMIT_TOOL = "bicameral.candidate_selection.commit"
HEARTBEAT_TOOL = "bicameral.candidate_session.heartbeat"
END_TOOL = "bicameral.candidate_session.end"

COMMAND_BY_TOOL: dict[str, str] = {
    PREPARE_TOOL: "candidate_selection.prepare",
    COMMIT_TOOL: "candidate_selection.commit",
    HEARTBEAT_TOOL: "candidate_session.heartbeat",
    END_TOOL: "candidate_session.end",
}

# Default selection policy (product UX). Commit still needs human_confirmed.
SELECTION_MODE_ACCEPT_DEFAULTS = "accept_defaults"
SELECTION_MODE_NOT_TRACK = "not_track"
SELECTION_MODE_EXPLICIT = "explicit"
DEFAULT_SELECTION_POLICY = "auto_promote_unless_rejected_or_contradiction"

_POSITIONAL_AUTHORITY_KEYS = frozenset(
    {
        "candidate_numbers",
        "candidate_positions",
        "display_numbers",
        "display_positions",
        "candidate_indexes",
        "candidate_indices",
    }
)

_SELECTION_FAILURE_CODES = frozenset(
    {
        "unsupported_contract_version",
        "malformed_identity",
        "empty_selection",
        "overlapping_dispositions",
        "candidate_set_not_found",
        "candidate_set_stale",
        "candidate_set_digest_mismatch",
        "candidate_set_identity_mismatch",
        "candidate_not_found",
        "candidate_not_pending",
        "dependency_closure_required",
        "actor_unauthorized",
        "current_product_owner_required",
        "governing_spec_changed",
        "accepted_model_base_changed",
        "policy_changed",
        "preview_not_found",
        "preview_stale",
        "human_confirmation_required",
        "confirmation_rejected",
        "atomic_append_unsupported",
        "atomic_append_failed",
        "partial_canonical_state_detected",
        "replay_verification_failed",
        "session_reconciliation_failed",
    }
)

_SESSION_FAILURE_CODES = frozenset(
    {
        "unsupported_contract_version",
        "malformed_identity",
        "candidate_set_not_found",
        "candidate_set_stale",
        "candidate_not_in_set",
        "session_lease_expired",
    }
)

# ── UX design disposition (bic:spec-governance-gate / CONF-MCP-DISPLAY-01) ──
#
# Surface is the packaged coding-host MCP session, not the hosted browser SPA.
# page_or_route locators are app:// MCP tool surfaces. Color is never authority.

UX_SURFACE_NAME = "mcp_candidate_selection"
UX_PAGE_CANDIDATE_SET = "app://mcp/tools/managed_preflight#candidate_set"
UX_PAGE_PREPARE = f"app://mcp/tools/{PREPARE_TOOL}"
UX_PAGE_COMMIT = f"app://mcp/tools/{COMMIT_TOOL}"
UX_PAGE_HEARTBEAT = f"app://mcp/tools/{HEARTBEAT_TOOL}"
UX_PAGE_END = f"app://mcp/tools/{END_TOOL}"
UX_PAGE_FAILURE = "app://mcp/tools/candidate_selection#failure"

# product_state → (title, visible_result, next_user_action)
PRESENTATION_STATE_MATRIX: dict[str, dict[str, str]] = {
    "candidate_set_review": {
        "title": "Review proposed Decision candidates",
        "visible_result": (
            "Exact governing-spec text (when supplied) with accessible source-span "
            "labels. Rows lead with proposed Decision text and source context; stable "
            "candidate IDs remain authoritative for transport. Default policy: "
            "auto-promote every non-contradiction candidate unless you explicitly "
            "reject it. Contradictions are held out of the default promote set. "
            "Session-only reject-all creates no durable suppression."
        ),
        "next_user_action": (
            "Use count-bearing actions (promote all N non-contradictory, reject all N "
            "for this session, or explicit promote/reject IDs). Then prepare and "
            "explicitly confirm."
        ),
    },
    "confirmation_required_promote": {
        "title": "Confirm promotion",
        "visible_result": (
            "Exact daemon preview of promote/reject effects, pending count, Product, "
            "and actor. Label states exact promote count (e.g. “Promote 3 selected "
            "candidates”). Resulting signoff is proposed (not owner-policy-approved). "
            "Agent cannot confirm. Opaque challenge is transport-only."
        ),
        "next_user_action": (
            "Read the confirmation summary, then explicitly confirm to commit, or "
            "revise the subset and prepare again."
        ),
    },
    "confirmation_required_promote_and_approve": {
        "title": "Confirm promotion with owner auto-approve",
        "visible_result": (
            "Exact daemon preview. Label states exact count and owner-policy approval "
            "(e.g. “Promote and approve 3 selected candidates as Product Owner”). "
            "Agent cannot confirm. Opaque challenge is transport-only."
        ),
        "next_user_action": (
            "Read the confirmation summary, then explicitly confirm (one action "
            "promotes and approves), or revise and reprepare."
        ),
    },
    "committed": {
        "title": "Selection committed",
        "visible_result": (
            "Daemon receipt with promoted/rejected IDs, Decision/event IDs when present, "
            "signoff, pending count, and replay/session reconciliation flags. Not release "
            "or Product acceptance."
        ),
        "next_user_action": "Continue coding, heartbeat the session, or end the session.",
    },
    "session_heartbeat_ok": {
        "title": "Session lease extended",
        "visible_result": "Daemon acknowledged heartbeat for the exact host session/lease.",
        "next_user_action": "Continue reviewing candidates or prepare a subset.",
    },
    "session_ended": {
        "title": "Candidate session ended",
        "visible_result": (
            "Transient CandidateSet and dispositions erased. Erasure is not rejection "
            "and creates no canonical event."
        ),
        "next_user_action": "Start a new preflight/session if more review is needed.",
    },
    "error_dependency_closure_required": {
        "title": "Selection incomplete",
        "visible_result": (
            "Daemon listed missing dependency candidate IDs. Subset was not expanded."
        ),
        "next_user_action": "Revise promote/reject IDs to close dependencies and reprepare.",
    },
    "error_candidate_set_stale": {
        "title": "Candidate set is stale",
        "visible_result": "Generation/digest no longer matches the active session set.",
        "next_user_action": "Rerun preflight for the current CandidateSet generation.",
    },
    "error_preview_stale": {
        "title": "Preview is stale",
        "visible_result": "The confirmation challenge no longer binds current state.",
        "next_user_action": "Prepare the intended stable-id subset again.",
    },
    "error_session_lease_expired": {
        "title": "Session lease expired",
        "visible_result": "Transient candidates were erased after reconnect grace.",
        "next_user_action": "Start a new session; do not reuse old IDs or previews.",
    },
    "error_human_confirmation_required": {
        "title": "Human confirmation required",
        "visible_result": "Commit was refused because no explicit human action was supplied.",
        "next_user_action": "Have the human explicitly confirm; agents must not self-confirm.",
    },
    "error_unsupported_contract_version": {
        "title": "Unsupported contract version",
        "visible_result": "MCP/daemon contract versions do not match.",
        "next_user_action": "Upgrade matching MCP and daemon components.",
    },
    "error_generic": {
        "title": "Selection or session error",
        "visible_result": "Typed daemon failure preserved without rewriting authority claims.",
        "next_user_action": "Follow the exact recovery guidance on the failure.",
    },
}


def _ux_block(
    presentation_state: str,
    *,
    page_or_route: str,
    extra: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    matrix = PRESENTATION_STATE_MATRIX.get(
        presentation_state, PRESENTATION_STATE_MATRIX["error_generic"]
    )
    block: dict[str, Any] = {
        "surface": UX_SURFACE_NAME,
        "page_or_route": page_or_route,
        "presentation_state": presentation_state,
        "title": matrix["title"],
        "visible_result": matrix["visible_result"],
        "next_user_action": matrix["next_user_action"],
        "color_is_authority": False,
        "display_position_is_authority": False,
        "agent_may_confirm": False,
        "is_product_acceptance": False,
        "is_release_activation": False,
    }
    if extra:
        block.update(dict(extra))
    return block


@dataclass(frozen=True)
class SurfaceContractError(ValueError):
    """A typed, fail-closed MCP surface error."""

    code: str
    detail: str

    def __str__(self) -> str:
        return f"{self.code}: {self.detail}"


def _schema(properties: dict[str, Any], required: list[str]) -> dict[str, Any]:
    return {
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": False,
    }


def _access_schema() -> dict[str, Any]:
    return _schema(
        {
            "contract_version": {"type": "integer", "const": 1},
            "product_id": {"type": "string", "minLength": 1},
            "host_session_id": {"type": "string", "minLength": 1},
            "session_lease_id": {"type": "string", "minLength": 1},
            "candidate_set_generation": {"type": "integer", "minimum": 1},
        },
        [
            "contract_version",
            "product_id",
            "host_session_id",
            "session_lease_id",
            "candidate_set_generation",
        ],
    )


TOOL_SCHEMAS: dict[str, dict[str, Any]] = {
    PREPARE_TOOL: _schema(
        {
            "contract_version": {"type": "integer", "const": 1, "default": 1},
            "selection_operation_id": {
                "type": "string",
                "format": "uuid",
                "description": (
                    "Optional idempotency identity. MCP generates one when omitted; "
                    "retries should reuse the daemon-returned identity."
                ),
            },
            "candidate_set_id": {"type": "string", "format": "uuid"},
            "candidate_set_generation": {"type": "integer", "minimum": 1},
            "candidate_set_digest": {
                "type": "string",
                "pattern": "^sha256:[0-9a-f]{64}$",
            },
            "product_id": {"type": "string", "minLength": 1},
            "host_session_id": {"type": "string", "minLength": 1},
            "session_lease_id": {"type": "string", "minLength": 1},
            "selection_mode": {
                "type": "string",
                "enum": [
                    SELECTION_MODE_ACCEPT_DEFAULTS,
                    SELECTION_MODE_NOT_TRACK,
                    SELECTION_MODE_EXPLICIT,
                ],
                "description": (
                    "accept_defaults: auto-promote non-contradiction candidates; "
                    "not_track: reject all; explicit: use promote/reject id lists only."
                ),
            },
            "candidates": {
                "type": "array",
                "description": (
                    "Required for accept_defaults / not_track so MCP can apply the "
                    "default policy. Prefer the daemon CandidateSet.candidates array."
                ),
                "items": {"type": "object", "additionalProperties": True},
            },
            "promote_candidate_ids": {
                "type": "array",
                "items": {"type": "string", "format": "uuid"},
                "uniqueItems": True,
                "default": [],
            },
            "reject_candidate_ids": {
                "type": "array",
                "items": {"type": "string", "format": "uuid"},
                "uniqueItems": True,
                "default": [],
                "description": (
                    "Explicit rejects. Under accept_defaults these are withheld from auto-promote."
                ),
            },
            "also_promote_candidate_ids": {
                "type": "array",
                "items": {"type": "string", "format": "uuid"},
                "uniqueItems": True,
                "default": [],
                "description": (
                    "Optional under accept_defaults: force-promote held contradiction "
                    "candidates the human chose to include."
                ),
            },
            "actor_id": {
                "type": "string",
                "minLength": 1,
                "description": (
                    "Authenticated human actor identity. Agent or MCP identity is not "
                    "a substitute for daemon authority validation."
                ),
            },
        },
        [
            "candidate_set_id",
            "candidate_set_generation",
            "candidate_set_digest",
            "product_id",
            "host_session_id",
            "session_lease_id",
            "actor_id",
        ],
    ),
    COMMIT_TOOL: _schema(
        {
            "contract_version": {"type": "integer", "const": 1, "default": 1},
            "selection_operation_id": {"type": "string", "format": "uuid"},
            "selection_digest": {
                "type": "string",
                "pattern": "^sha256:[0-9a-f]{64}$",
            },
            "actor_id": {"type": "string", "minLength": 1},
            "human_confirmed": {
                "type": "boolean",
                "const": True,
                "description": (
                    "Must be supplied only after an explicit human confirmation action. "
                    "MCP never defaults or infers this value."
                ),
            },
            "confirmation_token": {
                "type": "object",
                "description": (
                    "Opaque daemon-issued single-use token. MCP transports it without "
                    "minting, validating, persisting, logging, or rewriting it."
                ),
                "additionalProperties": True,
            },
        },
        [
            "selection_operation_id",
            "selection_digest",
            "actor_id",
            "human_confirmed",
            "confirmation_token",
        ],
    ),
    HEARTBEAT_TOOL: _schema({"access": _access_schema()}, ["access"]),
    END_TOOL: _schema({"access": _access_schema()}, ["access"]),
}


def tool_definitions() -> tuple[dict[str, Any], ...]:
    """Return renderer-neutral MCP tool definitions for later server wiring."""

    descriptions = {
        PREPARE_TOOL: (
            "Submit an exact stable-id promote/reject subset to the daemon. "
            "Default product policy: selection_mode=accept_defaults auto-promotes every "
            "candidate without contradiction_refs unless listed in reject_candidate_ids; "
            "contradictions are held unless also_promote_candidate_ids includes them; "
            "selection_mode=not_track rejects all for this session only "
            "(no durable suppression). Human labels are count-bearing. "
            "The daemon validates currentness, dependency closure, authority, and policy "
            "and returns an exact no-write preview. Commit still requires explicit human "
            "confirmation. Display numbers and positions are forbidden."
        ),
        COMMIT_TOOL: (
            "Relay an explicit human-confirmed daemon token for one exact preview. MCP "
            "cannot self-confirm, alter effects, set policy, or append canonical state."
        ),
        HEARTBEAT_TOOL: (
            "Extend the exact daemon-owned CandidateSet session lease. This operation "
            "does not enumerate candidates or create Product state."
        ),
        END_TOOL: (
            "End the exact CandidateSet session and erase transient candidate and "
            "disposition state. Erasure is not rejection and creates no canonical event."
        ),
    }
    return tuple(
        {
            "name": name,
            "description": descriptions[name],
            "inputSchema": deepcopy(TOOL_SCHEMAS[name]),
        }
        for name in (PREPARE_TOOL, COMMIT_TOOL, HEARTBEAT_TOOL, END_TOOL)
    )


def command_for_tool(tool_name: str) -> str:
    try:
        return COMMAND_BY_TOOL[tool_name]
    except KeyError as exc:
        raise SurfaceContractError(
            "unsupported_candidate_selection_tool",
            f"Unsupported Managed MCP candidate-selection tool: {tool_name}",
        ) from exc


def shape_command_params(
    tool_name: str,
    arguments: Mapping[str, Any],
    *,
    operation_id_factory: Callable[[], str] | None = None,
) -> dict[str, Any]:
    """Shape one candidate-selection command without taking daemon authority."""

    if tool_name == PREPARE_TOOL:
        return build_prepare_params(arguments, operation_id_factory=operation_id_factory)
    if tool_name == COMMIT_TOOL:
        return build_commit_params(arguments)
    if tool_name in {HEARTBEAT_TOOL, END_TOOL}:
        return build_session_params(arguments)
    command_for_tool(tool_name)
    raise AssertionError("unreachable")


# ── Default selection policy helpers ──────────────────────────────────────
#
# Auto-promote every candidate that is not explicitly rejected and has no
# contradiction_refs. Contradictions are held (not promoted) until the human
# explicitly includes them. not_track = session-only reject all (no durable
# suppression). Commit still requires explicit human_confirmed (MCP never
# self-confirms). Human-facing labels are consequence-first and count-bearing
# (MCP #809); selection_mode machine keys remain stable for protocol callers.


SESSION_ONLY_REJECT_DETAIL = (
    "Session-only rejection for the active CandidateSet: no canonical event, "
    "tombstone, or durable suppression is created. Ending or expiring the session "
    "erases the disposition; the same ideas may be proposed again later."
)


def _candidate_has_contradiction(candidate: Mapping[str, Any]) -> bool:
    refs = candidate.get("contradiction_refs")
    return isinstance(refs, list) and len(refs) > 0


def _iter_candidate_maps(candidates: Any) -> list[dict[str, Any]]:
    if not isinstance(candidates, list):
        raise SurfaceContractError(
            "candidate_set_invalid",
            "candidates must be an array to resolve default selection.",
        )
    out: list[dict[str, Any]] = []
    for item in candidates:
        if not isinstance(item, Mapping):
            raise SurfaceContractError(
                "candidate_set_invalid",
                "Every candidate must be an object.",
            )
        out.append(dict(item))
    return out


def _source_context_labels(candidate: Mapping[str, Any]) -> list[str]:
    """Human-readable source labels from source_spans (not UUID authority)."""

    spans = candidate.get("source_spans") or []
    if not isinstance(spans, list):
        return []
    labels: list[str] = []
    for span in spans:
        if not isinstance(span, Mapping):
            continue
        path = span.get("artifact_path")
        if isinstance(path, str) and path.strip():
            start = span.get("start_byte")
            end = span.get("end_byte")
            if isinstance(start, int) and isinstance(end, int):
                labels.append(f"{path} bytes {start}–{end}")
            else:
                labels.append(path)
    return labels


def _decision_lead_label(
    *,
    proposed_decision: Any,
    candidate_id: str,
    source_labels: list[str] | None = None,
) -> str:
    """Primary human label: Decision text first; stable ID is not the lead."""

    text = proposed_decision.strip() if isinstance(proposed_decision, str) else ""
    if not text:
        text = "(no proposed Decision text)"
    if source_labels:
        return f"{text} — {'; '.join(source_labels)}"
    return text


def _count_bearing_promote_label(count: int, *, owner_policy_approve: bool) -> str:
    noun = "candidate" if count == 1 else "candidates"
    if owner_policy_approve:
        return f"Promote and approve {count} selected {noun} as Product Owner"
    return f"Promote {count} selected {noun}"


def _count_bearing_accept_defaults_label(promote_count: int) -> str:
    noun = "candidate" if promote_count == 1 else "candidates"
    return f"Promote all {promote_count} non-contradictory {noun}"


def _count_bearing_reject_all_label(total_count: int) -> str:
    noun = "candidate" if total_count == 1 else "candidates"
    return f"Reject all {total_count} {noun} for this session"


def compute_default_selection(
    candidates: Any,
    *,
    explicitly_rejected_ids: list[str] | None = None,
) -> dict[str, Any]:
    """Return promote/reject/held sets under the default auto-promote policy.

    - promote: no contradiction_refs and not explicitly rejected
    - reject: explicit rejects only (caller-supplied)
    - held_for_contradiction: has contradiction_refs and not rejected

    Human option labels are count-bearing (MCP #809). Machine selection_mode
    keys remain ``accept_defaults`` / ``not_track``.
    """

    rejected = list(explicitly_rejected_ids or [])
    rejected_set = set(rejected)
    promote: list[str] = []
    held: list[str] = []
    rows: list[dict[str, Any]] = []

    for cand in _iter_candidate_maps(candidates):
        cid = _required_string(cand.get("candidate_id"), "candidate_id")
        has_contra = _candidate_has_contradiction(cand)
        if cid in rejected_set:
            default_disposition = "reject"
        elif has_contra:
            default_disposition = "hold_contradiction"
            held.append(cid)
        else:
            default_disposition = "auto_promote"
            promote.append(cid)
        rows.append(
            {
                "candidate_id": cid,
                "default_disposition": default_disposition,
                "has_contradiction": has_contra,
                "proposed_decision": cand.get("proposed_decision"),
            }
        )

    promote_count = len(promote)
    reject_count = len(rejected)
    held_count = len(held)
    total_count = len(rows)
    # Remaining pending under defaults = held contradictions (not yet decided).
    remaining_pending_count = held_count

    return {
        "policy": DEFAULT_SELECTION_POLICY,
        "promote_candidate_ids": promote,
        "reject_candidate_ids": rejected,
        "held_for_contradiction_ids": held,
        "promote_count": promote_count,
        "reject_count": reject_count,
        "remaining_pending_count": remaining_pending_count,
        "held_contradiction_count": held_count,
        "candidates": rows,
        "options": {
            "accept_defaults": {
                "selection_mode": SELECTION_MODE_ACCEPT_DEFAULTS,
                "label": _count_bearing_accept_defaults_label(promote_count),
                "detail": (
                    f"Promote all {promote_count} non-contradictory candidates; keep "
                    f"{reject_count} explicit reject(s); leave {held_count} "
                    "contradiction(s) unpromoted unless you add them explicitly. "
                    "Does not commit until explicit human confirmation after prepare."
                ),
                "promote_count": promote_count,
                "reject_count": reject_count,
                "held_contradiction_count": held_count,
                "remaining_pending_count": remaining_pending_count,
                "will_append_canonical_events_on_confirm": promote_count > 0,
            },
            "not_track": {
                "selection_mode": SELECTION_MODE_NOT_TRACK,
                "label": _count_bearing_reject_all_label(total_count),
                "detail": SESSION_ONLY_REJECT_DETAIL,
                "promote_count": 0,
                "reject_count": total_count,
                "held_contradiction_count": 0,
                "remaining_pending_count": 0,
                "session_only": True,
                "creates_canonical_event": False,
                "creates_durable_suppression": False,
                "will_append_canonical_events_on_confirm": False,
            },
        },
    }


def compute_not_track_selection(candidates: Any) -> dict[str, Any]:
    """Reject every candidate for this session only (no durable suppression)."""

    ids = [
        _required_string(c.get("candidate_id"), "candidate_id")
        for c in _iter_candidate_maps(candidates)
    ]
    total = len(ids)
    return {
        "policy": "not_track_reject_all",
        "promote_candidate_ids": [],
        "reject_candidate_ids": ids,
        "held_for_contradiction_ids": [],
        "promote_count": 0,
        "reject_count": total,
        "remaining_pending_count": 0,
        "held_contradiction_count": 0,
        "session_only": True,
        "creates_canonical_event": False,
        "creates_durable_suppression": False,
        "label": _count_bearing_reject_all_label(total),
        "detail": SESSION_ONLY_REJECT_DETAIL,
    }


def _resolve_prepare_subset(arguments: Mapping[str, Any]) -> tuple[list[str], list[str], str]:
    """Resolve promote/reject lists from selection_mode or explicit ids."""

    mode_raw = arguments.get("selection_mode")
    mode = mode_raw.strip() if isinstance(mode_raw, str) else SELECTION_MODE_EXPLICIT

    if mode in {SELECTION_MODE_ACCEPT_DEFAULTS, SELECTION_MODE_NOT_TRACK}:
        candidates = arguments.get("candidates")
        if candidates is None and isinstance(arguments.get("candidate_set"), Mapping):
            candidates = arguments["candidate_set"].get("candidates")  # type: ignore[index]
        if candidates is None:
            raise SurfaceContractError(
                "candidates_required_for_selection_mode",
                f"selection_mode={mode} requires candidates[] (or candidate_set.candidates) "
                "so MCP can apply the default / not-track policy.",
            )
        if mode == SELECTION_MODE_NOT_TRACK:
            resolved = compute_not_track_selection(candidates)
        else:
            explicit_reject = _string_list(
                arguments.get("reject_candidate_ids"), "reject_candidate_ids"
            )
            resolved = compute_default_selection(
                candidates, explicitly_rejected_ids=explicit_reject
            )
            # Optional: human may still force-promote held contradictions.
            extra_promote = _string_list(
                arguments.get("also_promote_candidate_ids"), "also_promote_candidate_ids"
            )
            promote = list(resolved["promote_candidate_ids"])
            held = set(resolved["held_for_contradiction_ids"])
            reject_set = set(resolved["reject_candidate_ids"])
            for cid in extra_promote:
                if cid in reject_set:
                    raise SurfaceContractError(
                        "overlapping_dispositions",
                        f"Cannot also_promote rejected candidate {cid}.",
                    )
                if cid not in promote:
                    promote.append(cid)
                held.discard(cid)
            resolved = {
                **resolved,
                "promote_candidate_ids": promote,
                "held_for_contradiction_ids": sorted(held),
            }
        return (
            list(resolved["promote_candidate_ids"]),
            list(resolved["reject_candidate_ids"]),
            mode,
        )

    if mode != SELECTION_MODE_EXPLICIT and mode_raw is not None:
        raise SurfaceContractError(
            "unsupported_selection_mode",
            f"selection_mode must be accept_defaults, not_track, or explicit (received {mode!r}).",
        )

    promote = _string_list(arguments.get("promote_candidate_ids"), "promote_candidate_ids")
    reject = _string_list(arguments.get("reject_candidate_ids"), "reject_candidate_ids")
    return promote, reject, SELECTION_MODE_EXPLICIT


def build_prepare_params(
    arguments: Mapping[str, Any],
    *,
    operation_id_factory: Callable[[], str] | None = None,
) -> dict[str, Any]:
    """Build CandidateSelectionV1 from stable ids only.

    Supports selection_mode:
      - accept_defaults: auto-promote non-contradiction; honor reject_candidate_ids;
        optional also_promote_candidate_ids for held contradictions
      - not_track: reject all candidates for this session only (no durable suppression)
      - explicit (default): promote_candidate_ids / reject_candidate_ids as given

    MCP may generate an operation identity because it is an idempotency key, not
    Product authority. The daemon remains responsible for every currentness,
    closure, policy, and authority decision. Commit still requires human_confirmed.
    """

    forbidden = sorted(_POSITIONAL_AUTHORITY_KEYS.intersection(arguments))
    if forbidden:
        raise SurfaceContractError(
            "positional_authority_forbidden",
            "Candidate selection must use stable candidate ids, not display positions: "
            + ", ".join(forbidden),
        )

    contract_version = _contract_version(arguments, CANDIDATE_SELECTION_CONTRACT_VERSION)
    promote, reject, mode = _resolve_prepare_subset(arguments)
    if not promote and not reject:
        raise SurfaceContractError("empty_selection", "Select at least one stable candidate id.")
    overlap = sorted(set(promote).intersection(reject))
    if overlap:
        raise SurfaceContractError(
            "overlapping_dispositions",
            "The same candidate cannot be promoted and rejected: " + ", ".join(overlap),
        )

    factory = operation_id_factory or (lambda: str(uuid4()))
    operation_id = arguments.get("selection_operation_id") or factory()

    return {
        "contract_version": contract_version,
        "selection_operation_id": _required_string(operation_id, "selection_operation_id"),
        "candidate_set_id": _required_string(arguments.get("candidate_set_id"), "candidate_set_id"),
        "candidate_set_generation": _required_positive_int(
            arguments.get("candidate_set_generation"), "candidate_set_generation"
        ),
        "candidate_set_digest": _required_string(
            arguments.get("candidate_set_digest"), "candidate_set_digest"
        ),
        "product_id": _required_string(arguments.get("product_id"), "product_id"),
        "host_session_id": _required_string(arguments.get("host_session_id"), "host_session_id"),
        "session_lease_id": _required_string(arguments.get("session_lease_id"), "session_lease_id"),
        "promote_candidate_ids": promote,
        "reject_candidate_ids": reject,
        "actor_id": _required_string(arguments.get("actor_id"), "actor_id"),
    }


def build_commit_params(arguments: Mapping[str, Any]) -> dict[str, Any]:
    """Build CandidateSelectionCommitV1 after explicit human confirmation."""

    contract_version = _contract_version(arguments, CANDIDATE_SELECTION_CONTRACT_VERSION)
    if arguments.get("human_confirmed") is not True:
        raise SurfaceContractError(
            "human_confirmation_required",
            "Commit requires an explicit human confirmation action; MCP cannot infer it.",
        )
    token = arguments.get("confirmation_token")
    if not isinstance(token, Mapping) or not token:
        raise SurfaceContractError(
            "confirmation_token_required",
            "Commit requires the opaque token returned by the daemon preview.",
        )

    return {
        "contract_version": contract_version,
        "selection_operation_id": _required_string(
            arguments.get("selection_operation_id"), "selection_operation_id"
        ),
        "selection_digest": _required_string(arguments.get("selection_digest"), "selection_digest"),
        "actor_id": _required_string(arguments.get("actor_id"), "actor_id"),
        "human_confirmed": True,
        "confirmation_token": deepcopy(dict(token)),
    }


def build_session_params(arguments: Mapping[str, Any]) -> dict[str, Any]:
    """Build heartbeat/end access without deriving Product or session identity."""

    raw_access = arguments.get("access")
    if not isinstance(raw_access, Mapping):
        raise SurfaceContractError("session_access_required", "Expected an access object.")
    access = {
        "contract_version": _contract_version(raw_access, CANDIDATE_SESSION_CONTRACT_VERSION),
        "product_id": _required_string(raw_access.get("product_id"), "access.product_id"),
        "host_session_id": _required_string(
            raw_access.get("host_session_id"), "access.host_session_id"
        ),
        "session_lease_id": _required_string(
            raw_access.get("session_lease_id"), "access.session_lease_id"
        ),
        "candidate_set_generation": _required_positive_int(
            raw_access.get("candidate_set_generation"),
            "access.candidate_set_generation",
        ),
    }
    return {"access": access}


def render_candidate_set(
    candidate_set: Mapping[str, Any],
    *,
    exact_spec_text: str | None = None,
    annotation_colors: Mapping[str, str] | None = None,
) -> dict[str, Any]:
    """Render CandidateSetV1 while preserving the complete daemon object.

    Unknown and future daemon-authored fields remain present under
    ``daemon_candidate_set``. The human presentation is an additive projection,
    never a narrowing allowlist and never authority.
    """

    _require_payload_version(candidate_set, CANDIDATE_SESSION_CONTRACT_VERSION)
    raw = deepcopy(dict(candidate_set))
    candidates = raw.get("candidates")
    if not isinstance(candidates, list):
        raise SurfaceContractError(
            "candidate_set_invalid",
            "CandidateSetV1.candidates must be an array.",
        )

    colors = dict(annotation_colors or {})
    rendered_candidates: list[dict[str, Any]] = []
    annotations: list[dict[str, Any]] = []

    for candidate in candidates:
        if not isinstance(candidate, Mapping):
            raise SurfaceContractError(
                "candidate_set_invalid",
                "Every CandidateSetV1 candidate must be an object.",
            )
        candidate_raw = deepcopy(dict(candidate))
        candidate_id = _required_string(candidate_raw.get("candidate_id"), "candidate_id")
        spans = candidate_raw.get("source_spans") or []
        if not isinstance(spans, list):
            raise SurfaceContractError(
                "candidate_set_invalid",
                f"Candidate {candidate_id} source_spans must be an array.",
            )

        accessible_spans: list[dict[str, Any]] = []
        for span_index, span in enumerate(spans, start=1):
            if not isinstance(span, Mapping):
                raise SurfaceContractError(
                    "candidate_set_invalid",
                    f"Candidate {candidate_id} contains a non-object source span.",
                )
            span_raw = deepcopy(dict(span))
            path = _required_string(span_raw.get("artifact_path"), "source_span.artifact_path")
            start = _required_nonnegative_int(span_raw.get("start_byte"), "source_span.start_byte")
            end = _required_nonnegative_int(span_raw.get("end_byte"), "source_span.end_byte")
            if end < start:
                raise SurfaceContractError(
                    "candidate_set_invalid",
                    f"Candidate {candidate_id} contains an inverted source span.",
                )
            decision_for_span = candidate_raw.get("proposed_decision")
            decision_text = (
                decision_for_span.strip()
                if isinstance(decision_for_span, str) and decision_for_span.strip()
                else "(no proposed Decision text)"
            )
            annotation_id = f"candidate:{candidate_id}:span:{span_index}"
            # Decision text leads; stable ID is retained as a field, not the lead label.
            label = f"{decision_text} — {path}, bytes {start} through {end}"
            accessible = {
                "annotation_id": annotation_id,
                "candidate_id": candidate_id,
                "label": label,
                "span": span_raw,
            }
            color = colors.get(candidate_id)
            if color:
                accessible["color"] = color
            accessible_spans.append(accessible)
            annotations.append(accessible)

        has_contra = _candidate_has_contradiction(candidate_raw)
        default_disposition = "hold_contradiction" if has_contra else "auto_promote"
        source_labels = _source_context_labels(candidate_raw)
        proposed = candidate_raw.get("proposed_decision")
        human_label = _decision_lead_label(
            proposed_decision=proposed,
            candidate_id=candidate_id,
            source_labels=source_labels,
        )
        rendered_candidates.append(
            {
                "candidate_id": candidate_id,
                # Primary human label: Decision + source; UUID is not the lead.
                "label": human_label,
                "proposed_decision": proposed,
                "source_context": source_labels,
                "disposition": candidate_raw.get("disposition"),
                "dependency_candidate_ids": deepcopy(
                    candidate_raw.get("dependency_candidate_ids") or []
                ),
                "contradiction_refs": deepcopy(candidate_raw.get("contradiction_refs") or []),
                "source_annotations": accessible_spans,
                # Not yet committed — default *intent* for UX, not authority.
                "selected": False,
                "accepted": False,
                "approved": False,
                "default_disposition": default_disposition,
                "has_contradiction": has_contra,
            }
        )

    default_selection = compute_default_selection(candidates)
    binding = raw.get("binding") if isinstance(raw.get("binding"), Mapping) else {}
    product_id = binding.get("product_id") if isinstance(binding, Mapping) else None

    output: dict[str, Any] = {
        "display_contract": "MCPDisplayContractV1",
        "contract_version": CANDIDATE_SESSION_CONTRACT_VERSION,
        "daemon_candidate_set": raw,
        "presentation": {
            "candidate_set_id": raw.get("candidate_set_id"),
            "candidate_set_generation": raw.get("candidate_set_generation"),
            "candidate_set_digest": raw.get("candidate_set_digest"),
            "binding": deepcopy(raw.get("binding") or {}),
            "session_lease_id": raw.get("session_lease_id"),
            "lease_expires_at": raw.get("lease_expires_at"),
            "coverage_limitations": deepcopy(raw.get("coverage_limitations") or []),
            "candidates": rendered_candidates,
            "annotations": annotations,
            "annotation_semantics": {
                "color_is_optional": True,
                "color_is_authority": False,
                "stable_candidate_id_is_authority": True,
                "display_position_is_authority": False,
                "decision_text_leads_human_label": True,
            },
            "default_selection": default_selection,
            "selection_policy": DEFAULT_SELECTION_POLICY,
            "selection_options": default_selection["options"],
            "effect_counts": {
                "promote_count": default_selection["promote_count"],
                "reject_count": default_selection["reject_count"],
                "remaining_pending_count": default_selection["remaining_pending_count"],
                "held_contradiction_count": default_selection["held_contradiction_count"],
            },
            "product_id": product_id,
        },
        "authority": {
            "candidate_set": "daemon_transient_working_state",
            "candidate_disposition": "daemon_session_working_state",
            "canonical_write": "none",
            "selection_status": "defaults_proposed_not_committed",
            "approval_status": "not_approved",
        },
        "ux": _ux_block(
            "candidate_set_review",
            page_or_route=UX_PAGE_CANDIDATE_SET,
            extra={
                "candidate_count": len(rendered_candidates),
                "auto_promote_count": default_selection["promote_count"],
                "reject_count": default_selection["reject_count"],
                "remaining_pending_count": default_selection["remaining_pending_count"],
                "held_contradiction_count": default_selection["held_contradiction_count"],
                "selection_policy": DEFAULT_SELECTION_POLICY,
                "primary_action_labels": {
                    "accept_defaults": default_selection["options"]["accept_defaults"]["label"],
                    "not_track": default_selection["options"]["not_track"]["label"],
                },
                "primary_actions": [
                    "accept_defaults",
                    "not_track",
                    "reject_specific",
                ],
                "product_id": product_id,
            },
        ),
        "next_actions": [
            PREPARE_TOOL,
            HEARTBEAT_TOOL,
            END_TOOL,
        ],
    }
    if exact_spec_text is not None:
        if not isinstance(exact_spec_text, str):
            raise SurfaceContractError(
                "exact_spec_text_invalid",
                "Exact governing spec text must be a string.",
            )
        output["exact_spec"] = {
            "text": exact_spec_text,
            "utf8_byte_length": len(exact_spec_text.encode("utf-8")),
            "rewritten": False,
        }
    return output


def render_prepare_response(response: Mapping[str, Any]) -> dict[str, Any]:
    """Render an exact daemon preview or typed no-write failure."""

    failure = _extract_failure(response)
    if failure is not None:
        return render_failure(response, expected_codes=_SELECTION_FAILURE_CODES)

    preview = _find_mapping(
        response,
        "candidate_selection_preview",
        "selection_preview",
        "preview",
    )
    if preview is None:
        raise SurfaceContractError(
            "candidate_selection_preview_missing",
            "Daemon response did not contain CandidateSelectionPreviewV1.",
        )
    _require_payload_version(preview, CANDIDATE_SELECTION_CONTRACT_VERSION)
    raw_preview = deepcopy(dict(preview))
    challenge = raw_preview.get("confirmation_challenge")
    if not isinstance(challenge, Mapping) or not challenge:
        raise SurfaceContractError(
            "confirmation_challenge_missing",
            "A successful prepare response must include the daemon-issued challenge.",
        )

    signoff = raw_preview.get("resulting_signoff")
    promote_and_approve = signoff == "approved_by_owner_policy"
    promote_effects = raw_preview.get("promote_effects")
    reject_effects = raw_preview.get("reject_effects")
    if not isinstance(promote_effects, list):
        promote_effects = []
    if not isinstance(reject_effects, list):
        reject_effects = []
    promote_count = len(promote_effects)
    reject_count = len(reject_effects)
    pending_raw = raw_preview.get("pending_candidate_count")
    if pending_raw is None:
        pending_raw = raw_preview.get("pending_count")
    remaining_pending_count = pending_raw if isinstance(pending_raw, int) else 0
    product_id = raw_preview.get("product_id")
    actor_id = raw_preview.get("actor_id")

    action_label = _count_bearing_promote_label(
        promote_count, owner_policy_approve=promote_and_approve
    )
    presentation_state = (
        "confirmation_required_promote_and_approve"
        if promote_and_approve
        else "confirmation_required_promote"
    )
    signoff_wording = "owner-policy-approved" if promote_and_approve else "proposed"
    will_append = promote_count > 0
    if will_append and promote_and_approve:
        canonical_append_statement = (
            f"Confirming will append canonical Decision events for {promote_count} "
            "promoted candidate(s) and mark them approved under the current Product "
            "Owner policy. Session-only rejects create no canonical event."
        )
    elif will_append:
        canonical_append_statement = (
            f"Confirming will append canonical Decision events for {promote_count} "
            "promoted candidate(s) as proposed (not Product Owner auto-approval). "
            "Session-only rejects create no canonical event."
        )
    else:
        canonical_append_statement = (
            "No promote set: confirmation applies session-only rejects only and "
            "does not append canonical Decision events for rejects."
        )

    def _effect_item(effect: Any) -> dict[str, Any]:
        if not isinstance(effect, Mapping):
            return {"candidate_id": None, "label": "(invalid effect)"}
        cid = effect.get("candidate_id")
        decision = effect.get("proposed_decision")
        source_labels = _source_context_labels(effect)
        return {
            "candidate_id": cid,
            "proposed_decision": decision,
            "source_labels": source_labels,
            "label": _decision_lead_label(
                proposed_decision=decision if isinstance(decision, str) else None,
                candidate_id=str(cid) if cid is not None else "",
                source_labels=source_labels,
            ),
            "daemon_effect": deepcopy(dict(effect)),
        }

    promote_items = [_effect_item(e) for e in promote_effects]
    reject_items = [_effect_item(e) for e in reject_effects]

    confirmation_summary: dict[str, Any] = {
        "promote_count": promote_count,
        "reject_count": reject_count,
        "remaining_pending_count": remaining_pending_count,
        "held_contradiction_count": None,
        "resulting_signoff": signoff,
        "signoff_wording": signoff_wording,
        "product_id": product_id,
        "actor_id": actor_id,
        "will_append_canonical_events": will_append,
        "canonical_append_statement": canonical_append_statement,
        "promote_items": promote_items,
        "reject_items": reject_items,
        "session_only_reject_semantics": SESSION_ONLY_REJECT_DETAIL if reject_count else None,
        # Opaque challenge must never appear in human summary text.
        "challenge_in_human_summary": False,
    }
    summary_text_parts = [
        action_label,
        f"Product: {product_id}",
        f"Actor: {actor_id}",
        f"Signoff: {signoff_wording}",
        f"Promote {promote_count}; reject {reject_count}; pending {remaining_pending_count}",
        canonical_append_statement,
    ]
    for item in promote_items:
        summary_text_parts.append(f"Promote: {item['label']} [{item.get('candidate_id')}]")
    for item in reject_items:
        summary_text_parts.append(
            f"Reject (session-only): {item['label']} [{item.get('candidate_id')}]"
        )
    summary_text = "\n".join(str(p) for p in summary_text_parts if p is not None)

    return {
        "display_contract": "MCPDisplayContractV1",
        "phase": "prepare",
        "status": "confirmation_required",
        "daemon_response": deepcopy(dict(response)),
        "preview": raw_preview,
        "human_action": {
            "required": True,
            "label": action_label,
            "summary": summary_text,
            "confirmation_summary": confirmation_summary,
            "may_revise_subset": True,
            "agent_may_confirm": False,
            "display_position_is_authority": False,
            "stable_candidate_id_is_authority": True,
            "promote_count": promote_count,
            "reject_count": reject_count,
            "remaining_pending_count": remaining_pending_count,
            "resulting_signoff": signoff,
            "signoff_wording": signoff_wording,
            "product_id": product_id,
            "actor_id": actor_id,
            "will_append_canonical_events": will_append,
        },
        "confirmation_summary": confirmation_summary,
        "confirmation_transport": {
            "opaque": True,
            "persist": False,
            "log": False,
            "token_source": "daemon",
            "challenge": deepcopy(dict(challenge)),
        },
        "canonical_write": "none",
        "ux": _ux_block(
            presentation_state,
            page_or_route=UX_PAGE_PREPARE,
            extra={
                "confirmation_label": action_label,
                "resulting_signoff": signoff,
                "signoff_wording": signoff_wording,
                "promote_count": promote_count,
                "reject_count": reject_count,
                "remaining_pending_count": remaining_pending_count,
                "product_id": product_id,
                "actor_id": actor_id,
                "will_append_canonical_events": will_append,
                "challenge_in_human_summary": False,
            },
        ),
        "next_actions": [COMMIT_TOOL, PREPARE_TOOL, HEARTBEAT_TOOL, END_TOOL],
    }


def render_commit_response(response: Mapping[str, Any]) -> dict[str, Any]:
    """Render CandidateSelectionReceiptV1 or a typed no-write failure."""

    failure = _extract_failure(response)
    if failure is not None:
        return render_failure(response, expected_codes=_SELECTION_FAILURE_CODES)

    receipt = _find_mapping(
        response,
        "candidate_selection_receipt",
        "selection_receipt",
        "receipt",
    )
    if receipt is None:
        raise SurfaceContractError(
            "candidate_selection_receipt_missing",
            "Daemon response did not contain CandidateSelectionReceiptV1.",
        )
    _require_payload_version(receipt, CANDIDATE_SELECTION_CONTRACT_VERSION)
    raw_receipt = deepcopy(dict(receipt))
    return {
        "display_contract": "MCPDisplayContractV1",
        "phase": "commit",
        "status": "committed",
        "daemon_response": deepcopy(dict(response)),
        "receipt": raw_receipt,
        "canonical_write": "daemon_reported",
        "replay_verified": raw_receipt.get("replay_verified"),
        "session_reconciled": raw_receipt.get("session_reconciled"),
        "ux": _ux_block(
            "committed",
            page_or_route=UX_PAGE_COMMIT,
            extra={
                "replay_verified": raw_receipt.get("replay_verified"),
                "session_reconciled": raw_receipt.get("session_reconciled"),
            },
        ),
        "next_actions": [HEARTBEAT_TOOL, END_TOOL],
    }


def render_session_response(
    response: Mapping[str, Any],
    *,
    operation: str,
) -> dict[str, Any]:
    """Render heartbeat/end without inventing candidate or Product state."""

    if operation not in {"heartbeat", "end"}:
        raise SurfaceContractError(
            "candidate_session_operation_invalid",
            f"Unsupported candidate-session operation: {operation}",
        )
    failure = _extract_failure(response)
    if failure is not None:
        return render_failure(response, expected_codes=_SESSION_FAILURE_CODES)

    result = _find_mapping(response, f"candidate_session_{operation}_result", "result")
    if result is None:
        result = response if isinstance(response, Mapping) else None
    if not isinstance(result, Mapping):
        raise SurfaceContractError(
            "candidate_session_result_missing",
            f"Daemon response did not contain the {operation} result.",
        )
    _require_payload_version(result, CANDIDATE_SESSION_CONTRACT_VERSION)
    presentation_state = "session_ended" if operation == "end" else "session_heartbeat_ok"
    page = UX_PAGE_END if operation == "end" else UX_PAGE_HEARTBEAT
    return {
        "display_contract": "MCPDisplayContractV1",
        "phase": operation,
        "status": "ok",
        "daemon_response": deepcopy(dict(response)),
        "result": deepcopy(dict(result)),
        "canonical_write": "none",
        "erasure_is_rejection": False if operation == "end" else None,
        "ux": _ux_block(presentation_state, page_or_route=page),
    }


def render_failure(
    response: Mapping[str, Any],
    *,
    expected_codes: frozenset[str],
) -> dict[str, Any]:
    """Preserve a daemon-authored typed failure and exact recovery data."""

    failure = _extract_failure(response)
    if failure is None:
        raise SurfaceContractError("typed_failure_missing", "No daemon failure was present.")
    raw_failure = deepcopy(dict(failure))
    code = raw_failure.get("code") or raw_failure.get("error_code")
    if not isinstance(code, str) or not code:
        raise SurfaceContractError("typed_failure_invalid", "Failure code is missing.")
    recovery = _recovery_for_failure(code)
    state_key = f"error_{code}"
    if state_key not in PRESENTATION_STATE_MATRIX:
        state_key = "error_generic"
    return {
        "display_contract": "MCPDisplayContractV1",
        "status": "error",
        "error_code": code,
        "known_contract_code": code in expected_codes,
        "failure": raw_failure,
        "daemon_response": deepcopy(dict(response)),
        "canonical_write": "none_or_daemon_reported_only",
        "recovery": recovery,
        "ux": _ux_block(
            state_key,
            page_or_route=UX_PAGE_FAILURE,
            extra={
                "error_code": code,
                "recovery_action": recovery.get("action"),
            },
        ),
    }


def extract_candidate_set_surface(
    response: Mapping[str, Any],
    *,
    annotation_colors: Mapping[str, str] | None = None,
) -> dict[str, Any] | None:
    """Extract a CandidateSet from a preflight response when the daemon supplies one."""

    containers: list[Mapping[str, Any]] = [response]
    result = response.get("result")
    if isinstance(result, Mapping):
        containers.append(result)
    for container in tuple(containers):
        managed = container.get("managed_preflight")
        if isinstance(managed, Mapping):
            containers.append(managed)
        alpha = container.get("alpha_recall")
        if isinstance(alpha, Mapping):
            containers.append(alpha)

    candidate_set: Mapping[str, Any] | None = None
    exact_spec_text: str | None = None
    for container in containers:
        for key in ("candidate_set", "candidate_set_v1", "decision_candidate_set"):
            value = container.get(key)
            if isinstance(value, Mapping):
                candidate_set = value
                break
        if candidate_set is not None:
            exact_spec_text = _extract_exact_spec_text(container)
            break
    if candidate_set is None:
        return None
    return render_candidate_set(
        candidate_set,
        exact_spec_text=exact_spec_text,
        annotation_colors=annotation_colors,
    )


def _extract_exact_spec_text(container: Mapping[str, Any]) -> str | None:
    for key in ("exact_spec_text", "governing_spec_text", "spec_text"):
        value = container.get(key)
        if isinstance(value, str):
            return value
    governing_spec = container.get("governing_spec")
    if isinstance(governing_spec, Mapping):
        for key in ("text", "content", "exact_text"):
            value = governing_spec.get(key)
            if isinstance(value, str):
                return value
    return None


def _extract_failure(response: Mapping[str, Any]) -> Mapping[str, Any] | None:
    containers: list[Mapping[str, Any]] = [response]
    result = response.get("result")
    if isinstance(result, Mapping):
        containers.append(result)
    for container in containers:
        for key in ("failure", "error", "candidate_selection_failure", "session_failure"):
            value = container.get(key)
            if isinstance(value, Mapping):
                return value
        status = container.get("status")
        if status in {"error", "failed", "rejected"} and isinstance(container, Mapping):
            if isinstance(container.get("code"), str) or isinstance(
                container.get("error_code"), str
            ):
                return container
    return None


def _find_mapping(response: Mapping[str, Any], *keys: str) -> Mapping[str, Any] | None:
    containers: list[Mapping[str, Any]] = [response]
    result = response.get("result")
    if isinstance(result, Mapping):
        containers.append(result)
    for container in containers:
        for key in keys:
            value = container.get(key)
            if isinstance(value, Mapping):
                return value
    return None


def _recovery_for_failure(code: str) -> dict[str, Any]:
    recovery: dict[str, dict[str, Any]] = {
        "dependency_closure_required": {
            "action": "revise_selection",
            "detail": "Review the daemon-returned missing candidate ids and resubmit exact ids.",
        },
        "candidate_set_stale": {
            "action": "rerun_preflight",
            "detail": "Obtain the current CandidateSet generation and digest.",
        },
        "candidate_set_digest_mismatch": {
            "action": "rerun_preflight",
            "detail": "Do not reuse a display or subset from another CandidateSet digest.",
        },
        "preview_stale": {
            "action": "prepare_again",
            "detail": "Reprepare the same intended stable-id subset against current state.",
        },
        "policy_changed": {
            "action": "prepare_again",
            "detail": "Review a fresh preview under the current Product policy.",
        },
        "governing_spec_changed": {
            "action": "rerun_preflight",
            "detail": "Bind and review the current exact governing spec.",
        },
        "accepted_model_base_changed": {
            "action": "rerun_preflight",
            "detail": "Recompute candidates against the current accepted Product base.",
        },
        "human_confirmation_required": {
            "action": "obtain_explicit_human_confirmation",
            "detail": "An agent or MCP process cannot confirm on the human's behalf.",
        },
        "session_lease_expired": {
            "action": "start_new_session",
            "detail": "The transient CandidateSet was erased after its reconnect grace.",
        },
        "unsupported_contract_version": {
            "action": "upgrade_matching_components",
            "detail": "Use MCP and daemon versions that support the same exact contract.",
        },
    }
    return recovery.get(
        code,
        {
            "action": "inspect_daemon_failure",
            "detail": "Preserve the typed daemon failure and follow its exact recovery guidance.",
        },
    )


def _contract_version(payload: Mapping[str, Any], expected: int) -> int:
    value = payload.get("contract_version", expected)
    if isinstance(value, bool) or not isinstance(value, int):
        raise SurfaceContractError(
            "unsupported_contract_version",
            "contract_version must be an integer.",
        )
    if value != expected:
        raise SurfaceContractError(
            "unsupported_contract_version",
            f"Expected contract_version {expected}; received {value}.",
        )
    return value


def _require_payload_version(payload: Mapping[str, Any], expected: int) -> None:
    _contract_version(payload, expected)


def _required_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SurfaceContractError("malformed_identity", f"{field} must be a non-empty string.")
    return value


def _required_positive_int(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        raise SurfaceContractError("malformed_identity", f"{field} must be a positive integer.")
    return value


def _required_nonnegative_int(value: Any, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise SurfaceContractError(
            "candidate_set_invalid",
            f"{field} must be a non-negative integer.",
        )
    return value


def _string_list(value: Any, field: str) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise SurfaceContractError("malformed_identity", f"{field} must be an array.")
    output: list[str] = []
    seen: set[str] = set()
    for item in value:
        text = _required_string(item, field)
        if text in seen:
            raise SurfaceContractError(
                "malformed_identity",
                f"{field} contains duplicate stable id {text}.",
            )
        seen.add(text)
        output.append(text)
    return output
