#!/usr/bin/env bash
# Regenerate the model-based test traces replayed by src/daemon/tests/registry_mbt.rs.
# Run after changing product_registry.qnt, then commit the spec and traces together.
# Requires the Quint CLI (https://quint-lang.org). CI does not run Quint; it only
# replays the committed traces.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../tests/fixtures/product_registry_mbt"

rm -rf "$out"
mkdir -p "$out"
cd "$here"
quint run product_registry.qnt \
  --main=product_registry_2 \
  --invariant=safety \
  --max-steps=12 \
  --max-samples=600 \
  --n-traces=40 \
  --seed=0x5eed0065 \
  --out-itf="$out/trace_{seq}.itf.json"
