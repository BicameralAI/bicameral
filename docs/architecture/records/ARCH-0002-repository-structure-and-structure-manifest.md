---
id: ARCH-0002
title: Repository structure and the structure manifest
kind: decision
status: accepted
date: 2026-10-16
owners: [@jinhongkuan, @Knapp-Kevin]
governs: [repository-layout, code-ownership]
depends_on: [ARCH-0001]
supersedes: []
references: [https://github.com/BicameralAI/bicameral-factory/issues/969]
---

# ARCH-0002: Repository structure and the structure manifest

## Context

The Bicameral monorepo will receive the local runtime, dashboard, MCP, integrations, cloud service and installer from separate repositories. Without an explicit layout, imports can land in improvised locations, duplicate governance material can appear in parallel directories, and code ownership can drift from intent. Ownership rules also need to be reviewable, not just whatever a hand-edited CODEOWNERS file says.

Architecture records already have a single canonical location under ARCH-0001. The rest of the repository needs the same discipline.

## Decision

1. `governance/structure.json` is the single authoritative description of the repository layout. It declares the allowed root files and directories, each with a purpose.
2. Each declared directory has a child policy. `declared` means only files directly inside it and subdirectories that are themselves declared. `open` means any layout beneath it. Parents are declared before children, and only `declared` parents may declare children.
3. Each declared directory has a status. `active` locations hold current content. `reserved` locations name the intended source repository and hold only a README until an authorized, provenance-recorded import marks them `active`.
4. Declared directories have a README describing their purpose unless the manifest disables it.
5. Every file Git would commit, and every non-ignored directory including empty ones, must be in an authorized location. Symbolic links are rejected. Architecture records follow ARCH-0001: `ARCH-NNNN` files appear only in its canonical directory, and decision-directory names are rejected anywhere.
6. Code ownership is declared in the manifest (`default_owners` plus optional per-entry `owners`). `.github/CODEOWNERS` is generated deterministically from it and is never edited by hand.
7. `tools/governance/check_structure.py` enforces these rules and fails if CODEOWNERS is stale. It runs in the required `governance` check.

## Consequences

- Adding a location or changing ownership is an explicit, reviewable manifest change.
- Component imports have predetermined destinations and cannot silently activate a reserved location.
- Ownership in CODEOWNERS always matches the manifest. Repository rulesets that require code-owner review then enforce the declared ownership.
- Contributors must update the manifest before adding new top-level or declared-child directories, which is deliberate friction.
- Manifest and generator changes are high-risk changes under `GOVERNANCE.md`.
