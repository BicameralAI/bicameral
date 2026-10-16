# Terminal validation: MCP-assisted local-folder-to-Product binding (J-A5)

Source issue: [bicameral-bot#788](https://github.com/BicameralAI/bicameral-bot/issues/788)
(owner correction 2026-07-27), reconciled under
[bicameral-bot#1190](https://github.com/BicameralAI/bicameral-bot/issues/1190).

Evidence level: **real-process lifecycle acceptance** (real MCP server/client
process, production daemon binding handler, durable operator-local stores, and
the daemon-owned hosted bridge — no test doubles), plus an optional **browser
downstream witness**.

This is the single documented terminal validation command for the J-A5
`mcp_local_folder_product_binding` User Journey. It supersedes the earlier
"Hosted Workspace Binding" (`hosted_workspace_binding`) framing. Retained traces
carry `supersedes: hosted_workspace_binding` so historical evidence can be
correlated, but that older contract is no longer authoritative: prior green runs
against the superseded criteria set do not close the corrected journey.

## Corrected journey

```text
developer opens local folder
  -> invokes MCP bind
  -> MCP proposes association with a Product
  -> operator explicitly confirms
  -> daemon validates Product registration, capability, path safety, and repository evidence
  -> daemon persists LocalWorkspaceBinding
  -> MCP reports the local folder bound
```

| Term | Authority rule |
| --- | --- |
| `WorkspaceBindingProposal` | MCP may propose and render it transiently; MCP must not persist binding authority. |
| `LocalWorkspaceBinding` | The daemon alone validates and persists an explicitly confirmed binding. This persistence is the journey's authoritative commit. |
| `project_id` | Canonical Product authority key; never inferred solely from a Git remote or local path. |
| Repository remote | Validation evidence only. Matching SSH/HTTPS forms converge on one Product, but the remote is not authority. |
| `code_grounding_ready` | Daemon-authored downstream projection. A hosted browser render may witness the persisted binding but does not define whether binding succeeded. |

## Command

```bash
python3 scripts/validate_hosted_workspace_binding.py \
    --topology-receipt <path-to-cloud#135-topology-receipt.json> \
    --mcp-repo <checkout-of-BicameralAI/bicameral-mcp> \
    --mcp-commit <40-char bicameral-mcp commit> \
    --cloud-repo <checkout-of-BicameralAI/bicameral-cloud> \
    --cloud-commit <40-char bicameral-cloud commit> \
    --factory-commit <40-char factory commit> \
    --trace test-artifacts/local-folder-product-binding/<run>/trace.json
```

The script filename is unchanged because `scripts/run_integration_bot_mcp_topology.py`
and `scripts/run_operator_service_topology.py` import its daemon/repository
helpers; the journey identity, criteria, and retained artifacts inside it are
the corrected contract.

The command:

1. builds the production bot binary and the built dashboard SPA;
2. installs and runs the **real** `bicameral-mcp` server/client over stdio;
3. copies that exact SPA into a disposable Cloud runtime and starts Cloud as
   the sole UI host (needed only for the downstream witness);
4. starts the API-only production daemon (`bicameral … dashboard`) on a
   temporary git repository with both SSH and HTTPS remotes, then pairs a real
   hosted-bridge session so the harness can create a Product and read the
   daemon's own projections;
5. creates one canonical Product and invokes daemon-owned
   `product.continue_setup`, proving registration is present before binding;
6. has the real MCP process **propose** a Product association (`confirmed:false`)
   and confirms that nothing is persisted and no authority is claimed;
7. sends an explicit operator confirmation (`confirmed:true`) that reaches the
   production daemon, which alone validates `project_id`, registration,
   capability, and remote evidence before persisting a `LocalWorkspaceBinding`;
8. exercises SSH/HTTPS remote convergence and daemon restart + replay: the
   durable binding must survive byte-identical and the restarted daemon must
   report the binding itself;
9. optionally observes a real browser loading Cloud `/app` and rendering
   daemon-authored `code_grounding_ready` (downstream witness, non-gating);
10. exercises the fail-closed negatives (cancellation, unregistered Product,
    capability mismatch, contradictory candidate, unavailable daemon, failed
    rebind);
11. scans every Cloud/browser payload for local-path leakage;
12. writes a sanitized trace carrying the exact bot/MCP/Cloud commits, the SPA
    digest, the hosted-bridge contract fingerprint, the topology receipt digest,
    and the durable-store versions.

## Acceptance criteria

Required criteria gate the verdict. Every one must be `passed`; a `blocked`
criterion names the missing production capability and never fabricates success.

| Criterion | Gating |
| --- | --- |
| `propose_without_persisting_authority` | required |
| `confirm_reaches_daemon_and_persists` | required |
| `ssh_https_remotes_converge` | required |
| `binding_survives_restart_replay` | required |
| `cancellation_leaves_no_binding_or_pending` | required |
| `negative_paths_create_no_binding` | required |
| `failed_rebind_preserves_previous` | required |
| `no_local_path_in_cloud_browser_payloads_logs` | required |
| `trace_carries_component_fingerprints` | required |
| `browser_renders_code_grounding_ready_after_reconnect` | **downstream witness, non-gating** |

`binding_survives_restart_replay` is satisfied by the persisted
`LocalWorkspaceBinding` surviving restart byte-identical **and** the restarted
daemon reporting the binding (`has_workspace_binding` or
`workspace_readiness.binding_state = local_workspace_bound`). The downstream
`code_grounding_ready` / `event_store_healthy` projection is recorded as
evidence but does not gate the criterion.

The browser witness is recorded under `downstream_witnesses` in the trace with
`gating: false`. Its failure, absence (`--no-browser`), or a browser error is
classified separately and never changes `status` or
`terminal_user_journey_acceptance`. A passed witness cannot rescue a failed
required criterion either.

## Subprocess failure diagnostics

When a production subprocess fails (for example `real MCP workspace.bind failed
with exit code 1`), the harness no longer discards the reason. The retained
trace records `error_diagnostics`:

- `label`, `executable` (basename only — never the path or arguments),
  `returncode`, `timed_out`;
- `stdout_tail` / `stderr_tail`: the last 40 lines and at most 2000
  characters per stream, with `*_truncated` flags;
- `sanitized: true` and the bounds used.

Sanitization is deterministic and applied to every string in the trace before
it is written:

- secret-shaped values are redacted (`Bearer …`, `*token*`, `*secret*`,
  `*password*`, `api_key`, `nonce`, `poll_proof`, `csrf`, GitHub/AWS/Slack token
  shapes, PEM private keys);
- every harness-known local root (work root, bot/MCP/Cloud checkouts, operator
  home, workspace, trace directory) is replaced by a placeholder such as
  `<work-root>` or `<workspace>`;
- any remaining `/home/…`, `/Users/…`, `/tmp/…`, Windows drive, UNC, or `~/`
  path shape is replaced by `<local-path>`;
- ANSI escapes and control characters are stripped.

Loopback URLs, GitHub remotes, criterion ids, and daemon `error_kind` values
survive sanitization so the failure stays diagnosable.

## Requirements

- `git`, `node`/`npm`, `cargo` (Rust ≥ 1.85 for the edition-2024 dependency
  tree), `uv`, and (for the witness) Playwright Chromium. `--cdp-url` can attach
  to an existing Chrome/Chromium instance; `--no-browser` skips the witness.
- A checkout of `BicameralAI/bicameral-mcp` (`--mcp-repo`) at the pinned commit.
- A checkout of `BicameralAI/bicameral-cloud` (`--cloud-repo`) at the pinned commit.
- Optionally, a cloud#135 topology receipt (`--topology-receipt`).
- `BICAMERAL_TOPOLOGY_EXECUTION_PROVIDER=devin`; GitHub Actions is rejected
  before any production process starts.

## Pinned component references

The next terminal execution must run against current heads and record the
exact SHAs in the trace (`components` / `fingerprints`). Pins verified on
2026-09-10 while reconciling this harness:

| Component | Ref | Commit |
| --- | --- | --- |
| `BicameralAI/bicameral-bot` | `dev` (base of the #1190 PR) | `bc1c4c80047eea03abcb026f59d65afe0c2aa7da` |
| `BicameralAI/bicameral-mcp` | `dev` | `9a2d55706070bfa301611b7e42ce2d806230a387` |
| `BicameralAI/bicameral-mcp` | `v0.17.3` / `main` | `92a90651a37b8fd4e9c21c8974677668340719fb` |
| `BicameralAI/bicameral-cloud` | `dev` | `b4d3c14395e33f590610289d1c1eab6ea55680d5` |
| `BicameralAI/bicameral-cloud` | `main` | `86c3b66299e10f69312615e454e194a742cc8ba7` |
| Hosted-bridge contract | `protocol/hosted-bridge/contract.sha256` | read at run time |
| Topology anchor | cloud#135 receipt (`--topology-receipt`), historical merge `f673d1d24001e010edf9f9d19f5f34d43a88bf3f` | digest recorded when supplied |

Pass `--mcp-commit` / `--cloud-commit` so the run fails closed if a checkout
drifts from the intended pin. The Bot commit is read from the checkout that
runs the script.

## Registration and topology preconditions

The former `unregistered_project` blocker is a tested precondition, not an
expected failure. The harness uses the shipped Product flow:

1. `product.create` commits canonical creator ownership;
2. `product.continue_setup` writes the durable `ProjectRegistration` and opens
   replay through the daemon;
3. restart hydration reconstructs that registration; and
4. only then may MCP propose and confirm `workspace.bind`.

The daemon never serves the SPA or a fixture UI. Cloud is the only UI host, and
the browser talks directly to the API-only daemon through the production hosted
bridge contract. Any missing registration, creator authority, hydration, or
binding leaves a named required criterion red; a missing browser projection
leaves only the downstream witness red.

## Acceptance boundary

Harness reconciliation (#1190) is not journey completion. The J-A5 terminal
direct-execution run against the pinned refs above is a separate follow-up; its
sanitized trace requires human product review (Kevin Knapp is the named
reviewer for the first successful trace) before #788 is considered accepted.
This document grants no release, deployment, or `dev` → `main` authority and
does not revive retired `release-unit / merge-eligible` machinery.
