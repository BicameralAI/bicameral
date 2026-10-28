# Publication packet

> **Status reconciliation, 27 October 2026.** This packet originally documented a proposed private-to-public transition and a clean-root-commit publication plan. The owner and technical maintainer have since **intentionally made `BicameralAI/bicameral` public** to enable OSS/security configuration, including private vulnerability reporting. Public visibility is the **actual approved operating state**, not an unintended exposure and not evidence that the repository met all previously proposed launch conditions.
>
> **Authoritative open follow-through:** [Bicameral #22](https://github.com/BicameralAI/bicameral/issues/22) now tracks *post-publication* risk, rights, history and contributor-security acceptance. The original private-repository rename/new-root procedure and the "not configured"/"private" status statements below are **historical design/rehearsal records**, not current instructions to republish or revert public visibility. No force-push, history rewrite, archive freeze, license change or repository settings change is authorized by this note.
>
> **Owner disposition, 24 October:** [Jin's signed GO](https://github.com/BicameralAI/bicameral/issues/22#issuecomment-6380066910) accepts the public repository and its existing history at `4e99bb4`, makes his recorded rights override final, and requires `jinhongkuan` to be the sole bypass actor on Main and Release tags. #45 records that bypass requirement below. Do not treat a clean-root rewrite or the previous admin-bypass state as current instructions. This owner disposition does not establish independent rights-holder grants, complete the contributor-security negative tests, or approve a product release. The stale-approval test #46 is explicitly outside the GO; [Factory #971](https://github.com/BicameralAI/bicameral-factory/issues/971) qualification remains separate.

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
| GCP identifiers | `services/cloud/docs/adr/0008-environment-separation-alpha-exception.md:11`, `services/cloud/docs/google-desktop-credential-secret-boundary.md`. The deploy and `atlas-release-assignment-status.yml` workflow copies were removed from this repository. | Project ID and number, Workload Identity pool and provider, service-account addresses. Identifiers, not credentials. The workflow copies are gone. The docs still name the identifiers. | **Decision:** remove the workflow copies so a pull request cannot promote them to the root. Before any public workflow can assume those identities, security must confirm that each Workload Identity provider's attribute condition pins the exact deploying repository and ref. |
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

**Status: proposed, not configured.** Nothing in this section has been applied or tested. A control counts as in place only after two things are recorded on #22: it is configured on the destination repository, and the negative tests below have been run against it. #20 merged here with no approving review, so the current private repository does not enforce review.

**Plan entitlement.** Before relying on any rule, confirm what `BicameralAI`'s GitHub plan enforces, using the plan documentation current at the time:
- Rulesets and branch protection on **private** repositories need GitHub Team or higher.
- Organization rulesets that require a workflow from another repository also depend on the plan.
- If the plan does not enforce rulesets on private repositories, the private staging step in section 5 cannot prove them. Run these tests in a disposable **public** repository that holds only a test file, never the source.

### What a passing `governance` check proves

`governance` runs from the pull request's own copy of `.github/workflows/governance.yml` and `tools/governance/`. A contributor can delete that workflow, or weaken the checker, and still receive a green result from GitHub Actions. Pinning the check to the Actions app (`integration_id` 15368) only stops statuses posted by other apps or through the statuses API. **The check is not a security boundary on its own.** Two controls make it one.

1. **Code-owner review (required).** `.github/CODEOWNERS` assigns every path, including `/.github/` and `/tools/governance/`, to `@jinhongkuan` and `@Knapp-Kevin`. With `require_code_owner_review` and `require_last_push_approval` set, no change to a workflow or to the checker can merge unless a code owner approves its final commit. Reviewers must treat any diff under `.github/workflows/` or `tools/governance/` as security-relevant.
2. **Trusted-base check (recommended where the plan allows).** Keep the publication checker in a separate repository that pull-request authors cannot change, for example a private `BicameralAI/bicameral-policy`. An organization ruleset's `workflows` rule then runs that repository's workflow, at a pinned commit, on every pull request to the public repository. The workflow checks out the base checker and reads the pull request's files only as data: it never executes the pull request's code and needs no secrets. Where the plan does not offer this, control 1 is the boundary, and the limitation is recorded on #22.

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

`integration_id` 15368 is the GitHub Actions app; see "What a passing `governance` check proves" for what that pin does and does not do. With the trusted-base check, add the organization ruleset's `workflows` rule, naming the policy repository, its workflow path and a pinned commit. A second ruleset with `target: tag` and rules `creation`, `update` and `deletion`, with no bypass, keeps tags (and so releases) closed until releases are authorized.

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

Run them where the rules are enforced (see "Plan entitlement"). Use an account that is not a code owner, and a fork for the pull-request tests. Each must fail as described:

1. A push directly to `main` is rejected (`GH013`).
2. A force push to `main`, or deleting `main`, is rejected.
3. A pull request with no approval cannot merge, and the author cannot approve their own pull request.
4. Approve, then push a new commit: the approval is dismissed and the merge is blocked.
5. **Workflow deletion:** a fork pull request that deletes `.github/workflows/governance.yml` cannot merge. The required check never reports, and code-owner approval is still required.
6. **Checker change:** a fork pull request cannot merge without code-owner approval when it combines two changes: `tools/governance/check_publication.py` edited to always pass, and a forbidden file added (for example `.env`). With the trusted-base check in place, that check also fails.
7. **Path-filter bypass:** a fork pull request that changes only a file outside every component path still runs `governance`, which has no path filter, and still needs code-owner approval.
8. **Forged status:** a `governance` commit status set to `success` through the statuses API, or by another app, does not satisfy the required check.
9. **Workflow secrets and triggers:** a fork pull request that adds `${{ secrets.X }}` or `pull_request_target` to a root workflow fails `governance`, and its workflows wait for approval before running.
10. **Push protection:** a test secret (a revoked token) pushed on a branch is blocked.

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
