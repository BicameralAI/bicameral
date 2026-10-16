# External-Ingest Protocol Distribution Release Checklist

Use this checklist for every separately distributed Integrations → Bot protocol
change. CI receipts and local validation are evidence only; they do not create
Product state, authority, acceptance, or deployment approval.

## Contract preparation

- [ ] Classify the change as compatible or breaking; bump the integer protocol
  version only for a breaking wire or semantic change.
- [ ] Regenerate the Bot-owned JSON Schema from Rust and update
  `protocol/external-ingest/contract.json` plus its fingerprint.
- [ ] Confirm the schema and Bot runtime accept and reject the same receipt
  shapes, including negative fixtures.
- [ ] Open linked Bot and Integrations PRs and record exact commits, schema hash,
  and contract fingerprint in both.

## Producer and consumer validation

- [ ] Bot advertises the expected protocol version, supported versions, schema
  hash, fingerprint, delivery endpoint, and receipt requirement.
- [ ] Integrations checks capabilities before delivery and fails closed on an
  unknown version, schema hash, or fingerprint.
- [ ] A compatibility failure leaves the source cursor/checkpoint unchanged and
  emits a value-free typed diagnostic.
- [ ] Compatible delivery crosses real Integrations and Bot processes and
  produces a Bot receipt without bypassing redaction or Bot authority.

## CI/CD and rollout

- [ ] Bot CI verifies generated schema, manifest, fingerprint, Rust constants,
  runtime acceptance, and rejection cases.
- [ ] Integrations CI verifies its pinned Bot commit, schema hash, contract
  fingerprint, capability fixture, and delivery behavior.
- [ ] Deploy Bot support before switching Integrations to the new version.
- [ ] Exercise version skew in both directions and record the exact terminal
  commands and non-production deployment receipts.
- [ ] Prove rollback by restoring the previous producer pin without data loss or
  cursor advancement during incompatibility.
- [ ] Remove or deprecate the legacy endpoint only after dual-run parity and a
  named human acceptance decision.
