# Governance (tracked)

This directory holds the **tracked, public-facing** governance contract for
`bicameral-bot`. It declares *which classes* of artifact are local versus shared, and it
never reproduces local contents.

| File | Purpose |
|---|---|
| [`SIBLINGS.md`](SIBLINGS.md) | The sibling registry — every leak-guarded local tool (Qor-logic, FailSafe, contributor tooling) and the rules that keep it out of commits. |

Planned, per the
[three-layer governance boundary proposal](../plans/governance-boundary-three-layer-proposal-2026-06-08.md):

| File | Purpose |
|---|---|
| `BOUNDARY.md` | The contract: the three-layer table, the five invariants, the leak rules. |
| `PIN.json` | Pinned `bicameral-factory` governance-doctrine commit + sha256 (mirrors the attestation schema). |

**Doctrine is upstream.** The shared process rules (bic-logic) are owned in
`bicameral-factory` and consumed here by pin/hash — not copied. **Local artifacts are never
here**: Qor-logic's S.H.I.E.L.D. set, research, plans, reviews, and any sibling's scratch
stay local and gitignored. The local `docs/GOVERNANCE_INDEX.md` indexes those *local* Qor
artifacts; this directory is its tracked complement.
