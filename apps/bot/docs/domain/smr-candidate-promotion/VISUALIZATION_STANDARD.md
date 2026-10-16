# ADHD-friendly visualization standard

## Purpose

Release-week updates and durable review artifacts must make the current state, decision, evidence, and next action understandable without requiring a reader to reconstruct the workflow from prose or terminal logs.

This is an accessibility and execution-quality requirement, not decorative formatting.

## Required update shape

Every consequential update must lead with a compact status board:

```text
NOW      current action or decision
NEXT     single next action
BLOCKED  explicit blocker, or none
```

Then state, in this order:

1. **Outcome:** what is true now.
2. **Evidence:** the exact receipt, check, artifact, head, or observation supporting it.
3. **Boundary:** what the evidence does not authorize or prove.
4. **Next action:** one concrete action with an owner when known.

## Required visual qualities

- One primary decision or message per visual.
- Summary first, detail second, raw evidence last.
- Use explicit typed states. Do not collapse diagnostic, authoritative, and human-accepted outcomes into one green/red state or percentage.
- Show authority boundaries, handoffs, and canonical commit points directly.
- Distinguish current state, target state, failure state, and deferred state.
- Keep labels, legends, and the relevant next action visible without requiring cross-reference to another image.
- Never rely on color alone. Use text labels, shapes, ordering, or symbols as redundant signals.
- Include an accessible title and text description for every SVG or image.
- Pair every visual with a concise text equivalent in the adjacent Markdown.
- Avoid dense unranked lists. Rank by severity, execution order, or decision dependency.

## Durable image embedding and link integrity

Visual evidence embedded in GitHub issues, pull requests, release records, acceptance records, or other durable external surfaces must remain renderable after branches are renamed or deleted and after repository files are moved.

- Use an immutable commit SHA in every durable image embed URL.
- Do not use a branch name, pull-request branch, tag, or movable default-branch path as the sole image source for a durable record.
- A branch or `main` link may be provided separately as a **latest version** navigation link, but it does not replace the commit-pinned evidence embed.
- Relative image links are appropriate inside repository Markdown because the document and image are versioned together. They are not sufficient when copied into an issue, pull request comment, Slack message, acceptance record, or other external surface.
- Place the pinned commit SHA and repository source path next to the embedded visual or in the surrounding evidence text.
- Before moving or deleting a visual path, search the repository and relevant issue or pull-request thread for external references and update them in the same change sequence.
- A visual move is incomplete until the old external references are repaired and each replacement embed is verified to render for an authorized repository reader.
- Do not rewrite historical evidence to a newer image merely to keep it current. Preserve the original commit-pinned embed and add a separately labeled latest-version link when the visual evolves.

Recommended durable embed form:

```text
https://github.com/<owner>/<repo>/blob/<full-commit-sha>/<path-to-image>?raw=1
```

This rule prevents evidence from disappearing when humans perform the apparently irresistible act of reorganizing directories after publishing links to them.

## Release and test reporting

Test and release updates must preserve the full state vocabulary:

```text
not_run
blocked
diagnostic_failed
diagnostic_passed
authoritative_failed
authoritative_inconclusive
authoritative_passed
human_rejected
human_accepted
```

Every test group must expose:

- required positive proof;
- required negative controls;
- observed typed outcome;
- evidence location;
- limitation or unresolved risk;
- next action.

A chart, diagram, or status summary never substitutes for the machine-readable receipt. The visual explains the receipt; it does not manufacture authority.

## Acceptance checks

A consequential update is incomplete when a reviewer cannot answer these questions in under one minute:

1. What changed?
2. What is currently blocked?
3. What evidence supports the status?
4. What remains unproven?
5. What happens next?

If those answers require reading an entire thread, correlating several PR comments, or interpreting raw CI logs, the update has failed this standard.
