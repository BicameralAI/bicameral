#!/bin/sh
# Run a Python 3 interpreter on any OS: python3 (Linux/macOS), python or `py -3` (Windows, where
# `python3` is usually a Microsoft Store stub that fails). Usage: sh tools/npm-guard/py.sh script.py [args]
for c in python3 python; do
  if command -v "$c" >/dev/null 2>&1 && "$c" -c 'import sys; sys.exit(sys.version_info < (3, 6))' >/dev/null 2>&1; then
    exec "$c" "$@"
  fi
done
if command -v py >/dev/null 2>&1 && py -3 -c 'pass' >/dev/null 2>&1; then exec py -3 "$@"; fi
echo "npm-guard: Python 3 not found (tried python3, python, py -3)" >&2
exit 127
