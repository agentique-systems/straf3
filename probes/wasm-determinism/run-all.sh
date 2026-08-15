#!/usr/bin/env bash
# Reproduce the whole probe: native (glibc), native (musl), wasm under Node,
# wasm in real Chrome — then diff every pair that matters.
#
#   ./run-all.sh            # writes reports and comparisons into results/
#
# Requires: the pinned toolchain (rust-toolchain.toml at the repo root), the
# wasm32-unknown-unknown target, node, and google-chrome.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p results
PORT="${PORT:-8842}"

echo "── building ──"
cargo build --release --bin native-report
cargo build --release --lib --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/det_probe.wasm web/det_probe.wasm

echo "── native, glibc ──"
./target/release/native-report > results/native-glibc.json

if rustup target list --installed | grep -q x86_64-unknown-linux-musl; then
  echo "── native, musl ──"
  cargo build --release --bin native-report --target x86_64-unknown-linux-musl
  ./target/x86_64-unknown-linux-musl/release/native-report > results/native-musl.json
fi

echo "── wasm under node (fast proxy) ──"
node web/node-run.mjs web/det_probe.wasm > results/node.json 2> results/node-imports.txt

echo "── wasm in headless chrome (ground truth) ──"
node web/serve.mjs "$PORT" & SERVER=$!
trap 'kill $SERVER 2>/dev/null || true' EXIT
sleep 1
google-chrome --headless=new --disable-gpu --no-sandbox \
  --virtual-time-budget=120000 --dump-dom "http://127.0.0.1:$PORT/index.html" \
  > results/chrome-dom.html 2>/dev/null
node web/extract.mjs results/chrome-dom.html > results/chrome.json
rm -f results/chrome-dom.html

echo "── comparing ──"
node web/compare.mjs results/native-glibc.json results/chrome.json \
  > results/compare-glibc-vs-chrome.txt
node web/compare.mjs results/node.json results/chrome.json \
  > results/compare-node-vs-chrome.txt
if [ -f results/native-musl.json ]; then
  node web/compare.mjs results/native-musl.json results/chrome.json \
    > results/compare-musl-vs-chrome.txt
fi
# Explicit list, not a glob: digests.json itself must not be re-summarised.
REPORTS=(results/native-glibc.json results/node.json results/chrome.json)
[ -f results/native-musl.json ] && REPORTS+=(results/native-musl.json)
node web/summarise.mjs "${REPORTS[@]}" > results/digests.json

echo "done — see results/"
