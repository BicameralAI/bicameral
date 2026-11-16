#!/bin/sh
# Per-clone npm-guard setup. Run it through npm from apps/bot/dashboard:
#   npm run security:setup    enable the git hooks and the `git safe-pull` alias for this clone
#   npm run security:check    report what is set up; exit 1 if something is missing
#
# Setup changes only this clone's git config. It does not change your global npm config: the
# dashboard's own .npmrc already sets ignore-scripts and min-release-age for this repository.

root=$(git rev-parse --show-toplevel) || exit 1
cd "$root" || exit 1
hooks_dir=tools/npm-guard/hooks
dashboard=apps/bot/dashboard

if [ "$1" = "--check" ]; then
  ok=0
  hooks=$(git config --get core.hooksPath)
  [ "$hooks" = "$hooks_dir" ] && echo "ok:   git hooks enabled ($hooks_dir)" \
    || { echo "MISSING: git hooks (core.hooksPath is '${hooks:-unset}'; run npm run security:setup)"; ok=1; }
  git config --get alias.safe-pull >/dev/null && echo "ok:   git safe-pull alias" \
    || { echo "MISSING: git safe-pull alias (run npm run security:setup)"; ok=1; }
  sh tools/npm-guard/py.sh -c 'pass' 2>/dev/null && echo "ok:   Python 3 found (hooks and scans can run)" \
    || { echo "MISSING: Python 3 (the hooks and scans are Python scripts)"; ok=1; }
  if command -v npm >/dev/null 2>&1; then
    # Read the settings the way npm sees them inside the dashboard, where the project .npmrc applies.
    scripts=$(cd "$dashboard" && npm config get ignore-scripts 2>/dev/null)
    age=$(cd "$dashboard" && npm config get min-release-age 2>/dev/null)
    version=$(npm -v 2>/dev/null)
    [ "$scripts" = "true" ] && echo "ok:   npm ignore-scripts=true in $dashboard" \
      || { echo "MISSING: npm ignore-scripts=true in $dashboard"; ok=1; }
    # Older npm reads the key back too, but ignores it. Only npm 11.10 or newer enforces it.
    major=${version%%.*}; rest=${version#*.}; minor=${rest%%.*}
    case "$age" in ''|null|undefined|0) age=;; esac
    if [ -z "$age" ]; then
      echo "MISSING: npm min-release-age is not set in $dashboard/.npmrc"; ok=1
    elif [ "${major:-0}" -gt 11 ] 2>/dev/null || { [ "${major:-0}" -eq 11 ] && [ "${minor:-0}" -ge 10 ]; } 2>/dev/null; then
      echo "ok:   npm min-release-age=$age days (npm $version)"
    else
      echo "MISSING: npm min-release-age needs npm 11.10 or newer; you have npm ${version:-unknown}"; ok=1
    fi
  else
    echo "MISSING: npm"; ok=1
  fi
  exit $ok
fi

hooks=$(git config --get core.hooksPath)
if [ -n "$hooks" ] && [ "$hooks" != "$hooks_dir" ]; then
  echo "core.hooksPath is already '$hooks'; not overwriting. Call the hooks in $hooks_dir from your own hooks."
else
  git config core.hooksPath "$hooks_dir" && echo "enabled git hooks ($hooks_dir) for this clone"
fi
git config alias.safe-pull '!sh tools/npm-guard/safe-pull.sh' \
  && echo "added 'git safe-pull': scans incoming commits before it updates your files (use it instead of git pull)"
echo "optional: to protect npm installs in your other projects too, run"
echo "  npm config set ignore-scripts true --global && npm config set min-release-age 7 --global"
