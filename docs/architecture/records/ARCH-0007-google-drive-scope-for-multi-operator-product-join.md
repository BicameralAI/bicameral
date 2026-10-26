---
id: ARCH-0007
title: Google Drive scope for multi-operator Product Join
kind: decision
status: proposed
date: 2026-10-26
owners: [@jinhongkuan, @Knapp-Kevin]
governs: [google-drive-oauth-scope, shared-drive-product-join]
depends_on: [ARCH-0001]
supersedes: []
references: [https://github.com/BicameralAI/bicameral/issues/50, https://github.com/BicameralAI/bicameral/issues/51, https://github.com/BicameralAI/bicameral-bot/issues/1262]
---

# ARCH-0007: Google Drive scope for multi-operator Product Join

## Context

The two-account Shared Drive qualification in bicameral-bot#1262 demonstrated that the existing `drive.file` OAuth grant does not expose the same app-created ledger files to another member of the Shared Drive. Folder listings can return HTTP 200 with zero entries even when the user has access in Google's web UI. Treating that result as an empty authoritative ledger could create a split Product history. Google Picker selection did not resolve the per-file visibility limit.

Jin chose Option A on 2026-10-26: use Google's restricted `drive` scope to make Shared Drive membership effective for cross-operator Product Join. Issue #50 tracks the scope transition and Google verification; #51 owns implementation. This proposal records the governing contract and does not assert the deployment or verification has passed.

## Decision

1. The Drive connector will request the Google OAuth `drive` scope in place of `drive.file` for the Shared Drive Product path. Users must see accurate consent text and authorize the new grant. No silent privilege escalation or assumption that existing credentials already include the scope.
2. Existing manual owner-managed sharing and Shared Drive membership remain necessary. The system neither grants permissions automatically nor infers membership from an untrusted browser assertion.
3. Product discovery and Join bind the ledger locator to the verified Product identity, signed creation proof and Join ticket. The raw locator is stored server-side, never returned in a public Cloud directory or preview, and must not be substituted by browser input.
4. Failed access, revocation, missing ledger files or ambiguous listings fail closed. HTTP 200 with an empty list does not establish an empty ledger. No new local Product or empty ledger is created as a fallback for Join.
5. Before the wider scope is active, joining another operator's Drive Product returns a typed rejection explaining the unavailable capability, not a misleading daemon-unreachable error.
6. The decided Alpha rollout may use an unverified OAuth consent screen subject to Google's actual applicable authorized-user limit and policies. The owner must confirm Production publishing state, user counts, warnings, reconnect and refresh-token persistence, Limited Use privacy disclosures and any user-facing restrictions. This is not an assertion of general production approval.
7. Restricted-scope verification and the applicable recurring security assessment (including CASA if required by Google) are owner-held release obligations, with evidence and renewal recorded in #50. Neither passing local tests nor accepting this decision implies Google approval.
8. The trusted locator and cross-operator Join implementation in #51 may only activate after this record is accepted and the scope transition is qualified. Record independent two-account checks for member, nonmember, revoked access, wrong-ticket substitution and restart persistence.

## Consequences

- Cross-operator Shared Drive replay becomes architecturally supported, but its runtime capability remains blocked until acceptance, OAuth qualification and implementation evidence are complete.
- The broader Google permission expands the potential data-access surface, requiring least-use enforcement, transparent consent, policy compliance and security assessment.
- Narrower `drive.file` credentials remain incapable of silently upgrading; failures remain typed and non-destructive.
- Issue #50 owns OAuth implementation and external verification. Issue #51 owns Join/locator behavior and conformance evidence. This proposed record has no authority before its separate decision-owner approval.
