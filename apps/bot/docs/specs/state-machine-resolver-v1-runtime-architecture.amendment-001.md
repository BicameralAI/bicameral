# State Machine Resolver v1 runtime architecture amendment 001

**Base contract:** [`state-machine-resolver-v1-runtime-architecture.md`](./state-machine-resolver-v1-runtime-architecture.md)  
**Machine amendment:** [`state-machine-resolver-v1-runtime-architecture.contract.amendment-001.json`](./state-machine-resolver-v1-runtime-architecture.contract.amendment-001.json)  
**Issue:** [#937](https://github.com/BicameralAI/bicameral-bot/issues/937)  
**Contract PR:** [#942](https://github.com/BicameralAI/bicameral-bot/pull/942)  
**Disposition:** **AMEND**  
**Normative status:** This amendment supersedes the conflicting confirmation-v2 details in base contract section 7 and the companion contract's confirmation schema filename.

## NOW / NEXT / BLOCKED

| State | Meaning |
| --- | --- |
| **NOW** | Confirmation protocol v2 is restored to a true strict superset of shipped v1. |
| **NEXT** | Implementations must satisfy the base contract plus this amendment. |
| **BLOCKED** | Implementation and journey authorization remain outside this contract lane. |

## A15. Retain `requested_outcome`

The shipped v1 `ChallengeBinding` binds `requested_outcome: PromotionOutcome` and returns `OutcomeMismatch` when the presented value differs. Confirmation protocol v2 must retain that field. Omitting it would allow the exact human-reviewed disposition to change while every currently documented v2 digest and identity still matched.

The following field is therefore required in `ConfirmationChallengeV2`:

```yaml
requested_outcome: >-
  new_constraint |
  supersedes_existing_decision |
  keeps_both_with_scope |
  needs_signoff_review |
  dismissed_as_noise
```

The enum is the existing `PromotionOutcome` wire vocabulary. It is not a new Product state or approval family.

Action pairing is closed:

| `action` | Allowed `requested_outcome` |
| --- | --- |
| `promote_smr_claim_set` | `new_constraint`, `supersedes_existing_decision`, `keeps_both_with_scope`, `needs_signoff_review` |
| `reject_smr_claim_set` | `dismissed_as_noise` only |

A missing, unknown, or action-incompatible value fails closed with no canonical append. A changed value fails as the existing typed `OutcomeMismatch` authorization result.

## Confirmation binding digest

`confirmation_binding_digest` uses digest-profile v1 with artifact name `confirmation-binding`:

```text
"sha256:" || lowerhex(
  SHA-256(
    UTF8("bicameral/confirmation-binding/v1") || 0x00 ||
    JCS(binding_without_display_summary_and_confirmation_binding_digest)
  )
)
```

`requested_outcome` is included in the normalized binding value. `display_summary` and `confirmation_binding_digest` itself are excluded. `display_summary_digest` remains included.

## Machine artifact correction

The effective confirmation schema is:

```text
protocol/smr-claim-acceptance/schemas/confirmation-binding.v2.schema.json
```

The base companion contract's value `confirmation-challenge-v2.schema.json` does not identify an existing file and is superseded by the machine-readable amendment.

## Authority boundary

This amendment settles one missing machine field, its action semantics, its digest participation, and the effective schema reference. It authorizes no implementation, deployment, release, Product acceptance, Journey completion, or CodeGraph carrier decision.
