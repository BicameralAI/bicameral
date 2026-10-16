#!/bin/sh
set -eu
ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT INT TERM
FAKEBIN="$ROOT/fakebin"; RELEASE="$ROOT/release"; HOME_DIR="$ROOT/home"; STATE="$ROOT/state"; LOG="$ROOT/calls"
mkdir -p "$FAKEBIN" "$RELEASE/vtest" "$HOME_DIR"
cat > "$FAKEBIN/curl" <<'SH'
#!/bin/sh
url=''; out=''; noproxy=0
while [ "$#" -gt 0 ]; do case "$1" in -o) out="$2"; shift 2;; --max-time) shift 2;; --noproxy) noproxy=1; shift 2;; -*) shift;; *) url="$1"; shift;; esac; done
case "$url" in
  http://127.0.0.1:*/health)
    printf 'health\n' >> "$BICAMERAL_TEST_LOG"
    # A proxied loopback probe would never reach the daemon.
    [ "$noproxy" = 1 ] || exit 28
    [ "${BICAMERAL_TEST_HEALTH_FAIL:-0}" = 1 ] && exit 7
    [ -f "$BICAMERAL_TEST_STATE" ] && [ "$(cat "$BICAMERAL_TEST_STATE")" = running ] || exit 7
    exit 0 ;;
esac
cp "${url#file://}" "$out"
SH
cat > "$FAKEBIN/systemctl" <<'SH'
#!/bin/sh
[ "$1" = "--user" ] && [ "$2" = "show-environment" ] && exit 0
[ "$1" = "--user" ] && [ "$2" = "enable" ] && printf 'systemctl enable %s\n' "$3" >> "$BICAMERAL_TEST_LOG"
exit 0
SH
chmod +x "$FAKEBIN/curl" "$FAKEBIN/systemctl"
cat > "$ROOT/bicameral" <<'SH'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$BICAMERAL_TEST_LOG"
[ "$1" = service ] || exit 2
case "$2" in
  install) [ "${BICAMERAL_TEST_INSTALL_FAIL:-0}" = 1 ] && exit 9; : ;;
  status) if [ -f "$BICAMERAL_TEST_STATE" ] && [ "$(cat "$BICAMERAL_TEST_STATE")" = running ]; then echo 'Bicameral daemon: running'; else echo 'Bicameral daemon: stopped'; fi ;;
  start) printf running > "$BICAMERAL_TEST_STATE"; echo 'Bicameral daemon started.' ;;
  stop) printf stopped > "$BICAMERAL_TEST_STATE"; echo 'Bicameral daemon stopped.' ;;
  *) exit 2 ;;
esac
SH
chmod +x "$ROOT/bicameral"
tar -czf "$RELEASE/vtest/bicameral-x86_64-unknown-linux-gnu.tar.gz" -C "$ROOT" bicameral
HASH="$(sha256sum "$RELEASE/vtest/bicameral-x86_64-unknown-linux-gnu.tar.gz" | awk '{print $1}')"
printf '%s  %s\n' "$HASH" 'bicameral-x86_64-unknown-linux-gnu.tar.gz' > "$RELEASE/vtest/SHA256SUMS"
printf '{"tag":"vtest"}\n' > "$RELEASE/latest.json"
run_install() { HOME="$HOME_DIR" PATH="$FAKEBIN:$PATH" BICAMERAL_VERSION=vtest BICAMERAL_DOWNLOAD_BASE="file://$RELEASE" BICAMERAL_TEST_STATE="$STATE" BICAMERAL_TEST_LOG="$LOG" sh site/install.sh; }
: > "$LOG"; run_install
[ "$(cat "$STATE")" = running ]; grep -q '^service install$' "$LOG"; grep -q '^service start$' "$LOG"
grep -q '^systemctl enable bicameral$' "$LOG"; grep -q '^health$' "$LOG"
printf stopped > "$STATE"; : > "$LOG"; run_install
grep -q '^service install$' "$LOG"; grep -q '^service start$' "$LOG"; ! grep -q '^service stop$' "$LOG"
printf running > "$STATE"; : > "$LOG"; run_install
grep -q '^service stop$' "$LOG"; grep -q '^service start$' "$LOG"
printf running > "$STATE"; : > "$LOG"
if HOME="$HOME_DIR" PATH="$FAKEBIN:$PATH" BICAMERAL_VERSION=vtest BICAMERAL_DOWNLOAD_BASE="file://$RELEASE" BICAMERAL_TEST_STATE="$STATE" BICAMERAL_TEST_LOG="$LOG" BICAMERAL_TEST_INSTALL_FAIL=1 sh site/install.sh >"$ROOT/fail-out" 2>&1; then echo 'expected service repair failure' >&2; exit 1; fi
[ "$(cat "$STATE")" = stopped ]; grep -q '^service stop$' "$LOG"; ! grep -q '^service start$' "$LOG"
rm -f "$HOME_DIR/.local/bin/bicameral"; ln -s /bin/false "$HOME_DIR/.local/bin/bicameral"
if run_install >"$ROOT/out" 2>&1; then echo 'expected symlink refusal' >&2; exit 1; fi
grep -q 'refusing to replace symlinked executable' "$ROOT/out"
rm -f "$HOME_DIR/.local/bin/bicameral"
: > "$LOG"
if HOME="$HOME_DIR" PATH="$FAKEBIN:$PATH" BICAMERAL_VERSION=vtest BICAMERAL_DOWNLOAD_BASE="file://$RELEASE" BICAMERAL_TEST_STATE="$STATE" BICAMERAL_TEST_LOG="$LOG" BICAMERAL_TEST_HEALTH_FAIL=1 BICAMERAL_HEALTH_ATTEMPTS=2 sh site/install.sh >"$ROOT/health-out" 2>&1; then echo 'expected unreachable-daemon failure' >&2; exit 1; fi
grep -q 'is not answering at http://127.0.0.1:37373/health' "$ROOT/health-out"
: > "$LOG"
if HOME="$HOME_DIR" PATH="$FAKEBIN:$PATH" BICAMERAL_VERSION=vtest BICAMERAL_DOWNLOAD_BASE="file://$RELEASE" BICAMERAL_TEST_STATE="$STATE" BICAMERAL_TEST_LOG="$LOG" BICAMERAL_HEALTH_ATTEMPTS=abc sh site/install.sh >"$ROOT/attempts-out" 2>&1; then echo 'expected invalid attempts failure' >&2; exit 1; fi
grep -q 'BICAMERAL_HEALTH_ATTEMPTS must be a whole number' "$ROOT/attempts-out"
printf '%s\n' 'unix reconcile tests: PASS'
