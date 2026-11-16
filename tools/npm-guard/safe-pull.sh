#!/bin/sh
# Pull only if the incoming commits are clean. Fetches, scans, then fast-forwards/merges.
#   tools/npm-guard/safe-pull.sh [remote] [branch]      (default: the current branch's upstream)
# Git has no hook that runs before a fast-forward pull changes your files, so scanning BEFORE the
# update is the only way to stop an infected commit reaching your disk. Use this (or `git safe-pull`,
# set up by `npm run security:setup`) instead of `git pull`.
set -e
here=$(cd "$(dirname "$0")" && pwd)   # the installed copy, not the working tree
if [ $# -ge 2 ]; then target="$1/$2"; git fetch "$1" "$2"
else
  target=$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}') || { echo "no upstream set; pass: safe-pull <remote> <branch>" >&2; exit 1; }
  git fetch "${target%%/*}"
fi
sh "$here/py.sh" "$here/scan_incoming.py" --ref "$target" || {
  echo "npm-guard: NOT pulling $target. Your working tree is untouched." >&2; exit 1; }
echo "npm-guard: $target is clean, updating."
exec git merge "$target"
