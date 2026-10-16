#!/usr/bin/env bash
# V1 True E2E Golden-Path Verification
#
# Runs the true V1 golden-path end-to-end test through the ToolRequest
# Dispatcher. This is the primary local acceptance command for issue #278.
#
# Usage:
#   ./scripts/v1-true-golden-path.sh
#
# Dependencies exercised:
#   - #258 ToolRequest Dispatcher with capability handshake
#   - #259 ingest.submit_local source evidence path
#   - #260 source evidence links in history.list and search.query
#
# The test exercises the full user-visible V1 golden path:
#   MCP/local surface captures source/code/command context
#   → bot-owned ToolRequest Dispatcher routes ingest.submit_local
#   → bot materializes SourceSnapshot + EvidenceReference
#   → candidate reviewed/promoted via review.accept_candidate
#   → signoff lifecycle via review.approve_signoff
#   → history.list exposes source/evidence links
#   → search.query exposes source/evidence links
#   → Dashboard/Ledger View renders reviewable context
#   → RuntimeTrace correlates MCP/local surface and bot work
#   → no compliance/enforcement/readiness-to-ship claims (ADR-0022)
#
# Exit code 0 = all verification steps passed.

set -euo pipefail

echo "══════════════════════════════════════════════════════════════"
echo " V1 True E2E Golden-Path Verification (issue #278)"
echo "══════════════════════════════════════════════════════════════"
echo ""

cargo test \
  --test v1_true_golden_path_e2e \
  --package bicameral-gateway \
  -- --nocapture 2>&1

EXIT_CODE=$?

echo ""
if [ $EXIT_CODE -eq 0 ]; then
  echo "══════════════════════════════════════════════════════════════"
  echo " V1 True E2E Golden Path — ALL CHECKS PASSED"
  echo "══════════════════════════════════════════════════════════════"
else
  echo "══════════════════════════════════════════════════════════════"
  echo " V1 True E2E Golden Path — FAILED (exit code $EXIT_CODE)"
  echo "══════════════════════════════════════════════════════════════"
fi

exit $EXIT_CODE
