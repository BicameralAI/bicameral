#!/usr/bin/env bash
# Live SPA for local smoke — auto-refresh on source edits.
#
# Two modes:
#   vite (default)  — Vite HMR on :5173 from THIS bot checkout.
#                     Fastest feedback for Products/Settings UI work.
#                     Pair with gateway CORS: BICAMERAL_DEV_TRUST_VITE=1
#                     and/or BICAMERAL_HOSTED_BRIDGE_EXTRA_ORIGINS.
#
#   cloud           — `vite build --watch` into Cloud's vendored SPA dir so
#                     http://127.0.0.1:8000/app picks up rebuilds. Injects a
#                     tiny poll of /app/version.json so the browser reloads
#                     when the bundle fingerprint changes.
#
# Usage:
#   scripts/smoke-spa-live.sh              # vite HMR on :5173
#   scripts/smoke-spa-live.sh vite
#   scripts/smoke-spa-live.sh cloud
#   SMOKE_SPA_PORT=5174 scripts/smoke-spa-live.sh vite
#
# Env:
#   SMOKE_SPA_MODE=vite|cloud
#   SMOKE_SPA_PORT=5173
#   SMOKE_CLOUD_ROOT=~/github/bicameral-cloud
#   SMOKE_SPA_HOST=127.0.0.1

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DASHBOARD="$ROOT/dashboard"
MODE="${1:-${SMOKE_SPA_MODE:-vite}}"
HOST="${SMOKE_SPA_HOST:-127.0.0.1}"
PORT="${SMOKE_SPA_PORT:-5173}"
CLOUD_ROOT="${SMOKE_CLOUD_ROOT:-$(cd "$ROOT/../bicameral-cloud" 2>/dev/null && pwd || true)}"
CLOUD_SPA="${CLOUD_ROOT:+$CLOUD_ROOT/src/bicameral_cloud/static/dashboard}"
LOG_DIR="${SMOKE_SPA_LOG_DIR:-$HOME/tmp}"
mkdir -p "$LOG_DIR"

if [[ ! -d "$DASHBOARD" ]]; then
  echo "error: dashboard not found at $DASHBOARD" >&2
  exit 1
fi

cd "$DASHBOARD"
if [[ ! -d node_modules ]]; then
  echo "==> npm ci (dashboard)"
  npm ci --silent
fi

kill_port() {
  local p="$1"
  local pids
  pids="$(lsof -nP -iTCP:"$p" -sTCP:LISTEN -t 2>/dev/null || true)"
  if [[ -n "$pids" ]]; then
    echo "==> freeing :$p ($pids)"
    # shellcheck disable=SC2086
    kill $pids 2>/dev/null || true
    sleep 0.4
  fi
}

case "$MODE" in
  vite)
    kill_port "$PORT"
    echo "==> Vite HMR from workspace"
    echo "    root: $DASHBOARD"
    echo "    url:  http://${HOST}:${PORT}/"
    echo "    edits under dashboard/src auto-refresh (HMR)."
    echo "    tip: for Cloud same-origin smoke use mode 'cloud' instead."
    exec npm run dev -- --host "$HOST" --port "$PORT"
    ;;

  cloud)
    if [[ -z "${CLOUD_ROOT:-}" || ! -d "$CLOUD_ROOT" ]]; then
      echo "error: bicameral-cloud not found (set SMOKE_CLOUD_ROOT)" >&2
      exit 1
    fi
    if [[ ! -d "$CLOUD_SPA" ]]; then
      mkdir -p "$CLOUD_SPA"
    fi
    echo "==> Watch-build SPA → Cloud static (for :8000/app)"
    echo "    source: $DASHBOARD"
    echo "    dest:   $CLOUD_SPA"
    echo "    open:   http://127.0.0.1:8000/app  (Cloud must already be up)"
    echo "    reload: browser polls /app/version.json after each build"

    # Inject a lightweight reload poller into built index.html after each build.
    # Vite writes hashed assets; version.json fingerprint is rewritten after copy.
    watch_loop() {
      local partial="${CLOUD_SPA}.partial"
      local stamp
      rm -rf "$partial"
      echo "==> building → $partial"
      npm run build -- --outDir "$partial" --emptyOutDir true \
        >>"$LOG_DIR/smoke-spa-cloud-build.log" 2>&1

      stamp="$(date +%s)-$(find "$partial" -type f -print0 2>/dev/null | sort -z | xargs -0 shasum 2>/dev/null | shasum | awk '{print $1}')"

      if [[ -f "$partial/index.html" ]]; then
        python3 - "$partial/index.html" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
html = path.read_text(encoding="utf-8")
snippet = """
<script>
(function () {
  if (window.__bicSmokeReload) return;
  window.__bicSmokeReload = true;
  var last = null;
  async function tick() {
    try {
      var r = await fetch("/app/version.json?t=" + Date.now(), { cache: "no-store" });
      if (!r.ok) return;
      var j = await r.json();
      var key = j.bundle_sha256 || j.built_at || JSON.stringify(j);
      if (last && key && last !== key) location.reload();
      if (key) last = key;
    } catch (_) {}
  }
  setInterval(tick, 1500);
  tick();
})();
</script>
"""
if "__bicSmokeReload" not in html:
    if "</body>" in html:
        html = html.replace("</body>", snippet + "</body>", 1)
    else:
        html += snippet
    path.write_text(html, encoding="utf-8")
PY
      fi

      python3 - "$partial" "$stamp" <<'PY'
import json, hashlib, datetime, sys
from pathlib import Path
dest = Path(sys.argv[1])
stamp = sys.argv[2]
h = hashlib.sha256()
for p in sorted(dest.rglob("*")):
    if p.is_file() and p.name != "version.json":
        h.update(p.relative_to(dest).as_posix().encode())
        h.update(b"\0")
        h.update(p.read_bytes())
(dest / "version.json").write_text(
    json.dumps({
        "bot_commit": "local-smoke",
        "bundle_sha256": h.hexdigest(),
        "built_at": datetime.datetime.now(datetime.UTC)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
        "smoke_stamp": stamp,
    }, indent=2, sort_keys=True)
    + "\n",
    encoding="utf-8",
)
PY

      rm -rf "${CLOUD_SPA}.prev"
      if [[ -d "$CLOUD_SPA" ]]; then
        mv "$CLOUD_SPA" "${CLOUD_SPA}.prev"
      fi
      mv "$partial" "$CLOUD_SPA"
      rm -rf "${CLOUD_SPA}.prev"
      echo "==> vendored smoke SPA @ $(date -u +%H:%M:%S) stamp=${stamp:0:12}…"
    }

    # First build immediately
    watch_loop

    # Poll source tree (no fswatch dependency)
    echo "==> watching $DASHBOARD/src (rebuild on change)"
    last_sig=""
    while true; do
      sig="$(find "$DASHBOARD/src" "$DASHBOARD/index.html" "$DASHBOARD/vite.config.ts" \
        -type f \( -name '*.svelte' -o -name '*.ts' -o -name '*.css' -o -name '*.html' -o -name '*.js' \) \
        -print0 2>/dev/null | xargs -0 shasum 2>/dev/null | shasum | awk '{print $1}')"
      if [[ -n "$sig" && "$sig" != "$last_sig" ]]; then
        if [[ -n "$last_sig" ]]; then
          echo "==> change detected → rebuild"
          watch_loop || echo "warn: rebuild failed (will retry on next change)" >&2
        fi
        last_sig="$sig"
      fi
      sleep 1.5
    done
    ;;

  *)
    echo "usage: $0 [vite|cloud]" >&2
    exit 2
    ;;
esac
