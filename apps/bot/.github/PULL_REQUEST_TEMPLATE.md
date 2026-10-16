## Summary

-

## Authority boundary check

- [ ] Edges propose; the daemon decides what becomes canonical (ADR-0007). No surface writes canonical state directly.
- [ ] Local actions enter as `ToolRequest` with an enforced `AuthorityContext`; external/source evidence enters only through the `ExternalIngestEnvelope` gate.
- [ ] Source evidence, refs, timestamps, and prompt/manifest versions are preserved where relevant.

## Governance

- [ ] No local-process or sibling-tool artifacts are committed (the governance-boundary check passes). See `docs/governance/BOUNDARY.md`.
- [ ] For product PRs: a valid factory attestation is included under `.bicameral/factory-attestations/<factory_commit>.<run-id>.json` using a per-PR `<run-id>` such as the head branch slug (see `AGENTS.md`), or this is a docs/governance-only PR.
- [ ] The factory attestation answers the state-refinement checkpoint. If lifecycle behavior changed, it records release state, added/deferred states, invariants, and minimization review; otherwise it explicitly says no lifecycle behavior changed.

## Tests

- [ ] `cargo test --workspace --locked`
- [ ] `cargo fmt --all -- --check` and `cargo clippy --workspace --locked -- -D warnings`
- [ ] `python scripts/secret_scan.py`
