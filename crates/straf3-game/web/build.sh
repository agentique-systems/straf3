#!/usr/bin/env bash
# Build straf3 for the browser and report the shipped bundle size.
#
# Spec criterion 2 asks for a *measured* stage-D bundle size, so this script
# ends by printing the numbers rather than leaving them to be estimated:
# gzipped wasm plus the wasm-bindgen JS glue, which together are what a
# visitor actually downloads.
#
#   crates/straf3-game/web/build.sh [--debug]
#
# Then serve the directory and open it — any static server will do, but the
# module must be served over http, not file://:
#
#   python3 -m http.server -d crates/straf3-game/web 8080
#
# On a software-only machine (this WSL2 box is one), Chrome needs
#   --enable-unsafe-webgpu --use-angle=swiftshader
# before it will offer a WebGPU adapter at all.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
profile="release"
[[ "${1:-}" == "--debug" ]] && profile="debug"

target_dir="${CARGO_TARGET_DIR:-$repo/target}"
out="$here/pkg"

echo "==> cargo build (${profile}, wasm32-unknown-unknown)"
if [[ "$profile" == "release" ]]; then
    cargo build --manifest-path "$repo/Cargo.toml" -p straf3-game \
        --target wasm32-unknown-unknown --release
else
    cargo build --manifest-path "$repo/Cargo.toml" -p straf3-game \
        --target wasm32-unknown-unknown
fi

wasm="$target_dir/wasm32-unknown-unknown/$profile/straf3_game.wasm"
[[ -f "$wasm" ]] || { echo "no wasm at $wasm" >&2; exit 1; }

echo "==> wasm-bindgen"
rm -rf "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" "$wasm"

if command -v wasm-opt >/dev/null 2>&1 && [[ "$profile" == "release" ]]; then
    echo "==> wasm-opt -Oz"
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
        -o "$out/straf3_game_bg.wasm" "$out/straf3_game_bg.wasm"
fi

echo
echo "==> shipped size (what a visitor downloads)"
for f in "$out/straf3_game_bg.wasm" "$out/straf3_game.js"; do
    raw=$(stat -c %s "$f")
    gz=$(gzip -9 -c "$f" | wc -c)
    printf '  %-24s %9s B   %9s B gzipped\n' "$(basename "$f")" "$raw" "$gz"
done
total=$(cat "$out/straf3_game_bg.wasm" "$out/straf3_game.js" | gzip -9 -c | wc -c)
printf '  %-24s %9s     %9s B gzipped\n' "TOTAL" "" "$total"
