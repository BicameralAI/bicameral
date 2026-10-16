# Contributing to Bicameral Bot

Thank you for considering a contribution. Bicameral Bot is a local-first, grounded
governance loop: edges emit typed evidence and commands, a local daemon validates policy,
and canonical state materializes only through adapters and replay. Contributions must
respect that authority boundary — see [`README.md`](README.md) and [`docs/adr/`](docs/adr/).

## How to contribute

1. Fork the repository (or branch, if you have access).
2. Create a topic branch off the default branch — e.g. `feat/<short-name>` or
   `fix/<short-name>`.
3. Keep daemon, gateway, adapter, and mod behavior aligned with the authority boundaries in
   `README.md` and the ADRs. Adapters and mods emit candidates, evidence, hints, signals,
   and advisories through protocol-compatible paths; **they never write canonical state
   directly.**
4. Add or update tests when behavior changes.
5. Run the local checks below before opening a pull request.
6. Open a pull request against the default branch (`main`) using the repository's PR
   conventions. Keep the change focused and describe the evidence and expected behavior.

## Local checks

These mirror the CI gates (`.github/workflows/rust.yml`, `secret-scan.yml`):

```bash
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
python scripts/secret_scan.py
```

If you have [`pre-commit`](https://pre-commit.com/) installed, the secret scan also runs on
commit:

```bash
pre-commit run --all-files
```

## Product PRs and the factory attestation

For Bicameral **product** work, follow the factory instructions referenced in
[`AGENTS.md`](AGENTS.md): read `.bicameral/factory-context.local.json`, prefer the copied
`bic-*` skills under `.agents/skills/`, and create/validate the required factory
attestation under `.bicameral/factory-attestations/<factory_commit>.<run-id>.json`, where
`<run-id>` is a stable per-PR token such as the head branch slug. The per-run filename keeps
concurrent PRs that share a `factory_commit` from colliding on the same file at merge time
(the legacy `<factory_commit>.json` form remains valid for existing records but must not be
used for new attestations). The committed attestation is the **only** tracked factory
evidence; do not commit local factory install state, prompts, context bundles, run plans, or
raw logs.

## Bring your own tools — the sibling registry

**You are not required to adopt the maintainer's process tooling.** The only thing your PR
must satisfy is the shared **bic-logic** contract (the factory-owned process) plus a clean
leak guard. Everything you run *locally* — your governance system, an AI assistant, an IDE
plugin, or a homegrown framework — is welcome as a **registered sibling**.

A sibling is a local tool whose scratch/state must never leak into a commit or a tracked
reference. The maintainer's own process tool (**Qor-logic**) is itself just a registered
sibling — not an obligation placed on you.

To use your own tool:

1. **Register it** — add a row to [`docs/governance/SIBLINGS.md`](docs/governance/SIBLINGS.md)
   (tool name, scratch root(s), you as owner, the leak-guard rule).
2. **Gitignore its root** — add the root under the *Agent scratch* / *Governance* section of
   `.gitignore`, so the tool is provably un-committable.
3. **Keep artifacts local** — never commit the tool's output, and never name its internals
   in a tracked file.

A built-in default floor already gitignores the common agent-scratch roots (`.qor/`,
`.claude/`, `.cursor/`, `.windsurf/`, `.agent/`, `.agents/`, `.failsafe/`, …), so a tool you
forget to register still cannot leak. Registering simply makes the boundary explicit and
auditable. See the
[governance boundary proposal](docs/plans/governance-boundary-three-layer-proposal-2026-06-08.md)
for the full model.

## Issue reports

Use the issue templates for bugs, feature requests, and documentation problems. Include
source evidence, expected behavior, and any relevant protocol contracts.

## License

Bicameral Bot is dual-licensed under **Apache-2.0 OR MIT** (see
[`LICENSE-APACHE`](LICENSE-APACHE), [`LICENSE-MIT`](LICENSE-MIT), and [`NOTICE`](NOTICE)). By
contributing, you agree that your contributions will be licensed under those same terms,
at the user's option.
