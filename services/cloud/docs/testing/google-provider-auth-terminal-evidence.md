# Google provider-auth terminal evidence

Issue: `BicameralAI/bicameral-cloud#263`

This document defines the evidence boundary for the first real fresh-operator Google Drive OAuth topology. It separates deterministic bootstrap/security assertions that CI can prove from terminal provider assertions that require a real Google authorization and real provider-backed Drive behavior.

## v0.1.28 authority boundary

A bootstrap-only run is not terminal provider evidence. It must leave `terminal_acceptance` false.

For v0.1.28, terminal evidence must prove against exact Bot and Cloud candidate heads:

1. clean operator state with no manually injected Google OAuth client id/secret and no production-selectable synthetic token exchange;
2. enabled, unexpired, environment-matched Google native app configuration from the Cloud publisher;
3. exact Cloud client/scope binding, opaque state, PKCE S256, and the daemon loopback callback;
4. real Google browser consent and authorization-code exchange through the production connection service;
5. daemon-local provider credential custody and `ConnectionRegistry: connected` only after real provider completion;
6. Cloud-first Picker/resource selection through the governed native-client + PKCE contract and `drive.file` boundary;
7. exactly one real accessible Drive resource plus a real provider-backed Drive operation/readiness result;
8. bounded still-valid-cache behavior and wrong-environment/stale/tampered/disabled/rotated configuration negative paths;
9. sanitized teardown and an exact-component terminal receipt;
10. lifecycle-owner review of the first passing trace.

Ordinary daemon-process restart continuity is **not** a v0.1.28 acceptance requirement. The current #263 owner contract explicitly defers service-layer rehydration, cross-process keyring reopening, and ordinary end-to-end Google Auth persistence across daemon restart to v0.1.29. If a real v0.1.28 attempt naturally exercises refresh, retain that observation, but do not require process restart to manufacture terminal acceptance.

The historical Bot Release Unit proposal currently contains `restart-refresh-envelope`. Until that governed proposal is reconciled to the current #263 boundary, `scripts/google_provider_auth_evidence.py --mode terminal` fails closed rather than claiming a restart that was not required or observed.

Mocks and fixtures may validate deterministic negative paths, receipt safety, and parser behavior. They do not substitute for the real Google/provider/Drive assertions above.

## Stage A: deterministic bootstrap/security evidence

`scripts/google_provider_auth_evidence.py` is the non-terminal collector used before a real Google login. It:

- refuses execution when legacy/injected Google client credential environment variables are populated;
- rejects `BICAMERAL_GOOGLE_TOKEN_EXCHANGE_MOCK` on presence, including an empty value;
- fetches the live Cloud `GET /api/v1/provider-app-config/google` payload;
- validates schema, provider, native/public-client type, deployment environment, configuration version, scope-set version, issue/expiry window, and enabled state;
- rejects credential-bearing app-config extensions;
- starts Google connection through the daemon production connection service;
- verifies the generated Google authorization URL uses the exact Cloud client identity, opaque state, PKCE S256, the daemon loopback redirect, and exact Cloud-published scope set;
- requires `drive.file` and rejects broader Drive scopes;
- never persists the authorization URL, state value, PKCE challenge, raw client id, authorization code, or provider token;
- writes only bounded/hash observations and leaves terminal-provider assertions false.

Example against already-running exact candidates:

```bash
python scripts/google_provider_auth_evidence.py \
  --cloud-origin https://app.bicameral-ai.com \
  --daemon-origin http://127.0.0.1:37373 \
  --expected-environment production \
  --cloud-commit <exact-cloud-sha> \
  --bot-commit <exact-bot-sha> \
  --receipt evidence/google-provider-bootstrap.json
```

## Stage B: real Google browser/loopback observation

Use the transient authorization URL returned by the daemon connection-start service. Do not copy the URL into durable evidence.

Expected sequence:

1. Operator starts Drive Connect.
2. Browser visits Google using the daemon-authored authorization URL.
3. Operator completes Google consent using the provisioned Bicameral native/desktop application identity.
4. Google returns to `/api/v1/connections/google/callback`.
5. The daemon resolves the transient auth session, validates state, performs the PKCE-bound code exchange, stores the provider credential envelope locally, and updates `ConnectionRegistry` from the terminal provider result.
6. The browser receives only the static completion/failure surface. Query parameters are not reflected into HTML or retained as evidence.

Browser evidence may retain route/host/status/timing metadata. It must redact query values and provider credentials.

## Stage C: provider-backed Drive evidence

After the real browser flow:

1. Record a sanitized `ConnectionRegistry` projection showing Google connected.
2. Exercise the governed Picker/resource-selection path and bind exactly one accessible Drive resource.
3. Exercise the production downstream Drive path far enough to prove the provider credential is usable.
4. Persist only operation type, success/failure, bounded resource metadata, and timestamps required by the topology contract.

Do not stop/restart the daemon merely to satisfy v0.1.28. Process-restart continuity belongs to v0.1.29.

## Stage D: cache, rotation, and rejection paths

These checks may use controlled configuration responses, but they must exercise the real Bot resolver rather than a second implementation of its policy.

Required assertions:

- transient Cloud failure may use only a still-valid previously validated app-config cache;
- expired cached configuration is rejected;
- wrong deployment environment is rejected;
- malformed/tampered configuration is rejected;
- explicit Cloud disablement is terminal for bootstrap and clears/overrides cache;
- a newly published valid configuration is used for the next authorization attempt without changing the client identity pinned to an already-started auth session.

None of these paths may produce `ConnectionRegistry: connected` without real successful provider completion.

## Receipt contract

The final sanitized topology receipt binds:

- exact Bot and Cloud commits;
- app-config schema/config/scope-set versions;
- fresh-install injected-client/mock assertions;
- bounded bootstrap and loopback observations;
- registry before/after state;
- one real Drive resource and provider-backed readiness result;
- required negative-path outcomes;
- teardown result;
- explicit terminal acceptance boolean.

Forbidden durable material includes raw client id, client secret, authorization URL/code, OAuth state value, PKCE challenge/verifier, access token, refresh token, cookies, Cloud auth tokens, and daemon bridge/session tokens.

A receipt missing any required v0.1.28 terminal observation must not claim terminal acceptance. Evidence collection grants no merge, deployment, Product, SourceBinding, signoff, Decision Ledger, release, or provider-connection authority by itself.

## Direct-Devin terminal runner

The Direct-Devin session consumes exact Bot, Cloud, and Factory revisions. It must use the production Bot process and normal browser flow. It may not inject Google client credentials, select a synthetic token endpoint, edit source during the run, or transfer browser cookies/tokens between tools. Authentication failure stops the affected workflow for human reauthentication.

The runner modes are:

1. `bootstrap`: collect deterministic Stage A evidence.
2. `claim`: validate the admitted attempt binding and emit the bounded claim body.
3. `terminal`: validate bounded terminal observations, the current governed Release Unit checklist, cleanup, sanitization, recording, and delivery before emitting a terminal candidate.

The current Release Unit still carries the superseded `restart-refresh-envelope` checkpoint. Terminal mode intentionally rejects that proposal until the governed checklist is reconciled with #263. This prevents green component CI from quietly turning stale acceptance text into false operating evidence.
