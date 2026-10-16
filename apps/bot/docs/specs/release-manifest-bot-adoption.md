# Release Manifest Adoption: Bicameral Bot

Status: implemented for v0.1.12 candidate
Related Factory blueprint: https://github.com/BicameralAI/bicameral-factory/pull/290

## Purpose

Defines Bot's release-manifest role and its executable provenance gate. It does not implement a release controller or alter Bot product authority.

## Bot-owned release inputs

For each eligible Bot commit, CI must publish immutable provenance for the daemon used by the hosted bridge, compiled hosted SPA bundle, hosted-bridge protocol/origin-policy fingerprint, and integration-ingress/MCP-egress contract fingerprints. Branch names, mutable tags, fixtures, and Cloud-side rebuilds are not Bot release inputs.

## Journey closures

Hosted dashboard browser closure: Bot daemon bridge artifact + hosted SPA + hosted-bridge interface fingerprint.

Integration-to-Bot-to-MCP closure: Bot integration-ingress interface + Bot MCP ToolRequest interface.

Bot emits interface fingerprints but does not decide required cross-repository journeys or advance the production release pointer. A Bot change invalidates only closures whose declared Bot artifact, interface contract, or policy input changes. Initially, any changed Bot daemon or SPA artifact in the hosted closure invalidates that conservative closure.

## Implemented gates

1. CI binds source commit, daemon digest, SPA digest, and interface fingerprints.
2. Hosted bundle build rejects production fixture fallback.
3. Manifest consumers verify Bot provenance before Cloud assembly.
4. Bot-to-Cloud evidence is keyed to the exact hosted closure.
5. Integration/MCP evidence is keyed to its independent closure.

## Release assignment and staging

Atlas schema-v2 Release Plans are the sole authoritative source for Release
Unit assignment and staging. The Bot workspace Cargo version remains the
version anchor, while exact assignment receipts, topology checks, signed
staging readiness, version-scoped `dev` assembly, and delivery retain their
separate authorities.

The former repository-local shadow plan and advisory pull-request workflow were
removed because they duplicated the Cargo version without assigning, freezing,
staging, or delivering anything. Product repositories must not carry a second
mutable Release Plan beside Atlas.

## Authority boundaries

- Bot remains local authority for pairing, local data, canonical mutation, and projections.
- Release records and topology receipts are operational evidence, not Decision Ledger state.
- Bot must not infer production delivery from issue closure or a component test.

## Non-goals

No Cloud mutation authority and no new Bot canonical lifecycle state. Terminal delivery still depends on Cloud promotion, exact vendored-SPA topology, and served-identity verification.
