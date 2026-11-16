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
2. Drive access to a Product ledger is granted at the ledger folder. On an explicit owner action (setting up the Product's storage), the daemon may create the Shared Drive and ledger folder that hold the Product and share that ledger folder with the owner's Google Workspace domain, so current and future members of that domain can reach it without a per-person grant. That is the only permission change the system makes: it never adds individual accounts, never shares outside the owner's domain or by public link, never changes sharing on anything other than the Product's ledger folder, and never infers access from an untrusted browser assertion. Drive access to the folder is not Product membership; the signed Join ticket remains the Join authority (Decision 3). The owner can still manage Shared Drive membership by hand in Google Drive.
3. Product discovery and Join bind the ledger locator to the verified Product identity, signed creation proof and Join ticket. The raw locator is stored server-side, never returned in a public Cloud directory or preview, and must not be substituted by browser input. A locator is routing data, never a grant: signature, nonce, expiry, audience, and account/team binding remain the Join authority. The signed Join ticket must bind the creation event ID and digest as well as the Product identity. Before persisting a local route or replaying the ledger as that Product, the joining daemon reads the ledger's `CreateProduct` event and verifies its Product identity, event ID, and digest against the ticket. A missing, unreadable, or mismatched event fails closed; even a readable but wrong ledger is never treated as the requested Product.
4. Failed access, revocation, missing ledger files or ambiguous listings fail closed. HTTP 200 with an empty list does not establish an empty ledger. No new local Product or empty ledger is created as a fallback for Join. The daemon probes the ledger folder before every write and fails closed if the probe fails, so an operator who can create files in a folder it cannot read never appends events that no other operator can replay.
5. Before the wider scope is active, joining another operator's Drive Product returns a typed rejection explaining the unavailable capability, not a misleading daemon-unreachable error.
6. The decided Alpha rollout may use an unverified OAuth consent screen subject to Google's actual applicable authorized-user limit and policies. The owner must confirm Production publishing state, user counts, warnings, reconnect and refresh-token persistence, Limited Use privacy disclosures and any user-facing restrictions. This is not an assertion of general production approval.
7. Restricted-scope verification and the applicable recurring security assessment (including CASA if required by Google) are owner-held release obligations, with evidence and renewal recorded in #50. Neither passing local tests nor accepting this decision implies Google approval.
8. The trusted locator and cross-operator Join implementation in #51 may only activate after this record is accepted and the scope transition is qualified. Record independent two-account checks for member, nonmember, revoked access, wrong-ticket substitution (including a readable but wrong ledger), creation-event identity/digest verification, ledger read and write by a same-domain account that is not a Shared Drive member, and restart persistence.
9. For an existing Product created before trusted locator publication, its owner may republish the locator idempotently under the same verified Product identity and signed creation proof. Republish must not create a second Product, creation event, ledger, or membership and repeated attempts must preserve the same binding. A Product lacking a qualified locator remains unavailable for cross-operator Join rather than falling back to a new ledger.

## Consequences

- Cross-operator Shared Drive replay becomes architecturally supported, but its runtime capability remains blocked until acceptance, OAuth qualification and implementation evidence are complete.
- The broader Google permission expands the potential data-access surface, requiring least-use enforcement, transparent consent, policy compliance and security assessment.
- The daemon makes a Drive permission call, so a defect there can expose ledger data rather than only misread it. The call is owner-triggered, targets only the Product's ledger folder and grants only the owner's own domain; consent text and the Google scope justification must describe this use.
- Every account in the owner's domain can read and write the ledger folder once it holds the folder ID, so Drive access no longer separates Product members from other employees. Product membership is enforced by the signed Join ticket alone. People outside the owner's domain, and owners on accounts with no Workspace domain, have no automated sharing path.
- Narrower `drive.file` credentials remain incapable of silently upgrading; failures remain typed and non-destructive.
- Loss of Drive access (for example, removal from the Shared Drive) stops provider operations with a typed storage-access state, not a generic daemon-unreachable response where the provider can distinguish authorization failure. The event-store adapter must preserve that distinction; reporting only an `Unreachable` error internally while relying on the gateway Connection display is an implementation gap to qualify before activating Join. Loss of Drive access does not revoke canonical Product membership, which stays a Product-level fact.
- Issue #50 owns OAuth implementation and external verification. Issue #51 owns Join/locator behavior and conformance evidence, including the forward requirements for pre-write probing, ticket-to-ledger event matching, idempotent legacy locator publication, and typed provider-access errors. These are required acceptance evidence, not claims that the runtime implements them today. This proposed record has no authority before its separate decision-owner approval.
