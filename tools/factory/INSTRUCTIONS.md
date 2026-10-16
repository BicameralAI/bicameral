# Bicameral Factory Instructions

These instructions are for agents and collaborators doing Bicameral product work.

## Product-session setup

Use the same `skills` installer pattern as Matt Pocock's skills repo.

From the product repo root:

```bash
npx skills@latest add git@github.com:BicameralAI/bicameral-factory.git --copy
```

Select `bic:setup-bicameral-factory`, then run `/bic:setup-bicameral-factory` in the agent.

If the installer is unavailable, use the manual process below. Do not assume a fixed checkout path.

### 1. Find or create the factory checkout

Ask the collaborator where `BicameralAI/bicameral-factory` should live.

Good defaults to offer:

- sibling checkout: `../bicameral-factory`
- existing checkout from `BICAMERAL_FACTORY_REPO`
- custom absolute path chosen by the collaborator

If the chosen path does not exist, ask before cloning there:

```bash
git clone git@github.com:BicameralAI/bicameral-factory.git /path/to/bicameral-factory
```

Then refresh it:

```bash
git -C /path/to/bicameral-factory switch main
git -C /path/to/bicameral-factory pull --ff-only
```

### 2. Create local product-repo context

Create a local, gitignored context file in the product repo:

```text
.bicameral/factory-context.local.json
```

Add this local-only path to `.git/info/exclude` so setup state is not committed:

```text
.bicameral/factory-context.local.json
```

Write the context file with the selected factory path and current commit:

```json
{
  "factory_repo": "/absolute/path/to/bicameral-factory",
  "factory_commit": "<full git sha>",
  "factory_skill_namespace": "bic",
  "factory_skill_manifest": "/absolute/path/to/bicameral-factory/skills/factory-skill-manifest.json",
  "created_for": "<product repo name or path>",
  "notes": "Local-only setup file. Do not commit."
}
```

### 3. Verify copied `/bic:*` factory skills

Factory skills should be installed with copied files through `skills.sh`, not through a product-repo symlink:

```bash
npx skills@latest add git@github.com:BicameralAI/bicameral-factory.git --copy
```

The `/bic:` prefix distinguishes factory-shipped skills from a collaborator's personal unprefixed skills. Verify the factory manifest exists:

```bash
test -f /absolute/path/to/bicameral-factory/skills/factory-skill-manifest.json
```

### 4. Update the product repo's agent instruction

Ask which local agent instruction file to update. Common options:

- `AGENTS.md`
- `CLAUDE.md`
- `.cursorrules`

Append a short section like this:

```md
## Bicameral factory context

Before Bicameral product work, read `.bicameral/factory-context.local.json`, confirm the referenced `bicameral-factory` checkout/commit, and inspect copied factory skills under `.agents/skills/`.

Use copied `bic-*` factory skills from `.agents/skills/` instead of unprefixed personal skills for factory-governed work. For example, use `.agents/skills/bic-grill-with-docs/SKILL.md`, not a personal `/grill-with-docs`. Treat `.bicameral/factory-skills/` as a legacy symlink fallback only when copied `.agents/skills/` are absent.

For provenance, match installed `bic-*` skill hashes against `skills/factory-skill-manifest.json` in the referenced factory checkout. A similar name alone is not evidence that a factory-shipped skill was used.

Do not commit `.bicameral/factory-context.local.json`, prompts, context bundles, run plans, raw logs, `RUN_SUMMARY.md`, or local agent habits.

For product PRs, create and validate the required factory attestation under `.bicameral/factory-attestations/<factory_commit>.json`.
```

If the repo has no agent instruction file, ask before creating `AGENTS.md`.

### 5. Load context before planning or editing

Before planning or editing, read:

- the selected `.bicameral/factory-context.local.json`
- `<factory_repo>/CONTEXT.md`
- `<factory_repo>/CONTRIBUTING.md`
- `<factory_repo>/PAMA.md`
- `<factory_repo>/knowledge/product-validation-roadmap.md`
- installed copied `/bic:*` factory skills relevant to the work, verified against `skills/factory-skill-manifest.json` when attesting provenance
- relevant files under `<factory_repo>/product-lines/` when the task involves product positioning, release sequencing, pilot scope, OSS versus premium boundaries, hosted-service rationale, or buyer-facing prioritization
- relevant files under `<factory_repo>/charters/`
- relevant files under `<factory_repo>/blueprints/`

Before starting product implementation or planning, use the roadmap graph and
the Product Validation Roadmap project as a focus check. Treat roadmap issues
with GitHub issue type `validated` as validated product ground. Roadmap issues
with issue type `validating` may be advanced only when the work is explicitly
framed as validation work. Roadmap issues with issue type `deferred` are
hard-blocked: do not start implementation for that branch unless the caller
explicitly overrides the block in the session. If requested work advances a
roadmap branch, name the roadmap issue, its issue type, and why the session
should proceed. After completing the work, comment on the roadmap issue with the
session's relevance, evidence gathered, branch movement, and whether the issue
type should change.

Use the validated Tool-Agnostic or Miscellaneous roadmap issues for useful work
that does not advance a specific product-validation branch, including shared
tool contracts, privacy, security, reliability, maintenance, and operational
hygiene. These lanes prevent non-product-thesis work from being incorrectly
blocked as `deferred`.

## Using factory skills

Factory-shipped skills use the `/bic:` namespace when installed through `skills.sh`.

Use factory-shipped `/bic:*` skills such as `/bic:grill-with-docs`, `/bic:tdd`, and `/bic:pre-pr-factory-attestation` for factory-governed work. Do not silently substitute a collaborator's personal unprefixed skill with the same base name.

When provenance matters, compare the installed `/bic:*` skill file hashes to `skills/factory-skill-manifest.json`. A name match without a hash match is local setup divergence, not factory-shipped context.

## Product repo boundary

Use factory context to guide product work, but do not copy factory scratch artifacts into product repos.

Do not commit:

- `.bicameral/factory-context.local.json`
- prompts
- context bundles
- run plans
- raw logs
- `RUN_SUMMARY.md`
- local agent habits

The only expected factory-specific product PR artifact is:

```text
.bicameral/factory-attestations/<factory_commit>.json
```

Before opening or updating a product PR, validate the attestation:

```bash
python3 /absolute/path/to/bicameral-factory/scripts/validate-factory-attestation.py \
  .bicameral/factory-attestations \
  --context-root /absolute/path/to/bicameral-factory
```

Mention the attestation path in the PR body.

The attestation must include `roadmap_reconciliation`. Before creating or
updating a PR, look up the Product Validation Roadmap project, name the roadmap
issue used for the work, record its GitHub issue type and Project Status, check
existing linked PRs / PR Links, and ensure the PR body links the roadmap issue
with `Roadmap issue: #<number>` or an equivalent GitHub issue link. After work,
comment on the roadmap issue with the PR URL, relevance, evidence gathered,
branch movement, and whether the issue type or Project Status should change.
The validator fails if roadmap reconciliation is missing; `deferred` issue
types require an explicit override reason.

## Devin and manual factory runs

Use `playbooks/devin-bicameral-factory-run.md` as the canonical implementation playbook for Devin and as the manual-session checklist for collaborators using another AI coding environment.

Manual sessions must still preserve the product repo boundary:

- keep prompts, temporary notes, run summaries, and local logs out of the product PR diff;
- create the required factory attestation when the product repo requires one;
- run minimum deterministic product validation;
- include `Domain entity candidates` and `Factory learning signals` in the PR body, even when both are `None`.
