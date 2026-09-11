#!/usr/bin/env bash
# Fetch the pinned browser libraries into ui/vendor/ and verify them against vendor.lock.
# Development-time only: the fetched files are committed and embedded in the binary, so
# users never run this. `--record` rewrites the lock after a deliberate upgrade.
set -euo pipefail
cd "$(dirname "$0")/../ui/vendor"
record=0; [ "${1:-}" = "--record" ] && record=1
entries=(
  "htmx.min.js|2.0.10|https://unpkg.com/htmx.org@2.0.10/dist/htmx.min.js"
  "idiomorph-ext.min.js|0.8.0|https://unpkg.com/idiomorph@0.8.0/dist/idiomorph-ext.min.js"
  "echarts.common.min.js|6.1.0|https://unpkg.com/echarts@6.1.0/dist/echarts.common.min.js"
)
: > vendor.lock.new
for e in "${entries[@]}"; do
  IFS='|' read -r name version url <<<"$e"
  curl -fsSL "$url" -o "$name.tmp"
  sum=$(shasum -a 256 "$name.tmp" | cut -d' ' -f1)
  if [ $record -eq 0 ] && [ -f vendor.lock ]; then
    want=$(grep -E "^$name\b" vendor.lock | cut -f4 || true)
    if [ -n "$want" ] && [ "$want" != "$sum" ]; then echo "checksum mismatch for $name" >&2; rm -f "$name.tmp"; exit 1; fi
  fi
  mv "$name.tmp" "$name"
  printf '%s\t%s\t%s\t%s\n' "$name" "$version" "$url" "$sum" >> vendor.lock.new
done
if [ $record -eq 1 ] || [ ! -f vendor.lock ]; then mv vendor.lock.new vendor.lock; else rm vendor.lock.new; fi
ls -la
