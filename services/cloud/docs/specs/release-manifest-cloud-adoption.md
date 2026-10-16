# Release Manifest Adoption: Bicameral Cloud

Status: implemented for v0.1.12 candidate
Related Factory blueprint: https://github.com/BicameralAI/bicameral-factory/pull/290

## Purpose

Defines Cloud's responsibilities when serving the Bot-built hosted dashboard and the executable checks that bind deployment to those bytes.

## Cloud-owned release inputs

Cloud assembly consumes a manifest-selected Bot SPA bundle by immutable digest and produces a Cloud image digest bound to eligible Cloud commit, exact Bot SPA digest, static-serving/CDN configuration, and hosted dashboard/bridge compatibility policy. Cloud must not clone, build, copy, or deploy an assumed-current Bot dashboard from a floating ref.

## Delivery contract

The production runtime must expose a safe identity endpoint or response header containing active release ID, Cloud image digest, SPA digest, and hosted-bridge protocol version. A promotion controller, not a Cloud merge, advances the production release pointer. Cloud deploys only manifest-selected artifacts.

## Hosted closure

Bot hosted SPA + Bot hosted-bridge interface + Cloud image + static-serving/CDN configuration + browser policy.

An Integrations or MCP change does not rerun this browser topology if this complete closure and profile version are unchanged. A Cloud image, static-serving, identity endpoint, or browser-policy change invalidates it.

## Implemented gates

1. Cloud image assembly rejects an SPA digest different from the manifest.
2. Deployment accepts only manifest-selected artifacts.
3. Runtime identity is checked after deployment.
4. The production deploy job depends on a real browser + real Bot daemon topology over the exact vendored bytes; failure retains the prior Cloud Run revision.

## Authority boundaries

- Cloud renders/coordinates only; it gains no local daemon, Decision Ledger, source, or secret authority.
- Runtime identity reports delivery; it does not decide eligibility.
- Health-only status and staging success are not production delivery evidence.

## Non-goals

No new hosted canonical store or product authority. An external registry/pointer and deployed CDN smoke remain later hardening; this slice makes the repository deployment fail closed on the exact assembled tuple.
