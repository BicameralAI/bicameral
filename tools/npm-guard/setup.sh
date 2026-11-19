#!/bin/sh
# Per-clone npm-guard setup. Run it through npm from apps/bot/dashboard:
#   npm run security:setup    install the git hooks and the `git safe-pull` alias for this clone
#   npm run security:check    report what is set up; exit 1 if something is missing
#
# Setup copies the hooks and the scripts they call into the git directory (.git/npm-guard) and points
# core.hooksPath there. Git runs hooks after it updates the working tree, so hooks that ran from the
# working tree would run code from any branch you check out or pull. The installed copy changes only
# when you run setup again: do that on a reviewed main after tools/npm-guard changes.
#
# Setup changes only this clone's git config. It does not change your global npm config: the
# dashboard's own .npmrc already sets ignore-scripts and min-release-age for this repository.

root=$(git rev-parse --show-toplevel) || exit 1
cd "$root" || exit 1
source_dir=tools/npm-guard
dest=$(cd "$(git rev-parse --git-common-dir)" && pwd)/npm-guard
dashboard=apps/bot/dashboard
installed="py.sh scan_incoming.py check_dependency_changes.py registry_check.py safe-pull.sh"
hooks="pre-commit pre-push pre-merge-commit post-merge post-rewrite post-checkout"
alias_value="!sh \"$dest/safe-pull.sh\""

if [ "$1" = "--check" ]; then
  ok=0
  current=$(git config --get core.hooksPath)
  [ "$current" = "$dest" ] && echo "ok:   git hooks enabled ($dest)" \
    || { echo "MISSING: git hooks (core.hooksPath is '${current:-unset}'; run npm run security:setup)"; ok=1; }
  stale=
  for f in $installed; do cmp -s "$source_dir/$f" "$dest/$f" || stale="$stale $f"; done
  for f in $hooks; do cmp -s "$source_dir/hooks/$f" "$dest/$f" || stale="$stale $f"; done
  [ -z "$stale" ] && echo "ok:   installed hooks match $source_dir" \
    || { echo "MISSING: installed hooks differ from $source_dir:$stale (review the change, then run npm run security:setup)"; ok=1; }
  [ "$(git config --get alias.safe-pull)" = "$alias_value" ] && echo "ok:   git safe-pull alias" \
    || { echo "MISSING: git safe-pull alias (run npm run security:setup)"; ok=1; }
  sh "$source_dir/py.sh" -c 'pass' 2>/dev/null && echo "ok:   Python 3 found (hooks and scans can run)" \
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

current=$(git config --get core.hooksPath)
case "$current" in
  ''|"$dest"|"$source_dir/hooks") ;;
  *) echo "core.hooksPath is already '$current'; not overwriting."
     echo "Run 'sh $source_dir/setup.sh' after you remove it, or call the hooks in $dest from your own hooks."
     exit 1;;
esac
mkdir -p "$dest" || exit 1
for f in $installed; do cp "$source_dir/$f" "$dest/$f" || exit 1; done
for f in $hooks; do cp "$source_dir/hooks/$f" "$dest/$f" && chmod 755 "$dest/$f" || exit 1; done
git config core.hooksPath "$dest" && echo "installed git hooks in $dest and enabled them for this clone"
git config alias.safe-pull "$alias_value" \
  && echo "added 'git safe-pull': scans incoming commits before it updates your files (use it instead of git pull)"
echo "optional: to protect npm installs in your other projects too, run"
echo "  npm config set ignore-scripts true --global && npm config set min-release-age 7 --global"
