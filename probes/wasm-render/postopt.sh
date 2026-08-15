#!/usr/bin/env bash
# Re-measure already-bundled stages through wasm-opt -Oz and both compressors.
# Separate from measure.sh because wasm-opt only became available partway
# through the probe and rebuilding to get it would cost another 10 minutes.
set -uo pipefail

printf '%-34s %10s %10s %10s %10s %10s\n' stage bindgen wasm-opt gzip brotli js+gz
for d in pkg/*/; do
  s=$(basename "$d")
  BG="$d/straf3_wasm_render_probe_bg.wasm"
  [[ -f "$BG" ]] || continue
  BOUND=$(stat -c %s "$BG")

  if [[ ! -f "$d/opt.wasm" ]]; then
    wasm-opt -Oz --all-features "$BG" -o "$d/opt.wasm" 2>/dev/null \
      || wasm-opt -Oz "$BG" -o "$d/opt.wasm" 2>/dev/null \
      || cp "$BG" "$d/opt.wasm"
  fi
  OPT=$(stat -c %s "$d/opt.wasm")
  GZ=$(gzip -9 -c "$d/opt.wasm" | wc -c)
  BR=$(python3 -c "import brotli,sys;sys.stdout.write(str(len(brotli.compress(open(sys.argv[1],'rb').read(),quality=11))))" "$d/opt.wasm" 2>/dev/null || echo -)
  JSGZ=$(gzip -9 -c "$d"/straf3_wasm_render_probe.js | wc -c)

  printf '%-34s %10s %10s %10s %10s %10s\n' "$s" "$BOUND" "$OPT" "$GZ" "$BR" "$JSGZ"
done
