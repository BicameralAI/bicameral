# Architecture records

Bicameral records architecture as numbered, reviewable files. This page explains how they work. The current state is in the generated [index](INDEX.md) ([`index.json`](index.json) for tools).

## Where records live

Every record is `records/ARCH-NNNN-slug.md`, in [`records/`](records/) and nowhere else. The structure check rejects `ARCH-NNNN` files elsewhere and rejects `adr/`, `adrs/`, `decisions/`, `architecture-decisions/` and `design-docs/` directories anywhere in the repository, so there is one canonical location.

## Record format

A record starts with front matter in a deliberately small syntax: `key: value` or `key: [a, b]`, nothing else.

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | `ARCH-NNNN` | Must match the filename |
| `title` | text | Must match the first heading, `# ARCH-NNNN: Title` |
| `kind` | `decision`, `design`, `contract` or `reference` | Decides the required sections |
| `status` | `proposed`, `accepted`, `superseded`, `deprecated` or `rejected` | See below |
| `date` | `YYYY-MM-DD` | Date of the current status |
| `owners` | list of `@handles` | Accountable owners |
| `governs` | list of kebab-case topics | Topics the record decides (or, if proposed, would decide) |
| `depends_on` | list of IDs | Records this one builds on |
| `supersedes` | list of IDs | Records this one replaces (or, if proposed, would replace) |
| `references` | list of IDs or links | Related material with no dependency |

All ten fields are required; no others are allowed. Use `[]` for an empty list.

Required sections by kind:

| Kind | Sections |
| --- | --- |
| `decision` | `## Context`, `## Decision`, `## Consequences` |
| `design` | `## Context`, `## Design` |
| `contract` | `## Scope`, `## Contract` |
| `reference` | none |

Start from [TEMPLATE.md](TEMPLATE.md).

## Authority

| Status | Authority |
| --- | --- |
| `accepted` | Authoritative for the topics it governs |
| `proposed` | Intent only; may claim topics but decides nothing |
| `superseded`, `deprecated`, `rejected` | Historical; kept for traceability |

The checker enforces these rules:

1. An accepted record governs at least one topic.
2. At most one accepted record governs any topic.
3. Proposed records may claim topics that an accepted record already governs. That is how a replacement is proposed.
4. An accepted record depends only on accepted records.
5. A proposed record depends only on accepted or proposed records, never historical ones.
6. Dependencies (and supersession chains) are acyclic.
7. Every referenced ID exists, and no record references itself.
8. An ID appears in at most one of `depends_on`, `supersedes` and `references` in a record.
9. Every `ARCH-NNNN` mentioned in a record's text is declared in one of those fields.
10. A proposed record may supersede an accepted one. The accepted record keeps its authority until the proposal is accepted.
11. When an accepted (or later superseded or deprecated) record supersedes another, the predecessor must have status `superseded`.
12. A superseded record must be superseded by a record whose replacement took effect (accepted, superseded or deprecated). A proposal or a rejected record is not enough.
13. Authority in the index is derived only from accepted records.
14. Reverse links (`required_by`, `superseded_by`) and pending replacements are generated into the index; writing them into a record is an error.

## Changing architecture

- **Propose**: add a record with `status: proposed`.
- **Accept**: a separate reviewed pull request changes the status to `accepted`, with decision owner approval ([GOVERNANCE.md](../../GOVERNANCE.md)). Records it depends on must already be accepted or be accepted in the same pull request.
- **Replace**: propose a new record that lists the old one in `supersedes`. When it is accepted, the same pull request sets the old record to `superseded`.
- **Retire**: set `deprecated` when a decision no longer applies and nothing replaces it. Records that depend on it must be updated first.

After any change run `python3 tools/governance/check.py --write` and commit the regenerated `INDEX.md` and `index.json`.
