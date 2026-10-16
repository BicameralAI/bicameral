# ClaimTrace fixtures for #1004 (kernel inputs)

**Issue:** BicameralAI/bicameral-bot#1004  
**Purpose:** Supply **fixture ClaimTraces as kernel inputs** so composition (#985) can stack without a live LLM.

## Rules

1. Fixtures are **test/stack inputs** to a real validator + deterministic kernel once the crate lands.
2. They are **not** a production success path and must not be selected by env flags in daemon production code.
3. Live extraction (`SRC-TRACE-*`, `SRC-PROFILE-01`) remains #958; do not claim those obligations from fixtures alone.
4. Historical procedure: PR #963 tree (`c190bddb`). Current conformance binds the **new** #1004 PR head only.

## Layout

| Path | Role |
|---|---|
| `valid-adjacent-states.json` | Happy-path adjacent-state ClaimTrace fixture (from #963 tests) |
| `ClaimSemanticTraceV1.fragment.schema.json` | Schema fragment used by #963-era harness (shape reference) |

## How Silong should use this on a stack

1. Rebase/stack composition on `agent/1004-reintroduce-smr-kernel` (or its PR head).
2. Feed files under this directory (or copies) into the public kernel validate/lower/analyze API once present.
3. For additional edge cases (invalid refs, authority laundering, incomplete coverage), add sibling JSON fixtures here in the #1004 PR or a stacked commit — still no live LLM.

## Kernel status

`bicameral-smr` is restored on this branch from historical #963 procedure. Use:

```bash
cargo test -p bicameral-smr
```

Public fixtures under this directory remain the composition/stack input contract for #985 (no live LLM).
