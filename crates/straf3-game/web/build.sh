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
# `web-release` and not `release`: see the profile's comment in the root
# Cargo.toml. Plain `release` is tuned for a native binary and costs 22 % more
# download for nothing the browser can use.
profile="web-release"
[[ "${1:-}" == "--debug" ]] && profile="debug"

target_dir="${CARGO_TARGET_DIR:-$repo/target}"
out="$here/pkg"

# `--no-default-features --features render`, and the omission is the point:
# the default set adds `devtools`, which is the egui overlay.
# `docs/web/ARCHITECTURE.md` §0 item 7 decided the browser bundle is "WebGPU
# only, no compiled-in WebGL2 fallback, no `egui`", and building the default
# feature set here silently broke that — measured at 2.6 MB gzipped against
# probes/wasm-render stage D's 171 KB, with egui, epaint, ecolor and emath all
# in the shipped wasm. A player on a `/play/<map>` link never asked for a
# speedometer, and paying fifteen times the download for one is not a trade
# anybody made deliberately.
features=(--no-default-features --features render)

echo "==> cargo build (${profile}, wasm32-unknown-unknown, features: render)"
if [[ "$profile" == "debug" ]]; then
    cargo build --manifest-path "$repo/Cargo.toml" -p straf3-game \
        --target wasm32-unknown-unknown "${features[@]}"
else
    cargo build --manifest-path "$repo/Cargo.toml" -p straf3-game \
        --target wasm32-unknown-unknown --profile "$profile" "${features[@]}"
fi

wasm="$target_dir/wasm32-unknown-unknown/$profile/straf3_game.wasm"
[[ -f "$wasm" ]] || { echo "no wasm at $wasm" >&2; exit 1; }

echo "==> wasm-bindgen"
rm -rf "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" "$wasm"

if command -v wasm-opt >/dev/null 2>&1 && [[ "$profile" != "debug" ]]; then
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

# Assert the shape ARCHITECTURE §0 item 7 decided, rather than trusting the
# feature flags above to have meant it. A byte-scan is the check that cannot be
# fooled by a transitive dependency re-introducing the overlay: the size alone
# would not say *what* got in, and by the time anyone measured the size again
# it would be somebody else's problem.
echo
echo "==> bundle shape (ARCHITECTURE §0 item 7: WebGPU only, no WebGL2, no egui)"
banned=0
for symbol in egui epaint ecolor; do
    hits=$(grep -c -a -o "$symbol" "$out/straf3_game_bg.wasm" || true)
    if [[ "$hits" -gt 0 ]]; then
        echo "  FAIL  '$symbol' appears $hits time(s) in the shipped wasm" >&2
        banned=1
    else
        echo "  ok    no '$symbol'"
    fi
done
[[ "$banned" -eq 0 ]] || {
    echo "The overlay is in the browser bundle. Build with --no-default-features --features render." >&2
    exit 1
}
