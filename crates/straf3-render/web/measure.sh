#!/usr/bin/env bash
# Measure the shipped size of the real web build — spec rev 6 criterion 2's
# "stage D".
#
# The pipeline is deliberately identical to probes/wasm-render/measure.sh
# (cargo -> wasm-bindgen -> wasm-opt -Oz -> gzip -9), because the whole value
# of the number is that it can be compared against stages A, B and C, and a
# pipeline that differed anywhere would make that comparison a guess. Results
# are appended to that same probe's sizes.txt so the record stays in one place.
#
#   ./measure.sh <label> [--profile-of-record | --probe-equivalent]
#
# --profile-of-record  (default) the root workspace's release profile as it
#                      actually is: opt-level 3, thin LTO, unwinding panics,
#                      symbols kept. This is what shipping from this repo today
#                      would produce.
# --probe-equivalent   opt-level z, fat LTO, panic=abort, stripped — matching
#                      the probe's own [profile.release], so the number is
#                      apples-to-apples with stages A/B/C. Set with `--config`
#                      overrides so no file in the repo has to change.
set -euo pipefail

cd "$(dirname "$0")/../../.."   # repo root

LABEL="${1:?usage: measure.sh <label> [--probe-equivalent]}"
MODE="${2:---profile-of-record}"

CONFIG=()
if [[ "$MODE" == "--probe-equivalent" ]]; then
  CONFIG=(
    --config 'profile.release.opt-level="z"'
    --config 'profile.release.lto="fat"'
    --config 'profile.release.panic="abort"'
    --config 'profile.release.strip=true'
    --config 'profile.release.codegen-units=1'
  )
fi

FEATURES=()
if [[ -n "${MEASURE_FEATURES:-}" ]]; then
  FEATURES=(--features "$MEASURE_FEATURES")
fi

OUT="crates/straf3-render/web/pkg/$LABEL"
mkdir -p "$OUT"

cargo build -p straf3-render --release --target wasm32-unknown-unknown \
  --example web-demo "${CONFIG[@]}" "${FEATURES[@]}"

WASM="target/wasm32-unknown-unknown/release/examples/web_demo.wasm"
RAW=$(stat -c %s "$WASM")

wasm-bindgen "$WASM" --out-dir "$OUT" --target web --no-typescript
BG="$OUT/web_demo_bg.wasm"
BOUND=$(stat -c %s "$BG")

# -Oz with the feature set wasm-bindgen's output actually uses. Overwriting
# $BG rather than leaving an opt.wasm beside it, because a stale unoptimised
# file next to an optimised one is exactly how the stage-B line in sizes.txt
# came to disagree with the report that quoted it.
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
  --enable-reference-types --enable-mutable-globals \
  "$BG" -o "$OUT/opt.wasm"
mv "$OUT/opt.wasm" "$BG"
OPT=$(stat -c %s "$BG")

GZ=$(gzip -9 -c "$BG" | wc -c)
BR="n/a"
command -v brotli >/dev/null 2>&1 && BR=$(brotli -q 11 -c "$BG" | wc -c)
JS=$(stat -c %s "$OUT/web_demo.js")
JSGZ=$(gzip -9 -c "$OUT/web_demo.js" | wc -c)

cp crates/straf3-render/web/index.html "$OUT/index.html"

printf '%s\n' \
  "$LABEL profile=release($MODE) raw=$RAW bindgen=$BOUND wasmopt=$OPT gzip=$GZ brotli=$BR js=$JS jsgz=$JSGZ" \
  | tee -a probes/wasm-render/sizes.txt
