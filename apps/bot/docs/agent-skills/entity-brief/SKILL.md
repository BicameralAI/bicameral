---
name: entity-brief
description: >
  Ticket-scoped briefing of novel entities, how they relate, and where
  each durable fact persists. Use when reviewing a PR or issue that
  introduces domain terms, when asking where an event store / SQLite /
  Drive / fixture lives, or when the user runs /entity-brief.
---

# /entity-brief

Standup-shaped owner briefing. One ticket or PR. Two jobs: name the nouns,
show where the bytes live. Visuals are mandatory.

## Resolve the target

Accept a PR/issue URL, or `git diff <base>...HEAD` on the current branch.
Default base is the PR base or `origin/dev`.

## Gather

1. Read `CONTEXT.md` and `docs/domain/*.md` in the target repo before the diff.
   Existing glossary terms win. Do not invent synonyms.
2. Read the diff, linked issue, and any spec/schema the diff binds.
3. For each durable fact in the change, find the actual store in code or spec:
   event-store adapter (`git` / `drive_folder` / `memory`), daemon SQLite,
   ConfirmationChallenge memory, operator registry JSON, Cloud, fixture file.
   Cite `file:line`. If the store is unspecified, say `unverified`.

## Classify each term

`existing` — already in CONTEXT/domain docs.
`novel` — introduced by this change and not in the glossary.
`rejected` — explicitly must not exist.
`alias` — same thing as an existing term; name the canonical term.

Novel terms that look reusable: flag for `/bic:promote-domain-entity`. Do not
promote in this skill.

## Visuals (required)

Emit **two** diagrams in Mermaid. Do not skip them. Do not use an image model
for labeled topology (labels will be wrong). ASCII is allowed only as a
fallback if Mermaid cannot be emitted.

1. **Entities** — `flowchart LR`. Boxes are terms. Edges are relations
   (`stages`, `appends`, `replays into`, `is not`). Rejected terms stay on the
   canvas, drawn as rejected.
2. **Persistence** — `flowchart TB`. Boxes are stores. Edges show which
   fact is written or read. Separate live path from test/fixture path.

If the host can write files, also emit a single self-contained HTML page with
the same two diagrams as CSS/HTML (exact labels, no screenshot of Mermaid).

## Output

Sections, in order:

1. Title `Entity brief — <PR or issue>`, plus target and UTC observed time.
2. One sentence: what changed.
3. Table **Entities**: Term, Class, Is, Is not, Relates to, Evidence.
4. Table **Persistence**: Fact, Store, Substrate, Path or identity, Not this, Evidence.
5. Heading **Entity map**, then a mermaid `flowchart LR` fence.
6. Heading **Persistence map**, then a mermaid `flowchart TB` fence.
7. **Open owner questions**: one bullet, or `none`.

Keep it to the change. Do not dump the whole domain model. Empty tables are
allowed if the PR adds no nouns and no store.
