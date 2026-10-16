#!/usr/bin/env bash
# Run the deterministic Hosted Project Creation browser UserJourney (issue #114).
#
# Starts all required local services in-process (a loopback fake Supabase
# design-partner allowlist + the live Cloud app with an isolated in-memory
# store) and drives a real headless browser through the create -> discovery
# refresh -> "needs binding" -> reload-persistence path against the real Cloud
# registration/discovery HTTP contracts. No production credentials are used.
#
# Usage:
#   scripts/run_hosted_project_creation_journey.sh
#
# First run only, install the journey harness + browser:
#   pip install -e '.[dev,journey]'
#   python -m playwright install chromium
set -euo pipefail

cd "$(dirname "$0")/.."

if ! python -c "import playwright" >/dev/null 2>&1; then
  echo "Playwright is not installed. Install the journey harness first:" >&2
  echo "  pip install -e '.[dev,journey]'" >&2
  echo "  python -m playwright install chromium" >&2
  exit 1
fi

exec pytest -q -m journey tests/journey "$@"
