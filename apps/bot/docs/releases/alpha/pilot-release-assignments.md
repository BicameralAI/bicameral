# Pilot Release — Assignments

**Goal:** download link live Sunday, June 14, 2026. External team goes link → running workspace → first reviewed decision in under 15 minutes, unassisted.

**Project board:** [BicameralAI / Pilot Release (project #9)](https://github.com/orgs/BicameralAI/projects/9)

---

## Jin (@jinhongkuan) — Release Captain

- Integrate download link + install instructions on the site — due Sat Jun 13 ([bicameral#120](https://github.com/BicameralAI/bicameral/issues/120))
- Tag `v0.1.0-alpha`, verify release CI tarballs (3 platforms + checksums) — due Sat Jun 13 ([bicameral-bot#292](https://github.com/BicameralAI/bicameral-bot/issues/292))
- Dogfood gate participant: fresh-machine tarball install + golden path — Fri Jun 12 ([bicameral-bot#293](https://github.com/BicameralAI/bicameral-bot/issues/293))

## Silong (@silongtan) — Bot / MCP Surface

- Finalize dashboard for alpha: SetupWizard polish, typed errors for deferred commands — gate Fri Jun 12 ([bicameral-bot#289](https://github.com/BicameralAI/bicameral-bot/issues/289))
- Scope cut audit: all deferred commands return typed unsupported errors, never crash ([bicameral-bot#290](https://github.com/BicameralAI/bicameral-bot/issues/290))
- Verify `PROTOCOL_VERSION` capability handshake against tagged daemon — gate Fri Jun 12 ([bicameral-mcp#584](https://github.com/BicameralAI/bicameral-mcp/issues/584))
- Stress test Devin agent workflow through MCP — before Sun Jun 14 ([bicameral-mcp#585](https://github.com/BicameralAI/bicameral-mcp/issues/585))
- Dogfood gate participant — Fri Jun 12 ([bicameral-bot#293](https://github.com/BicameralAI/bicameral-bot/issues/293))

## Kevin (@Knapp-Kevin) — Docs / Substrate & Integrations QA

- Promote `manual_setup.md` to `QUICKSTART.md` on main — due Thu Jun 11 ([bicameral-bot#291](https://github.com/BicameralAI/bicameral-bot/issues/291))
- Stress test Drive folder substrate — before Sun Jun 14 ([bicameral-bot#294](https://github.com/BicameralAI/bicameral-bot/issues/294))
- Stress test Linear integration, or document gap and cut from pilot scope — before Sun Jun 14 ([bicameral-integrations#93](https://github.com/BicameralAI/bicameral-integrations/issues/93))
- Dogfood gate participant — Fri Jun 12 ([bicameral-bot#293](https://github.com/BicameralAI/bicameral-bot/issues/293))

## Lovis (@lovisaxon-ops) — Pilot Pipeline

- Beta kit: pain-point one-pager (context fragmentation, not features) + download link + weekly-call calendar ask — due Sat Jun 13 ([bicameral#121](https://github.com/BicameralAI/bicameral/issues/121))

## Unassigned (post-alpha backlog)

- Token telemetry: implement spend events per spec — post-alpha priority #1, owner TBD ([bicameral-bot#295](https://github.com/BicameralAI/bicameral-bot/issues/295))

---

## Timeline

| Date | Milestone |
|---|---|
| Thu Jun 11 | Scope freeze; QUICKSTART on main; handshake verified |
| Fri Jun 12 | Dogfood gate — tarball-only, golden path ≤ 15 min, two-bucket triage |
| Sat Jun 13 | Ship-blockers fixed; tag `v0.1.0-alpha`; site link + beta kit ready |
| Sun Jun 14 | Clean-machine smoke test of full public path; link live |
