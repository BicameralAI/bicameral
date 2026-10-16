# V1 Code-Grounding Fixture Workspace

Deterministic, committed fixture for local code-grounding tests.

## Purpose

This fixture provides stable Rust source files and symbols that tests
can copy into a temporary git worktree without network access.  Test
helpers initialise the temp worktree, commit the fixture content, and
optionally mutate it to exercise every `GraphEvidenceState` variant
defined by ADR-0010.

## Evidence / readiness states — not compliance verdicts

All outputs produced through these fixtures are **evidence states** and
**graph-readiness observations**, not compliance verdicts, enforcement
decisions, or feature-readiness claims.

Codegraph evidence is advisory/reviewable context unless accepted
governance events replay it (ADR-0010, ADR-0015).

## Fixture files

| File | Scenario it supports |
|---|---|
| `src/lib.rs` | Verified symbol (`process_order`), file, and range binding |
| `src/types.rs` | Verified struct symbol (`OrderId`) |
| `src/ambiguous.rs` | Ambiguous symbol — defines `SharedName` also in `src/duplicate.rs` |
| `src/duplicate.rs` | Ambiguous symbol — defines `SharedName` also in `src/ambiguous.rs` |
| `src/moved_target.rs` | Rename/move scenario baseline (copy, then rename in temp) |
| `data/config.json` | Non-Rust target (symbol search returns `NotFound` since resolver indexes `.rs` only) |

## Scenario coverage

| Scenario | `GraphEvidenceState` | How to reproduce |
|---|---|---|
| Verified file | `Verified` | Bind to `src/lib.rs` at committed snapshot |
| Verified range | `Verified` | Bind to `src/lib.rs:1-5` at committed snapshot |
| Verified symbol | `Verified` | Bind symbol `process_order` in `src/lib.rs` |
| Not found | `NotFound` | Bind to `src/nonexistent.rs` |
| Ambiguous | `Ambiguous` | Bind symbol `SharedName` scoped to `src/ambiguous.rs` |
| Unsupported | `Unsupported` | Call `bind_symbol_to_code` with `symbol: None` |
| Non-Rust symbol | `NotFound` | Symbol-resolve `name` in `data/config.json` (resolver indexes `.rs` only) |
| Stale / changed | `UnknownStale` | Modify `src/lib.rs` after snapshot without committing |
| Moved / renamed | `NotFound` | Rename `src/moved_target.rs` to `src/renamed.rs`, bind old path |
