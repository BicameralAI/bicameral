# Agent instructions

Before doing Bicameral product work, read and follow the factory instructions:

`~/github/bicameral-factory/INSTRUCTIONS.md`

## Bicameral factory context

Before Bicameral product work, read `.bicameral/factory-context.local.json`, confirm the referenced `bicameral-factory` checkout/commit, and inspect copied factory skills under `.agents/skills/`.

Factory skills installed by `skills.sh` are local file-backed procedures. For factory-governed work, prefer the copied `bic-*` skills under `.agents/skills/`, such as `.agents/skills/bic-grill-with-docs/SKILL.md`, `.agents/skills/bic-tdd/SKILL.md`, and `.agents/skills/bic-pre-pr-factory-attestation/SKILL.md`.

Treat `.bicameral/factory-skills/` as the legacy symlink fallback only. Do not prefer it when `.agents/skills/` contains the copied `bic-*` factory skills.

For provenance, compare installed `bic-*` skill file hashes against `<factory_repo>/skills/factory-skill-manifest.json`. A similar name alone is not evidence that a factory-shipped skill was used.

Do not commit `.bicameral/factory-context.local.json`, `.bicameral/factory-skills`, `.agents/skills`, prompts, context bundles, run plans, raw logs, `RUN_SUMMARY.md`, or local agent habits.

For product PRs, create and validate the required factory attestation under `.bicameral/factory-attestations/<factory_commit>.<run-id>.json`, where `<run-id>` is a stable per-PR token such as the head branch slug (e.g. `<factory_commit>.devin-1781821202-my-feature.json`). The per-run filename keeps concurrent PRs that share a `factory_commit` from colliding on the same file at merge time; the legacy `<factory_commit>.json` form is still accepted for existing records but must not be used for new attestations. The attestation must include the state-refinement checkpoint. If lifecycle behavior changed, record the smallest honest release state, states added, states deferred, invariants, and minimization review. If lifecycle behavior did not change, say so explicitly, leave states added/deferred empty, and record that canonical state, authority, and persistence semantics did not change.
