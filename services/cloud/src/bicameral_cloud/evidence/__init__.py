"""Self-service Cloud Run IAM and Supabase reconciliation tooling (cloud#141).

This package prepares every command, parser, comparison, and typed outcome needed
to turn the owner-only live evidence export required by the Cloud #106 evidence gate
into a *deterministic* result. Nothing here needs live GCP or Supabase credentials to
run or parse:

* Collectors describe the **exact command** an authorized owner runs and parse its
  JSON output into a redaction-safe ``EvidenceItem``.
* When a CLI tool, credential, or piece of infrastructure is absent, collection
  fails **typed** as ``CollectionStatus.ENVIRONMENT_INCONCLUSIVE``
  rather than producing a false pass.
* The comparator maps a produced ``EvidenceBundle``
  against the repository's tracked expectations and reports agreement, live-only drift,
  tracked-only configuration, and unknown/inconclusive fields.

The layer emits evidence, comparisons, and typed outcomes only. It never mutates live
IAM, ingress, schema, grants, or RLS, and it never embeds secrets, tokens, confidential
project identifiers, raw customer data, or private source bodies.
"""

from __future__ import annotations

from bicameral_cloud.evidence.bundle import (
    EvidenceBundle,
    EvidenceItem,
    build_bundle,
    validate_bundle,
)
from bicameral_cloud.evidence.comparator import (
    ComparisonFinding,
    ComparisonReport,
    Disposition,
    compare,
)
from bicameral_cloud.evidence.outcomes import (
    TOOL_VERSION,
    CollectionResult,
    CollectionStatus,
)
from bicameral_cloud.evidence.redaction import Redactor, sanitized_digest

__all__ = [
    "TOOL_VERSION",
    "CollectionResult",
    "CollectionStatus",
    "ComparisonFinding",
    "ComparisonReport",
    "Disposition",
    "EvidenceBundle",
    "EvidenceItem",
    "Redactor",
    "build_bundle",
    "compare",
    "sanitized_digest",
    "validate_bundle",
]
