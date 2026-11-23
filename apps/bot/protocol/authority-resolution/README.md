# Authority resolution conformance fixtures

Synthetic fixtures and a test harness for [#55](https://github.com/BicameralAI/bicameral/issues/55), written against the target behavior of ARCH-0005 (accepted: deterministic, replay-stable governed context reads). They change no product behavior. The resolver does not exist yet, so every row that needs it is **BLOCKED**, never PASS.

- Fixtures: `fixtures/*.json`
- Test: `apps/bot/crates/bicameral-api/tests/authority_resolution_conformance.rs`
- Run: `cd apps/bot && cargo test -p bicameral-api --test authority_resolution_conformance`

## What each fixture holds

Every fixture is invented data (`synthetic://` source URIs, an invented Product). It follows the forward-defining payload schemas in `../spec-registry/` and adds only these keys around them:

| Key | Meaning |
|---|---|
| `snapshots[]` | One entry per snapshot: `source_snapshot`, `governed_domain`, `signoff` (or `null`), `supersede_snapshot` (or `null`), and `replayed_state`, one of `tracked`, `approved`, `rejected`, `superseded` (the states in ARCH-0004). |
| `check_records[]` | Stored `CheckRecord` payloads: one unordered digest pair each, stored in lexicographic order. A signoff's embedded `check_records` must be copies of these and cite the signed-off digest. The harness does not require a signoff to embed every record its approval needed: how the stored set and the signoff copy relate is pending #40. |
| `access_context` | `{ "product_id" }`. A test-only stand-in for the existing access/read context (ARCH-0005 decisions 1 and 2). Not a proposed access-context shape. |
| `replay_fault` | Optional. `"partial"` marks the replay input as incomplete. A test-only fault switch, not a proposed field of the #40/#42 replay. |
| `query` | `{ "product_id", "as_of" }`. Any non-null `as_of` must expect a refusal; its format is not decided. |
| `expected` | `{ "refusal", "governing", "excluded", "typed_unknown" }`: `refusal` is `null` or `{ "reason" }` when the whole read is refused; `excluded` entries carry a reason; `typed_unknown` entries are `{ "label", "snapshot_addrs" }`, a scoped unknown naming the snapshots it covers. |

The labels are **test-local**. ARCH-0005 asks for no new broad status enum. None of these meanings matches an existing type exactly: `GraphScopeCoverage` describes code-graph indexing and `FreshnessCaveatCode` graph refresh. `SourceFreshness` (`Stale`, `Offline`) is the candidate to reuse once stale or inaccessible inputs get fixtures.

- Refusals: `invalid_replay`, `as_of_not_supported`, `partial_replay`
- Exclusion reasons, each tied to a replayed state: `not_approved` (tracked), `superseded`, `rejected`. An approved snapshot is never excluded; it governs or is a typed unknown.
- Unknowns: `missing_check_record`, for an approved snapshot that lacks a `conflict: false` record with another approved source

Removed: `conflict_unresolved` (a `conflict: true` record between approved snapshots is now an `invalid_replay` refusal, and a missing record a `missing_check_record` unknown); `out_of_scope` (no scope input exists yet); and the unused `extract_failed`, `stale_source`, `partial_corpus`, `inaccessible_source` and `unsupported_relation`. Labels come back with the fixtures that need them.

## Cases

Every row is **BLOCKED (ARCH-0005 resolver not implemented; governed replay pending #40/#42)**; the `as-of-is-not-supported` row adds that `as_of` is Not decided in ARCH-0005 and needs decision-owner sign-off. The expected outcome is the target the harness checks fixtures against, not a claim about any implementation.

| Fixture | Expected (target) | Basis |
|---|---|---|
| `tracked-is-not-governing` | A tracked snapshot is excluded; nothing governs | ARCH-0005 fixture 1 |
| `supersession-with-later-draft` | The approved successor governs; the superseded and the later tracked draft are excluded | ARCH-0005 fixture 3 |
| `two-approved-non-conflicting` | Two approved snapshots of different sources with a `conflict: false` record both govern; two valid sources are not a tie | ARCH-0004 approval rule; the 2026-11-20 code-owner answer 2 on #55 |
| `conflicting-approvals-are-invalid-replay` | A `conflict: true` record between two approved snapshots refuses the read (`invalid_replay`); no winner is picked | ARCH-0004 (such an approval is refused, so the replay is invalid); the same answer 2 |
| `missing-check-record-is-scoped-unknown` | C governs; A and B, which have no record between them, are a scoped `missing_check_record` unknown, never read as `conflict: false` | ARCH-0005 decision 4 and fixture 2; the same answer 2 |
| `as-of-is-not-supported` | Any non-null `as_of` refuses the read (`as_of_not_supported`) and is never answered from current state | ARCH-0005 "Not decided", fixture 3; the 2026-11-20 code-owner answer 3 on #55 |
| `partial-replay-is-refused` | A partial replay refuses the read (`partial_replay`), never a reassuring empty result | ARCH-0005 fixture 6 |
| `same-source-approved-twice-is-invalid-replay` | Two approved snapshots of one source refuse the read (`invalid_replay`); the newer one is never picked by `captured_at` | ARCH-0004 (a newer approval supersedes in the same commit); ARCH-0005 decision 1 |

`conflicting-approvals-are-invalid-replay` and `missing-check-record-is-scoped-unknown` both describe states that valid ARCH-0004 replay cannot produce. The first refuses the whole read as a corrupt replay; the second keeps the rest of the scope readable with a scoped unknown. Both are outcomes the 2026-11-20 code-owner answer 2 on #55 allows ("scoped unknown/refusal"): an approved pair with no check record may expect either the scoped unknown (as this fixture does) or the `invalid_replay` refusal; a test checks both. A `missing_check_record` unknown names both ends of the pair, because fixtures do not record which approval came later, so neither end may govern.

When several required refusal conditions hold at once, a fixture may expect any of them; the harness sets no order. An optional refusal (a missing pair) never satisfies a required one: an `as_of` read, for example, must still be refused as `as_of_not_supported`.

`same-source-approved-twice-is-invalid-replay` must refuse rather than report a scoped unknown. ARCH-0005 decision 4 allows either for invalid input; requiring the refusal here is this harness's fail-closed choice, and the owner may revise it.

Not covered yet:

- Scope filtering and disclosure: not in this change.
- Two different sources with the same digest: waiting on the decision in #40/#39. The harness rejects a fixture in which two approved snapshots of different sources share a digest, rather than give it an outcome.
- Historical `as_of` semantics: Not decided in ARCH-0005, so this harness defines none.
- Replay stability across restart and rebuild (ARCH-0005 decision 5): needs a resolver.

## What the passing tests check today

Payload shape is checked by the repository's existing registry validator, not by a second schema implementation in Rust: the test runs `python3 -I scripts/validate_authority_resolution_fixtures.py` (from `apps/bot`), which reuses `errors_for` from `scripts/validate_spec_registry_schemas.py` on every `source_snapshot`, `governed_domain`, `signoff`, `supersede_snapshot` and `check_records` payload. It needs `python3` (3.10 or newer).

The Rust test checks the rules that span several objects, in this order (`check_all`):

1. Every fixture is marked synthetic and contains no URL, email or local path.
2. `source_digest` recomputes from `claims`, `atoms`, `claim_assertions`, `relations`, and ids and references inside a domain are consistent.
3. `Signoff` is bound to its own snapshot and domain digest. A `supersede_snapshot` event names the snapshot it replaces and carries that snapshot's own `source_uri`. Its successor must be another snapshot in the fixture, and may belong to another source (ARCH-0004).
4. `CheckRecord` pairs are distinct, lexicographically ordered, cite known digests, and carry a boolean `conflict`.
5. `query` holds exactly `product_id` and `as_of`; `query` and `access_context` name the fixture's Product; `replay_fault` is a known value.
6. A signoff's embedded check records are stored check records that cite the signed-off digest.
7. No two approved snapshots of different sources share a digest (undecided, #40/#39).
8. The expected outcome:
   - The fixture's `status` starts with `BLOCKED`.
   - `expected` holds exactly its four parts, and a refusal holds exactly `reason`.
   - The read must be refused for a non-null `as_of`, a partial replay, a `conflict: true` record between approved snapshots, or one source approved twice; it may be refused (`invalid_replay`) for an approved pair with no check record; it may not be refused otherwise. A refusal lists nothing else.
   - Otherwise, every snapshot appears exactly once: governing (approved, with a `conflict: false` record against every other approved snapshot of another source, per ARCH-0004), excluded with a reason that matches its replayed state, or in a `missing_check_record` unknown (approved, and lacking such a record).

Each rule added for #55 (the `conflict` type, rules 5 to 8) has a test. A negative test changes one fixture just enough to break that rule, runs every rule in order, and asserts the rule's own message, so an earlier rule cannot make it pass for the wrong reason. The one permissive rule (an approved pair with no check record may refuse) has a positive test, `a_missing_check_record_may_also_refuse_the_read`. The payload rules carried over from #67 keep their existing tests and positive controls (an empty failed extract, a cross-source successor).

## Digest recipe

SHA-256 hex of one compact UTF-8 JSON object whose only keys are `atoms`, `claim_assertions`, `claims` and `relations`; object keys sorted recursively, array order as stored, provenance fields outside the hash (ARCH-0003). The empty claim set hashes the bytes `{"atoms":[],"claim_assertions":[],"claims":[],"relations":[]}` and has the digest `db4cd0ee0b2a1dbdbe2e0aea7ae0d7564d8ca2fb47d5b5ab458eb22c1114cbd9`; the test asserts it. To recompute one by hand:

```python
import hashlib, json
obj = {k: domain[k] for k in ("claims", "atoms", "claim_assertions", "relations")}
print(hashlib.sha256(json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())
```
