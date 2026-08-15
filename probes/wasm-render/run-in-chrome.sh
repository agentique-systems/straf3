#!/usr/bin/env bash
# Serve one measured stage and load it in real Chrome, headless. Dumps the DOM
# (which the page mirrors console output into) plus a screenshot, so "it
# rendered" is an observation and not an inference.
#
#   ./run-in-chrome.sh <stage-label> [extra chrome flags...]
set -uo pipefail

STAGE="$1"; shift
DIR="pkg/$STAGE"
[[ -d "$DIR" ]] || { echo "no such stage: $DIR"; exit 1; }

PORT=$(( 8300 + RANDOM % 400 ))
python3 -m http.server "$PORT" --directory "$DIR" >/dev/null 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT
sleep 1

PROFILE=$(mktemp -d)
OUT="$DIR/chrome"
mkdir -p "$OUT"

# --enable-unsafe-swiftshader: this box has no hardware GPU (WSL2, lavapipe).
# It makes Chrome accept a software WebGPU adapter instead of refusing one, so
# the probe tests the code path rather than the driver.
google-chrome \
  --headless=new \
  --no-sandbox \
  --disable-dev-shm-usage \
  --user-data-dir="$PROFILE" \
  --enable-unsafe-swiftshader \
  --enable-features=Vulkan,WebGPU \
  --virtual-time-budget=20000 \
  --window-size=1280,720 \
  --screenshot="$OUT/screen.png" \
  --dump-dom \
  "$@" \
  "http://127.0.0.1:$PORT/index.html" 2>"$OUT/stderr.log" >"$OUT/dom.html"

echo "--- probe lines ---"
grep -oE '\[[a-z]+\] [^<]*|PROBE [^<]*' "$OUT/dom.html" | sed 's/&quot;/"/g' | head -40
echo "--- screenshot ---"
ls -l "$OUT/screen.png" 2>/dev/null || echo "no screenshot"
