# Publication packet

The decision record for publishing this repository's source (#22). It holds:
- the content decisions still owed;
- the rights approvals still owed;
- the GitHub settings the public repository needs, with the tests that prove them;
- the publication procedure.

Nothing here is an approval. An item is approved only when the named person records it on #22, with a link to the evidence.

## 1. Destination and candidate

| Item | Proposal |
| --- | --- |
| Organization | `BicameralAI` |
| Name | `bicameral`. First rename this private repository (for example to `bicameral-provenance`), keeping it private as the provenance record. GitHub redirects a renamed repository's old name only until a new repository takes that name. |
| Default branch | `main` |
| History | One parentless commit of the approved tree, with no other refs or tags ([Publication history](README.md#publication-history)) |
| Source | The `main` commit approved on #22. Recompute the tree at publication time; do not reuse a rehearsal commit. |

Rehearsal at `a740271`:
- **Tree:** `4e199cbaf0841beaf3f071e47e655c85adfbfc96`. Its candidate commit `4a356c5` had a placeholder identity.
- **Checks:** `check.py` and `check_publication.py --history` pass.
- **Isolation:** a fresh bare repository holding only that commit has one ref, one commit and 2,802 objects. The original Factory attestation and the three documents removed in #24 are absent.
- **gitleaks 8.21.2,** directory and commit modes: 17 files, all synthetic test fixtures (fake AWS and GitHub token patterns, a truncated PEM, test JWT secrets, a public-docs Anthropic key ID, a base64 `test-secret` webhook secret).

## 2. Content decision register

Each row is either fixed here or waits on the named decision-maker.

| Material | Files | Classification | Disposition |
| --- | --- | --- | --- |
| Design-partner first name | `apps/bot/DESIGN_PARTNER_ALIGNMENT_DELIVERY.md`, `packages/mcp/DESIGN_PARTNER_ALIGNMENT_DELIVERY.md`, `apps/bot/docs/specs/librarian-recall-readme.md`, `factory-alpha-librarian-lifecycle-delta.md`, `crates/bicameral-api/src/grounding.rs` (comment), and three Integrations docs | A customer's first name; the documents call the work "customer-specific" and "customer monitoring" | **Fixed:** replaced with "design partner"; the two `ALEX_ALIGNMENT_DELIVERY.md` files were renamed. **Kept:** one entry in `packages/integrations/docs/META_LEDGER.md` (line 7181), because that ledger is hash-chained and editing it breaks the Integrations governance gate. **Decision (Jin):** accept that entry, or re-seal the ledger. |
| MythologIQ asset adaptation | `apps/bot/docs/specs/factory-alpha-librarian-lifecycle-delta.md` (section "MythologIQ Asset Adaptation Boundaries", lines 210–580), `codegenome-uor-recall-evaluation-526.md:19,84`, `graph-provider-abstraction.md:46` | Describes the shape of CodeGenome, FailSafe and related MythologIQ Labs LLC assets, and what Bicameral may borrow from them, at concept level. No MythologIQ code is included. | **Decision (Kevin for MythologIQ Labs LLC, Jin for Bicameral):** keep, if the referenced MythologIQ repositories (`MythologIQ-Labs-LLC/CodeGenome`, `qor-logic`) are public or MythologIQ consents; otherwise remove the section and the two links. Recommended: keep the Bicameral-side constraints ("Forbidden: …") and drop the MythologIQ asset descriptions. |
| MythologIQ mentions elsewhere | `packages/mcp/CHANGELOG.md` ("built via QorLogic SDLC" links), `*/docs/governance/SIBLINGS.md` (FailSafe "leak-prevention only"), PR template exclusion list, one test fixture path | Attribution and boundary statements | Keep. |
| Internal pilot staffing | `apps/bot/docs/releases/alpha/pilot-release-assignments.md` and `.pdf`, `ship-plan-2026-06-14.md` (linked from `state-refinement-backlog.md`) | 2026-06 alpha release assignments naming team members by GitHub handle, plus one person not on the repository (`lovisaxon-ops`). The PDF duplicates the Markdown. | **Decision (Jin):** recommended to remove the PDF, and to keep the Markdown only if `lovisaxon-ops` consents (otherwise remove that section). |
| Internal Slack channel | `services/cloud/scripts/google_provider_auth_evidence.py:597` (validator pattern), `services/cloud/tests/test_google_provider_auth_evidence.py:312`, `apps/bot/scripts/tests/test_finalize_operator_service_topology.py:236-239` | A workspace name (`bicameralgroup`) and a channel ID, which the evidence validator requires as the delivery target. An identifier, not a credential: it grants nothing without workspace membership. | **Decision (Jin):** keep (recommended; changing it changes validator behavior), or move the channel into configuration in a separate change. |
| GCP identifiers | `services/cloud/.github/workflows/deploy-cloud-run.yml` and three `atlas-release-assignment-status.yml` files (inert component copies; GitHub runs only root workflows), `services/cloud/docs/adr/0008-environment-separation-alpha-exception.md:11`, `services/cloud/docs/google-desktop-credential-secret-boundary.md` | Project ID and number, Workload Identity pool and provider, service-account addresses. Identifiers, not credentials. | **Decision (Jin, with security):** keep. Before publication, security must confirm that each Workload Identity provider's attribute condition pins the exact deploying repository and ref, so that no workflow in a public `BicameralAI` repository can obtain those identities. |
| Private-repository links | About 200 links to the source repositories and Factory issues | Provenance; they return 404 for the public | Keep (recorded in #22). |

## 3. Rights approval matrix

Source: `git shortlog` of each source repository at its pinned SHA, and of `bicameral-factory` for the 22 allowlisted files. "On record" means a written statement already linked from #22 or Factory #969. **No row below is secured.** The target license is Apache-2.0 (J-03).

| Rights holder | Material | Historical license | On record | Missing | Decides |
| --- | --- | --- | --- | --- | --- |
| BicameralAI (company) | All components | Bot MIT OR Apache-2.0; MCP and Integrations BSL 1.1 (Licensor BicameralAI); Cloud, Installer and Factory unlicensed | J-03 (target license) | A signed statement that BicameralAI owns, or is licensed for, the work listed below and authorizes Apache-2.0 publication of each component | Jin (authorized officer) |
| Jin Hong Kuan (personal and company addresses) | Bot, MCP, Integrations, Cloud; 22 Factory files (sole or main author) | As above | J-03, as owner | A written assignment or license to BicameralAI covering personal-address commits | Jin |
| Kevin Knapp (personal address; aliases WulfForge, Knapp-Kevin, MythologIQ) | Bot, MCP, Integrations (main author), Cloud, Installer (sole author) | As above | None | A written assignment or license to BicameralAI for personal-address commits; Installer publication authorization | Kevin; accepted by Jin |
| MythologIQ Labs LLC (Kevin's company) | `apps/bot/.github/workflows/{sbom,scorecard,dependency-review}.yml` (`791dbec`), `packages/mcp/.github/workflows/sbom.yml` (`7ba8db9`) | Bot MIT OR Apache-2.0; MCP BSL 1.1 | None | Grant from MythologIQ Labs LLC's authorized signatory, or removal of the four files (inert component copies) | Kevin (for the LLC); Jin |
| Silong Tan (personal address) | MCP (274 commits), Bot (192), one Factory file | As above | None | Written assignment or license (employment or contractor terms) | Silong; Jin |
| `ljschwarzer` (bicameral-ai.com) | MCP (1 commit) | BSL 1.1 | None | Confirmation that employment terms cover the work | Jin |
| Aritro (WhatBytes) | Bot (8 commits, including `a83f833`, `63e71bc`) | MIT OR Apache-2.0 | None | WhatBytes contractor agreement with IP assignment, or a contributor grant | Jin, with WhatBytes |
| Mayur Sonar (WhatBytes) | Bot (2 commits, including `1641878`) | MIT OR Apache-2.0 | None | Same as Aritro | Jin, with WhatBytes |
| `rev-9w` | Bot (4 commits, specification documents) | MIT OR Apache-2.0 | None | Contributor grant, or confirmation that the documents are covered | Jin |
| Agent and bot accounts (Devin, Claude, dependabot, github-actions) | All components | — | — | Not rights holders. Their output belongs to whoever operated them, so it is covered by that operator's row | — |
| ZeroClaw Labs | Code adapted into six Bot crates | MIT OR Apache-2.0 | `NOTICE` preserved | None: Apache-2.0 is one of the offered licenses and the notice is kept | — |
| svelte, esm-env, Vite contributors | Vendored dashboard bundle | MIT | `services/cloud/src/bicameral_cloud/static/DASHBOARD_THIRD_PARTY_NOTICES.md` | None | — |

**Open decision: `packages/mcp/LICENSE` (Jin, with counsel).** The file already holds the Apache-2.0 text. The #969 correction says license files are not flipped before the copyright holders sign. The options are:
- (a) restore BSL 1.1 until the MCP rows above are signed, then switch at publication;
- (b) record that keeping Apache-2.0 in a private repository grants nothing to anyone until publication, and accept it.

Either is consistent if publication waits for the signatures.

## 4. GitHub settings for the public repository

None of this can be enforced from documentation, and none of it has been applied yet. #20 merged with no approving review, so the current private repository does not enforce review. On GitHub Free, rulesets and branch protection are not enforced on private repositories; on a public repository they are.

### Ruleset for `main`

`gh api -X POST repos/BicameralAI/<repo>/rulesets --input main.json` with:

```json
{
  "name": "main",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "bypass_actors": [],
  "rules": [
    {"type": "deletion"},
    {"type": "non_fast_forward"},
    {"type": "pull_request", "parameters": {
      "required_approving_review_count": 1,
      "require_code_owner_review": true,
      "require_last_push_approval": true,
      "dismiss_stale_reviews_on_push": true,
      "required_review_thread_resolution": true,
      "allowed_merge_methods": ["squash"]}},
    {"type": "required_status_checks", "parameters": {
      "strict_required_status_checks_policy": true,
      "do_not_enforce_on_create": false,
      "required_status_checks": [{"context": "governance", "integration_id": 15368}]}}
  ]
}
```

`integration_id` 15368 is the GitHub Actions app. Pinning it means a commit status named `governance` that is posted by any other source cannot satisfy the check. A second ruleset with `target: tag` and rules `creation`, `update` and `deletion`, with no bypass, keeps tags (and so releases) closed until releases are authorized.

### Repository settings

```bash
R=repos/BicameralAI/<repo>
gh api -X PUT $R/actions/permissions/workflow -f default_workflow_permissions=read -F can_approve_pull_request_reviews=false
gh api -X PUT $R/actions/permissions/fork-pr-contributor-approval -f approval_policy=all_external_contributors
gh api -X PUT $R/private-vulnerability-reporting
gh api -X PUT $R/vulnerability-alerts
gh api -X PATCH $R -F 'security_and_analysis[secret_scanning][status]=enabled' \
  -F 'security_and_analysis[secret_scanning_push_protection][status]=enabled'
```

Also needed:
- Confirm that no self-hosted runner group is available to the repository.
- Decide whether the Claude review app may run on fork pull requests, since it spends organization credit.
- After private vulnerability reporting is enabled, name it in `SECURITY.md` as the reporting channel.

### Negative tests (run before visibility changes; record each result on #22)

Use the new repository while it is still private, plus one account that is not a code owner. Each test must fail as described:

1. A push directly to `main` is rejected (`GH013`).
2. A force push to `main` is rejected.
3. A pull request with no approval cannot merge, and the author cannot approve their own pull request.
4. Approve, then push a new commit: the approval is dismissed and the merge is blocked.
5. A fork pull request that adds `${{ secrets.X }}` or `pull_request_target` to a root workflow fails `governance` (`check_publication.py`), and its workflows wait for approval.
6. A commit status named `governance`, posted through the statuses API, does not satisfy the required check.
7. A test secret (a revoked token) pushed on a branch is blocked by push protection.

## 5. Publication procedure

1. **Approvals.** The rights rows in section 3 and the decisions in section 2 are recorded on #22.
2. **Create the destination.** Rename this repository. Create the empty private `BicameralAI/bicameral` with no template, no README and no license file.
3. **Build and push the commit.**

   ```bash
   SOURCE=<approved main commit>
   git checkout "$SOURCE" && python3 tools/governance/check.py
   COMMIT=$(GIT_AUTHOR_NAME=BicameralAI GIT_AUTHOR_EMAIL=<org address> \
     GIT_COMMITTER_NAME=BicameralAI GIT_COMMITTER_EMAIL=<org address> \
     git commit-tree "$SOURCE^{tree}" -m "Bicameral source, published from private commit $SOURCE")
   python3 tools/governance/check_publication.py --history "$COMMIT"
   git push https://github.com/BicameralAI/bicameral.git "$COMMIT:refs/heads/main"
   ```

4. **Verify what is reachable.**

   ```bash
   git clone --mirror https://github.com/BicameralAI/bicameral.git pub.git
   git -C pub.git for-each-ref          # exactly refs/heads/main
   git -C pub.git rev-list --all --count  # 1
   test "$(git -C pub.git rev-parse main^{tree})" = "$(git rev-parse "$SOURCE^{tree}")"
   gitleaks git --redact pub.git        # synthetic fixtures only (section 1)
   ```

   Do not open pull requests, issues or releases on the new repository before step 7; each pull request creates a permanent `refs/pull/*` ref.
5. **Settings and tests.** Apply section 4 and run its negative tests.
6. **Independent review.** Source, license and security reviewers record their sign-off on #22 against the exact commit.
7. **Owner go/no-go, then publish.** Jin records go on #22, then changes visibility to public. From a signed-out browser, confirm:
   - one commit;
   - `LICENSE`, `NOTICE` and the dashboard notices are visible;
   - the security policy and private vulnerability reporting are visible;
   - Actions show no secrets.
