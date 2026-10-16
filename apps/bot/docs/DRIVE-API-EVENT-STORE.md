# Google Drive API event-store adapter

## Status

This document describes the provider-backed Google Drive event-store adapter core introduced by Bot #1086 / PR #1088.

The implementation is **supporting provider infrastructure**, not a claim that the production Google Drive journey is complete. `DriveApiAdapter` exists and is testable behind an injected `DriveApiTransport`, but production HTTP, durable credential consumption, validated folder-resource wiring, hosted `product.create` composition, and terminal real-provider evidence remain separate work.

For the existing local folder-sync substrate, see [`DRIVE-FOLDER-SETUP.md`](DRIVE-FOLDER-SETUP.md). The two paths intentionally preserve the same event representation while differing in how durability is established.

## Adapter boundary

`EventStoreAdapter` remains substrate-neutral. The provider-backed implementation lives in `crates/bicameral-runtime/src/drive_api_event_store.rs` and preserves the established Drive/basic-store semantics:

- one canonical `EventStoreEntry` per `<event_uuid>.json` object;
- deterministic replay ordered by timestamp, then UUID;
- `DurableReplay` is the only advertised substrate capability;
- provider/auth/network failures are typed failures, never empty-success replay;
- provider I/O and credential ownership stay behind `DriveApiTransport` rather than leaking tokens into `EventStoreAdapter` or canonical state.

The local `DriveFolderAdapter` uses a synced filesystem and local freshness markers. The provider-backed `DriveApiAdapter` requires stronger append evidence: success is not returned until the provider object is downloaded and its bytes exactly match the canonical serialized event.

## Append and idempotency contract

For a canonical event append, the adapter core:

1. serializes the exact `EventStoreEntry` using the established JSON representation;
2. derives `<event_uuid>.json`;
3. asks the injected provider transport for matching direct children in the validated folder;
4. creates the provider object when no exact candidate exists;
5. downloads the chosen provider object;
6. rejects success unless the downloaded bytes exactly match the canonical serialized bytes;
7. returns a safe `DriveAppendConfirmation` witness after exact readback.

A retry of the same event reuses and verifies the existing provider object rather than creating a second canonical event. The same event UUID with different bytes is a conflict. Multiple matching provider objects are also a conflict rather than an arbitrary winner.

The full #1086 contract additionally requires the production transport to prevent an unrelated Drive object with the same display name from being silently accepted as canonical identity. That provider-side identity binding belongs in the real Google transport and remains required before #1086 itself can close.

## Confirmation witness

`DriveAppendConfirmation` carries only safe provider facts needed by later composition:

- provider file id;
- provider revision/version when available;
- selected-resource fingerprint;
- exact event digest;
- confirmation timestamp;
- whether the object was newly created or an exact retry.

It is **not** canonical Product state and cannot authorize Product creation, membership, SourceBinding, Decision state, publication, deployment, release, or provider-resource selection. The later #1087 composition path is responsible for any signed `ProductCreationCommitProof` after provider-confirmed durability.

## Dependencies for production wiring

The core intentionally does not own these responsibilities:

| Dependency | Owner | Why it remains separate |
|---|---|---|
| Connection registry/credential reconciliation | #1083 | Persisted metadata must not remain falsely `connected` when the credential is absent. |
| Durable refreshable Google credentials | #1084 | Provider transport must consume credentials through the approved daemon secret boundary. |
| Google folder selection/readiness | #1085 | The adapter must receive a daemon-validated folder id rather than invent resource authority. |
| Real Google Drive REST transport | #1086 residual | Must implement folder-bounded create/list/get/download, shared-drive flags, and provider identity binding. |
| `product.create` composition + signed proof | #1087 | Product authority and provider durability proof belong above the storage transport. |

Until those dependencies are composed, `DriveApiAdapter` is a deterministic provider-adapter core only.

## Deterministic tests in PR #1088

The embedded unit suite covers:

- exact provider readback before append confirmation;
- safe provider confirmation shape;
- same-event retry idempotency;
- same-event-id/different-content conflict;
- duplicate provider-object conflict;
- deterministic replay ordering;
- safe skipping of non-event JSON and non-JSON objects;
- fail-closed provider authentication failure during health checks;
- `DurableReplay`-only capability declaration.

Repository gates for this work are:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace -- -D warnings
```

GitHub Actions also runs the Factory attestation, Governance Boundary, Secret Scan, Dependency Review, dashboard checks, and hosted-bridge conformance. A PR is not considered deterministic-CI complete until all applicable checks pass on the exact final head.

## What these tests do not prove

The deterministic suite does not prove:

- real Google OAuth/token refresh;
- OS-backed secret persistence;
- Google Picker/folder-resource selection;
- real `files.create`, `files.list`, or `files.get` behavior;
- shared-drive behavior against a real account;
- provider object identity binding under Google metadata;
- terminal browser/daemon/provider topology;
- Product creation or commit-proof activation.

Those are intentionally separate evidence layers. Component tests are supporting evidence, not a substitute for terminal provider validation.

## Relationship to the local Drive-folder substrate

Both Drive modes preserve the same canonical event representation, but their durability evidence differs:

| Property | `DriveFolderAdapter` | `DriveApiAdapter` core |
|---|---|---|
| Storage access | Local synced filesystem | Injected provider transport |
| Sync dependency | External Drive Desktop/rclone/Syncthing | Provider API transport |
| Append confirmation | Local file materialization | Exact provider readback |
| Freshness | Local sync marker | Provider reachability at check time |
| Conflict posture | Existing local-folder semantics | Exact retry/conflict handling, no arbitrary duplicate winner |
| Production status | Existing local substrate | Core implemented; real Google transport/composition still pending |

Do not describe the local synced-folder adapter as Google REST-backed, and do not describe the provider adapter core as a production-ready Google event store until the remaining #1083–#1087 composition and terminal evidence are complete.

## Production REST transport (#1086 residual slice)

`bicameral_runtime::drive_api_transport` provides the production
implementation of `DriveApiTransport` plus the credential composition:

- `HttpDriveApiTransport` calls the Google Drive REST API directly:
  `files.list` for marker-bound discovery and folder-bounded replay,
  multipart/related `files.create` uploads, `alt=media` downloads for
  exact readback, and a fail-closed `files.get` folder probe (folder MIME,
  not trashed, `canAddChildren`). Every request carries
  `supportsAllDrives=true` (+`includeItemsFromAllDrives=true` on lists) so
  My Drive and shared-drive folders behave identically. List pagination is
  followed to exhaustion.
- **Provider-side canonical identity binding**: every created event object
  is stamped with the `appProperties` marker
  `bicameralCanonicalEvent=<uuid>.json`. Retry discovery and replay
  listing filter on that marker, so an unrelated same-name Drive object
  can neither be adopted as the canonical event on retry nor inject
  entries into replay. Exact byte readback remains the confirmation
  contract on top of the marker.
- `KeyringGoogleAccessTokenProvider` composes the #1084 credential seam:
  access tokens come from the canonical `connection.google/oauth_token`
  envelope (legacy `google_drive` alias during the compatibility window);
  an expired access token with a stored refresh token triggers a
  `refresh_token` grant and the rotated envelope is re-persisted
  canonically, preserving the refresh token when Google omits it. Missing,
  unusable, or refresh-rejected credentials fail closed as
  `Authentication` errors. Token material never appears in errors, logs,
  or returned metadata.
- `production_drive_event_store(folder_id, secrets_store, oauth_config)`
  composes the adapter from the #1085 validated folder authority and the
  daemon credential boundary. The folder id must come from validated
  resource selection; the function never invents one.
- The core trait is synchronous; HTTP work runs on short-lived scoped OS
  threads so daemon async contexts never block or nest runtimes.

Deterministic provider-backed evidence lives in
`crates/bicameral-runtime/tests/drive_api_transport_stub.rs`, which runs
the full adapter append/readback path, refresh rotation, marker-bound
queries, pagination, probe fail-closed matrix, and typed failure mapping
against a local HTTP stub. Live Google behavior and the terminal Product
Create journey remain unproven until #1087 composition and terminal QA.
### Restack hardening (#1134)

The transport is restacked from historical PR #1099 onto the current
lineage with four corrections: native/public refresh grants omit
`client_secret` entirely (no empty pseudo-secret), a refresh lock plus
persisted-envelope re-read enforces exactly one effective refresh
generation under concurrency, a deterministic 60-second near-expiry skew
(`REFRESH_SKEW_SECONDS`) refreshes tokens before they can die in flight,
and refresh rejection or a secret-store write failure remains fail-closed
`Authentication`.
