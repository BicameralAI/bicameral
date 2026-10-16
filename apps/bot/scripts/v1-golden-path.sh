#!/usr/bin/env bash
# V1 Local Golden-Path Verification
#
# Runs the full V1 golden-path end-to-end test against a fresh in-memory
# gateway. This is the primary local acceptance command for issue #236.
#
# Usage:
#   ./scripts/v1-golden-path.sh
#
# Dependencies exercised:
#   - #232 synthetic code-grounding fixture
#   - #233 V1 reviewable-context projection read model
#   - #234 Ledger View projection-backed reads
#
# The test exercises:
#   ingest source evidence
#   → accept candidate → Decision(proposed)
#   → approve signoff → Decision(approved)
#   → attach verified code evidence from fixture workspace
#   → rebuild reviewable-context projection from event-store replay
#   → read Ledger View from projection
#   → dashboard Ledger View reads from projection-backed path
#   → confirm no compliance/enforcement/readiness-to-ship claims
#
# Exit code 0 = all verification steps passed.

set -euo pipefail

echo "══════════════════════════════════════════════════════════════"
echo " V1 Local Golden-Path Verification (issue #236)"
echo "══════════════════════════════════════════════════════════════"
echo ""

cargo test \
  --test v1_golden_path_e2e \
  --package bicameral-gateway \
  -- --nocapture 2>&1

EXIT_CODE=$?

echo ""
if [ $EXIT_CODE -eq 0 ]; then
  echo "══════════════════════════════════════════════════════════════"
  echo " V1 Golden Path — ALL CHECKS PASSED"
  echo "══════════════════════════════════════════════════════════════"
else
  echo "══════════════════════════════════════════════════════════════"
  echo " V1 Golden Path — FAILED (exit code $EXIT_CODE)"
  echo "══════════════════════════════════════════════════════════════"
fi

exit $EXIT_CODE
