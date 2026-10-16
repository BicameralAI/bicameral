#!/usr/bin/env bash
# Build the Bicameral dashboard SPA (bicameral-bot/dashboard) and vendor the
# production bundle into this repo so the cloud image serves it via StaticFiles.
#
# "Cloud serves UI" (ADR-0005 follow-on): the Docker build context is this repo
# only, so the built SPA must live under src/ to ship in the image's `COPY src`.
#
# Usage:
#   scripts/vendor_dashboard.sh [path-to-dashboard-dir]
# Defaults to ../bicameral-bot/dashboard relative to this repo.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DASHBOARD_DIR="${1:-$REPO_ROOT/../bicameral-bot/dashboard}"
DEST="$REPO_ROOT/src/bicameral_cloud/static/dashboard"
BOT_REPO_DIR="$(cd "$DASHBOARD_DIR/.." && pwd)"
BOT_SHA="${DASHBOARD_BOT_SHA:-$(git -C "$BOT_REPO_DIR" rev-parse HEAD 2>/dev/null || true)}"
SOURCE_REPOSITORY="${DASHBOARD_SOURCE_REPOSITORY:-BicameralAI/bicameral-bot}"
SOURCE_RUN_URL="${DASHBOARD_SOURCE_RUN_URL:-}"
EXPECTED_CONTRACT_SHA256="${HOSTED_BRIDGE_CONTRACT_SHA256:-}"
DASHBOARD_ARTIFACT_SHA256="${DASHBOARD_ARTIFACT_SHA256:-}"
EXPECTED_BOT_DESCRIPTOR_DIGEST="${BOT_DESCRIPTOR_DIGEST:-}"
CONTRACT_SOURCE_DIR="$BOT_REPO_DIR/protocol/hosted-bridge"
CONTRACT_DEST_DIR="$REPO_ROOT/src/bicameral_cloud/contracts"
BOT_DESCRIPTOR_SOURCE="$BOT_REPO_DIR/release-dashboard/bot-release-descriptor.json"
BOT_DESCRIPTOR_DEST="$REPO_ROOT/release-inputs/bot-release-descriptor.json"

if [ ! -d "$DASHBOARD_DIR" ]; then
  echo "error: dashboard dir not found: $DASHBOARD_DIR" >&2
  echo "pass the path explicitly: scripts/vendor_dashboard.sh /path/to/dashboard" >&2
  exit 1
fi

if [ ! -f "$CONTRACT_SOURCE_DIR/contract.json" ] || [ ! -f "$CONTRACT_SOURCE_DIR/contract.sha256" ]; then
  echo "error: Bot checkout is missing the hosted-bridge contract manifest" >&2
  exit 1
fi

echo "==> Pinning Bot-owned hosted-bridge contract"
mkdir -p "$CONTRACT_DEST_DIR"
cp "$CONTRACT_SOURCE_DIR/contract.json" "$CONTRACT_DEST_DIR/hosted_bridge_contract.json"
awk '{print $1 "  hosted_bridge_contract.json"}' \
  "$CONTRACT_SOURCE_DIR/contract.sha256" > "$CONTRACT_DEST_DIR/hosted_bridge_contract.sha256"

CONTRACT_SHA256="$(python3 - "$CONTRACT_DEST_DIR" "$EXPECTED_CONTRACT_SHA256" <<'PY'
import hashlib
import sys
from pathlib import Path

directory = Path(sys.argv[1])
expected_from_dispatch = sys.argv[2]
content = (directory / "hosted_bridge_contract.json").read_bytes()
actual = hashlib.sha256(content).hexdigest()
recorded = (directory / "hosted_bridge_contract.sha256").read_text().split()[0]
if actual != recorded:
    raise SystemExit(f"contract fingerprint mismatch: recorded {recorded}, got {actual}")
if expected_from_dispatch and actual != expected_from_dispatch:
    raise SystemExit(
        f"dispatch contract fingerprint mismatch: expected {expected_from_dispatch}, got {actual}"
    )
print(actual)
PY
)"

echo "==> Pinning Bot release descriptor"
python3 - "$BOT_DESCRIPTOR_SOURCE" "$BOT_DESCRIPTOR_DEST" "$BOT_SHA" "$EXPECTED_BOT_DESCRIPTOR_DIGEST" <<'PY'
from __future__ import annotations

import hashlib
import json
import re
import shutil
import sys
from pathlib import Path

source = Path(sys.argv[1])
destination = Path(sys.argv[2])
expected_commit = sys.argv[3]
expected_digest = sys.argv[4]
if not source.is_file():
    raise SystemExit("immutable dashboard artifact omitted the Bot release descriptor")
if not re.fullmatch(r"sha256:[a-f0-9]{64}", expected_digest):
    raise SystemExit("dispatch omitted a valid Bot descriptor digest")
payload = json.loads(source.read_text(encoding="utf-8"))
if payload.get("schema_version") != 1 or payload.get("component") != "bot":
    raise SystemExit("Bot release descriptor schema/component mismatch")
if payload.get("commit") != expected_commit:
    raise SystemExit("Bot release descriptor commit does not match dispatch")
unsigned = {key: value for key, value in payload.items() if key != "descriptor_digest"}
actual = "sha256:" + hashlib.sha256(
    json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode()
).hexdigest()
if payload.get("descriptor_digest") != actual or actual != expected_digest:
    raise SystemExit("Bot release descriptor digest does not match immutable dispatch")
destination.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(source, destination)
PY

if [ "${DASHBOARD_ALREADY_BUILT:-0}" = "1" ]; then
  echo "==> Using verified prebuilt dashboard artifact in $DASHBOARD_DIR/dist"
  test -f "$DASHBOARD_DIR/dist/index.html"
else
  echo "==> Building dashboard in $DASHBOARD_DIR"
  ( cd "$DASHBOARD_DIR" && npm ci --silent && npm run build )
fi

echo "==> Vendoring dist -> $DEST"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -r "$DASHBOARD_DIR/dist/." "$DEST/"

# The release artifact is immutable evidence. Even whitespace normalization
# after verification creates a different SPA than the one Bot published.
if ! diff -qr "$DASHBOARD_DIR/dist" "$DEST" >/dev/null; then
  echo "error: vendored dashboard bytes differ from the verified source artifact" >&2
  exit 1
fi

echo "==> Writing dashboard version metadata"
python3 - "$DEST" "$BOT_SHA" "$SOURCE_REPOSITORY" "$SOURCE_RUN_URL" "$CONTRACT_SHA256" "$DASHBOARD_ARTIFACT_SHA256" "$EXPECTED_BOT_DESCRIPTOR_DIGEST" <<'PY'
from __future__ import annotations

import datetime
import hashlib
import json
import sys
from pathlib import Path

dest = Path(sys.argv[1])
bot_sha = sys.argv[2]
source_repository = sys.argv[3]
source_run_url = sys.argv[4]
contract_sha256 = sys.argv[5]
artifact_sha256 = sys.argv[6]
bot_descriptor_digest = sys.argv[7]

hasher = hashlib.sha256()
for path in sorted(p for p in dest.rglob("*") if p.is_file() and p.name != "version.json"):
    hasher.update(path.relative_to(dest).as_posix().encode())
    hasher.update(b"\0")
    hasher.update(path.read_bytes())

version = {
    "source_repository": source_repository,
    "bot_commit": bot_sha,
    "bundle_sha256": hasher.hexdigest(),
    "artifact_sha256": artifact_sha256 or None,
    "bot_descriptor_digest": bot_descriptor_digest,
    "hosted_bridge_contract_sha256": contract_sha256,
    "source_run_url": source_run_url or None,
    "built_at": datetime.datetime.now(datetime.UTC)
    .replace(microsecond=0)
    .isoformat()
    .replace("+00:00", "Z"),
}
(dest / "version.json").write_text(json.dumps(version, indent=2, sort_keys=True) + "\n")
PY

echo "==> Vendored files:"
find "$DEST" -type f | sed "s|$REPO_ROOT/||"
echo "Done. Commit src/bicameral_cloud/static/dashboard to ship the SPA."
