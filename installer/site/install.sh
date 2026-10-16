#!/bin/sh
set -eu

BIN="bicameral"
INSTALL_DIR="${BICAMERAL_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${BICAMERAL_VERSION:-latest}"
DOWNLOAD_BASE="${BICAMERAL_DOWNLOAD_BASE:-https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral}"

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'bicameral install: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

detect_target() {
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    darwin) os="apple-darwin" ;;
    linux) os="unknown-linux-gnu" ;;
    *) fail "unsupported OS: $os" ;;
  esac

  case "$arch" in
    arm64|aarch64) arch="aarch64" ;;
    x86_64|amd64) arch="x86_64" ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  printf '%s-%s' "$arch" "$os"
}

download() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$out"
  else
    fail "missing curl or wget"
  fi
}

sha256_file() {
  file="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  else
    fail "missing shasum or sha256sum"
  fi
}

sign_macos_binary() {
  binary="$1"
  target="$2"

  case "$target" in
    *-apple-darwin)
      command -v codesign >/dev/null 2>&1 || fail "missing required command: codesign"
      codesign --force --sign - "$binary" >/dev/null 2>&1 || fail "failed to sign macOS binary"
      ;;
  esac
}

normalize_tag() {
  case "$1" in
    v*) printf '%s' "$1" ;;
    *) printf 'v%s' "$1" ;;
  esac
}

manifest_value() {
  key="$1"
  file="$2"
  sed -n "s/.*\"$key\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$file" | head -n 1
}

need uname
need tar
need awk
need sed
need find
need mktemp

DOWNLOAD_BASE="${DOWNLOAD_BASE%/}"
target="$(detect_target)"
asset="${BIN}-${target}.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ "$VERSION" = "latest" ]; then
  download "$DOWNLOAD_BASE/latest.json" "$tmp/latest.json"
  tag="$(manifest_value tag "$tmp/latest.json")"
  if [ -z "$tag" ]; then
    version="$(manifest_value version "$tmp/latest.json")"
    [ -n "$version" ] || fail "latest.json missing tag or version"
    tag="$(normalize_tag "$version")"
  fi
else
  tag="$(normalize_tag "$VERSION")"
fi

base="$DOWNLOAD_BASE/$tag"

log "Installing Bicameral $tag for $target"
download "$base/$asset" "$tmp/$asset"
download "$base/SHA256SUMS" "$tmp/SHA256SUMS"

expected="$(awk -v file="$asset" '$2 == file { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS does not contain $asset"

actual="$(sha256_file "$tmp/$asset")"
[ "$expected" = "$actual" ] || fail "checksum mismatch for $asset"

mkdir -p "$tmp/extract"
tar -xzf "$tmp/$asset" -C "$tmp/extract"

binary="$(find "$tmp/extract" -type f -name "$BIN" | head -n 1)"
[ -n "$binary" ] || fail "archive did not contain $BIN"
sign_macos_binary "$binary" "$target"

if [ -L "$INSTALL_DIR" ]; then
  fail "refusing to install through symlinked directory: $INSTALL_DIR"
fi
mkdir -p "$INSTALL_DIR"
installed="$INSTALL_DIR/$BIN"
if [ -L "$installed" ]; then
  fail "refusing to replace symlinked executable: $installed"
fi
if [ -e "$installed" ] && [ ! -f "$installed" ]; then
  fail "refusing to replace non-regular executable path: $installed"
fi

incoming_hash="$(sha256_file "$binary")"
binary_changed=1
if [ -f "$installed" ]; then
  installed_hash="$(sha256_file "$installed")"
  if [ "$installed_hash" = "$incoming_hash" ]; then
    binary_changed=0
  fi
fi

if [ "$binary_changed" -eq 1 ]; then
  staged="$INSTALL_DIR/.${BIN}.install.$$"
  cp "$binary" "$staged"
  chmod 755 "$staged"
  mv -f "$staged" "$installed"
  log "Installed verified $BIN to $installed"
else
  log "Verified $BIN is already current at $installed"
fi

case "$target" in
  *-unknown-linux-gnu)
    command -v systemctl >/dev/null 2>&1 || fail "systemd is required for the Bicameral user service"
    if ! systemctl --user show-environment >/dev/null 2>&1; then
      if [ -r /proc/sys/kernel/osrelease ] && grep -qi microsoft /proc/sys/kernel/osrelease; then
        fail "WSL detected but the systemd user manager is unavailable; enable systemd in WSL before installing Bicameral"
      fi
      fail "the systemd user manager is unavailable for this session"
    fi
    ;;
  *-apple-darwin)
    command -v launchctl >/dev/null 2>&1 || fail "launchd is required for the Bicameral user service"
    ;;
esac

log "Reconciling Bicameral user service..."
service_state="$("$installed" service status 2>/dev/null || true)"
case "$service_state" in
  *running*)
    log "Bicameral service is already running; stopping it before service repair."
    "$installed" service stop
    ;;
esac

# Repair service configuration only after any existing daemon is stopped. This
# fails closed: if repair cannot complete, an older daemon is not left running
# for the browser to mistake as successful reconciliation.
"$installed" service install
"$installed" service start
service_state="$("$installed" service status 2>/dev/null || true)"
case "$service_state" in
  *running*) ;;
  *) fail "Bicameral service did not reach running state after reconciliation" ;;
esac

# Older releases link the systemd user unit without enabling it, so the
# daemon would not return after logout/reboot (install#19). Enabling is
# idempotent and touches only Bicameral's own user unit.
case "$target" in
  *-unknown-linux-gnu)
    unit="bicameral"
    if [ -n "${BICAMERAL_SERVICE_INSTANCE:-}" ]; then
      unit="bicameral-$BICAMERAL_SERVICE_INSTANCE"
    fi
    systemctl --user enable "$unit" >/dev/null 2>&1 \
      || fail "could not enable the Bicameral user service ($unit) to start at login"
    ;;
esac

# "running" to the service manager is not the same as reachable by the
# browser. Wait for the local gateway to answer before telling the user to
# return to the browser.
health_url="http://127.0.0.1:${BICAMERAL_GATEWAY_PORT:-37373}/health"
# Loopback must never go through a configured HTTP proxy.
probe_health() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsS --noproxy '*' --max-time 2 "$health_url" -o /dev/null >/dev/null 2>&1
  else
    no_proxy='*' NO_PROXY='*' http_proxy='' HTTP_PROXY='' \
      wget -q -t 1 -T 2 -O /dev/null "$health_url" >/dev/null 2>&1
  fi
}
health_attempts="${BICAMERAL_HEALTH_ATTEMPTS:-60}"
case "$health_attempts" in
  ''|*[!0-9]*) fail "BICAMERAL_HEALTH_ATTEMPTS must be a whole number" ;;
esac
attempt=0
until probe_health; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge "$health_attempts" ]; then
    fail "Bicameral started but is not answering at $health_url. Run: $installed doctor"
  fi
  sleep 1
done

log "Bicameral is installed, running, and reachable."
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    log "Optional CLI convenience: add $INSTALL_DIR to PATH for future terminal commands."
    ;;
esac
log "Return to https://app.bicameral-ai.com/connect; the open page will detect this service automatically."
