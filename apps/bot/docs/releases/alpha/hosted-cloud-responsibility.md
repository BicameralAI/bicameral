# Hosted Cloud Responsibility For Alpha

Alpha cloud is a coordination and advisory layer. It is not Bicameral's
canonical project, ledger, or local runtime authority.

## Cloud Owns

- Hosted project discovery and non-canonical project previews (`HostedProjectPreview`).
- Team/session coordination needed to help users find or join the same project,
  including minting a narrow `ProductJoinTicket` for same-domain auto-join
  (see `docs/specs/domain-product-onboarding-contract.md`, issue #765).
- Lightweight hosted amenities, such as daemon pairing help. (Invite/rendezvous
  is a legacy/deferred amenity, not the alpha join default.)
- Optional hosted advisories, such as broader graph, conflict, or grounding signals.
- Non-secret hosted metadata and receipts needed to debug cloud-assisted flows.

## Cloud Must Not Own

- Decision Ledger events or accepted governance facts.
- Canonical project settings from the selected event-store substrate.
- Local provider tokens, OAuth refresh tokens, local paths, or code indexes.
- SourceSnapshot bodies, SourceEvidence, or private code by default.
- Signoff, compliance, no-conflict, enforcement, or BindingEvidence authority.

## Runtime Rule

```text
hosted cloud may discover, preview, relay, advise, and coordinate
local daemon validates local authority and access
selected event store remains canonical for project and ledger state
```

## Alpha Cut Line

Hosted cloud can make setup and team discovery easier. It cannot make a project
true, mutate the Decision Ledger, or replace local daemon validation.

Related: `docs/specs/explicit-project-registration-hosted-local-discovery.md`,
ADR-0003, ADR-0007, ADR-0023.
