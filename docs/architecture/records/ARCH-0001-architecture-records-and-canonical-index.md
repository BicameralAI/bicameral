---
id: ARCH-0001
title: Architecture records and the canonical architecture index
kind: decision
status: proposed
date: 2026-10-16
owners: [@jinhongkuan, @Knapp-Kevin]
governs: [architecture-records, architecture-index]
depends_on: []
supersedes: []
references: [https://github.com/BicameralAI/bicameral-factory/issues/969]
---

# ARCH-0001: Architecture records and the canonical architecture index

## Context

Bicameral is being consolidated from several repositories into one product repository. Those repositories record architecture in different places and formats, and the status of a document is often implicit: a draft and an adopted decision can look the same. The consolidation plan in Factory #969 requires that proposed contracts are not treated as accepted just because they are present in the monorepo.

Readers, reviewers and tools need one place to find architecture decisions, and a reliable way to tell which decision currently governs a topic.

## Decision

1. Architecture is recorded as `ARCH-NNNN-slug.md` files in `docs/architecture/records/`, which is the only permitted location. Separate decision directories (`adr`, `decisions` and similar) are not allowed.
2. Each record has the restricted front matter documented in `docs/architecture/README.md`: `id`, `title`, `kind`, `status`, `date`, `owners`, `governs`, `depends_on`, `supersedes` and `references`, and the sections its kind requires.
3. Authority comes from status. Only `accepted` records are authoritative, and each topic has at most one accepted record. `proposed` records express intent. `superseded`, `deprecated` and `rejected` records are historical.
4. A proposed replacement can name an accepted record it would supersede without affecting that record's authority. Supersession takes effect only when the replacement is accepted and the predecessor is marked `superseded` in the same change.
5. `docs/architecture/INDEX.md` and `docs/architecture/index.json` are generated from the records. They expose authority by topic, the accepted, proposed and historical sets, and derived reverse links. Reverse links are never written by hand.
6. `tools/governance/check_architecture.py` enforces these rules and fails if the generated index is stale. It runs in the required `governance` check.
7. Acceptance or supersession is a reviewed pull request that needs decision owner approval, as set out in `GOVERNANCE.md`.

## Consequences

- One answer to "what governs this topic": the accepted-authority table in the generated index.
- Proposals can be merged for discussion without becoming binding.
- Every record must declare the records it mentions, so dependency and supersession links are always machine-checkable.
- The front matter is intentionally less expressive than YAML. That keeps the checker in the standard library and makes the format unambiguous, at the cost of some convenience.
- Records imported from other repositories must be rewritten into this format, with their original status checked rather than assumed.
