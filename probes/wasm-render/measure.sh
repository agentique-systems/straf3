#!/usr/bin/env bash
# Measure the shipped size of one build stage. Not an estimate — the numbers
# come from the actual artifact that a browser would download.
#
#   ./measure.sh <label> [--features ...] [--profile ...]
set -euo pipefail

LABEL="$1"; shift
PROFILE="release"
if [[ "${1:-}" == "--profile" ]]; then PROFILE="$2"; shift 2; fi

OUT="pkg/$LABEL"
mkdir -p "$OUT"

cargo build --target wasm32-unknown-unknown --profile "$PROFILE" "$@"

WASM="target/wasm32-unknown-unknown/$PROFILE/straf3_wasm_render_probe.wasm"
RAW=$(stat -c %s "$WASM")

wasm-bindgen "$WASM" --out-dir "$OUT" --target web --no-typescript
BG="$OUT/straf3_wasm_render_probe_bg.wasm"
BOUND=$(stat -c %s "$BG")

OPT="n/a"
if command -v wasm-opt >/dev/null 2>&1; then
  if wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
       --enable-reference-types --enable-mutable-globals \
       "$BG" -o "$OUT/opt.wasm" 2>/dev/null; then
    mv "$OUT/opt.wasm" "$BG"
    OPT=$(stat -c %s "$BG")
  fi
fi

GZ=$(gzip -9 -c "$BG" | wc -c)
BR="n/a"
command -v brotli >/dev/null 2>&1 && BR=$(brotli -q 11 -c "$BG" | wc -c)
JS=$(stat -c %s "$OUT/straf3_wasm_render_probe.js")
JSGZ=$(gzip -9 -c "$OUT/straf3_wasm_render_probe.js" | wc -c)

cp web/index.html "$OUT/index.html"

printf '%s\n' "$LABEL profile=$PROFILE raw=$RAW bindgen=$BOUND wasmopt=$OPT gzip=$GZ brotli=$BR js=$JS jsgz=$JSGZ" \
  | tee -a sizes.txt
