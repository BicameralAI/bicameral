# Sibling Registry

**Status**: Active · **Last reviewed**: 2026-06-08

This file is the single source of truth for **sibling tools** — local process,
governance, or AI tooling that touches this checkout but must **never** leak into a
commit or a tracked reference.

It is the third layer of the
[governance boundary proposal](../plans/governance-boundary-three-layer-proposal-2026-06-08.md):
the **only** mandatory layer for a pull request is the shared **bic-logic** contract.
Below that line, your choice of local tooling is free — as long as the tool is
**registered as a sibling** here and its artifacts stay out of the repo.

> The maintainer's own process tool (**Qor-logic**) is itself a registered sibling, not a
> requirement placed on contributors. You are encouraged to bring whatever helps you work.

## What a sibling is

A sibling is any tool that:

- runs **locally** against this checkout (a governance system, an agent harness, an IDE
  assistant, a homegrown framework, a sibling product like FailSafe);
- writes **scratch/state** into the working tree (a dotdir, generated docs, logs, plans);
- must be **leak-guarded** — its roots are gitignored and the boundary guard refuses to
  stage them;
- is **never referenced** by any tracked bicameral file (no tracked doc names a sibling's
  internals).

A sibling has **no product authority**. It never produces Decisions, gates, or compliance
outcomes in the Bicameral product sense — that authority lives only in `protocol/`,
governance policy, and event-store replay.

## The rules (invariants)

1. **Local-only** — a sibling's artifacts are never committed.
2. **Leak-guarded** — every registered root is in `.gitignore`, and the boundary guard
   (once landed) blocks any staged path under it.
3. **Never-referenced** — no tracked file names a sibling's internal files, paths, or APIs.
4. **Registry ⇔ `.gitignore` agree** — every root below has a matching `.gitignore` entry,
   and every agent-scratch `.gitignore` entry has a row here. Drift is a guard failure.

## Registry

| Sibling | Root(s) | Owner | Rule |
|---|---|---|---|
| **Qor-logic** | `.qor/`, and the S.H.I.E.L.D. docs (`docs/META_LEDGER.md`, `docs/CONCEPT.md`, `docs/ARCHITECTURE_PLAN.md`, `docs/SHADOW_GENOME.md`, `docs/BACKLOG.md`, `docs/FEATURE_INDEX.md`, `docs/SYSTEM_STATE.md`, `docs/GOVERNANCE_INDEX.md`, `docs/research/`, `docs/plans/`, `docs/reviews/`, `docs/research-brief-*.md`) | maintainer | local process governance; never tracked |
| **FailSafe** | `.failsafe/` | sibling MythologIQ product | leak-prevention only; never referenced |
| **Bicameral factory (local)** | `.bicameral/factory-context.local.json`, `.bicameral/factory-skills/`, `.agents/skills/` | factory tooling | local install state; tracked attestations under `.bicameral/factory-attestations/` are the **only** committed factory evidence |
| **Claude Code** | `.claude/`, `CLAUDE.md` | contributor (AI assistant) | agent scratch; never tracked |
| **Cursor** | `.cursor/`, `CURSOR.md` | contributor (AI assistant) | agent scratch; never tracked |
| **Windsurf** | `.windsurf/` | contributor (AI assistant) | agent scratch; never tracked |
| **Gemini / Copilot** | `GEMINI.md`, `COPILOT.md` | contributor (AI assistant) | agent scratch; never tracked |
| **Generic agent scratch** | `.agent/`, `.agents/`, `plan-*.md`, `RUN_SUMMARY.md` | contributor | run plans, context bundles, raw logs; never tracked |

## How to register your own tool

You do **not** need maintainer permission to use your own tooling. To register it:

1. **Add a row** to the table above: tool name, its scratch root(s), you as owner, and the
   rule (`local-only, leak-guarded, never-referenced`).
2. **Add the root to `.gitignore`** under the *Agent scratch* / *Governance* section, so the
   tool is provably un-committable.
3. **Keep artifacts out of tracked files** — never commit the tool's output and never name
   its internals in a tracked doc.

That's it. Open the change as a normal PR; the only thing it must satisfy is the shared
bic-logic contract and a clean leak guard. See [`CONTRIBUTING.md`](../../CONTRIBUTING.md) →
*Bring your own tools*.

> A built-in default floor of common agent-scratch roots blocks the usual offenders even
> before you register, so forgetting to register cannot cause a leak — registering simply
> makes the boundary explicit and auditable.
