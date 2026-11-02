# Authority resolution conformance fixtures

Synthetic fixtures and a test harness for [#55](https://github.com/BicameralAI/bicameral/issues/55), written against the target behavior of ARCH-0005 (accepted: deterministic, replay-stable governed context reads). They change no product behavior. The resolver does not exist yet, so every row that needs it is **BLOCKED**, never PASS.

- Fixtures: `fixtures/*.json`
- Test: `apps/bot/crates/bicameral-api/tests/authority_resolution_conformance.rs`
- Run: `cd apps/bot && cargo test -p bicameral-api --test authority_resolution_conformance`

## What each fixture holds

Every fixture is invented data (`synthetic://` source URIs, an invented Product). It follows the forward-defining payload schemas in `../spec-registry/` and adds only three things around them:

| Key | Meaning |
|---|---|
| `snapshots[]` | One entry per snapshot: `source_snapshot`, `governed_domain`, `signoff` (or `null`), `supersede_snapshot` (or `null`), and `replayed_state`, one of `tracked`, `approved`, `rejected`, `superseded` (the states in ARCH-0004). |
| `check_records[]` | `CheckRecord` payloads: one unordered digest pair each, stored in lexicographic order. |
| `query` / `expected` | The read to resolve, and the typed outcome it must produce: `governing`, `excluded` (with a reason), `unresolved`, `typed_unknown`. |

The reason labels in `expected` and the `typed_unknown` labels are **test-local**. ARCH-0005 asks for no new broad status enum, so they are not a product type.

## Cases

| Case | #55 row | Status |
|---|---|---|
| `tracked-is-not-governing` | Draft never wins (tracked is not approved) | BLOCKED (ARCH-0005) |
| `supersession-with-later-draft` | Supersession; draft never wins | BLOCKED (ARCH-0005) |

More cases (equal-authority tie, wrong scope, replay and restart, missing `CheckRecord`, failed extract, polarity, historical `as_of`) are added in follow-up commits. The historical `as_of` row stays BLOCKED for a second reason: ARCH-0005 lists `as_of` as not decided, so this harness does not define its semantics.

## What the passing tests check today

- every fixture is marked synthetic and contains no URL, email or local path
- each payload has only keys the registry schemas allow, and the required ones
- `source_digest` recomputes from `claims`, `atoms`, `claim_assertions`, `relations`
- `Signoff` is bound to its own domain digest; `supersede_snapshot` names a successor of the same source
- `CheckRecord` pairs are distinct, lexicographically ordered, and cite known digests
- every snapshot is either governing (approved only) or excluded with a reason

## Digest recipe

SHA-256 hex of one compact UTF-8 JSON object whose only keys are `atoms`, `claim_assertions`, `claims` and `relations`; object keys sorted recursively, array order as stored, provenance fields outside the hash (ARCH-0003). The empty claim set hashes the bytes `{"atoms":[],"claim_assertions":[],"claims":[],"relations":[]}` and has the digest `db4cd0ee0b2a1dbdbe2e0aea7ae0d7564d8ca2fb47d5b5ab458eb22c1114cbd9`; the test asserts it. To recompute one by hand:

```python
import hashlib, json
obj = {k: domain[k] for k in ("claims", "atoms", "claim_assertions", "relations")}
print(hashlib.sha256(json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())
```
