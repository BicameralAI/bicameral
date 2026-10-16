# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities to **security@bicameral-ai.com**. Do not disclose
suspected vulnerabilities publicly until maintainers have had a reasonable opportunity to
investigate and coordinate remediation.

Include a description of the issue, the affected surface (code graph, conflict oracle,
indexing, grounding), reproduction steps, and expected versus observed behavior.

## Supported versions

This repository tracks the Bicameral Cloud optimization layer before a tagged release line
exists. Security fixes are accepted against the default branch unless a release branch is
announced.

## Scope

Security reports are especially relevant when a change could:

- Cause Cloud to write or mutate canonical decision state (it is an oracle, not an
  authority — ADR-0001)
- Convert an advisory signal or suggestion into direct blocking authority
- Leak organization-scale code-graph data, source payloads, tokens, secrets, or customer
  data — including any **sibling-tool artifacts** — into a commit, log, response, or tracked
  reference
- Cross tenant or organization isolation boundaries in the hosted graph
- Degrade grounding so results are no longer evidence-backed

## Response

Maintainers will acknowledge reports, triage severity, and publish remediation details when
disclosure is appropriate.
